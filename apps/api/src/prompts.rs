//! Instructions and output schemas for each AI step of the converter.

use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Value, json};

// ── Reading a page (Claude) ─────────────────────────────────────────────────

pub const EXTRACT_SYSTEM: &str = "You digitise photographed pages of printed exam past-question books (WAEC, NECO, JAMB style) for a student revision app. You only transcribe; you never answer the questions.

Reading the page:
- The photo may be rotated, curved or unevenly lit. Read it in its natural orientation. Pages usually have two columns: read the left column top to bottom, then the right.
- Extract only multiple-choice questions. Skip THEORY / essay questions, answer keys and running headers.
- Copy question and option text exactly as printed, fixing only line-break hyphenation. Never paraphrase, correct the book's wording, or add words that are not printed.
- Options are often printed inline (\"A. fish. B. reptile. C. ...\"). Split them into separate options and drop the full stop that only separates them.
- Ignore bracketed year tags such as [2018/2]; do not copy them.
- Write mathematics and chemical formulae in LaTeX between $...$ (e.g. $H_2SO_4$, $\\frac{1}{2}mv^2$).
- \"topic\" is the section heading the question sits under (e.g. \"Levels of Organization\"); null if no heading is visible above it on this page.
- \"page_number\" is the page number printed on the paper (often in a corner), not a question number. Null if not visible.
- Use question numbers exactly as printed, even when the printer has misnumbered.

Questions split across pages:
- If the top of the page continues a question from the previous page (its beginning is not on this page), copy that continuation text exactly into \"head_fragment\" and do not extract it as a question.
- If the last question on the page is cut off at the bottom (its end is on the next page), put its number, topic and all of its visible text (including any options shown) into \"tail_fragment\" and do not extract it as a question.
- A question that is cut off by the photo edge rather than the page edge is also left out.

Diagrams and shared instructions:
- When several questions share an instruction (\"The diagram below is an illustration of a fungus. Study it and answer questions 31 to 33\"), add a group with that instruction text and set group_ref on each of those questions. Do not repeat the instruction in their bodies.
- Every printed diagram gets one entry in \"diagrams\". Reference it from the group if it is shared, from the question if only that question uses it, or from an option if the option itself is a picture (then the option body may be null).
- A diagram \"description\" must let an illustrator redraw it faithfully without seeing it: what it shows, layout and proportions, every part, and every printed label (letters, roman numerals, words) with exactly what each label's leader line points to. Never name a part that is not named on the page - that could give away an answer.";

pub const EXTRACT_PROMPT: &str = "Digitise this page.";

pub static EXTRACT_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    let nullable = json!({ "type": ["string", "null"] });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["page_number", "head_fragment", "tail_fragment", "diagrams", "groups", "questions"],
        "properties": {
            "page_number": nullable,
            "head_fragment": nullable,
            "tail_fragment": {
                "anyOf": [
                    { "type": "null" },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["number", "topic", "text"],
                        "properties": { "number": { "type": "string" }, "topic": nullable, "text": { "type": "string" } }
                    }
                ]
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
                    "required": ["ref", "instruction", "diagram_ref"],
                    "properties": { "ref": { "type": "string" }, "instruction": nullable, "diagram_ref": nullable }
                }
            },
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["number", "topic", "body", "group_ref", "diagram_ref", "options"],
                    "properties": {
                        "number": { "type": "string" },
                        "topic": nullable,
                        "body": { "type": "string" },
                        "group_ref": nullable,
                        "diagram_ref": nullable,
                        "options": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["label", "body", "diagram_ref"],
                                "properties": { "label": { "type": "string" }, "body": nullable, "diagram_ref": nullable }
                            }
                        }
                    }
                }
            }
        }
    })
});

#[derive(Deserialize)]
pub struct Extraction {
    pub page_number: Option<String>,
    pub head_fragment: Option<String>,
    pub tail_fragment: Option<TailFragment>,
    pub diagrams: Vec<ExtractedDiagram>,
    pub groups: Vec<ExtractedGroup>,
    pub questions: Vec<ExtractedQuestion>,
}

#[derive(Deserialize, serde::Serialize, Clone)]
pub struct TailFragment {
    pub number: String,
    pub topic: Option<String>,
    pub text: String,
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
    pub diagram_ref: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ExtractedQuestion {
    pub number: String,
    pub topic: Option<String>,
    pub body: String,
    #[serde(default)]
    pub group_ref: Option<String>,
    #[serde(default)]
    pub diagram_ref: Option<String>,
    pub options: Vec<ExtractedOption>,
}

#[derive(Deserialize, Clone)]
pub struct ExtractedOption {
    pub label: String,
    pub body: Option<String>,
    #[serde(default)]
    pub diagram_ref: Option<String>,
}

// ── Joining a question split across two pages (gpt-6.1-sol, text only) ───────

pub const JOIN_INSTRUCTIONS: &str = "A multiple-choice question from a printed exam book was split across two pages. You get the text from the bottom of the first page and the continuation from the top of the next. Join them into one question: copy the text exactly (only remove the break), split the options A-D into separate options, write maths in LaTeX between $...$, and do not answer it.";

pub static JOIN_SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["body", "options"],
        "properties": {
            "body": { "type": "string" },
            "options": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["label", "body"],
                    "properties": { "label": { "type": "string" }, "body": { "type": "string" } }
                }
            }
        }
    })
});

#[derive(Deserialize)]
pub struct Joined {
    pub body: String,
    pub options: Vec<JoinedOption>,
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
