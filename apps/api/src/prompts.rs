//! Instructions and output schemas for each AI step of the converter.

use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Value, json};

// ── Reading a page (Claude) ─────────────────────────────────────────────────

pub const EXTRACT_SYSTEM: &str = "You digitise pages of exam past-question books and exam papers (WAEC, NECO, NABTEB, JAMB) for a student revision app. Pages are phone photos or PDF scans. You only transcribe and classify; you never answer the questions.

Reading the page:
- The photo may be rotated, curved or unevenly lit. Read it in its natural orientation. Pages often have two columns: read the left column top to bottom, then the right.
- Only read the page itself. Ignore the edge of a facing page, fingers, watermarks and website stamps (e.g. \"www.myschoolgist.com\", \"Uploaded on ...\"), running headers and footers, print timestamps and answer keys.
- Skip instructions to candidates, cover pages and worked examples (\"Example\", \"Specimen answer\"); they are not questions.
- Extract every question: multiple-choice (\"objective\", usually under OBJ) and written (\"theory\", under THEORY, ESSAY or Paper 2/3). Use question numbers exactly as printed, even when misnumbered.
- Copy question and option text exactly as printed, fixing only line-break hyphenation. Never paraphrase, correct the wording, or add words that are not printed.
- Options are often printed inline (\"A. fish. B. reptile. C. ...\"). Split them into separate options and drop the full stop that only separates them.
- Write mathematics and chemical formulae in LaTeX between $...$ (e.g. $H_2SO_4$, $\\frac{1}{2}mv^2$).
- \"page_number\" is the page number printed on the paper (often in a corner), not a question number. Null if not visible.

Theory questions:
- \"body\" is the lead text before the first lettered part (may be empty). Each part goes in \"parts\" with its \"path\": the labels from the top, joined by \"/\" without brackets: (a) is \"a\", (a)(ii) is \"a/ii\", (b)(i) is \"b/i\". List parents before their children, in printed order.
- A part's \"body\" is its own text only. A sentence printed after a part's sub-parts that belongs to that part (\"Give reasons for your answers in (d)(i) and (ii).\") is its \"closing_text\".
- \"marks\" is the mark printed for the question or part (\"[4 marks]\", \"(2 marks)\"); null if none. Objective questions have no parts and null marks.
- Tables to complete or fill in are written as Markdown tables in the text, with blank cells left empty.

Where each question was set:
- Books print a tag after each question, e.g. [2012/1], (2005/4) or [WAEC 2014, Q7]: the year and the question's number in that exam. Put them in \"year\" and \"original_number\". Do not copy the tag into the question text.
- A page or section header can say which exam, year, paper or sitting the questions under it are from (\"BECE 2020 Mathematics\", \"WASSCE May/June 2015 Paper 2\", \"BECE/NECO/2020\"). Apply it to every question under it, and to the questions under the context carried from the previous page until the page shows a new one.
- \"exam\" is one of the exam codes you are given, only if the page or carried context names it; WAEC's SSCE/WASSCE/GCE is WASSCE, NECO's SSCE is SSCE, the junior exam is BECE, JAMB's is UTME.
- \"sitting\": may_june for May/June or WAEC June; june_july for June/July; nov_dec for Nov/Dec or GCE. Null if not printed.
- \"paper\" as printed (\"1\", \"2\", \"3\"); null if not printed. OBJ/THEORY alone does not name a paper.
- Leave any of these null when neither the page nor the carried context says. The app fills the rest from the source's details; never guess.
- \"carry\" is the context in force at the bottom of the page, for the next page: the current section heading and any exam, year, paper and sitting a header set. Null fields where nothing is in force.

Topics:
- \"topic_ref\" is the syllabus topic the question is about, from the list you are given (e.g. \"t12\"). Pick the most specific topic that fits; a theme only if none of its topics does. A printed section heading is a strong hint, but the question's content decides. Null only if nothing in the list fits.

Questions split across pages:
- If the top of the page continues a question from the previous page (its beginning is not on this page), copy that continuation text exactly into \"head_fragment\" and do not extract it as a question.
- If the last question on the page is cut off at the bottom (its end is on the next page), put it in \"tail_fragment\" (its number, kind, topic_ref, the year/number from its tag if visible, and all its visible text including any options or parts) and do not extract it as a question.
- A question cut off by the photo edge rather than the page edge is also left out.

Diagrams and shared instructions:
- When several questions share an instruction (\"The diagram below is an illustration of a fungus. Study it and answer questions 31 to 33\"), add a group with that instruction text and set group_ref on each of those questions. Do not repeat the instruction in their bodies.
- Students see a group's questions together but renumbered, so also give \"display_instruction\": the same instruction with every reference to question numbers removed (\"The diagram below is an illustration of a fungus. Study it and answer questions 31 to 33\" becomes \"The diagram below is an illustration of a fungus.\"). Change nothing else. Use null if nothing is left.
- Set \"refers_to_number\" to true when a question's own text, parts or options mention another question by its number (e.g. \"using your answer to question 31\"); otherwise false.
- Every printed diagram gets one entry in \"diagrams\". Reference it from the group if it is shared, from the question or part if only that one uses it, or from an option if the option itself is a picture (then the option body may be null).
- A diagram \"description\" must let an illustrator redraw it faithfully without seeing it: what it shows, layout and proportions, every part, and every printed label (letters, roman numerals, words) with exactly what each label's leader line points to. Never name a part that is not named on the page - that could give away an answer.";

/// What the reader is told about the page beyond the image itself.
pub struct PageContext<'a> {
    pub source_title: &'a str,
    pub source_kind: &'a str,
    pub layout: &'a str,
    pub subject: &'a str,
    pub level: &'a str,
    /// "WAEC WASSCE 1993 Paper 2 May/June" — the source's defaults, for orientation only.
    pub source_exam: Option<String>,
    /// Exam codes the reader may use, e.g. "WASSCE (WAEC)".
    pub exam_codes: Vec<String>,
    /// From the previous page, as JSON.
    pub carried: Option<&'a serde_json::Value>,
    /// (ref, "Theme > Topic") in syllabus order.
    pub topics: Vec<(String, String)>,
}

pub fn extract_prompt(c: &PageContext) -> String {
    let layout = match c.layout {
        "by_topic" => "a book arranged by topic",
        "by_year" => "a book arranged by year",
        _ => "a single exam paper",
    };
    let mut p = format!(
        "Digitise this page.\n\nSource: \"{}\" ({}, {}). Subject: {} ({}).",
        c.source_title,
        c.source_kind,
        layout,
        c.subject,
        c.level.to_uppercase()
    );
    if let Some(exam) = &c.source_exam {
        p += &format!(
            " The source's details say: {exam}. Don't copy these into questions; the app fills them in."
        );
    }
    p += &format!("\n\nExam codes: {}.", c.exam_codes.join(", "));
    match c.carried {
        Some(carried) => p += &format!("\n\nCarried from the previous page: {carried}"),
        None => p += "\n\nNothing is carried from a previous page.",
    }
    p += "\n\nSyllabus topics (ref: theme > topic):";
    for (r, name) in &c.topics {
        p += &format!("\n{r}: {name}");
    }
    p
}

pub static EXTRACT_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    let nullable = json!({ "type": ["string", "null"] });
    let year = json!({ "type": ["integer", "null"] });
    let sitting =
        json!({ "type": ["string", "null"], "enum": ["may_june", "june_july", "nov_dec", null] });
    let marks = json!({ "type": ["number", "null"] });
    let kind = json!({ "type": "string", "enum": ["objective", "theory"] });
    let question = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["number", "kind", "topic_ref", "exam", "year", "paper", "sitting", "original_number",
                     "body", "marks", "group_ref", "diagram_ref", "refers_to_number", "options", "parts"],
        "properties": {
            "number": { "type": "string" },
            "kind": kind,
            "topic_ref": nullable,
            "exam": nullable,
            "year": year,
            "paper": nullable,
            "sitting": sitting,
            "original_number": nullable,
            "body": { "type": "string" },
            "marks": marks,
            "group_ref": nullable,
            "diagram_ref": nullable,
            "refers_to_number": { "type": "boolean" },
            "options": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["label", "body", "diagram_ref"],
                    "properties": { "label": { "type": "string" }, "body": nullable, "diagram_ref": nullable }
                }
            },
            "parts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["path", "body", "closing_text", "marks", "diagram_ref"],
                    "properties": {
                        "path": { "type": "string" },
                        "body": nullable,
                        "closing_text": nullable,
                        "marks": marks,
                        "diagram_ref": nullable
                    }
                }
            }
        }
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["page_number", "head_fragment", "tail_fragment", "carry", "diagrams", "groups", "questions"],
        "properties": {
            "page_number": nullable,
            "head_fragment": nullable,
            "tail_fragment": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["number", "kind", "topic_ref", "year", "original_number", "text"],
                        "properties": {
                            "number": { "type": "string" },
                            "kind": kind,
                            "topic_ref": nullable,
                            "year": year,
                            "original_number": nullable,
                            "text": { "type": "string" }
                        }
                    }
                ]
            },
            "carry": {
                "type": "object",
                "additionalProperties": false,
                "required": ["heading", "exam", "year", "paper", "sitting"],
                "properties": { "heading": nullable, "exam": nullable, "year": year, "paper": nullable, "sitting": sitting }
            },
            "diagrams": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["ref", "description"],
                    "properties": { "ref": { "type": "string" }, "description": { "type": "string" } }
                }
            },
            "groups": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["ref", "instruction", "display_instruction", "diagram_ref"],
                    "properties": { "ref": { "type": "string" }, "instruction": nullable, "display_instruction": nullable, "diagram_ref": nullable }
                }
            },
            "questions": { "type": "array", "items": question }
        }
    })
});

#[derive(Deserialize)]
pub struct Extraction {
    pub page_number: Option<String>,
    pub head_fragment: Option<String>,
    pub tail_fragment: Option<TailFragment>,
    #[serde(default)]
    pub carry: Option<Value>,
    pub diagrams: Vec<ExtractedDiagram>,
    pub groups: Vec<ExtractedGroup>,
    pub questions: Vec<ExtractedQuestion>,
}

#[derive(Deserialize, serde::Serialize, Clone)]
pub struct TailFragment {
    pub number: String,
    #[serde(default = "objective")]
    pub kind: String,
    #[serde(default)]
    pub topic_ref: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub original_number: Option<String>,
    pub text: String,
    /// Filled by the worker from `topic_ref` and the page's carried context, so the
    /// join (which runs later, without the page) knows where the question was set.
    #[serde(default)]
    pub topic_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub exam: Option<String>,
    #[serde(default)]
    pub paper: Option<String>,
    #[serde(default)]
    pub sitting: Option<String>,
}

fn objective() -> String {
    "objective".into()
}

#[derive(Deserialize)]
pub struct ExtractedDiagram {
    #[serde(rename = "ref")]
    pub key: String,
    pub description: String,
}

#[derive(Deserialize)]
pub struct ExtractedGroup {
    #[serde(rename = "ref")]
    pub key: String,
    pub instruction: Option<String>,
    /// The instruction without question numbers, shown to students.
    #[serde(default)]
    pub display_instruction: Option<String>,
    pub diagram_ref: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ExtractedQuestion {
    pub number: String,
    #[serde(default = "objective")]
    pub kind: String,
    #[serde(default)]
    pub topic_ref: Option<String>,
    /// As printed on the page or carried from the previous one; the source fills the rest.
    #[serde(default)]
    pub exam: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub paper: Option<String>,
    #[serde(default)]
    pub sitting: Option<String>,
    #[serde(default)]
    pub original_number: Option<String>,
    pub body: String,
    #[serde(default)]
    pub marks: Option<f64>,
    #[serde(default)]
    pub group_ref: Option<String>,
    #[serde(default)]
    pub diagram_ref: Option<String>,
    /// The text mentions another question by number ("your answer to question 31").
    #[serde(default)]
    pub refers_to_number: bool,
    pub options: Vec<ExtractedOption>,
    #[serde(default)]
    pub parts: Vec<ExtractedPart>,
}

#[derive(Deserialize, Clone)]
pub struct ExtractedOption {
    pub label: String,
    pub body: Option<String>,
    #[serde(default)]
    pub diagram_ref: Option<String>,
}

/// One part of a theory question; `path` is "a", "a/ii" etc.
#[derive(Deserialize, serde::Serialize, Clone)]
pub struct ExtractedPart {
    pub path: String,
    pub body: Option<String>,
    #[serde(default)]
    pub closing_text: Option<String>,
    #[serde(default)]
    pub marks: Option<f64>,
    #[serde(default)]
    pub diagram_ref: Option<String>,
}

// ── Joining a question split across two pages (gpt-6.1-sol, text only) ───────

pub const JOIN_INSTRUCTIONS: &str = "A question from a printed exam book was split across two pages. You get its kind, the text from the bottom of the first page and the continuation from the top of the next. Join them into one question and copy the text exactly (only remove the break); write maths in LaTeX between $...$; do not answer it.
- objective: split the options A-E into separate options; no parts.
- theory: \"body\" is the lead text before the first lettered part; each part goes in \"parts\" with its path (\"a\", \"a/ii\"), its own text, any closing sentence that belongs to it, and its printed marks; no options.
- A tag such as [2012/1] is the year and the question's number in that exam: put them in \"year\" and \"original_number\" (null if there is no tag) and leave the tag out of the text.";

pub static JOIN_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    let nullable = json!({ "type": ["string", "null"] });
    let marks = json!({ "type": ["number", "null"] });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["body", "year", "original_number", "marks", "options", "parts"],
        "properties": {
            "body": { "type": "string" },
            "year": { "type": ["integer", "null"] },
            "original_number": nullable,
            "marks": marks,
            "options": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["label", "body"],
                    "properties": { "label": { "type": "string" }, "body": { "type": "string" } }
                }
            },
            "parts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["path", "body", "closing_text", "marks"],
                    "properties": {
                        "path": { "type": "string" },
                        "body": nullable,
                        "closing_text": nullable,
                        "marks": marks
                    }
                }
            }
        }
    })
});

#[derive(Deserialize)]
pub struct Joined {
    pub body: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub original_number: Option<String>,
    #[serde(default)]
    pub marks: Option<f64>,
    pub options: Vec<JoinedOption>,
    #[serde(default)]
    pub parts: Vec<ExtractedPart>,
}

#[derive(Deserialize)]
pub struct JoinedOption {
    pub label: String,
    pub body: String,
}

// ── Answering (gpt-6.1-sol and gpt-6-luna) ──────────────────────────────────

pub const ANSWER_INSTRUCTIONS: &str = "You are an expert examiner for West African (WAEC, NECO, JAMB) secondary school exams. Answer every multiple-choice question with the standard answer expected by the Nigerian syllabus. Each question starts with an item tag in square brackets, e.g. [7]. For each question give: \"item\" = that tag's number, the letter of the single best option (just the letter, e.g. \"C\"), a one or two sentence explanation a student would find helpful, and your honest confidence: high = sure this is the expected answer; medium = defensible but another option could be marked correct, or the question is ambiguous; low = guessing or the text is unclear.";

pub static ANSWER_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["answers"],
        "properties": {
            "answers": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["item", "answer", "explanation", "confidence"],
                    "properties": {
                        "item": { "type": "integer" },
                        "answer": { "type": "string" },
                        "explanation": { "type": "string" },
                        "confidence": { "type": "string", "enum": ["high", "medium", "low"] }
                    }
                }
            }
        }
    })
});

#[derive(Deserialize, Clone)]
pub struct Answer {
    /// The `[n]` tag of the question in `answer_input`, 1-based.
    pub item: i64,
    pub answer: String,
    pub explanation: String,
    pub confidence: String,
}

#[derive(Deserialize)]
pub struct Answers {
    pub answers: Vec<Answer>,
}

/// Question text as the answering models see it, diagrams given by description.
pub struct QuestionForAnswer<'a> {
    pub number: &'a str,
    pub topic: Option<&'a str>,
    pub instruction: Option<&'a str>,
    pub body: &'a str,
    pub diagram: Option<&'a str>,
    pub options: Vec<(&'a str, String)>,
}

/// Questions are tagged [1], [2], ... in order; answers are matched back by that tag,
/// never by printed number (books repeat and misprint numbers).
pub fn answer_input(questions: &[QuestionForAnswer]) -> String {
    questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let mut t = format!("[{}] Question {}", i + 1, q.number);
            if let Some(topic) = q.topic {
                t += &format!(" [topic: {topic}]");
            }
            if let Some(i) = q.instruction {
                t += &format!("\n{i}");
            }
            if let Some(d) = q.diagram {
                t += &format!("\n(Diagram: {d})");
            }
            t += &format!("\n{}", q.body);
            for (label, body) in &q.options {
                t += &format!("\n{label}. {body}");
            }
            t
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ── Drawing diagrams (gpt-6.1-sol) ──────────────────────────────────────────

pub const DRAW_INSTRUCTIONS: &str = "You redraw diagrams from photographed exam papers as clean, sharp SVG for a student revision app.
- Redraw only the requested diagram, faithfully: same structure, proportions, orientation and every printed label (letters, roman numerals, words) with leader lines pointing to exactly the same parts. Do not add, remove or rename labels, and do not add a title or caption - naming a part could give away an answer.
- Style: clean textbook line art. Strokes #1f2937 at 1.5-2.5px, light fills only where they aid reading, labels in Helvetica/Arial at 14-18px.
- Start with <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"...\"> with no width/height. Put a white background rect first. No scripts, event handlers, links, external references or embedded images.";

pub fn draw_prompt(
    description: &str,
    instruction: Option<&str>,
    redraw_note: Option<&str>,
) -> String {
    let mut p = format!(
        "Redraw this diagram from the photo as SVG.\n\nIt is introduced by: {}\n\nDescription:\n{description}",
        instruction.unwrap_or("(no instruction)")
    );
    if let Some(note) = redraw_note {
        p += &format!("\n\nA reviewer rejected the previous drawing. Fix this:\n{note}");
    }
    p
}

pub static DRAW_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["svg"],
        "properties": { "svg": { "type": "string" } }
    })
});
