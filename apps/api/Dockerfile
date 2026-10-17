# Tutor API. Build context: repository root (Cargo workspace).
FROM rust:1.97-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS plan
COPY Cargo.toml Cargo.lock ./
COPY apps/api apps/api
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json --bin tutor-api

FROM chef AS build
COPY --from=plan /app/recipe.json recipe.json
# Dependencies only, cached until Cargo.lock changes.
RUN cargo chef cook --release --recipe-path recipe.json --bin tutor-api
COPY Cargo.toml Cargo.lock ./
COPY apps/api apps/api
COPY crates crates
RUN cargo build --release --bin tutor-api

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* && useradd --system --create-home --uid 10001 app
USER app
# Claude Code CLI: the converter runs it with the owner's subscription token
# (CLAUDE_CODE_OAUTH_TOKEN, from `claude setup-token`). Pinned; bump deliberately.
ARG CLAUDE_CODE_VERSION=2.1.302
RUN curl -fsSL https://claude.ai/install.sh | bash -s ${CLAUDE_CODE_VERSION}
ENV PATH=/home/app/.local/bin:$PATH DISABLE_AUTOUPDATER=1
COPY --from=build /app/target/release/tutor-api /usr/local/bin/tutor-api
ENV PORT=8080 RUST_LOG=tutor_api=info,tower_http=info
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s CMD curl -fs http://localhost:8080/health || exit 1
CMD ["tutor-api"]
