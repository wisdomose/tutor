//! Background processing: uploaded pages → Claude reads them → each question is
//! matched against the bank → gpt-6.1-sol and gpt-6-luna answer the new objective
//! ones → questions saved and flagged → diagrams drawn by gpt-6.1-sol.
//!
//! Pages are processed one at a time so the owner's Claude Pro allowance is not
//! drained in one go. When Claude reports its usage limit the queue pauses until
//! the reset time. Diagrams run in their own loop since they only use OpenAI.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use tutor_claude::UsageLimit;
use tutor_openai::{LUNA, Part, SOL};
use uuid::Uuid;

use crate::AppState;
use crate::matcher::{self, Candidate, Decision, Key, Text};
use crate::prompts::{self, Answer, Answers, Extraction, QuestionForAnswer, TailFragment};

const MAX_ATTEMPTS: i32 = 3;
const IDLE: Duration = Duration::from_secs(5);

pub fn spawn(state: AppState) {
    tokio::spawn(page_loop(state.clone()));
    tokio::spawn(diagram_loop(state));
}

// ── Pages ───────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ClaimedPage {
    id: Uuid,
    photo_key: String,
    attempts: i32,
}

/// The source a page belongs to, with the exam defaults its questions start from.
#[derive(sqlx::FromRow)]
struct PageSource {
    source_id: Uuid,
    title: String,
    kind: String,
    layout: String,
    subject_id: Uuid,
    subject: String,
    level: String,
    exam_id: Option<Uuid>,
    year: Option<i32>,
    paper: Option<String>,
    sitting: Option<String>,
    position: i32,
}

async fn page_source(db: &PgPool, page_id: Uuid) -> sqlx::Result<PageSource> {
    sqlx::query_as(
        "select s.id as source_id, s.title, s.kind, s.layout, s.subject_id, sub.name as subject, sub.level,
                s.exam_id, s.year, s.paper, s.sitting, p.position
         from pages p join sources s on s.id = p.source_id join subjects sub on sub.id = s.subject_id
         where p.id = $1",
    )
    .bind(page_id)
    .fetch_one(db)
    .await
}

/// (id, body code, exam code, level).
type ExamRow = (Uuid, String, String, String);

async fn load_exams(db: &PgPool) -> sqlx::Result<Vec<ExamRow>> {
    sqlx::query_as(
        "select e.id, b.code, e.code, e.level from exams e join exam_bodies b on b.id = e.body_id
         order by b.code, e.code",
    )
    .fetch_all(db)
    .await
}

/// A subject's syllabus topics in syllabus order, labelled "Theme > Topic".
async fn load_topics(db: &PgPool, subject_id: Uuid) -> sqlx::Result<Vec<(Uuid, String)>> {
    sqlx::query_as(
        "select t.id, case when p.id is null then t.name else p.name || ' > ' || t.name end
         from syllabus_topics t left join syllabus_topics p on p.id = t.parent_id
         where t.subject_id = $1
         order by coalesce(p.position, t.position), coalesce(p.name, t.name), p.id is not null, t.position, t.name",
    )
    .bind(subject_id)
    .fetch_all(db)
    .await
}

const SITTINGS: [&str; 3] = ["may_june", "june_july", "nov_dec"];

/// The source's defaults in words, e.g. "WAEC WASSCE, 1993, Paper 2, may_june".
fn describe_defaults(src: &PageSource, exams: &[ExamRow]) -> Option<String> {
    let exam = src
        .exam_id
        .and_then(|id| exams.iter().find(|e| e.0 == id))
        .map(|e| format!("{} {}", e.1, e.2));
    let parts: Vec<String> = [
        exam,
        src.year.map(|y| y.to_string()),
        src.paper.as_ref().map(|p| format!("Paper {p}")),
        src.sitting.clone(),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Where a question says it was set, as read; anything missing comes from the source.
struct Printed<'a> {
    exam: Option<&'a str>,
    year: Option<i32>,
    paper: Option<&'a str>,
    sitting: Option<&'a str>,
    original_number: Option<&'a str>,
}

fn resolve_key(src: &PageSource, exams: &[ExamRow], p: Printed, number: &str) -> Key {
    let (exam_id, exam_code) = match p
        .exam
        .and_then(|e| matcher::find_exam(e, &src.level, exams))
    {
        Some(e) => (Some(e.0), Some(e.2.clone())),
        None => (
            src.exam_id,
            src.exam_id
                .and_then(|id| exams.iter().find(|e| e.0 == id))
                .map(|e| e.2.clone()),
        ),
    };
    // In a year-ordered book or a paper the printed number is the exam's own.
    let original_number = p
        .original_number
        .map(norm_number)
        .filter(|n| !n.is_empty())
        .or_else(|| (src.layout != "by_topic").then(|| norm_number(number)));
    Key {
        exam_id,
        exam_code,
        year: p.year.filter(|y| (1950..=2100).contains(y)).or(src.year),
        paper: p
            .paper
            .and_then(matcher::norm_paper)
            .or_else(|| src.paper.clone()),
        sitting: p
            .sitting
            .filter(|s| SITTINGS.contains(s))
            .map(str::to_string)
            .or_else(|| src.sitting.clone()),
        original_number,
    }
}

/// A question as read, before it is matched and saved.
struct Draft {
    kind: String,
    number: String,
    key: Key,
    /// (id, "Theme > Topic").
    topic: Option<(Uuid, String)>,
    marks: Option<f64>,
    body: String,
    options: Vec<prompts::ExtractedOption>,
    parts: Vec<prompts::ExtractedPart>,
    group_ref: Option<String>,
    diagram_ref: Option<String>,
    refers_to_number: bool,
}

fn part_text(body: Option<&str>, closing: Option<&str>) -> String {
    format!("{} {}", body.unwrap_or(""), closing.unwrap_or(""))
        .trim()
        .to_string()
}

/// The page's diagrams and groups once saved, keyed by the reader's refs.
struct PageRefs<'a> {
    page_id: Uuid,
    page_number: Option<&'a str>,
    diagrams: &'a HashMap<String, Uuid>,
    /// (id, whether the group has a diagram).
    groups: &'a HashMap<String, (Uuid, bool)>,
}

impl Draft {
    fn text(&self) -> Text {
        Text {
            body: self.body.clone(),
            options: self
                .options
                .iter()
                .map(|o| (o.label.clone(), o.body.clone().unwrap_or_default()))
                .collect(),
            parts: self
                .parts
                .iter()
                .map(|p| part_text(p.body.as_deref(), p.closing_text.as_deref()))
                .collect(),
        }
    }

    fn group(&self, r: &PageRefs) -> Option<(Uuid, bool)> {
        self.group_ref
            .as_deref()
            .and_then(|g| r.groups.get(g))
            .copied()
    }

    fn diagram_id(&self, r: &PageRefs) -> Option<Uuid> {
        self.diagram_ref
            .as_deref()
            .and_then(|d| r.diagrams.get(d))
            .copied()
    }

    fn has_diagram(&self, r: &PageRefs) -> bool {
        self.diagram_ref.is_some()
            || self.group(r).is_some_and(|(_, has)| has)
            || self.options.iter().any(|o| o.diagram_ref.is_some())
            || self.parts.iter().any(|p| p.diagram_ref.is_some())
    }

    /// How the answering models see it; diagrams are given by description.
    fn for_answer<'a>(
        &'a self,
        instruction: Option<&'a str>,
        diagram_text: &'a HashMap<String, String>,
        group_diagram: Option<&'a str>,
    ) -> QuestionForAnswer<'a> {
        let describe = |r: Option<&str>| r.and_then(|r| diagram_text.get(r)).map(String::as_str);
        QuestionForAnswer {
            number: &self.number,
            topic: self.topic.as_ref().map(|t| t.1.as_str()),
            instruction,
            body: &self.body,
            diagram: describe(self.diagram_ref.as_deref()).or(describe(group_diagram)),
            options: self
                .options
                .iter()
                .map(|o| {
                    let text = match (&o.body, describe(o.diagram_ref.as_deref())) {
                        (Some(b), _) => b.clone(),
                        (None, Some(d)) => format!("(picture: {d})"),
                        (None, None) => String::new(),
                    };
                    (o.label.as_str(), text)
                })
                .collect(),
        }
    }
}

/// Two questions on one page with the same key (a book listing one question twice).
fn same_key(a: &Draft, b: &Draft) -> bool {
    let fits = |x: &Option<String>, y: &Option<String>| x.is_none() || y.is_none() || x == y;
    a.kind == b.kind
        && a.key.exam_id == b.key.exam_id
        && a.key.year == b.key.year
        && a.key.original_number == b.key.original_number
        && fits(&a.key.sitting, &b.key.sitting)
        && fits(&a.key.paper, &b.key.paper)
}

#[derive(PartialEq)]
enum Plan {
    Decided(Decision),
    /// The same as an earlier question on this page.
    SameAs(usize),
}

async fn page_loop(state: AppState) {
    // Anything left mid-way by a restart goes back in the queue.
    if let Err(e) = sqlx::query("update pages set status = 'queued' where status = 'processing'")
        .execute(&state.db)
        .await
    {
        tracing::error!("resetting pages: {e}");
    }
    loop {
        match claim_page(&state.db).await {
            Ok(Some(page)) => {
                let id = page.id;
                if let Err(e) = process_page(&state, page).await {
                    tracing::error!(page = %id, "processing page: {e:#}");
                }
            }
            Ok(None) => tokio::time::sleep(IDLE).await,
            Err(e) => {
                tracing::error!("claiming page: {e}");
                tokio::time::sleep(IDLE * 2).await;
            }
        }
    }
}

async fn claim_page(db: &PgPool) -> sqlx::Result<Option<ClaimedPage>> {
    let blocked: bool = sqlx::query_scalar(
        "select paused or coalesce(limited_until > now(), false) from queue_state",
    )
    .fetch_one(db)
    .await?;
    if blocked {
        return Ok(None);
    }
    sqlx::query_as(
        "update pages set status = 'processing', attempts = attempts + 1, error = null
         where id = (
           select id from pages
           where status = 'queued' and (next_attempt_at is null or next_attempt_at <= now())
           order by uploaded_at, position
           limit 1
           for update skip locked)
         returning id, photo_key, attempts",
    )
    .fetch_optional(db)
    .await
}

async fn process_page(state: &AppState, page: ClaimedPage) -> anyhow::Result<()> {
    let result = read_and_answer(state, &page).await;
    match result {
        Ok(()) => {
            if let Err(e) = join_split_questions(state, page.id).await {
                tracing::warn!(page = %page.id, "joining split questions: {e:#}");
            }
            Ok(())
        }
        Err(e) if e.downcast_ref::<UsageLimit>().is_some() => {
            let limit = e.downcast_ref::<UsageLimit>().expect("checked above");
            tracing::warn!(page = %page.id, "{limit}; pausing the queue");
            sqlx::query(
                "update queue_state set
                   limited_until = coalesce(to_timestamp($1), now() + interval '1 hour'),
                   limit_message = $2, updated_at = now()",
            )
            .bind(limit.resets_at.map(|t| t as f64))
            .bind(&limit.message)
            .execute(&state.db)
            .await?;
            // Not the page's fault: put it back without using up an attempt.
            sqlx::query(
                "update pages set status = 'queued', attempts = attempts - 1 where id = $1",
            )
            .bind(page.id)
            .execute(&state.db)
            .await?;
            Ok(())
        }
        Err(e) => {
            let message = format!("{e:#}");
            let (status, retry_in) = if page.attempts < MAX_ATTEMPTS {
                ("queued", Some(10 * 60 * page.attempts as i64))
            } else {
                ("failed", None)
            };
            sqlx::query(
                "update pages set status = $2, error = $3,
                   next_attempt_at = now() + make_interval(secs => $4)
                 where id = $1",
            )
            .bind(page.id)
            .bind(status)
            .bind(&message)
            .bind(retry_in.unwrap_or(0) as f64)
            .execute(&state.db)
            .await?;
            Err(e)
        }
    }
}

async fn read_and_answer(state: &AppState, page: &ClaimedPage) -> anyhow::Result<()> {
    let src = page_source(&state.db, page.id).await?;
    let exams = load_exams(&state.db).await?;
    let topics = load_topics(&state.db, src.subject_id).await?;
    let topic_refs: HashMap<String, &(Uuid, String)> = topics
        .iter()
        .enumerate()
        .map(|(i, t)| (format!("t{}", i + 1), t))
        .collect();
    // The heading and exam in force at the bottom of the previous page, if it's been read.
    let previous: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
        "select status, carried_context from pages where source_id = $1 and position < $2
         order by position desc limit 1",
    )
    .bind(src.source_id)
    .bind(src.position)
    .fetch_optional(&state.db)
    .await?;
    let carried = previous
        .filter(|(status, _)| status == "done")
        .and_then(|(_, c)| c);

    let photo = state
        .storage
        .get(&page.photo_key)
        .await
        .context("loading photo")?;
    let photo_b64 = B64.encode(&photo);
    let prompt = prompts::extract_prompt(&prompts::PageContext {
        source_title: &src.title,
        source_kind: &src.kind,
        layout: &src.layout,
        subject: &src.subject,
        level: &src.level,
        source_exam: describe_defaults(&src, &exams),
        exam_codes: exams
            .iter()
            .filter(|e| e.3 == src.level)
            .map(|e| format!("{} ({})", e.2, e.1))
            .collect(),
        carried: carried.as_ref(),
        topics: topics
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("t{}", i + 1), t.1.clone()))
            .collect(),
    });

    // 1. Claude reads the page.
    let read = state
        .claude
        .run(tutor_claude::Request {
            system: prompts::EXTRACT_SYSTEM,
            prompt: &prompt,
            image: Some(tutor_claude::Image {
                media_type: "image/jpeg",
                base64: &photo_b64,
            }),
            schema: &prompts::EXTRACT_SCHEMA,
            effort: "medium",
        })
        .await?;
    log_usage(
        &state.db,
        Usage {
            page: Some(page.id),
            provider: "claude",
            model: "claude",
            purpose: "extract",
            ..Default::default()
        }
        .tokens(read.input_tokens, read.output_tokens, read.cost_usd),
    )
    .await;
    let mut extraction: Extraction = serde_json::from_value(read.output)
        .context("Claude's reading did not match the expected shape")?;
    disambiguate_numbers(&mut extraction.questions);
    for q in &mut extraction.questions {
        for o in &mut q.options {
            if let Some(body) = &mut o.body {
                *body = tidy_option(body);
            }
        }
    }
    let carry_field = |k: &str| {
        extraction
            .carry
            .as_ref()
            .and_then(|c| c.get(k))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let carry_year = extraction
        .carry
        .as_ref()
        .and_then(|c| c.get("year"))
        .and_then(|v| v.as_i64())
        .map(|y| y as i32);
    let (carry_exam, carry_paper, carry_sitting) = (
        carry_field("exam"),
        carry_field("paper"),
        carry_field("sitting"),
    );
    if let Some(tail) = &mut extraction.tail_fragment {
        tail.topic_id = tail
            .topic_ref
            .as_deref()
            .and_then(|r| topic_refs.get(r))
            .map(|t| t.0);
        tail.exam = carry_exam;
        tail.paper = carry_paper;
        tail.sitting = carry_sitting;
        tail.year = tail.year.or(carry_year);
    }

    let drafts: Vec<Draft> = extraction
        .questions
        .iter()
        .map(|q| {
            let theory = q.kind == "theory";
            Draft {
                kind: if theory { "theory" } else { "objective" }.into(),
                number: q.number.clone(),
                key: resolve_key(
                    &src,
                    &exams,
                    Printed {
                        exam: q.exam.as_deref(),
                        year: q.year,
                        paper: q.paper.as_deref(),
                        sitting: q.sitting.as_deref(),
                        original_number: q.original_number.as_deref(),
                    },
                    &q.number,
                ),
                topic: q
                    .topic_ref
                    .as_deref()
                    .and_then(|r| topic_refs.get(r))
                    .map(|t| (*t).clone()),
                marks: q.marks,
                body: q.body.clone(),
                options: if theory {
                    Vec::new()
                } else {
                    q.options.clone()
                },
                parts: if theory { q.parts.clone() } else { Vec::new() },
                group_ref: q.group_ref.clone(),
                diagram_ref: q.diagram_ref.clone(),
                refers_to_number: q.refers_to_number,
            }
        })
        .collect();

    // 2. Is each one already in the bank?
    let page_number = extraction.page_number.as_deref();
    let mut plans: Vec<Plan> = Vec::with_capacity(drafts.len());
    for (i, d) in drafts.iter().enumerate() {
        let mut plan = Plan::Decided(decide_question(&state.db, &src, page_number, d).await?);
        let fresh = Plan::Decided(Decision::New {
            possible_duplicate: false,
        });
        if plan == fresh && d.key.matchable() {
            let earlier = (0..i).find(|&j| {
                matches!(plans[j], Plan::Decided(Decision::New { .. })) && same_key(&drafts[j], d)
            });
            if let Some(j) = earlier {
                if drafts[j].text().agrees_with(&d.text()) {
                    plan = Plan::SameAs(j);
                } else if d.kind != "theory" {
                    plan = Plan::Decided(Decision::New {
                        possible_duplicate: true,
                    });
                }
            }
        }
        plans.push(plan);
    }

    // 3. Both models answer the new objective questions.
    let diagram_text: HashMap<String, String> = extraction
        .diagrams
        .iter()
        .map(|d| (d.key.clone(), d.description.clone()))
        .collect();
    let groups: HashMap<&str, &prompts::ExtractedGroup> = extraction
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g))
        .collect();
    let to_answer: Vec<usize> = (0..drafts.len())
        .filter(|&i| {
            drafts[i].kind == "objective"
                && matches!(plans[i], Plan::Decided(ref d) if needs_answers(d))
        })
        .collect();
    let for_answer: Vec<QuestionForAnswer> = to_answer
        .iter()
        .map(|&i| {
            let d = &drafts[i];
            let group = d.group_ref.as_deref().and_then(|r| groups.get(r));
            d.for_answer(
                group.and_then(|g| g.instruction.as_deref()),
                &diagram_text,
                group.and_then(|g| g.diagram_ref.as_deref()),
            )
        })
        .collect();
    let mut sol: Vec<Option<Answer>> = vec![None; drafts.len()];
    let mut luna: Vec<Option<Answer>> = vec![None; drafts.len()];
    if !for_answer.is_empty() {
        let (s, l) = answer_both(state, Some(page.id), &for_answer).await?;
        for ((&i, s), l) in to_answer.iter().zip(s).zip(l) {
            sol[i] = s;
            luna[i] = l;
        }
    }
    let unanswered: Vec<&str> = to_answer
        .iter()
        .filter(|&&i| sol[i].is_none() || luna[i].is_none())
        .map(|&i| drafts[i].number.as_str())
        .collect();
    if !unanswered.is_empty() {
        tracing::warn!(page = %page.id, ?unanswered, "answers missing for some questions");
    }

    // 4. Save everything in one transaction.
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "update pages set page_number = $2, head_fragment = $3, tail_fragment = $4, carried_context = $5,
           status = 'done', processed_at = now(), error = null, next_attempt_at = null
         where id = $1",
    )
    .bind(page.id)
    .bind(&extraction.page_number)
    .bind(&extraction.head_fragment)
    .bind(extraction.tail_fragment.as_ref().map(|t| json!(t)))
    .bind(&extraction.carry)
    .execute(&mut *tx)
    .await?;

    let mut diagram_ids = HashMap::new();
    for d in &extraction.diagrams {
        let id: Uuid = sqlx::query_scalar(
            "insert into diagrams (page_id, description) values ($1, $2) returning id",
        )
        .bind(page.id)
        .bind(&d.description)
        .fetch_one(&mut *tx)
        .await?;
        diagram_ids.insert(d.key.clone(), id);
    }
    let mut group_ids = HashMap::new();
    for g in &extraction.groups {
        let id: Uuid = sqlx::query_scalar(
            "insert into question_groups (page_id, instruction, display_instruction, diagram_id)
             values ($1, $2, $3, $4) returning id",
        )
        .bind(page.id)
        .bind(&g.instruction)
        .bind(&g.display_instruction)
        .bind(g.diagram_ref.as_deref().and_then(|r| diagram_ids.get(r)))
        .fetch_one(&mut *tx)
        .await?;
        group_ids.insert(g.key.clone(), (id, g.diagram_ref.is_some()));
    }

    let refs = PageRefs {
        page_id: page.id,
        page_number,
        diagrams: &diagram_ids,
        groups: &group_ids,
    };
    let mut saved: Vec<Option<Uuid>> = vec![None; drafts.len()];
    for (i, d) in drafts.iter().enumerate() {
        saved[i] = match &plans[i] {
            Plan::Decided(decision) => {
                save_draft(
                    &mut tx,
                    &src,
                    &refs,
                    d,
                    decision,
                    sol[i].as_ref(),
                    luna[i].as_ref(),
                )
                .await?
            }
            Plan::SameAs(j) => {
                let Some(id) = saved[*j] else { continue };
                add_appearance(&mut tx, id, &src, &refs, &d.number).await?;
                Some(id)
            }
        };
    }
    // Questions that were already saved leave groups and diagrams nobody uses;
    // drop them so the diagrams aren't drawn and paid for.
    sqlx::query(
        "delete from question_groups g where g.page_id = $1
           and not exists (select 1 from questions q where q.group_id = g.id)",
    )
    .bind(page.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "delete from diagrams d where d.page_id = $1
           and not exists (select 1 from questions q where q.diagram_id = d.id)
           and not exists (select 1 from options o where o.diagram_id = d.id)
           and not exists (select 1 from question_groups g where g.diagram_id = d.id)
           and not exists (select 1 from question_parts p where p.diagram_id = d.id)",
    )
    .bind(page.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// gpt-6.1-sol and gpt-6-luna answer the same questions in parallel. Each result
/// is aligned with the questions passed in (index i = question i).
async fn answer_both(
    state: &AppState,
    page: Option<Uuid>,
    questions: &[QuestionForAnswer<'_>],
) -> anyhow::Result<(Vec<Option<Answer>>, Vec<Option<Answer>>)> {
    let input = prompts::answer_input(questions);
    let input = input.as_str();
    let count = questions.len();
    let openai = state.openai.as_ref().context("OPENAI_API_KEY is not set")?;
    let ask = |model: &'static str| async move {
        let res = openai
            .respond(tutor_openai::Request {
                model,
                instructions: prompts::ANSWER_INSTRUCTIONS,
                input: vec![Part::Text(input)],
                schema_name: "answers",
                schema: &prompts::ANSWER_SCHEMA,
            })
            .await
            .with_context(|| format!("{model} answering"))?;
        log_usage(
            &state.db,
            Usage {
                page,
                provider: "openai",
                model,
                purpose: "answer",
                ..Default::default()
            }
            .tokens(
                Some(res.input_tokens as i64),
                Some(res.output_tokens as i64),
                res.cost_usd,
            ),
        )
        .await;
        let answers: Answers = serde_json::from_value(res.output)?;
        let mut aligned: Vec<Option<Answer>> = vec![None; count];
        for a in answers.answers {
            if let Some(slot) = usize::try_from(a.item - 1)
                .ok()
                .and_then(|i| aligned.get_mut(i))
            {
                *slot = Some(a);
            }
        }
        anyhow::Ok(aligned)
    };
    let (sol, luna) = tokio::join!(ask(SOL), ask(LUNA));
    Ok((sol?, luna?))
}

/// Looks a question up by when it was set; see `matcher` for the rules.
async fn decide_question(
    db: &PgPool,
    src: &PageSource,
    page_number: Option<&str>,
    d: &Draft,
) -> anyhow::Result<Decision> {
    if let Some(page_number) = page_number {
        let seen: bool = sqlx::query_scalar(
            "select exists(select 1 from question_appearances
               where source_id = $1 and page_number = $2 and number = $3)",
        )
        .bind(src.source_id)
        .bind(page_number)
        .bind(&d.number)
        .fetch_one(db)
        .await?;
        if seen {
            return Ok(Decision::AlreadyOnPage);
        }
    }
    if !d.key.matchable() {
        return Ok(Decision::New {
            possible_duplicate: false,
        });
    }
    let rows: Vec<(Uuid, Option<String>, String)> = sqlx::query_as(
        "select id, sitting, body from questions
         where origin = 'source' and subject_id = $1 and exam_id = $2 and year = $3
           and original_number = $4 and kind = $5
           and ($6::text is null or sitting is null or sitting = $6)
           and ($7::text is null or paper is null or paper = $7)",
    )
    .bind(src.subject_id)
    .bind(d.key.exam_id)
    .bind(d.key.year)
    .bind(&d.key.original_number)
    .bind(&d.kind)
    .bind(&d.key.sitting)
    .bind(&d.key.paper)
    .fetch_all(db)
    .await?;
    let mut candidates = Vec::with_capacity(rows.len());
    for (id, sitting, body) in rows {
        let options: Vec<(String, String)> = sqlx::query_as(
            "select label, coalesce(body, '') from options where question_id = $1 order by label",
        )
        .bind(id)
        .fetch_all(db)
        .await?;
        let parts: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
            "select body, closing_text from question_parts where question_id = $1 order by position",
        )
        .bind(id)
        .fetch_all(db)
        .await?;
        candidates.push(Candidate {
            id,
            sitting,
            text: Text {
                body,
                options,
                parts: parts
                    .iter()
                    .map(|(b, c)| part_text(b.as_deref(), c.as_deref()))
                    .collect(),
            },
        });
    }
    Ok(matcher::decide(
        &d.key,
        d.kind == "theory",
        &d.text(),
        &candidates,
    ))
}

/// New objective questions are answered, and so are saved ones this copy adds options to.
fn needs_answers(d: &Decision) -> bool {
    matches!(
        d,
        Decision::New { .. }
            | Decision::Existing {
                conflict: false,
                adds_options: true,
                ..
            }
    )
}

/// A book left options out (A-D of an A-E question): add the ones this copy prints and
/// take the new answers, since the right one may be among them. A question a reviewer
/// has already settled keeps its answer and goes back for review instead.
async fn add_missing_options(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    r: &PageRefs<'_>,
    d: &Draft,
    sol: Option<&Answer>,
    luna: Option<&Answer>,
) -> anyhow::Result<()> {
    for o in &d.options {
        let diagram_id = o
            .diagram_ref
            .as_deref()
            .and_then(|x| r.diagrams.get(x))
            .copied();
        sqlx::query(
            "insert into options (question_id, label, body, diagram_id, is_correct) values ($1, $2, $3, $4, false)
             on conflict (question_id, label) do nothing",
        )
        .bind(id)
        .bind(&o.label)
        .bind(o.body.as_deref().filter(|b| !b.is_empty()).or(if diagram_id.is_none() { Some("") } else { None }))
        .bind(diagram_id)
        .execute(&mut **tx)
        .await?;
    }
    let resolved: bool =
        sqlx::query_scalar("select resolved_at is not null from questions where id = $1")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?;
    if resolved || sol.is_none() {
        flag(tx, id, "options_added").await?;
        return Ok(());
    }
    let labels: Vec<String> =
        sqlx::query_scalar("select label from options where question_id = $1 order by label")
            .bind(id)
            .fetch_all(&mut **tx)
            .await?;
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    // has_diagram and the other non-answer reasons are already on the question.
    let verdict = judge(&labels, sol, luna, false);
    sqlx::query(
        "update options set is_correct = coalesce(label = $2, false) where question_id = $1",
    )
    .bind(id)
    .bind(&verdict.correct)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "update questions set explanation = $2, confidence = $3, second_answer = $4, second_explanation = $5,
           check_reasons = array(select x from unnest(check_reasons) x
                                 where x not in ('no_answer', 'conflicting_answers', 'low_confidence')) || $6::text[]
         where id = $1",
    )
    .bind(id)
    .bind(sol.map(|a| a.explanation.as_str()))
    .bind(sol.map(|a| a.confidence.as_str()).filter(|c| ["high", "medium", "low"].contains(c)))
    .bind(luna.map(|a| a.answer.as_str()))
    .bind(luna.map(|a| a.explanation.as_str()))
    .bind(&verdict.reasons)
    .execute(&mut **tx)
    .await?;
    sqlx::query("update questions set needs_check = cardinality(check_reasons) > 0 where id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Saves a question as decided. Returns the question it now lives in, if any.
async fn save_draft(
    tx: &mut Transaction<'_, Postgres>,
    src: &PageSource,
    r: &PageRefs<'_>,
    d: &Draft,
    decision: &Decision,
    sol: Option<&Answer>,
    luna: Option<&Answer>,
) -> anyhow::Result<Option<Uuid>> {
    match decision {
        Decision::AlreadyOnPage => {
            tracing::info!(
                number = d.number,
                page_number = r.page_number,
                "page read before; question skipped"
            );
            Ok(None)
        }
        Decision::Existing {
            id,
            conflict,
            adds_options,
        } => {
            add_appearance(tx, *id, src, r, &d.number).await?;
            if *adds_options {
                add_missing_options(tx, *id, r, d, sol, luna).await?;
            }
            if *conflict {
                // Keep the saved copy as it is; a reviewer compares the two.
                flag(tx, *id, "source_conflict").await?;
            } else {
                // Fill only what the saved copy lacks.
                sqlx::query(
                    "update questions set sitting = coalesce(sitting, $2), paper = coalesce(paper, $3),
                       topic_id = coalesce(topic_id, $4), diagram_id = coalesce(diagram_id, $5),
                       group_id = coalesce(group_id, $6), marks = coalesce(marks, $7::numeric)
                     where id = $1",
                )
                .bind(id)
                .bind(&d.key.sitting)
                .bind(&d.key.paper)
                .bind(d.topic.as_ref().map(|t| t.0))
                .bind(d.diagram_id(r))
                .bind(d.group(r).map(|g| g.0))
                .bind(d.marks)
                .execute(&mut **tx)
                .await?;
            }
            tracing::info!(number = d.number, %id, conflict, "question already in the bank; added this appearance");
            Ok(Some(*id))
        }
        Decision::New { possible_duplicate } => Ok(Some(
            insert_new(tx, src, r, d, *possible_duplicate, sol, luna).await?,
        )),
    }
}

async fn add_appearance(
    tx: &mut Transaction<'_, Postgres>,
    question_id: Uuid,
    src: &PageSource,
    r: &PageRefs<'_>,
    number: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "insert into question_appearances (question_id, source_id, page_id, page_number, number)
         values ($1, $2, $3, $4, $5) on conflict do nothing",
    )
    .bind(question_id)
    .bind(src.source_id)
    .bind(r.page_id)
    .bind(r.page_number)
    .bind(number)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Puts a question (back) in the review queue for `reason`.
async fn flag(tx: &mut Transaction<'_, Postgres>, id: Uuid, reason: &str) -> sqlx::Result<()> {
    sqlx::query(
        "update questions set needs_check = true, resolved_at = null,
           check_reasons = case when $2::text = any(check_reasons) then check_reasons
                                else array_append(check_reasons, $2::text) end
         where id = $1",
    )
    .bind(id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Inserts a new question with its options or parts, flags and first appearance.
async fn insert_new(
    tx: &mut Transaction<'_, Postgres>,
    src: &PageSource,
    r: &PageRefs<'_>,
    d: &Draft,
    possible_duplicate: bool,
    sol: Option<&Answer>,
    luna: Option<&Answer>,
) -> anyhow::Result<Uuid> {
    let objective = d.kind == "objective";
    let has_diagram = d.has_diagram(r);
    let labels: Vec<&str> = d.options.iter().map(|o| o.label.as_str()).collect();
    let mut verdict = if objective {
        judge(&labels, sol, luna, has_diagram)
    } else {
        // Theory isn't answered yet; only what a reviewer must look at.
        Verdict {
            correct: None,
            reasons: if has_diagram {
                vec!["has_diagram"]
            } else {
                Vec::new()
            },
        }
    };
    if d.refers_to_number {
        verdict.reasons.push("refers_to_number");
    }
    if possible_duplicate {
        verdict.reasons.push("possible_duplicate");
    }

    let id: Uuid = sqlx::query_scalar(
        "insert into questions (kind, origin, subject_id, source_id, page_id, page_number, group_id, diagram_id,
           number, body, explanation, confidence, second_answer, second_explanation, needs_check, check_reasons,
           exam_id, year, paper, sitting, original_number, topic_id, marks)
         values ($1, 'source', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
           $16, $17, $18, $19, $20, $21, $22::numeric)
         returning id",
    )
    .bind(&d.kind)
    .bind(src.subject_id)
    .bind(src.source_id)
    .bind(r.page_id)
    .bind(r.page_number)
    .bind(d.group(r).map(|g| g.0))
    .bind(d.diagram_id(r))
    .bind(&d.number)
    .bind(&d.body)
    .bind(sol.map(|a| a.explanation.as_str()))
    .bind(sol.map(|a| a.confidence.as_str()).filter(|c| ["high", "medium", "low"].contains(c)))
    .bind(luna.map(|a| a.answer.as_str()))
    .bind(luna.map(|a| a.explanation.as_str()))
    .bind(!verdict.reasons.is_empty())
    .bind(&verdict.reasons)
    .bind(d.key.exam_id)
    .bind(d.key.year)
    .bind(&d.key.paper)
    .bind(&d.key.sitting)
    .bind(&d.key.original_number)
    .bind(d.topic.as_ref().map(|t| t.0))
    .bind(d.marks)
    .fetch_one(&mut **tx)
    .await?;
    add_appearance(tx, id, src, r, &d.number).await?;

    for o in &d.options {
        let diagram_id = o
            .diagram_ref
            .as_deref()
            .and_then(|x| r.diagrams.get(x))
            .copied();
        sqlx::query(
            "insert into options (question_id, label, body, diagram_id, is_correct) values ($1, $2, $3, $4, $5)
             on conflict (question_id, label) do nothing",
        )
        .bind(id)
        .bind(&o.label)
        .bind(o.body.as_deref().filter(|b| !b.is_empty()).or(if diagram_id.is_none() { Some("") } else { None }))
        .bind(diagram_id)
        .bind(verdict.correct.as_deref() == Some(o.label.as_str()))
        .execute(&mut **tx)
        .await?;
    }
    insert_parts(tx, id, &d.parts, r.diagrams).await?;
    Ok(id)
}

/// "(a)/(ii)", "a/ii", "A / II." → "a/ii".
fn norm_path(path: &str) -> String {
    path.split('/')
        .map(|s| s.trim().trim_matches(['(', ')', '.', ' ']).to_lowercase())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Saves a theory question's parts as a tree; `position` is the printed order.
async fn insert_parts(
    tx: &mut Transaction<'_, Postgres>,
    question_id: Uuid,
    parts: &[prompts::ExtractedPart],
    diagrams: &HashMap<String, Uuid>,
) -> sqlx::Result<()> {
    let mut ids: HashMap<String, Uuid> = HashMap::new();
    for (i, p) in parts.iter().enumerate() {
        let mut path = norm_path(&p.path);
        if path.is_empty() {
            path = (i + 1).to_string();
        }
        let (parent_path, label) = match path.rsplit_once('/') {
            Some((parent, label)) => (Some(parent), label),
            None => (None, path.as_str()),
        };
        // If the reader skipped a parent, hang the part on the nearest one it gave.
        let parent = parent_path.and_then(|mut pp| {
            loop {
                if let Some(id) = ids.get(pp) {
                    break Some(*id);
                }
                match pp.rsplit_once('/') {
                    Some((up, _)) => pp = up,
                    None => break None,
                }
            }
        });
        let id: Uuid = sqlx::query_scalar(
            "insert into question_parts (question_id, parent_id, label, position, body, closing_text, marks, diagram_id)
             values ($1, $2, $3, $4, $5, $6, $7::numeric, $8) returning id",
        )
        .bind(question_id)
        .bind(parent)
        .bind(label)
        .bind(i as i32 + 1)
        .bind(&p.body)
        .bind(&p.closing_text)
        .bind(p.marks)
        .bind(p.diagram_ref.as_deref().and_then(|d| diagrams.get(d)))
        .fetch_one(&mut **tx)
        .await?;
        ids.insert(path.clone(), id);
    }
    Ok(())
}

pub struct Verdict {
    /// Label of the option to mark correct (gpt-6.1-sol's pick), if it matches one.
    pub correct: Option<String>,
    pub reasons: Vec<&'static str>,
}

/// Decides the saved answer and the review flags from both models' answers.
pub fn judge(
    labels: &[&str],
    sol: Option<&Answer>,
    luna: Option<&Answer>,
    has_diagram: bool,
) -> Verdict {
    let pick = |a: Option<&Answer>| {
        a.and_then(|a| option_letter(&a.answer)).and_then(|l| {
            labels
                .iter()
                .find(|x| x.eq_ignore_ascii_case(&l))
                .map(|x| x.to_string())
        })
    };
    let (sol_pick, luna_pick) = (pick(sol), pick(luna));
    let mut reasons = Vec::new();
    if sol_pick.is_none() {
        reasons.push("no_answer");
    } else if sol_pick != luna_pick {
        reasons.push("conflicting_answers");
    }
    if has_diagram {
        reasons.push("has_diagram");
    }
    if sol.is_some_and(|a| a.confidence == "low") {
        reasons.push("low_confidence");
    }
    Verdict {
        correct: sol_pick,
        reasons,
    }
}

/// Books sometimes print the same number twice on a page (a misprint). The second
/// becomes "4 (2nd)" so it is neither dropped as a duplicate nor confused with the
/// first when matching answers.
fn disambiguate_numbers(questions: &mut [prompts::ExtractedQuestion]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for q in questions {
        let base = norm_number(&q.number);
        let count = seen.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count > 1 {
            let suffix = match *count {
                2 => "2nd",
                3 => "3rd",
                _ => "th",
            };
            q.number = if *count > 3 {
                format!("{base} ({count}{suffix})")
            } else {
                format!("{base} ({suffix})")
            };
        }
    }
}

/// Inline options are printed "A. fish. B. reptile."; the full stop only separates
/// them. Drop a single trailing one, but keep "etc.", "2.5", "i.e. x." and the like.
fn tidy_option(body: &str) -> String {
    let t = body.trim();
    match t.strip_suffix('.') {
        Some(rest)
            if !rest.is_empty()
                && !rest.ends_with('.')
                && !rest.contains('.')
                && !rest.ends_with("etc") =>
        {
            rest.trim_end().to_string()
        }
        _ => t.to_string(),
    }
}

/// "C", "C.", "(c)", "C. Ginger" → "C".
fn option_letter(answer: &str) -> Option<String> {
    let s = answer.trim().trim_start_matches(['(', '[', ' ']);
    let mut chars = s.chars();
    let first = chars.next()?;
    let next = chars.next();
    (first.is_ascii_alphabetic() && next.is_none_or(|c| !c.is_ascii_alphabetic()))
        .then(|| first.to_ascii_uppercase().to_string())
}

/// "Q31", "31.", "Question 31", "No. 31" → "31"; "Question 4 (2nd)" → "4 (2nd)".
fn norm_number(n: &str) -> String {
    let mut s = n.trim();
    for prefix in ["question", "no.", "no", "q.", "q"] {
        if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
            s = s[prefix.len()..].trim_start();
            break;
        }
    }
    s.trim_end_matches(['.', ':']).trim().to_lowercase()
}

// ── Questions split across pages ────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct SplitPair {
    first_id: Uuid,
    first_number: String,
    tail: sqlx::types::Json<TailFragment>,
    head: String,
}

/// Joins a question cut off at the bottom of page N with the continuation at the
/// top of page N+1 (same source), whichever of the two was processed last.
/// The joined question is saved under page N, where it starts.
async fn join_split_questions(state: &AppState, page_id: Uuid) -> anyhow::Result<()> {
    let pairs: Vec<SplitPair> = sqlx::query_as(
        "select p.id as first_id, p.page_number as first_number,
                p.tail_fragment as tail, n.head_fragment as head
         from pages p
         join pages n on n.source_id = p.source_id and n.status = 'done' and n.head_fragment is not null
           and p.page_number ~ '^[0-9]+$' and n.page_number ~ '^[0-9]+$'
           and n.page_number::int = p.page_number::int + 1
         where p.status = 'done' and p.tail_fragment is not null and not p.tail_joined
           and (p.id = $1 or n.id = $1)",
    )
    .bind(page_id)
    .fetch_all(&state.db)
    .await?;
    if pairs.is_empty() {
        return Ok(());
    }

    let openai = state.openai.as_ref().context("OPENAI_API_KEY is not set")?;
    let exams = load_exams(&state.db).await?;
    for pair in pairs {
        let tail = &pair.tail.0;
        let input = format!(
            "Kind: {}\n\nBottom of the first page:\n{}\n\nTop of the next page:\n{}",
            tail.kind, tail.text, pair.head
        );
        let res = openai
            .respond(tutor_openai::Request {
                model: SOL,
                instructions: prompts::JOIN_INSTRUCTIONS,
                input: vec![Part::Text(&input)],
                schema_name: "question",
                schema: &prompts::JOIN_SCHEMA,
            })
            .await?;
        log_usage(
            &state.db,
            Usage {
                page: Some(pair.first_id),
                provider: "openai",
                model: SOL,
                purpose: "join",
                ..Default::default()
            }
            .tokens(
                Some(res.input_tokens as i64),
                Some(res.output_tokens as i64),
                res.cost_usd,
            ),
        )
        .await;
        let joined: prompts::Joined = serde_json::from_value(res.output)?;

        let src = page_source(&state.db, pair.first_id).await?;
        let topic = match tail.topic_id {
            Some(id) => {
                sqlx::query_scalar::<_, String>("select name from syllabus_topics where id = $1")
                    .bind(id)
                    .fetch_optional(&state.db)
                    .await?
                    .map(|name| (id, name))
            }
            None => None,
        };
        let theory = tail.kind == "theory";
        let draft = Draft {
            kind: if theory { "theory" } else { "objective" }.into(),
            number: tail.number.clone(),
            key: resolve_key(
                &src,
                &exams,
                Printed {
                    exam: tail.exam.as_deref(),
                    year: tail.year.or(joined.year),
                    paper: tail.paper.as_deref(),
                    sitting: tail.sitting.as_deref(),
                    original_number: tail
                        .original_number
                        .as_deref()
                        .or(joined.original_number.as_deref()),
                },
                &tail.number,
            ),
            topic,
            marks: joined.marks,
            body: joined.body.clone(),
            options: if theory {
                Vec::new()
            } else {
                joined
                    .options
                    .iter()
                    .map(|o| prompts::ExtractedOption {
                        label: o.label.clone(),
                        body: Some(o.body.clone()),
                        diagram_ref: None,
                    })
                    .collect()
            },
            parts: if theory {
                joined.parts.clone()
            } else {
                Vec::new()
            },
            group_ref: None,
            diagram_ref: None,
            refers_to_number: false,
        };
        let decision = decide_question(&state.db, &src, Some(&pair.first_number), &draft).await?;
        let (sol, luna) = if !theory && needs_answers(&decision) {
            let no_diagrams = HashMap::new();
            let (mut s, mut l) = answer_both(
                state,
                Some(pair.first_id),
                &[draft.for_answer(None, &no_diagrams, None)],
            )
            .await?;
            (s.remove(0), l.remove(0))
        } else {
            (None, None)
        };

        let (no_diagrams, no_groups) = (HashMap::new(), HashMap::new());
        let refs = PageRefs {
            page_id: pair.first_id,
            page_number: Some(&pair.first_number),
            diagrams: &no_diagrams,
            groups: &no_groups,
        };
        let mut tx = state.db.begin().await?;
        save_draft(
            &mut tx,
            &src,
            &refs,
            &draft,
            &decision,
            sol.as_ref(),
            luna.as_ref(),
        )
        .await?;
        sqlx::query("update pages set tail_joined = true where id = $1")
            .bind(pair.first_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(page = %pair.first_id, number = tail.number, "joined a question split across pages");
    }
    Ok(())
}

// ── Diagrams ────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ClaimedDiagram {
    id: Uuid,
    page_id: Option<Uuid>,
    description: String,
    redraw_note: Option<String>,
    /// The page photo it was read from; none for our own questions' diagrams.
    photo_key: Option<String>,
    instruction: Option<String>,
}

async fn diagram_loop(state: AppState) {
    if let Err(e) = sqlx::query("update diagrams set status = 'queued' where status = 'drawing'")
        .execute(&state.db)
        .await
    {
        tracing::error!("resetting diagrams: {e}");
    }
    loop {
        let claimed: sqlx::Result<Option<ClaimedDiagram>> = sqlx::query_as(
            "with d as (
               update diagrams set status = 'drawing', error = null
               where id = (select id from diagrams where status = 'queued' order by created_at limit 1 for update skip locked)
               returning id, page_id, description, redraw_note)
             select d.id, d.page_id, d.description, d.redraw_note, p.photo_key,
                    (select instruction from question_groups g where g.diagram_id = d.id limit 1) as instruction
             from d left join pages p on p.id = d.page_id",
        )
        .fetch_optional(&state.db)
        .await;
        match claimed {
            Ok(Some(d)) => {
                let id = d.id;
                if let Err(e) = draw(&state, d).await {
                    tracing::error!(diagram = %id, "drawing: {e:#}");
                    let _ = sqlx::query(
                        "update diagrams set status = 'failed', error = $2 where id = $1",
                    )
                    .bind(id)
                    .bind(format!("{e:#}"))
                    .execute(&state.db)
                    .await;
                }
            }
            Ok(None) => tokio::time::sleep(IDLE).await,
            Err(e) => {
                tracing::error!("claiming diagram: {e}");
                tokio::time::sleep(IDLE * 2).await;
            }
        }
    }
}

async fn draw(state: &AppState, d: ClaimedDiagram) -> anyhow::Result<()> {
    let openai = state.openai.as_ref().context("OPENAI_API_KEY is not set")?;
    let photo = match &d.photo_key {
        Some(key) => Some(B64.encode(state.storage.get(key).await?)),
        None => None,
    };
    let prompt = prompts::draw_prompt(
        &d.description,
        d.instruction.as_deref(),
        d.redraw_note.as_deref(),
    );
    let res = openai
        .respond(tutor_openai::Request {
            model: SOL,
            instructions: prompts::DRAW_INSTRUCTIONS,
            input: photo
                .as_deref()
                .map(Part::Jpeg)
                .into_iter()
                .chain([Part::Text(&prompt)])
                .collect(),
            schema_name: "diagram",
            schema: &prompts::DRAW_SCHEMA,
        })
        .await?;
    log_usage(
        &state.db,
        Usage {
            page: d.page_id,
            diagram: Some(d.id),
            provider: "openai",
            model: SOL,
            purpose: "draw",
            ..Default::default()
        }
        .tokens(
            Some(res.input_tokens as i64),
            Some(res.output_tokens as i64),
            res.cost_usd,
        ),
    )
    .await;
    let svg = res.output["svg"]
        .as_str()
        .context("no svg in the response")?;
    let svg = sanitize_svg(svg)?;
    sqlx::query(
        "update diagrams set svg = $2, status = 'done', error = null, reviewed_at = null where id = $1",
    )
    .bind(d.id)
    .bind(svg)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Accepts only a self-contained SVG: no scripts, event handlers, links or embedded content.
/// (Diagrams are also served with a restrictive CSP and shown through <img>.)
pub fn sanitize_svg(svg: &str) -> anyhow::Result<String> {
    let mut s = svg.trim();
    if s.starts_with("<?xml") {
        s = s[s.find("?>").map_or(0, |i| i + 2)..].trim_start();
    }
    anyhow::ensure!(
        s.starts_with("<svg") && s.ends_with("</svg>"),
        "not a standalone <svg> element"
    );
    let lower = s.to_ascii_lowercase();
    for banned in [
        "<script",
        "javascript:",
        "<foreignobject",
        "<image",
        "href=",
        "<iframe",
        "<use",
        "url(http",
        "@import",
    ] {
        anyhow::ensure!(
            !lower.contains(banned),
            "SVG contains disallowed `{banned}`"
        );
    }
    let bytes = lower.as_bytes();
    for (i, w) in lower.match_indices("on") {
        let _ = w;
        let before_ok = i > 0 && bytes[i - 1].is_ascii_whitespace();
        let rest = &lower[i + 2..];
        let name_len = rest.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
        let after = rest[name_len..].trim_start();
        anyhow::ensure!(
            !(before_ok && name_len > 0 && after.starts_with('=')),
            "SVG contains an event handler"
        );
    }
    Ok(s.to_string())
}

// ── Usage log ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct Usage {
    page: Option<Uuid>,
    diagram: Option<Uuid>,
    provider: &'static str,
    model: &'static str,
    purpose: &'static str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cost_usd: Option<f64>,
}

impl Usage {
    fn tokens(
        self,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cost_usd: Option<f64>,
    ) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cost_usd,
            ..self
        }
    }
}

/// Best effort: a failed log line never fails the page.
async fn log_usage(db: &PgPool, u: Usage) {
    let res = sqlx::query(
        "insert into ai_usage (page_id, diagram_id, provider, model, purpose, input_tokens, output_tokens, cost_usd)
         values ($1, $2, $3, $4, $5, $6, $7, $8::float8::numeric)",
    )
    .bind(u.page)
    .bind(u.diagram)
    .bind(u.provider)
    .bind(u.model)
    .bind(u.purpose)
    .bind(u.input_tokens.map(|t| t as i32))
    .bind(u.output_tokens.map(|t| t as i32))
    .bind(u.cost_usd)
    .execute(db)
    .await;
    if let Err(e) = res {
        tracing::warn!("logging AI usage: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ans(answer: &str, confidence: &str) -> Answer {
        Answer {
            item: 1,
            answer: answer.into(),
            explanation: String::new(),
            confidence: confidence.into(),
        }
    }

    #[test]
    fn agreeing_models_are_not_flagged() {
        let v = judge(
            &["A", "B", "C", "D"],
            Some(&ans("C", "high")),
            Some(&ans("C. Ginger", "medium")),
            false,
        );
        assert_eq!(v.correct.as_deref(), Some("C"));
        assert!(v.reasons.is_empty());
    }

    #[test]
    fn conflicting_answers_are_flagged() {
        let v = judge(
            &["A", "B", "C", "D"],
            Some(&ans("D", "high")),
            Some(&ans("C", "high")),
            false,
        );
        assert_eq!(v.correct.as_deref(), Some("D"));
        assert_eq!(v.reasons, vec!["conflicting_answers"]);
    }

    #[test]
    fn no_answer_diagram_and_low_confidence() {
        let v = judge(&["A", "B"], Some(&ans("E", "low")), None, true);
        assert_eq!(v.correct, None);
        assert_eq!(
            v.reasons,
            vec!["no_answer", "has_diagram", "low_confidence"]
        );
    }

    #[test]
    fn repeated_numbers_get_a_suffix() {
        let q = |n: &str| -> prompts::ExtractedQuestion {
            serde_json::from_value(json!({ "number": n, "body": "", "options": [] })).unwrap()
        };
        let mut qs = vec![q("4"), q("5"), q("4."), q("4")];
        disambiguate_numbers(&mut qs);
        let numbers: Vec<_> = qs.iter().map(|q| q.number.as_str()).collect();
        assert_eq!(numbers, ["4", "5", "4 (2nd)", "4 (3rd)"]);
    }

    fn source(layout: &str, exam_id: Option<Uuid>) -> PageSource {
        PageSource {
            source_id: Uuid::nil(),
            title: "Prep50 Biology".into(),
            kind: "book".into(),
            layout: layout.into(),
            subject_id: Uuid::nil(),
            subject: "Biology".into(),
            level: "sss".into(),
            exam_id,
            year: None,
            paper: None,
            sitting: None,
            position: 1,
        }
    }

    fn printed<'a>(
        exam: Option<&'a str>,
        year: Option<i32>,
        number: Option<&'a str>,
    ) -> Printed<'a> {
        Printed {
            exam,
            year,
            paper: None,
            sitting: None,
            original_number: number,
        }
    }

    #[test]
    fn keys_from_tags_and_source_defaults() {
        let waec = Uuid::from_u128(1);
        let neco = Uuid::from_u128(2);
        let exams: Vec<ExamRow> = vec![
            (waec, "WAEC".into(), "WASSCE".into(), "sss".into()),
            (neco, "NECO".into(), "SSCE".into(), "sss".into()),
        ];
        // Prep50: the tag gives year and number, the source says WAEC.
        let k = resolve_key(
            &source("by_topic", Some(waec)),
            &exams,
            printed(None, Some(2012), Some("1")),
            "1",
        );
        assert_eq!(
            (k.exam_id, k.year, k.original_number.as_deref()),
            (Some(waec), Some(2012), Some("1"))
        );
        assert!(k.matchable());
        // No tag in a topic book: the book's own numbering means nothing.
        let k = resolve_key(
            &source("by_topic", Some(waec)),
            &exams,
            printed(None, None, None),
            "14",
        );
        assert_eq!(k.original_number, None);
        assert!(!k.matchable());
        // A printed exam beats the source's.
        let k = resolve_key(
            &source("by_year", Some(waec)),
            &exams,
            printed(Some("NECO SSCE"), Some(2019), None),
            "7.",
        );
        assert_eq!(
            (k.exam_id, k.original_number.as_deref()),
            (Some(neco), Some("7"))
        );
    }

    #[test]
    fn part_paths() {
        assert_eq!(norm_path("(a)/(ii)"), "a/ii");
        assert_eq!(norm_path("A / II."), "a/ii");
        assert_eq!(norm_path("d"), "d");
    }

    #[test]
    fn question_numbers_normalise() {
        assert_eq!(norm_number("Question 2"), "2");
        assert_eq!(norm_number("Q31."), "31");
        assert_eq!(norm_number("Question 4 (2nd)"), norm_number("4 (2nd)"));
        assert_eq!(norm_number(" 12 "), "12");
    }

    #[test]
    fn option_full_stops() {
        assert_eq!(tidy_option("stolon."), "stolon");
        assert_eq!(tidy_option("10.0 N"), "10.0 N");
        assert_eq!(tidy_option("dogs, cats, etc."), "dogs, cats, etc.");
        assert_eq!(
            tidy_option("It is an alga. It has a large vacuole."),
            "It is an alga. It has a large vacuole."
        );
    }

    #[test]
    fn option_letters() {
        assert_eq!(option_letter("(c)").as_deref(), Some("C"));
        assert_eq!(option_letter("B. stolon").as_deref(), Some("B"));
        assert_eq!(option_letter("Blood"), None);
    }

    #[test]
    fn svg_sanitizer() {
        assert!(sanitize_svg("<svg viewBox=\"0 0 1 1\"><rect/></svg>").is_ok());
        assert!(sanitize_svg("<svg><script>alert(1)</script></svg>").is_err());
        assert!(sanitize_svg("<svg><rect onload=\"x()\"/></svg>").is_err());
        assert!(sanitize_svg("<svg><text>Moon phases</text></svg>").is_ok());
        assert!(sanitize_svg("<div></div>").is_err());
    }
}
