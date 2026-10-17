# Build progress — question converter

Handover notes: if a session stops mid-way, continue from the first unchecked
item. Every checked item is committed and pushed.

## Agreed workflow

1. Upload photos of book pages (subject chosen on upload, ≤ 20 per batch, resized
   in the browser).
2. **Claude** (subscription, `claude` CLI, `crates/claude`) reads each photo:
   questions, options, topic, printed page number, diagram descriptions, and
   questions split across pages.
3. **gpt-6.1-sol** and **gpt-6-luna** (OpenAI, `crates/openai`) each answer every
   question. Sol's answer + explanation is saved; Luna's is kept for reviewers.
4. **gpt-6.1-sol** redraws each diagram as SVG.
5. Flags (`questions.check_reasons`): `conflicting_answers` (Sol ≠ Luna),
   `has_diagram`, `low_confidence` (Sol said low), `no_answer` (answer matches no
   option). Shown on the review page as "Conflicting answers", "Has diagram",
   "Low confidence", "No answer".
6. Pages are processed one at a time. When Claude hits the Pro usage limit the
   queue pauses and resumes later on its own; it can also be paused by hand.

Decisions and test results: see the git history and `docs/design-summary.md`
(older product notes; the exam/year/paper fields there were dropped).
UI style: `docs/design/README.md`.

## Steps

- [x] Monorepo, CI → GHCR → Coolify, health checks (database, R2, Claude)
- [x] Sticker UI style and components
- [x] Migration 0002: second opinion, diagram review, page fragments, queue state, AI usage log
- [x] `crates/openai`: answering (Sol + Luna) and diagram drawing
- [x] Worker: page queue → Claude extraction → answers → flags → diagram queue; pause on usage limit; split-question joining
- [x] API routes: upload, pages, review, questions, diagrams, queue control + OpenAPI
- [ ] `packages/api-client` generated from OpenAPI
- [ ] Web: `/` upload, `/uploads`, `/uploads/:pageId`
- [ ] Web: `/review`, `/review/:questionId`
- [ ] Web: `/questions`, `/questions/:questionId`
- [ ] Deploy and test end to end with the two sample pages

## Notes from testing (two Biology sample pages, 51 questions)

- Answers are matched to questions by an `[n]` item tag, never by printed number:
  books misprint numbers (page 4 prints the arthropod question as a second "4"),
  and models sometimes reply "Question 2" instead of "2".
- A number repeated on one page is saved as "4 (2nd)" so it isn't dropped as a duplicate.
- Local end-to-end test: `docker run postgres:18-alpine` on :55432, run the API with
  `LOCAL_STORAGE_DIR`, upload with `curl -F subject_id=… -F photos=@page.jpg :8091/pages`.

## Environment (Coolify app `tutor-api`)

`DATABASE_URL`, `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`,
`R2_BUCKET`, `CLAUDE_CODE_OAUTH_TOKEN`, `OPENAI_API_KEY`, `WEB_ORIGIN`.
Website (`tutor-web`): `API_URL`.
