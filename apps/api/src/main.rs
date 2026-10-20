//! Tutor API: the question converter. Photos uploaded through the website are
//! read by Claude, answered by gpt-6.1-sol and gpt-6-luna, and their diagrams
//! redrawn by gpt-6.1-sol (see `worker`); the routes in `routes` serve the
//! upload and review website.
//!
//! `tutor-api openapi` prints the OpenAPI document and exits.

mod matcher;
mod prompts;
mod routes;
mod worker;

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tutor_claude::Claude;
use tutor_openai::OpenAi;
use tutor_storage::Storage;
use utoipa::{OpenApi, ToSchema};

#[derive(Clone)]
pub struct AppState {
    db: PgPool,
    storage: Storage,
    claude: Arc<Claude>,
    /// None when OPENAI_API_KEY is missing; pages then fail with that message.
    openai: Option<OpenAi>,
    /// Outcome of the startup checks that are too slow or costly to run per request.
    checks: Arc<RwLock<StartupChecks>>,
}

#[derive(Clone)]
struct StartupChecks {
    storage: String,
    claude: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("openapi") {
        println!("{}", routes::ApiDoc::openapi().to_pretty_json()?);
        return Ok(());
    }

    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tutor_api=info,tower_http=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = tutor_db::connect(&database_url).await?;
    tutor_db::MIGRATOR.run(&db).await?;

    let openai = match OpenAi::from_env() {
        Ok(client) => Some(client),
        Err(e) => {
            tracing::error!("{e:#}; questions can't be answered until it is set");
            None
        }
    };
    let state = AppState {
        db,
        storage: Storage::from_env()?,
        claude: Arc::new(Claude::from_env()),
        openai,
        checks: Arc::new(RwLock::new(StartupChecks {
            storage: "pending".into(),
            claude: "pending".into(),
        })),
    };
    tokio::spawn(run_startup_checks(state.clone()));
    worker::spawn(state.clone());

    let cors = match std::env::var("WEB_ORIGIN") {
        Ok(origin) => CorsLayer::new()
            .allow_origin([origin.parse()?])
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any),
        Err(_) => CorsLayer::permissive(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route(
            "/openapi.json",
            get(|| async { Json(routes::ApiDoc::openapi()) }),
        )
        .merge(routes::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("listening on {port}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// Round-trips a small object through storage and asks Claude for a one-word
/// JSON reply, so a deploy shows at once whether R2 and the subscription token work.
async fn run_startup_checks(state: AppState) {
    let storage = &state.storage;
    let key = "healthcheck/startup.txt";
    let storage_result = async {
        storage.put(key, "ok".into()).await?;
        let read = storage.get(key).await?;
        storage.delete(key).await?;
        anyhow::ensure!(read.as_ref() == b"ok", "read back different bytes");
        Ok(())
    };
    let storage_status = match storage_result.await {
        Ok(()) => format!("ok ({})", storage.backend),
        Err(e) => format!("error ({}): {e:#}", storage.backend),
    };
    state.checks.write().await.storage = storage_status;

    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ok"],
        "properties": { "ok": { "type": "boolean" } }
    });
    let claude = &state.claude;
    let claude_status = match claude.version().await {
        Err(e) => format!("error: {e:#}"),
        Ok(version) => match claude
            .run(tutor_claude::Request {
                system: "You are a health check. Reply with ok set to true.",
                prompt: "Health check.",
                image: None,
                schema: &schema,
                effort: "low",
            })
            .await
        {
            Ok(r) if r.output["ok"] == true => format!("ok ({version})"),
            Ok(r) => format!("unexpected reply ({version}): {}", r.output),
            Err(e) => format!("error ({version}): {e:#}"),
        },
    };
    tracing::info!(%claude_status, "claude startup check");
    state.checks.write().await.claude = claude_status;
}

#[derive(Serialize, ToSchema)]
pub struct Health {
    database: String,
    storage: String,
    claude: String,
    openai: String,
}

/// Database is checked live; storage and Claude report their startup checks.
#[utoipa::path(get, path = "/health", responses((status = 200, body = Health), (status = 503, body = Health)))]
async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let database = match sqlx::query("select 1").execute(&state.db).await {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error: {e}"),
    };
    let checks = state.checks.read().await.clone();
    let openai = if state.openai.is_some() {
        "ok (key set)"
    } else {
        "error: OPENAI_API_KEY is not set"
    };
    let status = if database == "ok" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(Health {
            database,
            storage: checks.storage,
            claude: checks.claude,
            openai: openai.into(),
        }),
    )
}
