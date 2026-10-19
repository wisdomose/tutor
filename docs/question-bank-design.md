# Question bank redesign — agreed 2026-10-19

Not built yet. Replaces the question-bank part of `design-summary.md` and the
"one book per subject, multiple-choice only" assumptions in migration 0001.

## Why

- Theory questions are saved when a page is first read, so no page is read twice.
  They are answered later, not now.
- One subject has many books, and some sources are single exam papers (PDFs).
- Junior (JSS) and senior (SSS) subjects are separate, even with the same name.
- Questions carry their exam source (body, exam, year, paper, sitting).
- Some questions are written by our team, not taken from a book.
- Topics come from one merged syllabus list per subject. SSS: WAEC's syllabus is
  the frame (third-party copies from waecsyllabus.com, as WAEC doesn't publish its
  own) with topics only JAMB's official UTME syllabus has added in. JSS: not
  loaded yet (no NERDC or BECE syllabus found online).

## Source formats seen

- **Topic-grouped book** (SSS Civic Education). Tags such as `[2017/3]` are
  year/original question number. The exam body is not printed, so it comes from
  the source's defaults.
  Prep50 (SSCE 1988–2021) says on its cover that every question is WAEC's. Its
  sitting is unknown, so that default stays blank. There is also a Prep50 JAMB
  series.
- **Year-ordered book** (Lasswell Evergreen, BECE Mathematics 2020). Every page
  header gives the subject, year and exam. The footer `BECE/NECO/2020` names the
  body that runs the exam. Options go to E. Photos may show a thumb and the edge of
  the facing page.
- **Single exam paper, PDF** (WAEC June 1993 SSCE Biology 2, scanned, watermarked).
  The source is known for the whole file. Skip the instruction page, the worked
  example and the watermark. Theory parts nest: 1(a)(i)(ii), and 4(d) has a lead
  paragraph, sub-parts and a closing sentence.

## Reference lists

**subjects** (changed): `level` (`jss` | `sss`), `name`, `syllabuses` (the documents
used, with URL and whether official); unique (level, name). Subjects are imported
with their topics from the syllabuses, never typed in by hand.

**syllabus_topics** (new): `subject_id`, `parent_id` (theme → topic), `name`,
`position`, `bodies` (which syllabuses list it: WAEC, JAMB, …).

Syllabus outlines live in `crates/db/syllabus/<level>/<subject>.txt`;
`crates/db/syllabus/generate.py` turns them into a migration. JAMB's Use of English
is merged into English Language. Left out: WAEC subjects for other countries
(Basketry, Ceramics, Textiles, Forestry, Fisheries Alt A, Animal Husbandry Alt A,
ICT core, West African Traditional Religion, Business Management, Clerical Office
Duties) and the practical-only Shorthand and Typewriting.

**exam_bodies** (new): `code`, `name`. WAEC, NECO, NABTEB, JAMB.

**exams** (new): `body_id`, `code`, `name`, `level`. One row per real exam,
under its current name. Rows: WAEC WASSCE, NECO SSCE, NECO BECE, NABTEB NBC,
JAMB UTME. There are no aliases: the source says which exam it is (Prep50
SSCE is set to WAEC WASSCE), and the exam name printed in the book is not used.
A source covering more than one exam is not handled yet; we'll design for it when
we have one.

## Where questions come from

**sources** (new): a book or a single paper.
- `kind`: `book` | `paper`
- `layout`: `by_topic` | `by_year` | `single_paper`
- `title`, `publisher`, `edition`, `note`
- `subject_id`; the level comes from the subject
- Defaults for anything the pages don't print: `exam_id`, `year`, `paper`, `sitting`

**uploads** (new): `source_id`, `kind` (`photo` | `pdf`), `file_key`,
`original_name`. A PDF is split into one page per sheet.

**pages** (changed):
- `subject_id` is replaced by `source_id` and `upload_id`.
- `position` is the order within the source. A source's pages are processed in
  this order.
- `carried_context` holds the heading and exam/year in force at the bottom of the
  page, and is passed to the next page.
- Everything else stays as it is.

## The bank

**questions** (changed):
- `kind`: `objective` | `theory`
- `origin`: `source` | `ours`
- Source questions: `source_id`, `page_id`, `page_number`, `number` as printed.
  These are where the question was first seen; every appearance is in
  `question_appearances`.
- Exam, all nullable: `exam_id`, `year`, `paper`, `sitting`, `original_number`.
  The printed tag overrides the source's defaults.
- Sitting is stored as printed: `may_june` | `june_july` | `nov_dec`.
- Our questions: `author_id` (→ staff)
- `topic_id` (→ syllabus_topics). Every question gets one, whatever the
  source's layout: Claude picks the syllabus topic from what the question is
  about. In a topic-grouped book the printed heading is a strong hint; in a
  year-ordered book or a single paper the question's content is all there is.
  The heading is not stored, and reviewers can correct the match.
- `marks` (theory)
- Unchanged: group, diagram, body, explanation, confidence, second answer,
  review flags.
- Rules:
  - `source` questions need a source and a page; `ours` need an author and no page.
  - A question is saved once, however many sources print it (see "One question,
    many sources").
  - Sol/Luna answer only objective questions for now.

**question_parts** (new): the part tree for theory questions.
- `question_id`
- `parent_id`
- `label` ("a", "ii")
- `position`
- `body` (lead text)
- `closing_text` (e.g. "Give reasons for your answers in (d)(i) and (ii)")
- `marks`
- `diagram_id`

**diagrams** (changed): `page_id` becomes optional, because our questions have no
page.

**staff** (new, minimal): authors of our questions. It becomes the real user table
when login arrives.

**question_appearances** (new): `question_id`, `source_id`, `page_id`,
`page_number` and `number` as printed; unique (source_id, page_number, number). Lets a page that is
uploaded again be recognised, shows reviewers every photo of a question, and
shows how often an exam repeated it.

**Unchanged**: options (objective only), question_groups, queue_state, ai_usage.

## One question, many sources

A past question is identified by when it was set, not by its wording. The same
text in WAEC 2015 and WAEC 2017 is two questions.

- **Key**: exam + subject + year + sitting + paper (as printed; otherwise
  objective vs theory) + original number.
- **Full key match**: don't save a new copy. Fill in only the saved question's
  blank fields (sitting, paper, topic, a missing diagram…); keep every value
  already there. Add an appearance row. Skip Sol/Luna: it is already answered.
  - If the new copy disagrees with a saved value (different wording, swapped
    options, a differently labelled diagram, or a different question entirely
    because a book mis-tagged it), keep the saved value and flag
    `source_conflict`. A reviewer compares both photos and decides. Nothing
    changes automatically.
- **Incomplete key** (e.g. Prep50 has no sitting): use the parts that are
  known to find candidates, then compare text only as a safety check. Text never
  finds a match by itself.
  - One candidate whose text agrees → merge as above.
  - The text differs, or there are several candidates → save as new and flag
    `possible_duplicate` for a reviewer to merge or keep.
- **JAMB UTME**: year + number matches only for the fixed-paper years before
  computer-based testing started in 2013. After that, each book's
  JAMB questions are saved as their own questions and never matched.
- **No year printed, or one of ours**: no key, never matched.

## Left for later

- Paper-level rules ("Answer three questions only", section marks), for a
  mock-exam mode.
- Answering theory questions.
- Student-side tables.

## Knock-on work

- Syllabus topic lists must exist for a subject before its pages are uploaded,
  so Claude has a list to match against.
- Upload page: choose or create a source; accept photos and PDFs. (Done:
  the browser renders each PDF page to a 2400px JPEG and sends it with the
  stored PDF's `upload_id`, so the reader sees PDFs and photos the same way.)
- Claude prompt:
  - Extract theory questions, with their part trees.
  - Read tags as year/question number.
  - Match each question to a syllabus topic.
  - Ignore watermarks, worked examples and the facing page.
- Worker: match each extracted question against the bank before saving.
- A question editor for our own questions.
