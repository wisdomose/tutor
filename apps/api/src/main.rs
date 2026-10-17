//! Tutor API. For now: health checks proving the database, photo storage and
//! Claude are wired up, plus the subject list and the OpenAPI document the
//! TypeScript client is generated from.
//!
//! `tutor-api openapi` prints the OpenAPI document and exits.

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
use tutor_storage::Storage;
use utoipa::{OpenApi, ToSchema};

#[derive(OpenApi)]
#[openapi(info(title = "Tutor API"), paths(health, list_subjects))]
struct ApiDoc;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    /// Outcome of the startup checks that are too slow or costly to run per request.
    checks: Arc<RwLock<StartupChecks>>,
}

#[derive(Clone, Serialize, ToSchema)]
struct StartupChecks {
    storage: String,
    claude: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("openapi") {
        println!("{}", ApiDoc::openapi().to_pretty_json()?);
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

    let storage = Storage::from_env()?;
    let checks = Arc::new(RwLock::new(StartupChecks {
        storage: "pending".into(),
        claude: "pending".into(),
    }));
    tokio::spawn(run_startup_checks(storage.clone(), checks.clone()));

    let cors = match std::env::var("WEB_ORIGIN") {
        Ok(origin) => CorsLayer::new().allow_origin([origin.parse()?]),
        Err(_) => CorsLayer::permissive(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/subjects", get(list_subjects))
        .route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(AppState { db, checks });

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
async fn run_startup_checks(storage: Storage, checks: Arc<RwLock<StartupChecks>>) {
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
    checks.write().await.storage = storage_status;

    let claude = Claude::from_env();
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ok"],
        "properties": { "ok": { "type": "boolean" } }
    });
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
    checks.write().await.claude = claude_status;
}

#[derive(Serialize, ToSchema)]
struct Health {
    database: String,
    storage: String,
    claude: String,
}

/// Database is checked live; storage and Claude report their startup checks.
#[utoipa::path(get, path = "/health", responses((status = 200, body = Health), (status = 503, body = Health)))]
async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let database = match sqlx::query("select 1").execute(&state.db).await {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("error: {e}"),
    };
    let checks = state.checks.read().await.clone();
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
        }),
    )
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
struct Subject {
    id: uuid::Uuid,
    name: String,
}

#[utoipa::path(get, path = "/subjects", responses((status = 200, body = Vec<Subject>)))]
async fn list_subjects(State(state): State<AppState>) -> Result<Json<Vec<Subject>>, StatusCode> {
    sqlx::query_as::<_, Subject>("select id, name from subjects order by name")
        .fetch_all(&state.db)
        .await
        .map(Json)
        .map_err(|e| {
            tracing::error!("listing subjects: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}
