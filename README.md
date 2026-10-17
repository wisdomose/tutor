# Tutor

Past-question study app. This repo currently holds the **question converter**:
photograph a page of a printed past-question book, and Claude reads the
multiple-choice questions, answers them, redraws any diagrams as SVG, and saves
everything to Postgres. Uncertain answers and anything with a diagram are
flagged for a human to check on the review page.

| Path | What |
|---|---|
| `apps/api` | Rust API (axum, sqlx, utoipa) — upload, conversion worker, review, questions |
| `apps/web` | TanStack Start website — upload and review pages |
| `crates/db` | Postgres pool and migrations (`crates/db/migrations`) |
| `crates/claude` | Runs headless Claude Code with the owner's subscription token |
| `crates/storage` | Page photos: Cloudflare R2, or a local folder in development |
| `deploy/` | Server-side redeploy watcher |
| `docs/` | Product design notes |

## Develop

```sh
pnpm install
cp apps/api/.env.example apps/api/.env   # fill in DATABASE_URL
pnpm dev                                  # API on :8080, website on :3000
```

The API runs migrations at startup. `/health` reports the database, storage and
Claude. Locally Claude uses your signed-in Claude Code; in production it uses
`CLAUDE_CODE_OAUTH_TOKEN` from `claude setup-token`.

## Deploy

Pushing to `main` builds `ghcr.io/wisdomose/tutor-api` and `tutor-web` in GitHub
Actions. Nothing is built on the server: `deploy/coolify-watch.sh` runs from
cron there and asks Coolify to redeploy when an image's `:latest` changes.
