//! HTTP API for the upload and review website.

use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

use crate::AppState;

const MAX_PHOTOS: usize = 20;
const MAX_PHOTO_BYTES: usize = 8 * 1024 * 1024;

#[derive(OpenApi)]
#[openapi(
    info(title = "Tutor API"),
    paths(
        crate::health,
        list_subjects,
        queue_status,
        pause_queue,
        resume_queue,
        usage_summary,
        upload_pages,
        list_pages,
        get_page,
        retry_page,
        delete_page,
        review_queue,
        list_questions,
        list_topics,
        get_question,
        update_question,
        resolve_question,
        delete_question,
        redraw_diagram,
        accept_diagram,
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/subjects", get(list_subjects))
        .route("/queue", get(queue_status))
        .route("/queue/pause", post(pause_queue))
        .route("/queue/resume", post(resume_queue))
        .route("/usage", get(usage_summary))
        .route(
            "/pages",
            get(list_pages)
                .post(upload_pages)
                .layer(DefaultBodyLimit::max(
                    MAX_PHOTOS * MAX_PHOTO_BYTES + 1024 * 1024,
                )),
        )
        .route("/pages/{id}", get(get_page).delete(delete_page))
        .route("/pages/{id}/retry", post(retry_page))
        .route("/pages/{id}/photo", get(page_photo))
        .route("/review", get(review_queue))
        .route("/questions", get(list_questions))
        .route("/questions/topics", get(list_topics))
        .route(
            "/questions/{id}",
            get(get_question)
                .put(update_question)
                .delete(delete_question),
        )
        .route("/questions/{id}/resolve", post(resolve_question))
        .route("/diagrams/{id}/svg", get(diagram_svg))
        .route("/diagrams/{id}/redraw", post(redraw_diagram))
        .route("/diagrams/{id}/accept", post(accept_diagram))
}

// ── Errors ──────────────────────────────────────────────────────────────────

pub struct ApiError(StatusCode, String);

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(ErrorBody { error: self.1 })).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ApiError(StatusCode::NOT_FOUND, "not found".into()),
            e => {
                tracing::error!("database: {e}");
                ApiError(StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
            }
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!("{e:#}");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
    }
}

fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// Numeric-aware ordering for printed page and question numbers.
const QUESTION_ORDER: &str = "s.name,
  (case when q.page_number ~ '^[0-9]+$' then q.page_number::int end) nulls last,
  (case when q.number ~ '^[0-9]+$' then q.number::int end) nulls last, q.number, q.created_at";

// ── Subjects ────────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Subject {
    id: Uuid,
    name: String,
}

#[utoipa::path(get, path = "/subjects", responses((status = 200, body = Vec<Subject>)))]
async fn list_subjects(State(s): State<AppState>) -> ApiResult<Vec<Subject>> {
    Ok(Json(
        sqlx::query_as("select id, name from subjects order by name")
            .fetch_all(&s.db)
            .await?,
    ))
}

// ── Queue ───────────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct QueueStatus {
    /// Paused by hand.
    paused: bool,
    /// Claude's usage limit was hit; pages resume after this time.
    limited_until: Option<DateTime<Utc>>,
    limit_message: Option<String>,
    queued: i64,
    processing: i64,
    failed: i64,
    diagrams_pending: i64,
}

#[utoipa::path(get, path = "/queue", responses((status = 200, body = QueueStatus)))]
async fn queue_status(State(s): State<AppState>) -> ApiResult<QueueStatus> {
    Ok(Json(
        sqlx::query_as(
            "select q.paused,
                    case when q.limited_until > now() then q.limited_until end as limited_until,
                    case when q.limited_until > now() then q.limit_message end as limit_message,
                    (select count(*) from pages where status = 'queued') as queued,
                    (select count(*) from pages where status = 'processing') as processing,
                    (select count(*) from pages where status = 'failed') as failed,
                    (select count(*) from diagrams where status in ('queued', 'drawing')) as diagrams_pending
             from queue_state q",
        )
        .fetch_one(&s.db)
        .await?,
    ))
}

#[utoipa::path(post, path = "/queue/pause", responses((status = 200, body = QueueStatus)))]
async fn pause_queue(State(s): State<AppState>) -> ApiResult<QueueStatus> {
    sqlx::query("update queue_state set paused = true, updated_at = now()")
        .execute(&s.db)
        .await?;
    queue_status(State(s)).await
}

/// Resumes a hand pause and also retries straight away after a usage-limit pause.
#[utoipa::path(post, path = "/queue/resume", responses((status = 200, body = QueueStatus)))]
async fn resume_queue(State(s): State<AppState>) -> ApiResult<QueueStatus> {
    sqlx::query("update queue_state set paused = false, limited_until = null, limit_message = null, updated_at = now()")
        .execute(&s.db)
        .await?;
    queue_status(State(s)).await
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct UsageSummary {
    pages_done: i64,
    /// Charged to the OpenAI account.
    openai_usd: f64,
    /// What Claude's work would have cost on the API; it runs on the Claude subscription.
    claude_equivalent_usd: f64,
}

#[utoipa::path(get, path = "/usage", responses((status = 200, body = UsageSummary)))]
async fn usage_summary(State(s): State<AppState>) -> ApiResult<UsageSummary> {
    Ok(Json(
        sqlx::query_as(
            "select (select count(*) from pages where status = 'done') as pages_done,
                    coalesce(sum(cost_usd) filter (where provider = 'openai'), 0)::float8 as openai_usd,
                    coalesce(sum(cost_usd) filter (where provider = 'claude'), 0)::float8 as claude_equivalent_usd
             from ai_usage",
        )
        .fetch_one(&s.db)
        .await?,
    ))
}

// ── Pages ───────────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct PageSummary {
    id: Uuid,
    subject_id: Uuid,
    subject: String,
    page_number: Option<String>,
    /// queued, processing, done or failed.
    status: String,
    error: Option<String>,
    attempts: i32,
    next_attempt_at: Option<DateTime<Utc>>,
    uploaded_at: DateTime<Utc>,
    processed_at: Option<DateTime<Utc>>,
    question_count: i64,
    flagged_count: i64,
    /// The last question runs onto the next page, which hasn't been processed yet.
    waiting_for_next_page: bool,
}

const PAGE_SELECT: &str = "select p.id, p.subject_id, s.name as subject, p.page_number, p.status, p.error, p.attempts,
    p.next_attempt_at, p.uploaded_at, p.processed_at,
    (select count(*) from questions q where q.page_id = p.id) as question_count,
    (select count(*) from questions q where q.page_id = p.id and q.needs_check and q.resolved_at is null) as flagged_count,
    (p.tail_fragment is not null and not p.tail_joined) as waiting_for_next_page
  from pages p join subjects s on s.id = p.subject_id";

/// Upload up to 20 JPEG photos for one subject. Fields: `subject_id`, then one `photos` part per file.
#[utoipa::path(post, path = "/pages", request_body(content_type = "multipart/form-data"),
    responses((status = 200, body = Vec<PageSummary>), (status = 400, body = ErrorBody)))]
async fn upload_pages(
    State(s): State<AppState>,
    mut form: Multipart,
) -> ApiResult<Vec<PageSummary>> {
    let mut subject_id: Option<Uuid> = None;
    let mut photos = Vec::new();
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        match field.name() {
            Some("subject_id") => {
                let text = field.text().await.map_err(|e| bad_request(e.to_string()))?;
                subject_id = Some(
                    text.trim()
                        .parse()
                        .map_err(|_| bad_request("invalid subject_id"))?,
                );
            }
            Some("photos") => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(e.to_string()))?;
                if bytes.len() > MAX_PHOTO_BYTES {
                    return Err(bad_request("a photo is larger than 8 MB"));
                }
                if !bytes.starts_with(&[0xFF, 0xD8]) {
                    return Err(bad_request("photos must be JPEG"));
                }
                photos.push(bytes);
            }
            _ => {}
        }
    }
    let subject_id = subject_id.ok_or_else(|| bad_request("subject_id is required"))?;
    if photos.is_empty() || photos.len() > MAX_PHOTOS {
        return Err(bad_request(format!(
            "send between 1 and {MAX_PHOTOS} photos"
        )));
    }
    let exists: bool = sqlx::query_scalar("select exists(select 1 from subjects where id = $1)")
        .bind(subject_id)
        .fetch_one(&s.db)
        .await?;
    if !exists {
        return Err(bad_request("unknown subject"));
    }

    let mut ids = Vec::new();
    for photo in photos {
        let id = Uuid::new_v4();
        let key = format!("pages/{id}.jpg");
        s.storage.put(&key, photo).await?;
        sqlx::query("insert into pages (id, subject_id, photo_key) values ($1, $2, $3)")
            .bind(id)
            .bind(subject_id)
            .bind(&key)
            .execute(&s.db)
            .await?;
        ids.push(id);
    }
    Ok(Json(
        sqlx::query_as(&format!(
            "{PAGE_SELECT} where p.id = any($1) order by p.uploaded_at"
        ))
        .bind(&ids)
        .fetch_all(&s.db)
        .await?,
    ))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageFilter {
    subject_id: Option<Uuid>,
    /// queued, processing, done or failed.
    status: Option<String>,
    /// Default 50, at most 200.
    limit: Option<i64>,
}

#[utoipa::path(get, path = "/pages", params(PageFilter), responses((status = 200, body = Vec<PageSummary>)))]
async fn list_pages(
    State(s): State<AppState>,
    Query(f): Query<PageFilter>,
) -> ApiResult<Vec<PageSummary>> {
    Ok(Json(
        sqlx::query_as(&format!(
            "{PAGE_SELECT} where ($1::uuid is null or p.subject_id = $1) and ($2::text is null or p.status = $2)
             order by p.uploaded_at desc limit $3"
        ))
        .bind(f.subject_id)
        .bind(f.status)
        .bind(f.limit.unwrap_or(50).clamp(1, 200))
        .fetch_all(&s.db)
        .await?,
    ))
}

#[derive(Serialize, ToSchema)]
pub struct PageDetail {
    page: PageSummary,
    /// API path of the original photo.
    photo_url: String,
    /// The question this page ends with that runs onto the next page, until joined.
    tail_fragment: Option<String>,
    questions: Vec<QuestionSummary>,
}

#[utoipa::path(get, path = "/pages/{id}", responses((status = 200, body = PageDetail), (status = 404, body = ErrorBody)))]
async fn get_page(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<PageDetail> {
    let page: PageSummary = sqlx::query_as(&format!("{PAGE_SELECT} where p.id = $1"))
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    let tail_fragment: Option<String> = sqlx::query_scalar(
        "select tail_fragment->>'text' from pages where id = $1 and not tail_joined",
    )
    .bind(id)
    .fetch_one(&s.db)
    .await?;
    let questions = sqlx::query_as(&format!(
        "{QUESTION_SELECT} where q.page_id = $1 order by {QUESTION_ORDER}"
    ))
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(PageDetail {
        page,
        photo_url: format!("/pages/{id}/photo"),
        tail_fragment,
        questions,
    }))
}

async fn page_photo(State(s): State<AppState>, Path(id): Path<Uuid>) -> Result<Response, ApiError> {
    let key: String = sqlx::query_scalar("select photo_key from pages where id = $1")
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    let bytes = s.storage.get(&key).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        bytes,
    )
        .into_response())
}

/// Removes a page's questions, groups and diagrams (not the page or photo).
async fn clear_page(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("delete from questions where page_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("delete from question_groups where page_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("update ai_usage set diagram_id = null where diagram_id in (select id from diagrams where page_id = $1)")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("delete from diagrams where page_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Re-runs a page from scratch: its questions are deleted and it goes back in the queue.
#[utoipa::path(post, path = "/pages/{id}/retry", responses((status = 200, body = PageSummary)))]
async fn retry_page(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<PageSummary> {
    let mut tx = s.db.begin().await?;
    clear_page(&mut tx, id).await?;
    sqlx::query(
        "update pages set status = 'queued', attempts = 0, error = null, next_attempt_at = null,
           head_fragment = null, tail_fragment = null, tail_joined = false, processed_at = null
         where id = $1 and status <> 'processing'",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(
        sqlx::query_as(&format!("{PAGE_SELECT} where p.id = $1"))
            .bind(id)
            .fetch_one(&s.db)
            .await?,
    ))
}

#[utoipa::path(delete, path = "/pages/{id}", responses((status = 204)))]
async fn delete_page(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let key: String = sqlx::query_scalar("select photo_key from pages where id = $1")
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    let mut tx = s.db.begin().await?;
    clear_page(&mut tx, id).await?;
    sqlx::query("delete from pages where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    if let Err(e) = s.storage.delete(&key).await {
        tracing::warn!("deleting photo {key}: {e:#}");
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Questions ───────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct QuestionSummary {
    id: Uuid,
    subject_id: Uuid,
    subject: String,
    page_id: Uuid,
    page_number: Option<String>,
    number: String,
    topic: Option<String>,
    body: String,
    needs_check: bool,
    /// conflicting_answers, has_diagram, low_confidence, no_answer.
    check_reasons: Vec<String>,
    resolved_at: Option<DateTime<Utc>>,
}

const QUESTION_SELECT: &str =
    "select q.id, q.subject_id, s.name as subject, q.page_id, q.page_number, q.number, q.topic,
    q.body, q.needs_check, q.check_reasons, q.resolved_at
  from questions q join subjects s on s.id = q.subject_id";

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ReviewFilter {
    subject_id: Option<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct QuestionList {
    total: i64,
    items: Vec<QuestionSummary>,
}

/// Flagged questions not yet resolved, in book order.
#[utoipa::path(get, path = "/review", params(ReviewFilter), responses((status = 200, body = QuestionList)))]
async fn review_queue(
    State(s): State<AppState>,
    Query(f): Query<ReviewFilter>,
) -> ApiResult<QuestionList> {
    let items: Vec<QuestionSummary> = sqlx::query_as(&format!(
        "{QUESTION_SELECT} where q.needs_check and q.resolved_at is null and ($1::uuid is null or q.subject_id = $1)
         order by {QUESTION_ORDER}"
    ))
    .bind(f.subject_id)
    .fetch_all(&s.db)
    .await?;
    Ok(Json(QuestionList {
        total: items.len() as i64,
        items,
    }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct QuestionFilter {
    subject_id: Option<Uuid>,
    topic: Option<String>,
    page_number: Option<String>,
    /// Searches question and option text.
    q: Option<String>,
    /// Default 50, at most 200.
    limit: Option<i64>,
    offset: Option<i64>,
}

#[utoipa::path(get, path = "/questions", params(QuestionFilter), responses((status = 200, body = QuestionList)))]
async fn list_questions(
    State(s): State<AppState>,
    Query(f): Query<QuestionFilter>,
) -> ApiResult<QuestionList> {
    let filter = "where ($1::uuid is null or q.subject_id = $1)
        and ($2::text is null or q.topic = $2)
        and ($3::text is null or q.page_number = $3)
        and ($4::text is null or q.body ilike '%' || $4 || '%'
             or exists (select 1 from options o where o.question_id = q.id and o.body ilike '%' || $4 || '%'))";
    let search = f.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let total: i64 = sqlx::query_scalar(&format!("select count(*) from questions q {filter}"))
        .bind(f.subject_id)
        .bind(&f.topic)
        .bind(&f.page_number)
        .bind(search)
        .fetch_one(&s.db)
        .await?;
    let items = sqlx::query_as(&format!(
        "{QUESTION_SELECT} {filter} order by {QUESTION_ORDER} limit $5 offset $6"
    ))
    .bind(f.subject_id)
    .bind(&f.topic)
    .bind(&f.page_number)
    .bind(search)
    .bind(f.limit.unwrap_or(50).clamp(1, 200))
    .bind(f.offset.unwrap_or(0).max(0))
    .fetch_all(&s.db)
    .await?;
    Ok(Json(QuestionList { total, items }))
}

/// Distinct topics, optionally for one subject.
#[utoipa::path(get, path = "/questions/topics", params(ReviewFilter), responses((status = 200, body = Vec<String>)))]
async fn list_topics(
    State(s): State<AppState>,
    Query(f): Query<ReviewFilter>,
) -> ApiResult<Vec<String>> {
    Ok(Json(
        sqlx::query_scalar(
            "select distinct topic from questions where topic is not null and ($1::uuid is null or subject_id = $1)
             order by topic",
        )
        .bind(f.subject_id)
        .fetch_all(&s.db)
        .await?,
    ))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Diagram {
    id: Uuid,
    /// queued, drawing, done or failed.
    status: String,
    error: Option<String>,
    redraw_note: Option<String>,
    reviewed_at: Option<DateTime<Utc>>,
    /// API path of the SVG; present when status is done.
    #[sqlx(skip)]
    svg_url: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct OptionDetail {
    id: Uuid,
    label: String,
    body: Option<String>,
    is_correct: bool,
    diagram: Option<Diagram>,
}

#[derive(Serialize, ToSchema)]
pub struct QuestionDetail {
    #[serde(flatten)]
    summary: QuestionSummary,
    explanation: Option<String>,
    /// gpt-6.1-sol's confidence in the saved answer.
    confidence: Option<String>,
    /// gpt-6-luna's pick (an option label) and reasoning.
    second_answer: Option<String>,
    second_explanation: Option<String>,
    /// Shared instruction, e.g. "Study the diagram and answer questions 31 to 33".
    instruction: Option<String>,
    /// The question's own diagram, or the shared one from its group.
    diagram: Option<Diagram>,
    options: Vec<OptionDetail>,
    photo_url: String,
    /// Next flagged, unresolved question in book order (for "Looks right → next").
    next_flagged_id: Option<Uuid>,
}

async fn load_diagram(s: &AppState, id: Option<Uuid>) -> sqlx::Result<Option<Diagram>> {
    let Some(id) = id else { return Ok(None) };
    let d: Option<Diagram> = sqlx::query_as(
        "select id, status, error, redraw_note, reviewed_at from diagrams where id = $1",
    )
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    Ok(d.map(|mut d| {
        if d.status == "done" {
            d.svg_url = Some(format!(
                "/diagrams/{}/svg?v={}",
                d.id,
                d.reviewed_at.map_or(0, |t| t.timestamp())
            ));
        }
        d
    }))
}

#[derive(sqlx::FromRow)]
struct QuestionExtra {
    explanation: Option<String>,
    confidence: Option<String>,
    second_answer: Option<String>,
    second_explanation: Option<String>,
    instruction: Option<String>,
    diagram_id: Option<Uuid>,
}

#[derive(sqlx::FromRow)]
struct OptionRow {
    id: Uuid,
    label: String,
    body: Option<String>,
    is_correct: bool,
    diagram_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/questions/{id}", responses((status = 200, body = QuestionDetail), (status = 404, body = ErrorBody)))]
async fn get_question(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<QuestionDetail> {
    Ok(Json(question_detail(&s, id).await?))
}

async fn question_detail(s: &AppState, id: Uuid) -> Result<QuestionDetail, ApiError> {
    let summary: QuestionSummary = sqlx::query_as(&format!("{QUESTION_SELECT} where q.id = $1"))
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    let extra: QuestionExtra = sqlx::query_as(
        "select q.explanation, q.confidence, q.second_answer, q.second_explanation, g.instruction,
                coalesce(q.diagram_id, g.diagram_id) as diagram_id
         from questions q left join question_groups g on g.id = q.group_id where q.id = $1",
    )
    .bind(id)
    .fetch_one(&s.db)
    .await?;
    let rows: Vec<OptionRow> = sqlx::query_as(
        "select id, label, body, is_correct, diagram_id from options where question_id = $1 order by label",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    let mut options = Vec::new();
    for o in rows {
        options.push(OptionDetail {
            id: o.id,
            label: o.label,
            body: o.body,
            is_correct: o.is_correct,
            diagram: load_diagram(s, o.diagram_id).await?,
        });
    }
    let next_flagged_id: Option<Uuid> = sqlx::query_scalar(&format!(
        "with ordered as (
           select q.id, row_number() over (order by {QUESTION_ORDER}) as n
           from questions q join subjects s on s.id = q.subject_id
           where (q.needs_check and q.resolved_at is null) or q.id = $1)
         select id from ordered
         where n > (select n from ordered where id = $1)
         order by n limit 1"
    ))
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    let photo_url = format!("/pages/{}/photo", summary.page_id);
    Ok(QuestionDetail {
        summary,
        explanation: extra.explanation,
        confidence: extra.confidence,
        second_answer: extra.second_answer,
        second_explanation: extra.second_explanation,
        instruction: extra.instruction,
        diagram: load_diagram(s, extra.diagram_id).await?,
        options,
        photo_url,
        next_flagged_id,
    })
}

#[derive(Deserialize, ToSchema)]
pub struct OptionEdit {
    id: Uuid,
    body: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct QuestionEdit {
    body: String,
    topic: Option<String>,
    explanation: Option<String>,
    options: Vec<OptionEdit>,
    /// The option to mark correct; null leaves the question without an answer.
    correct_option_id: Option<Uuid>,
    /// Also mark the question as checked.
    resolve: bool,
}

/// Saves a reviewer's corrections.
#[utoipa::path(put, path = "/questions/{id}", request_body = QuestionEdit,
    responses((status = 200, body = QuestionDetail), (status = 400, body = ErrorBody)))]
async fn update_question(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(edit): Json<QuestionEdit>,
) -> ApiResult<QuestionDetail> {
    if edit.body.trim().is_empty() {
        return Err(bad_request("the question text can't be empty"));
    }
    let mut tx = s.db.begin().await?;
    sqlx::query(
        "update questions set body = $2, topic = $3, explanation = $4,
           resolved_at = case when $5 then now() else resolved_at end
         where id = $1",
    )
    .bind(id)
    .bind(edit.body.trim())
    .bind(
        edit.topic
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty()),
    )
    .bind(
        edit.explanation
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty()),
    )
    .bind(edit.resolve)
    .execute(&mut *tx)
    .await?;
    for o in &edit.options {
        sqlx::query(
            "update options set body = $3 where id = $1 and question_id = $2
               and ($3 is not null or diagram_id is not null)",
        )
        .bind(o.id)
        .bind(id)
        .bind(o.body.as_deref().map(str::trim))
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("update options set is_correct = false where question_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if let Some(correct) = edit.correct_option_id {
        let n =
            sqlx::query("update options set is_correct = true where id = $1 and question_id = $2")
                .bind(correct)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        if n.rows_affected() == 0 {
            return Err(bad_request(
                "correct_option_id is not an option of this question",
            ));
        }
    }
    tx.commit().await?;
    Ok(Json(question_detail(&s, id).await?))
}

#[utoipa::path(post, path = "/questions/{id}/resolve", responses((status = 200, body = QuestionDetail)))]
async fn resolve_question(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<QuestionDetail> {
    sqlx::query("update questions set resolved_at = now() where id = $1")
        .bind(id)
        .execute(&s.db)
        .await?;
    Ok(Json(question_detail(&s, id).await?))
}

#[utoipa::path(delete, path = "/questions/{id}", responses((status = 204)))]
async fn delete_question(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    sqlx::query("delete from questions where id = $1")
        .bind(id)
        .execute(&s.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Diagrams ────────────────────────────────────────────────────────────────

/// The SVG itself, locked down so it can never run script even if opened directly.
async fn diagram_svg(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let svg: Option<String> = sqlx::query_scalar("select svg from diagrams where id = $1")
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    let svg = svg.ok_or(ApiError(StatusCode::NOT_FOUND, "not drawn yet".into()))?;
    let mut res = svg.into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60"),
    );
    Ok(res)
}

#[derive(Deserialize, ToSchema)]
pub struct RedrawRequest {
    /// What was wrong, e.g. "label III should point to the rhizoids".
    note: Option<String>,
}

#[utoipa::path(post, path = "/diagrams/{id}/redraw", request_body = RedrawRequest, responses((status = 200, body = Diagram)))]
async fn redraw_diagram(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<RedrawRequest>,
) -> ApiResult<Diagram> {
    sqlx::query(
        "update diagrams set status = 'queued', redraw_note = $2, error = null, reviewed_at = null
         where id = $1 and status <> 'drawing'",
    )
    .bind(id)
    .bind(req.note.as_deref().map(str::trim).filter(|n| !n.is_empty()))
    .execute(&s.db)
    .await?;
    load_diagram(&s, Some(id))
        .await?
        .map(Json)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not found".into()))
}

#[utoipa::path(post, path = "/diagrams/{id}/accept", responses((status = 200, body = Diagram)))]
async fn accept_diagram(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Diagram> {
    sqlx::query("update diagrams set reviewed_at = now() where id = $1 and status = 'done'")
        .bind(id)
        .execute(&s.db)
        .await?;
    load_diagram(&s, Some(id))
        .await?
        .map(Json)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "not found".into()))
}
