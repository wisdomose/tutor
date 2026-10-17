-- Converter pipeline: second-opinion answers, diagram review, questions split
-- across pages, queue control and an AI usage log.

-- gpt-6-luna's answer, kept beside gpt-6.1-sol's (the saved one) for reviewers.
alter table questions
  add column second_answer text,
  add column second_explanation text;

-- check_reasons values: conflicting_answers, has_diagram, low_confidence, no_answer.

alter table diagrams
  add column reviewed_at timestamptz;

alter table pages
  -- Text at the top of the page that continues a question from the previous page.
  add column head_fragment text,
  -- A question that starts at the bottom of this page and runs onto the next one.
  add column tail_fragment jsonb,
  add column tail_joined boolean not null default false,
  -- When a failed or usage-limited page may be tried again.
  add column next_attempt_at timestamptz;

-- Single row controlling the processing queue.
create table queue_state (
  id boolean primary key default true check (id),
  paused boolean not null default false,       -- paused by hand
  limited_until timestamptz,                   -- Claude usage limit hit; resume after this
  limit_message text,
  updated_at timestamptz not null default now()
);
insert into queue_state default values;

-- What each AI call cost, so spend per page and per book can be seen.
create table ai_usage (
  id uuid primary key default gen_random_uuid(),
  page_id uuid references pages(id) on delete set null,
  diagram_id uuid references diagrams(id) on delete set null,
  provider text not null,                     -- claude, openai
  model text not null,
  purpose text not null,                      -- extract, answer, draw, join
  input_tokens int,
  output_tokens int,
  cost_usd numeric(10, 6),                    -- API price; for Claude, the subscription equivalent
  created_at timestamptz not null default now()
);
create index ai_usage_page on ai_usage(page_id);
