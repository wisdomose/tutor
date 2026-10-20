//! Deciding whether a question just read is one the bank already has.
//!
//! A past question is identified by when it was set (exam, subject, year, sitting,
//! paper, kind and its number in that exam), never by its wording. Text is only
//! compared once the key has found a candidate, as a safety check.
//! See docs/question-bank-design.md, "One question, many sources".

use std::collections::HashSet;

/// JAMB moved to computer-based tests in 2013; from then on each candidate's
/// questions differ, so a year and number no longer name one question.
pub const UTME_LAST_FIXED_YEAR: i32 = 2012;

/// When a question was set. Every field is optional; see [`Key::matchable`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Key {
    pub exam_id: Option<uuid::Uuid>,
    pub exam_code: Option<String>,
    pub year: Option<i32>,
    pub paper: Option<String>,
    pub sitting: Option<String>,
    pub original_number: Option<String>,
}

impl Key {
    /// Exam, year and number are the least a match needs; UTME only before 2013.
    pub fn matchable(&self) -> bool {
        self.exam_id.is_some()
            && self.original_number.is_some()
            && self.year.is_some_and(|y| {
                self.exam_code.as_deref() != Some("UTME") || y <= UTME_LAST_FIXED_YEAR
            })
    }
}

/// What to do with a question just read.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// This page was read before (uploaded again); the question is already saved.
    AlreadyOnPage,
    /// Save it as a new question; flag it if a near match exists.
    New { possible_duplicate: bool },
    /// The same question is saved: add an appearance and fill its blanks.
    /// `conflict` when this copy disagrees with the saved one; `adds_options` when
    /// this copy prints options the saved one left out (a book that drops E).
    Existing {
        id: uuid::Uuid,
        conflict: bool,
        adds_options: bool,
    },
}

/// A saved question found by key.
pub struct Candidate {
    pub id: uuid::Uuid,
    pub sitting: Option<String>,
    pub text: Text,
}

/// Decides from the key's candidates (already filtered to compatible keys, same kind).
///
/// Theory pieces that share a tag but read differently are kept apart without a flag:
/// topic books split one exam question's parts across topics under the same tag.
pub fn decide(key: &Key, theory: bool, text: &Text, candidates: &[Candidate]) -> Decision {
    let agreeing: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| text.agrees_with(&c.text))
        .collect();
    match (agreeing.as_slice(), candidates) {
        ([], []) => Decision::New {
            possible_duplicate: false,
        },
        // One saved copy reads the same: it's this question.
        ([c], _) => Decision::Existing {
            id: c.id,
            conflict: false,
            adds_options: text.has_options_missing_from(&c.text),
        },
        ([], _) if theory => Decision::New {
            possible_duplicate: false,
        },
        // The only copy with the same full key reads differently.
        ([], [c]) if key.sitting.is_some() && c.sitting.is_some() => Decision::Existing {
            id: c.id,
            conflict: true,
            adds_options: false,
        },
        _ => Decision::New {
            possible_duplicate: true,
        },
    }
}

/// The text of a question as compared between copies.
#[derive(Clone, Debug, Default)]
pub struct Text {
    pub body: String,
    /// (label, body); pictures have an empty body.
    pub options: Vec<(String, String)>,
    /// Theory parts as "body closing", in printed order.
    pub parts: Vec<String>,
}

const BODY_SIMILARITY: f64 = 0.9;
const OPTION_SIMILARITY: f64 = 0.85;

fn sorted_options(t: &Text) -> Vec<(String, String)> {
    let mut o: Vec<(String, String)> = t
        .options
        .iter()
        .map(|(l, b)| (l.to_ascii_uppercase(), b.clone()))
        .collect();
    o.sort();
    o
}

impl Text {
    /// Same wording (allowing small reading differences) and the same options under the
    /// same letters. One copy may stop early (A-D against A-E). Swapped options or a
    /// different question disagree.
    pub fn agrees_with(&self, other: &Text) -> bool {
        let whole = |t: &Text| format!("{} {}", t.body, t.parts.join(" "));
        if similarity(&whole(self), &whole(other)) < BODY_SIMILARITY {
            return false;
        }
        let (mine, theirs) = (sorted_options(self), sorted_options(other));
        let (short, long) = if mine.len() <= theirs.len() {
            (&mine, &theirs)
        } else {
            (&theirs, &mine)
        };
        // Objective questions keep at least two options in common.
        if short.len() < 2 && !long.is_empty() {
            return false;
        }
        short
            .iter()
            .zip(long.iter())
            .all(|((la, a), (lb, b))| la == lb && similarity(a, b) >= OPTION_SIMILARITY)
    }

    /// This copy has option letters the other doesn't.
    pub fn has_options_missing_from(&self, other: &Text) -> bool {
        self.options
            .iter()
            .any(|(l, _)| !other.options.iter().any(|(o, _)| o.eq_ignore_ascii_case(l)))
    }
}

/// Lower case letters and digits only, LaTeX commands and punctuation dropped.
fn normalise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // \frac, \times ...: the command name isn't wording.
            while chars.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                chars.next();
            }
            out.push(' ');
        } else if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Dice coefficient over character pairs of the normalised text, ignoring spacing:
/// 1.0 same, 0.0 nothing shared.
pub fn similarity(a: &str, b: &str) -> f64 {
    let squash = |s: &str| normalise(s).replace(' ', "");
    let (a, b) = (squash(a), squash(b));
    if a == b {
        return 1.0;
    }
    let pairs = |s: &str| {
        let c: Vec<char> = s.chars().collect();
        c.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>()
    };
    let (pa, pb) = (pairs(&a), pairs(&b));
    if pa.is_empty() || pb.is_empty() {
        return 0.0;
    }
    let mut pool: Vec<(char, char)> = pb.clone();
    let mut shared = 0;
    for p in &pa {
        if let Some(i) = pool.iter().position(|q| q == p) {
            pool.swap_remove(i);
            shared += 1;
        }
    }
    2.0 * shared as f64 / (pa.len() + pb.len()) as f64
}

/// Finds an exam by what the reader printed ("WASSCE", "WAEC WASSCE", "BECE").
/// `exams` is (id, body, code, level); only exams at `level` count.
pub fn find_exam<'a>(
    printed: &str,
    level: &str,
    exams: &'a [(uuid::Uuid, String, String, String)],
) -> Option<&'a (uuid::Uuid, String, String, String)> {
    let words: HashSet<String> = printed
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_uppercase())
        .collect();
    let mut found: Vec<_> = exams
        .iter()
        .filter(|(_, _, code, l)| l == level && words.contains(code))
        .collect();
    if found.len() > 1 {
        found.retain(|(_, body, _, _)| words.contains(body));
    }
    match found.as_slice() {
        [one] => Some(one),
        _ => None,
    }
}

/// "Paper 2", "II", "2." → "2".
pub fn norm_paper(p: &str) -> Option<String> {
    let s = p.trim();
    let s = s
        .strip_prefix("Paper")
        .or_else(|| s.strip_prefix("paper"))
        .or_else(|| s.strip_prefix("PAPER"))
        .unwrap_or(s)
        .trim()
        .trim_end_matches('.');
    let s = match s {
        "I" | "i" => "1",
        "II" | "ii" => "2",
        "III" | "iii" => "3",
        other => other,
    };
    (!s.is_empty()).then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn text(body: &str, options: &[(&str, &str)]) -> Text {
        Text {
            body: body.into(),
            options: options
                .iter()
                .map(|(l, b)| (l.to_string(), b.to_string()))
                .collect(),
            parts: vec![],
        }
    }

    fn key(sitting: Option<&str>) -> Key {
        Key {
            exam_id: Some(Uuid::nil()),
            exam_code: Some("WASSCE".into()),
            year: Some(2012),
            paper: None,
            sitting: sitting.map(str::to_string),
            original_number: Some("1".into()),
        }
    }

    const Q: (&str, &[(&str, &str)]) = (
        "The science of life is referred to as",
        &[
            ("A", "anatomy"),
            ("B", "biology"),
            ("C", "ecology"),
            ("D", "phylogeny"),
        ],
    );

    #[test]
    fn small_reading_differences_still_agree() {
        let a = text(Q.0, Q.1);
        let b = text(
            "The science of life is reffered to as:",
            &[
                ("A", "Anatomy."),
                ("B", "biology"),
                ("C", "ecology"),
                ("D", "phylogeny"),
            ],
        );
        assert!(a.agrees_with(&b));
    }

    #[test]
    fn swapped_options_disagree() {
        let a = text(Q.0, Q.1);
        let b = text(
            Q.0,
            &[
                ("A", "biology"),
                ("B", "anatomy"),
                ("C", "ecology"),
                ("D", "phylogeny"),
            ],
        );
        assert!(!a.agrees_with(&b));
    }

    #[test]
    fn different_question_disagrees() {
        let a = text(Q.0, Q.1);
        let b = text(
            "A change in the environment of an organism is termed",
            &[
                ("A", "sensitivity"),
                ("B", "impulse"),
                ("C", "response"),
                ("D", "stimulus"),
            ],
        );
        assert!(!a.agrees_with(&b));
    }

    #[test]
    fn decisions() {
        let t = text(Q.0, Q.1);
        let other = text("Something else entirely about cells", Q.1);
        let cand = |sitting: Option<&str>, text: Text| Candidate {
            id: Uuid::from_u128(7),
            sitting: sitting.map(str::to_string),
            text,
        };
        let new = Decision::New {
            possible_duplicate: false,
        };
        let dup = Decision::New {
            possible_duplicate: true,
        };
        let same = Decision::Existing {
            id: Uuid::from_u128(7),
            conflict: false,
            adds_options: false,
        };
        let conflict = Decision::Existing {
            id: Uuid::from_u128(7),
            conflict: true,
            adds_options: false,
        };

        assert_eq!(decide(&key(None), false, &t, &[]), new);
        // Full key: same text merges, different text is a conflict on the saved one.
        assert_eq!(
            decide(
                &key(Some("may_june")),
                false,
                &t,
                &[cand(Some("may_june"), t.clone())]
            ),
            same
        );
        assert_eq!(
            decide(
                &key(Some("may_june")),
                false,
                &t,
                &[cand(Some("may_june"), other.clone())]
            ),
            conflict
        );
        // Sitting unknown on either side: text agreeing merges, otherwise a new flagged copy.
        assert_eq!(
            decide(&key(None), false, &t, &[cand(Some("nov_dec"), t.clone())]),
            same
        );
        assert_eq!(
            decide(
                &key(None),
                false,
                &t,
                &[cand(Some("nov_dec"), other.clone())]
            ),
            dup
        );
        assert_eq!(
            decide(&key(Some("may_june")), false, &t, &[cand(None, other)]),
            dup
        );
        // Several candidates: the one that reads the same, else a reviewer decides.
        let differs = cand(None, text("Something else entirely about cells", Q.1));
        assert_eq!(
            decide(
                &key(None),
                false,
                &t,
                &[differs, cand(Some("may_june"), t.clone())]
            ),
            same
        );
        assert_eq!(
            decide(
                &key(None),
                false,
                &t,
                &[
                    cand(Some("may_june"), t.clone()),
                    cand(Some("nov_dec"), t.clone())
                ]
            ),
            dup
        );
    }

    #[test]
    fn a_book_that_drops_option_e_still_agrees() {
        let book = text(Q.0, Q.1);
        let mut paper = text(Q.0, Q.1);
        paper.options.push(("E".into(), "taxonomy".into()));
        assert!(book.agrees_with(&paper) && paper.agrees_with(&book));
        assert!(paper.has_options_missing_from(&book));
        assert!(!book.has_options_missing_from(&paper));
        let cand = Candidate {
            id: Uuid::nil(),
            sitting: None,
            text: book,
        };
        assert_eq!(
            decide(&key(Some("may_june")), false, &paper, &[cand]),
            Decision::Existing {
                id: Uuid::nil(),
                conflict: false,
                adds_options: true
            }
        );
    }

    #[test]
    fn theory_pieces_under_one_tag_are_kept_apart_unflagged() {
        let piece = |b: &str| Text {
            body: String::new(),
            options: vec![],
            parts: vec![b.into()],
        };
        let saved = Candidate {
            id: Uuid::nil(),
            sitting: Some("may_june".into()),
            text: piece("Describe briefly two levels of cell organization in living organisms."),
        };
        let other = piece(
            "Make a labelled drawing of a typical plant cell to show its essential features.",
        );
        assert_eq!(
            decide(&key(Some("may_june")), true, &other, &[saved]),
            Decision::New {
                possible_duplicate: false
            }
        );
    }

    #[test]
    fn utme_matches_only_before_2013() {
        let mut k = key(None);
        k.exam_code = Some("UTME".into());
        k.year = Some(2012);
        assert!(k.matchable());
        k.year = Some(2013);
        assert!(!k.matchable());
        k.exam_code = Some("WASSCE".into());
        assert!(k.matchable());
        k.year = None;
        assert!(!k.matchable());
    }

    #[test]
    fn exams_by_printed_name() {
        let e = |b: &str, c: &str, l: &str| {
            (Uuid::new_v4(), b.to_string(), c.to_string(), l.to_string())
        };
        let exams = vec![
            e("WAEC", "WASSCE", "sss"),
            e("NECO", "SSCE", "sss"),
            e("NECO", "BECE", "jss"),
            e("JAMB", "UTME", "sss"),
        ];
        assert_eq!(
            find_exam("WAEC WASSCE", "sss", &exams).map(|x| x.2.as_str()),
            Some("WASSCE")
        );
        assert_eq!(
            find_exam("SSCE", "sss", &exams).map(|x| x.2.as_str()),
            Some("SSCE")
        );
        assert_eq!(
            find_exam("BECE/NECO/2020", "jss", &exams).map(|x| x.2.as_str()),
            Some("BECE")
        );
        assert_eq!(find_exam("BECE", "sss", &exams), None);
        assert_eq!(find_exam("GCE", "sss", &exams), None);
    }

    #[test]
    fn papers() {
        assert_eq!(norm_paper("Paper 2").as_deref(), Some("2"));
        assert_eq!(norm_paper("II").as_deref(), Some("2"));
        assert_eq!(norm_paper("1").as_deref(), Some("1"));
        assert_eq!(norm_paper(" "), None);
    }

    #[test]
    fn latex_commands_are_not_wording() {
        assert!(similarity("$\\frac{1}{2}mv^2$", "1/2 mv2") > 0.9);
    }
}
