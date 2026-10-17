# Exam Past Questions App — Design Summary

A mobile study app for Nigerian students preparing for **WAEC, NECO and JAMB**, built on past questions. **Multiple choice only.**

> Update 2026-10-17: the question bank is **not** digital yet — it lives in printed hardback books. Photo ingestion (photo → structured questions → review → database) is the first piece of work. This resolves open item 1 below.

---

## 1. Product

- **Platform:** React Native (Expo), mobile.
- **Audience:** secondary school students (15–19), mostly mid-range Android phones, studying in short bursts. High stakes — JAMB score decides university admission.
- **Question bank:** already exists in digital form (format still to confirm).
- **First milestone:** browse by exam → subject → year → question view.
- **Design:** UI is being handled separately in Claude Design (brief in section 9).

---

## 2. Architecture

**Online-only for v1.** Content and user data both live server-side.

| Concern | Decision |
|---|---|
| Database | Postgres |
| Hosting | Existing VPS setup (Cloudflare Tunnel, systemd) |
| Images / diagrams | CDN, cached on first view |
| Math | LaTeX rendered with KaTeX |
| Auth | Better Auth — email OTP + social login |

**Tradeoff accepted:** online-only costs data on every view and risks losing a mock attempt on a dropped connection. In exchange it ships faster and content fixes (e.g. a miskeyed answer) go live instantly.

**Kept so offline stays cheap to add later:**
- UUIDs (not autoincrement integers) on sessions, answers and XP events.
- Fetch an **entire paper in one request** when a session starts and hold it in memory, rather than paging question by question. Survives brief drops and is the natural seam for a local cache later.

**Performance note:** runtime KaTeX inside WebViews is slow on low-end Android. Test pre-rendering expressions to SVG at import time.

---

## 3. Content schema

### Principles

1. **One row per appearance.** A question that recurs across years, exams or papers exists as a separate row each time, independently editable.
2. **Recycled questions are linked by an opaque `family_id`**, never by a pointer to a member row. Importing an older paper later (e.g. 2010 after starting at 2025) is a plain insert — nothing is rewritten, no stored id goes stale.
3. **Anything derived from ordering is queried, never stored.** Earliest occurrence = `MIN(year) WHERE family_id = ?`. Repeat count = `COUNT(*) GROUP BY family_id`.
4. **Subject is single-valued** per question. **Topics are many-to-many.**
5. **Correctness lives on the option**, not as a label on the question.

### Tables

```sql
exams(id, name)                         -- WAEC, NECO, JAMB
subjects(id, name)
topics(id, subject_id, parent_id, path, name)
question_families(id)                   -- opaque identity, no meaning

questions(
  id, family_id,                        -- family_id nullable until clustered
  exam_id, subject_id, year, paper, number,
  group_id,
  body, image_key, explanation, difficulty
)

question_groups(id, instruction, passage, image_key)
                                        -- comprehension passages, shared diagrams

question_topics(question_id, topic_id, is_primary)

options(id, question_id, label, body, is_correct)
```

### Constraints and indexes

```sql
-- A question's identity within the papers
UNIQUE (exam_id, subject_id, year, paper, number)

-- Browse path
INDEX (exam_id, subject_id, year)

-- Exactly one correct option per question
CREATE UNIQUE INDEX one_answer
  ON options(question_id) WHERE is_correct = true;

-- Topic lookups both ways
INDEX question_topics(topic_id, question_id)
INDEX question_topics(question_id, topic_id)
```

`paper` is part of the uniqueness key because the same question legitimately appears more than once in a year — WAEC May/June vs Nov/Dec GCE, and JAMB's multiple CBT session sets. Without it, imports collide and look like bad data.

### Why correctness is on `options`

- A stored label like `"B"` isn't enforced to exist and becomes meaningless once options are shuffled.
- `is_correct` plus the partial unique index catches "two right answers" or "no right answer" at insert time.
- Avoids the circular insert (question → options → update question) that an `answer_option_id` foreign key would need.
- Optional later: a `rationale` column on `options` explaining why a distractor is wrong.

### Topics

- **Scoped to subject** — every topic belongs to one subject, and a question's topics must match its subject.
- **`is_primary`** marks the main topic so counts don't double-count and ranking works.
- **Hierarchy kept shallow** (about two levels). Filtering a parent returns its children's questions; use a materialized `path` or closure table rather than recursive CTEs.
- **Names taken from the official JAMB/WAEC syllabus wording** — students recognise them, and it gives a coverage report of where the bank is thin.
- Multi-select topic filter defaults to **OR** with de-duplication.
- Tagging approach: batch LLM pass against the syllabus list (~85% accuracy expected), then spot-check by topic.

---

## 4. Query dimensions

Beyond exam / subject / year:

- **Topic / subtopic** — the most important; students revise by topic.
- **Repeat frequency** — via `family_id` ("appeared 5 times since 2008").
- **Full-text search** over question bodies.
- **Attempt state** — unattempted, previously wrong, bookmarked, due for review.
- **Difficulty** — manual at first, replaced by real percent-correct from attempt data.
- **Question shape** — calculation vs recall, diagram-based, passage-based vs standalone.
- **Content QA filters** — missing explanation, missing image (for cleaning the bank).
- **Constrained random sets** — e.g. 40 questions weighted across topics, for mocks.

---

## 5. User data — attempts and performance

```sql
sessions(
  id uuid, user_id, mode,               -- practice, mock, daily_challenge, review
  exam_id, subject_id, year,
  started_at, finished_at, duration_limit, config_json
)

answers(
  id uuid, session_id, question_id,
  selected_option_id,                   -- option id, NOT a label
  is_correct,                           -- denormalized, frozen at answer time
  answered_at, time_ms, changed_count
)

question_stats(                         -- derived, recomputable from answers
  user_id, question_id,
  attempts, correct, last_correct,
  last_answered_at, ease, due_at
)
```

- **Store `selected_option_id`**, so shuffling doesn't break history and you can see which distractor pulled students.
- **Keep every attempt; never overwrite.** Current state is derived.
- **`question_stats` is a cache, not truth** — always rebuildable from `answers`.
- **`is_correct` frozen on the answer row.** If an answer key is later corrected, flag affected rows via a QA pass.
- **Capture time per question from day one** — separates knowing from guessing.
- **Question ids must stay stable across content updates.** Never regenerate ids on import.

---

## 6. Gamification

### Principle

Every mechanic must push toward what actually raises the exam score. Retention without results is a product failure. **Nothing punishes a wrong answer**, because guessing is the correct strategy in these exams.

### Included

| Mechanic | Notes |
|---|---|
| **Streaks** | Low daily bar (~10 questions). One or two freezes per month. |
| **XP** | Weighted by difficulty. Getting a previously-wrong question right earns the most. Stops grinding easy questions. |
| **Levels** | Named, exam-flavoured titles rather than numbers. |
| **Topic mastery** | % mastery per syllabus topic. Both progress bar and diagnostic. |
| **Review queue** | Spaced repetition on wrong answers, shown as a daily count. Uses `question_stats.due_at`. |
| **Exam countdown** | "43 days to JAMB" with a readiness estimate. |
| **Predicted score** | e.g. "you'd score around 230 on JAMB", from accuracy weighted by topic coverage against the syllabus. The strongest motivator. |

### Excluded

- **Lives / hearts** — punish wrong answers, discourage guessing.
- **Combo streaks** — add pressure the real exam doesn't have.

### Deferred

- **Leaderboard** — planned later. When built: small ability-matched cohorts (~20), weekly reset, not global.
- Hook to prepare now — a daily rollup, which also feeds an activity heatmap:

```sql
daily_stats(user_id, date, xp, questions, correct, seconds)
```

Sync rollups, never raw answer rows, for leaderboards.

---

## 7. XP ledger

XP is stored as a ledger, not just a running total, so the formula can be tuned and past awards replayed or corrected.

```sql
xp_events(
  id uuid primary key,
  user_id uuid not null,
  source text not null,        -- answer, daily_challenge, streak_bonus, level_up
  ref_id uuid,                 -- the answer/session that produced it
  amount int not null,
  formula_version int not null,
  awarded_at timestamptz not null default now(),
  unique (user_id, source, ref_id)
)

user_xp_totals(user_id, total_xp, level, updated_at)
```

- **`formula_version`** — know which rule set produced each award; replay only what changed.
- **Unique `(user_id, source, ref_id)`** — idempotent awards. Retries and double-taps on flaky connections can't double-award.
- **`user_xp_totals` updated in the same transaction** as the insert — constant-time reads, ledger kept for repair and audit.
- Makes the future leaderboard auditable (e.g. clock-shifting to farm daily challenges).

---

## 8. Authentication — Better Auth

- **Email OTP + social login. No passwords** (`emailAndPassword` disabled) — no reset flow, no password support burden.
- Better Auth's **`account` table** is the identities table (one `user`, many provider accounts). Don't build a separate one.
- `xp_events.user_id`, `sessions.user_id` etc. reference Better Auth's `user.id`.

```ts
account: {
  accountLinking: {
    enabled: true,
    allowDifferentEmails: false,   // only link on matching verified email
    trustedProviders: ["google"],
  },
},
rateLimit: {
  enabled: true,
  storage: "database",
  customRules: {
    "/email-otp/send-verification-otp": { window: 30, max: 1 },
  },
},
```

- **`emailOTP` plugin** — implement `sendVerificationOTP` with your email sender. OTP codes avoid deep-link fragility and modern Android/iOS offer keyboard autofill for them.
- **Expo:** `expo()` server plugin, `expoClient` on the app with `expo-secure-store`, app scheme in `trustedOrigins`.
- **Apple sign-in is required on iOS** if Google sign-in ships.
- **Tight rate limit on the OTP send endpoint** — it's an email-cost attack surface.
- **No user enumeration** — same response whether or not an email exists.
- **Gate auth at first session start**, not at app open (forced sign-up at launch costs installs; XP and streaks need an account anyway).
- **Test:** a reported issue where linking a social account mid-session drops the existing session.

---

## 9. UI direction (handed to Claude Design)

- Gamified and energetic, not administrative — Duolingo / Brilliant feel.
- **Chunky pressable buttons with real depth:** darker base layer, face offset ~4px above, dropping onto the base on press. No soft shadows.
- Bold colour blocking; filled cards rather than white cards with thin borders.
- Rounded display typeface (Nunito, Baloo or similar). Radii 14–16px, large tap targets. Light and dark mode.
- Motion carries much of the "game" feel — XP bar filling, correct-answer spring, level-up moments (Reanimated).
- Tone: encouraging but not childish. A wrong answer is information, not failure. Celebrate streaks and level-ups, not individual questions.
- Timer only turns red near the end. Navigator as a horizontal strip around the current question, expandable to a full grid.

**Screens:** dashboard · exam/question screen · answer-revealed state · mock results · topic mastery.

Reference implementation for the pressable button:

```jsx
const OFFSET = 4;

<Pressable onPressIn={() => setDown(true)} onPressOut={() => setDown(false)}>
  <View style={{ backgroundColor: '#3C3489', borderRadius: 14 }}>
    <View style={{
      backgroundColor: '#534AB7',
      borderRadius: 14,
      paddingVertical: 16,
      alignItems: 'center',
      transform: [{ translateY: down ? 0 : -OFFSET }],
      marginBottom: down ? 0 : OFFSET,
    }}>
      <Text style={{ color: '#fff', fontWeight: '700', fontSize: 16 }}>
        Start challenge
      </Text>
    </View>
  </View>
</Pressable>
```

Use real question content (long Physics/Chemistry options, math, diagrams) when designing — option length is what breaks these layouts.

---

## 10. Open items

- [x] Format of the existing question bank — resolved 2026-10-17: printed books, ingested from photos.
- [ ] Whether `question_groups` carries its own exam/year/paper metadata or inherits it from its questions (awkward for a recycled comprehension passage).
- [ ] Test KaTeX pre-rendering to SVG vs runtime rendering on low-end Android.
- [ ] Test the Better Auth mid-session social-linking session drop.
- [ ] Clustering pass to populate `family_id` for recycled questions.
- [ ] Topic tagging of the bank against official syllabi.
- [ ] XP weights and level thresholds (first `formula_version`).
- [ ] Predicted-score model.

## Suggested next step

Write the import script once the bank's format is known: normalize into the schema above, enforce the uniqueness key and one-correct-option index, upload images to the CDN, and leave `family_id` and topics for follow-up passes.
