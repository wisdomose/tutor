//! Background processing: uploaded pages → Claude reads them → gpt-6.1-sol and
//! gpt-6-luna answer → questions saved and flagged → diagrams drawn by gpt-6.1-sol.
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
    subject_id: Uuid,
    photo_key: String,
    attempts: i32,
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
           order by uploaded_at
           limit 1
           for update skip locked)
         returning id, subject_id, photo_key, attempts",
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
    let photo = state
        .storage
        .get(&page.photo_key)
        .await
        .context("loading photo")?;
    let photo_b64 = B64.encode(&photo);

    // 1. Claude reads the page.
    let read = state
        .claude
        .run(tutor_claude::Request {
            system: prompts::EXTRACT_SYSTEM,
            prompt: prompts::EXTRACT_PROMPT,
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

    // 2. Both models answer every question.
    let diagram_text: HashMap<&str, &str> = extraction
        .diagrams
        .iter()
        .map(|d| (d.key.as_str(), d.description.as_str()))
        .collect();
    let groups: HashMap<&str, &prompts::ExtractedGroup> = extraction
        .groups
        .iter()
        .map(|g| (g.key.as_str(), g))
        .collect();
    let for_answer: Vec<QuestionForAnswer> = extraction
        .questions
        .iter()
        .map(|q| {
            let group = q.group_ref.as_deref().and_then(|r| groups.get(r));
            let diagram = q
                .diagram_ref
                .as_deref()
                .or(group.and_then(|g| g.diagram_ref.as_deref()))
                .and_then(|r| diagram_text.get(r).copied());
            QuestionForAnswer {
                number: &q.number,
                topic: q.topic.as_deref(),
                instruction: group.and_then(|g| g.instruction.as_deref()),
                body: &q.body,
                diagram,
                options: q
                    .options
                    .iter()
                    .map(|o| {
                        let text = match (
                            &o.body,
                            o.diagram_ref.as_deref().and_then(|r| diagram_text.get(r)),
                        ) {
                            (Some(b), _) => b.clone(),
                            (None, Some(d)) => format!("(picture: {d})"),
                            (None, None) => String::new(),
                        };
                        (o.label.as_str(), text)
                    })
                    .collect(),
            }
        })
        .collect();
    let (sol, luna) = if for_answer.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        answer_both(state, Some(page.id), &for_answer).await?
    };

    let unmatched: Vec<&str> = extraction
        .questions
        .iter()
        .enumerate()
        .filter(|(i, _)| sol[*i].is_none() || luna[*i].is_none())
        .map(|(_, q)| q.number.as_str())
        .collect();
    if !unmatched.is_empty() {
        tracing::warn!(page = %page.id, ?unmatched, "answers missing for some questions");
    }

    // 3. Save everything in one transaction.
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "update pages set page_number = $2, head_fragment = $3, tail_fragment = $4,
           status = 'done', processed_at = now(), error = null, next_attempt_at = null
         where id = $1",
    )
    .bind(page.id)
    .bind(&extraction.page_number)
    .bind(&extraction.head_fragment)
    .bind(extraction.tail_fragment.as_ref().map(|t| json!(t)))
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
        diagram_ids.insert(d.key.as_str(), id);
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
        group_ids.insert(g.key.as_str(), (id, g.diagram_ref.is_some()));
    }

    for (i, q) in extraction.questions.iter().enumerate() {
        let group = q.group_ref.as_deref().and_then(|r| group_ids.get(r));
        let has_diagram = q.diagram_ref.is_some()
            || group.is_some_and(|(_, has)| *has)
            || q.options.iter().any(|o| o.diagram_ref.is_some());
        let options: Vec<NewOption> = q
            .options
            .iter()
            .map(|o| NewOption {
                label: o.label.clone(),
                body: o.body.clone(),
                diagram_id: o
                    .diagram_ref
                    .as_deref()
                    .and_then(|r| diagram_ids.get(r))
                    .copied(),
            })
            .collect();
        insert_question(
            &mut tx,
            NewQuestion {
                subject_id: page.subject_id,
                page_id: page.id,
                page_number: extraction.page_number.as_deref(),
                group_id: group.map(|(id, _)| *id),
                diagram_id: q
                    .diagram_ref
                    .as_deref()
                    .and_then(|r| diagram_ids.get(r))
                    .copied(),
                number: &q.number,
                topic: q.topic.as_deref(),
                body: &q.body,
                options,
                has_diagram,
                refers_to_number: q.refers_to_number,
                sol: sol[i].as_ref(),
                luna: luna[i].as_ref(),
            },
        )
        .await?;
    }
    // Questions skipped as duplicates (a re-uploaded page) leave groups and diagrams
    // nobody uses; drop them so the diagrams aren't drawn and paid for.
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
           and not exists (select 1 from question_groups g where g.diagram_id = d.id)",
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

struct NewOption {
    label: String,
    body: Option<String>,
    diagram_id: Option<Uuid>,
}

struct NewQuestion<'a> {
    subject_id: Uuid,
    page_id: Uuid,
    page_number: Option<&'a str>,
    group_id: Option<Uuid>,
    diagram_id: Option<Uuid>,
    number: &'a str,
    topic: Option<&'a str>,
    body: &'a str,
    options: Vec<NewOption>,
    has_diagram: bool,
    refers_to_number: bool,
    sol: Option<&'a Answer>,
    luna: Option<&'a Answer>,
}

/// Inserts a question with its options and flags. Returns false when the same
/// subject + page + number already exists (a re-uploaded page).
async fn insert_question(
    tx: &mut Transaction<'_, Postgres>,
    q: NewQuestion<'_>,
) -> anyhow::Result<bool> {
    let labels: Vec<&str> = q.options.iter().map(|o| o.label.as_str()).collect();
    let mut verdict = judge(&labels, q.sol, q.luna, q.has_diagram);
    if q.refers_to_number {
        verdict.reasons.push("refers_to_number");
    }

    let id: Option<Uuid> = sqlx::query_scalar(
        "insert into questions (subject_id, page_id, page_number, group_id, diagram_id, number, topic, body,
           explanation, confidence, second_answer, second_explanation, needs_check, check_reasons)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
         on conflict (subject_id, page_number, number) where page_number is not null do nothing
         returning id",
    )
    .bind(q.subject_id)
    .bind(q.page_id)
    .bind(q.page_number)
    .bind(q.group_id)
    .bind(q.diagram_id)
    .bind(q.number)
    .bind(q.topic)
    .bind(q.body)
    .bind(q.sol.map(|a| a.explanation.as_str()))
    .bind(q.sol.map(|a| a.confidence.as_str()).filter(|c| ["high", "medium", "low"].contains(c)))
    .bind(q.luna.map(|a| a.answer.as_str()))
    .bind(q.luna.map(|a| a.explanation.as_str()))
    .bind(!verdict.reasons.is_empty())
    .bind(&verdict.reasons)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(id) = id else {
        tracing::info!(number = q.number, page_number = ?q.page_number, "question already saved; skipped");
        return Ok(false);
    };

    for o in &q.options {
        sqlx::query(
            "insert into options (question_id, label, body, diagram_id, is_correct) values ($1, $2, $3, $4, $5)
             on conflict (question_id, label) do nothing",
        )
        .bind(id)
        .bind(&o.label)
        .bind(o.body.as_deref().filter(|b| !b.is_empty()).or(if o.diagram_id.is_none() { Some("") } else { None }))
        .bind(o.diagram_id)
        .bind(verdict.correct.as_deref() == Some(o.label.as_str()))
        .execute(&mut **tx)
        .await?;
    }
    Ok(true)
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
    subject_id: Uuid,
    first_number: String,
    tail: sqlx::types::Json<TailFragment>,
    head: String,
}

/// Joins a question cut off at the bottom of page N with the continuation at the
/// top of page N+1 (same subject), whichever of the two was processed last.
/// The joined question is saved under page N, where it starts.
async fn join_split_questions(state: &AppState, page_id: Uuid) -> anyhow::Result<()> {
    let pairs: Vec<SplitPair> = sqlx::query_as(
        "select p.id as first_id, p.subject_id, p.page_number as first_number,
                p.tail_fragment as tail, n.head_fragment as head
         from pages p
         join pages n on n.subject_id = p.subject_id and n.status = 'done' and n.head_fragment is not null
           and p.page_number ~ '^[0-9]+$' and n.page_number ~ '^[0-9]+$'
           and n.page_number::int = p.page_number::int + 1
         where p.status = 'done' and p.tail_fragment is not null and not p.tail_joined
           and (p.id = $1 or n.id = $1)",
    )
    .bind(page_id)
    .fetch_all(&state.db)
    .await?;

    let openai = state.openai.as_ref().context("OPENAI_API_KEY is not set")?;
    for pair in pairs {
        let tail = &pair.tail.0;
        let input = format!(
            "Bottom of the first page:\n{}\n\nTop of the next page:\n{}",
            tail.text, pair.head
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
        let question = QuestionForAnswer {
            number: &tail.number,
            topic: tail.topic.as_deref(),
            instruction: None,
            body: &joined.body,
            diagram: None,
            options: joined
                .options
                .iter()
                .map(|o| (o.label.as_str(), o.body.clone()))
                .collect(),
        };
        let (sol, luna) = answer_both(state, Some(pair.first_id), &[question]).await?;

        let mut tx = state.db.begin().await?;
        insert_question(
            &mut tx,
            NewQuestion {
                subject_id: pair.subject_id,
                page_id: pair.first_id,
                page_number: Some(&pair.first_number),
                group_id: None,
                diagram_id: None,
                number: &tail.number,
                topic: tail.topic.as_deref(),
                body: &joined.body,
                options: joined
                    .options
                    .iter()
                    .map(|o| NewOption {
                        label: o.label.clone(),
                        body: Some(o.body.clone()),
                        diagram_id: None,
                    })
                    .collect(),
                has_diagram: false,
                refers_to_number: false,
                sol: sol[0].as_ref(),
                luna: luna[0].as_ref(),
            },
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
    page_id: Uuid,
    description: String,
    redraw_note: Option<String>,
    photo_key: String,
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
             from d join pages p on p.id = d.page_id",
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
    let photo = B64.encode(state.storage.get(&d.photo_key).await?);
    let prompt = prompts::draw_prompt(
        &d.description,
        d.instruction.as_deref(),
        d.redraw_note.as_deref(),
    );
    let res = openai
        .respond(tutor_openai::Request {
            model: SOL,
            instructions: prompts::DRAW_INSTRUCTIONS,
            input: vec![Part::Jpeg(&photo), Part::Text(&prompt)],
            schema_name: "diagram",
            schema: &prompts::DRAW_SCHEMA,
        })
        .await?;
    log_usage(
        &state.db,
        Usage {
            page: Some(d.page_id),
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
        let q = |n: &str| prompts::ExtractedQuestion {
            number: n.into(),
            topic: None,
            body: String::new(),
            group_ref: None,
            diagram_ref: None,
            refers_to_number: false,
            options: vec![],
        };
        let mut qs = vec![q("4"), q("5"), q("4."), q("4")];
        disambiguate_numbers(&mut qs);
        let numbers: Vec<_> = qs.iter().map(|q| q.number.as_str()).collect();
        assert_eq!(numbers, ["4", "5", "4 (2nd)", "4 (3rd)"]);
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
