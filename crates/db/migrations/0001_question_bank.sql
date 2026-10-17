-- Question bank built from photographed past-question books.
-- One book per subject, so "subject + printed page" locates any question.

create table subjects (
  id uuid primary key default gen_random_uuid(),
  name text not null unique
);

-- One uploaded photo of one book page.
create table pages (
  id uuid primary key default gen_random_uuid(),
  subject_id uuid not null references subjects(id),
  photo_key text not null,                  -- object key in R2
  page_number text,                         -- number printed on the paper, read by Claude
  status text not null default 'queued'
    check (status in ('queued', 'processing', 'done', 'failed')),
  error text,
  attempts int not null default 0,
  uploaded_at timestamptz not null default now(),
  processed_at timestamptz
);
create index pages_queue on pages(status, uploaded_at);

-- Every AI-drawn diagram, whether it belongs to a group, a question or an option.
create table diagrams (
  id uuid primary key default gen_random_uuid(),
  page_id uuid not null references pages(id),  -- page it was drawn from
  description text not null,                   -- what Claude saw; drives the drawing
  svg text,
  status text not null default 'queued'
    check (status in ('queued', 'drawing', 'done', 'failed')),
  redraw_note text,                            -- reviewer's correction for the next attempt
  error text,
  created_at timestamptz not null default now()
);
create index diagrams_queue on diagrams(status, created_at);

-- Shared instruction/diagram ("Study it and answer questions 31 to 33").
create table question_groups (
  id uuid primary key default gen_random_uuid(),
  page_id uuid not null references pages(id),
  instruction text,
  diagram_id uuid references diagrams(id)
);

create table questions (
  id uuid primary key default gen_random_uuid(),
  subject_id uuid not null references subjects(id),
  page_id uuid not null references pages(id),  -- the page the question starts on
  group_id uuid references question_groups(id),
  diagram_id uuid references diagrams(id),     -- a diagram for this question only
  page_number text,                            -- copied from pages, so duplicates can be rejected here
  number text not null,                        -- as printed
  topic text,                                  -- section heading it sits under
  body text not null,
  explanation text,
  confidence text check (confidence in ('high', 'medium', 'low')),
  needs_check boolean not null default false,
  check_reasons text[] not null default '{}',
  resolved_at timestamptz,
  created_at timestamptz not null default now()
);
create index questions_subject on questions(subject_id);
create index questions_needs_check on questions(subject_id, created_at) where needs_check and resolved_at is null;

-- Re-uploading a page must not duplicate its questions.
create unique index questions_position on questions(subject_id, page_number, number)
  where page_number is not null;

create table options (
  id uuid primary key default gen_random_uuid(),
  question_id uuid not null references questions(id) on delete cascade,
  label text not null,
  body text,
  diagram_id uuid references diagrams(id),
  is_correct boolean not null default false,
  unique (question_id, label),
  check (body is not null or diagram_id is not null)
);
create unique index one_answer on options(question_id) where is_correct;

insert into subjects (name) values
  ('Biology'), ('Chemistry'), ('Physics'), ('Mathematics'), ('English Language'),
  ('Economics'), ('Government'), ('Geography'), ('Agricultural Science'),
  ('Literature in English'), ('Commerce'), ('Further Mathematics');
