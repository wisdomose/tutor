-- Question bank redesign (docs/question-bank-design.md): subjects by level,
-- syllabus topics, exams, sources (books and single papers), uploads, theory
-- questions with part trees, our own questions, and one row per question however
-- many sources print it.

-- The bank so far was two test pages; start from a clean slate. The usage log
-- keeps its costs (page and diagram links are set null).
delete from options;
delete from questions;
delete from question_groups;
delete from diagrams;
delete from pages;

-- ── Subjects by level ───────────────────────────────────────────────────────

-- Subjects and their topics are imported from the official syllabuses, not
-- typed in here; the hand-made list from 0001 goes.
delete from subjects;
alter table subjects add column level text not null check (level in ('jss', 'sss'));
-- The syllabus documents a subject's topics were taken from:
-- [{"body": "WAEC", "url": "...", "official": false}, ...]
alter table subjects add column syllabuses jsonb not null default '[]';
alter table subjects drop constraint subjects_name_key;
alter table subjects add constraint subjects_level_name unique (level, name);

-- One merged tree per subject: WAEC's syllabus is the frame, and topics only
-- another body's syllabus has (e.g. JAMB) are added into it.
create table syllabus_topics (
  id uuid primary key default gen_random_uuid(),
  subject_id uuid not null references subjects(id),
  parent_id uuid references syllabus_topics(id),  -- a theme holds topics
  name text not null,
  position int not null default 0,
  bodies text[] not null,                         -- syllabuses that list it: WAEC, JAMB, ...
  unique nulls not distinct (subject_id, parent_id, name)
);

-- ── Exams ───────────────────────────────────────────────────────────────────

create table exam_bodies (
  id uuid primary key default gen_random_uuid(),
  code text not null unique,
  name text not null
);

-- One row per real exam, under its current name. WAEC's school certificate is
-- WASSCE for every year, including those when it was called SSCE.
create table exams (
  id uuid primary key default gen_random_uuid(),
  body_id uuid not null references exam_bodies(id),
  code text not null,
  name text not null,
  level text not null check (level in ('jss', 'sss')),
  unique (body_id, code)
);

insert into exam_bodies (code, name) values
  ('WAEC', 'West African Examinations Council'),
  ('NECO', 'National Examinations Council'),
  ('NABTEB', 'National Business and Technical Examinations Board'),
  ('JAMB', 'Joint Admissions and Matriculation Board');

insert into exams (body_id, code, name, level)
select b.id, e.code, e.name, e.level
from (values
  ('WAEC', 'WASSCE', 'West African Senior School Certificate Examination', 'sss'),
  ('NECO', 'SSCE', 'Senior School Certificate Examination', 'sss'),
  ('NECO', 'BECE', 'Basic Education Certificate Examination', 'jss'),
  ('NABTEB', 'NBC', 'National Business Certificate', 'sss'),
  ('JAMB', 'UTME', 'Unified Tertiary Matriculation Examination', 'sss')
) as e(body, code, name, level)
join exam_bodies b on b.code = e.body;

-- ── People ──────────────────────────────────────────────────────────────────

-- Authors of our own questions; becomes the user table when login arrives.
create table staff (
  id uuid primary key default gen_random_uuid(),
  name text not null,
  created_at timestamptz not null default now()
);

-- ── Sources and uploads ─────────────────────────────────────────────────────

-- A book or a single exam paper. The exam fields are defaults for anything its
-- pages don't print.
create table sources (
  id uuid primary key default gen_random_uuid(),
  kind text not null check (kind in ('book', 'paper')),
  layout text not null check (layout in ('by_topic', 'by_year', 'single_paper')),
  title text not null,
  publisher text,
  edition text,
  note text,
  subject_id uuid not null references subjects(id),
  exam_id uuid references exams(id),
  year int,
  paper text,
  sitting text check (sitting in ('may_june', 'june_july', 'nov_dec')),
  created_at timestamptz not null default now(),
  check ((kind = 'paper') = (layout = 'single_paper'))
);

-- One photo or PDF as uploaded; a PDF becomes one page per sheet.
create table uploads (
  id uuid primary key default gen_random_uuid(),
  source_id uuid not null references sources(id),
  kind text not null check (kind in ('photo', 'pdf')),
  file_key text not null,
  original_name text,
  uploaded_at timestamptz not null default now()
);

alter table pages
  add column source_id uuid references sources(id),
  add column upload_id uuid references uploads(id),
  -- Order within the source; a source's pages are read in this order.
  add column position int,
  -- Heading and exam/year in force at the bottom of the page, passed to the next.
  add column carried_context jsonb;

alter table pages
  alter column source_id set not null,
  alter column upload_id set not null,
  alter column position set not null,
  add constraint pages_source_position unique (source_id, position),
  drop column subject_id;

-- ── Questions ───────────────────────────────────────────────────────────────

alter table questions
  add column kind text not null default 'objective' check (kind in ('objective', 'theory')),
  add column origin text not null default 'source' check (origin in ('source', 'ours')),
  add column source_id uuid references sources(id),
  add column exam_id uuid references exams(id),
  add column year int,
  add column paper text,
  add column sitting text check (sitting in ('may_june', 'june_july', 'nov_dec')),
  -- The number in the exam itself, from tags like [2017/3]; `number` is as printed in the source.
  add column original_number text,
  add column topic_id uuid references syllabus_topics(id),
  add column marks numeric(5, 2),
  add column author_id uuid references staff(id);

alter table questions alter column kind drop default, alter column origin drop default;

-- Topics now come from the syllabus list; the printed heading is not kept.
drop index questions_position;
alter table questions
  drop column topic,
  alter column page_id drop not null,
  add constraint questions_origin check (
    (origin = 'source' and source_id is not null and page_id is not null and author_id is null)
    or (origin = 'ours' and author_id is not null and source_id is null and page_id is null));

-- Looking up a question by when it was set (see "One question, many sources").
create index questions_exam_key on questions (exam_id, subject_id, year, original_number);
create index questions_topic on questions (topic_id);

-- check_reasons gains: possible_duplicate (a near match the key couldn't settle)
-- and source_conflict (a matched source disagrees with the saved question).

-- Every place a question is printed. The first is also on the question itself.
create table question_appearances (
  id uuid primary key default gen_random_uuid(),
  question_id uuid not null references questions(id) on delete cascade,
  source_id uuid not null references sources(id),
  page_id uuid not null references pages(id),
  page_number text,                  -- as printed
  number text not null,              -- as printed in this source
  created_at timestamptz not null default now()
);
create index question_appearances_question on question_appearances(question_id);
create index question_appearances_page on question_appearances(page_id);
-- A page uploaded again must not add its questions twice.
create unique index question_appearances_position
  on question_appearances(source_id, page_number, number) where page_number is not null;

-- Theory questions: a tree of parts, e.g. 4 → (d) → (i), (ii).
create table question_parts (
  id uuid primary key default gen_random_uuid(),
  question_id uuid not null references questions(id) on delete cascade,
  parent_id uuid references question_parts(id) on delete cascade,
  label text not null,               -- "a", "ii"
  position int not null,
  body text,                         -- lead text
  closing_text text,                 -- e.g. "Give reasons for your answers in (d)(i) and (ii)."
  marks numeric(5, 2),
  diagram_id uuid references diagrams(id)
);
create index question_parts_question on question_parts(question_id);

-- Our own questions have no page.
alter table diagrams alter column page_id drop not null;
alter table question_groups alter column page_id drop not null;
