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
const MAX_PDF_BYTES: usize = 200 * 1024 * 1024;

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
        list_exams,
        list_sources,
        create_source,
        upload_pdf,
        get_question,
        update_question,
        resolve_question,
        merge_question,
        delete_question,
        update_group,
        redraw_diagram,
        accept_diagram,
    )
)]
pub struct ApiDoc;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/subjects", get(list_subjects))
        .route("/topics", get(list_topics))
        .route("/exams", get(list_exams))
        .route("/sources", get(list_sources).post(create_source))
        .route(
            "/uploads/pdf",
            post(upload_pdf).layer(DefaultBodyLimit::max(MAX_PDF_BYTES + 1024 * 1024)),
        )
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
        .route(
            "/questions/{id}",
            get(get_question)
                .put(update_question)
                .delete(delete_question),
        )
        .route("/questions/{id}/resolve", post(resolve_question))
        .route("/questions/{id}/merge", post(merge_question))
        .route("/groups/{id}", axum::routing::put(update_group))
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
const QUESTION_ORDER: &str = "s.name, src.title nulls last,
  (case when q.page_number ~ '^[0-9]+$' then q.page_number::int end) nulls last,
  (case when q.number ~ '^[0-9]+$' then q.number::int end) nulls last, q.number, q.created_at";

// ── Subjects ────────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Subject {
    id: Uuid,
    /// jss or sss.
    level: String,
    name: String,
}

#[utoipa::path(get, path = "/subjects", responses((status = 200, body = Vec<Subject>)))]
async fn list_subjects(State(s): State<AppState>) -> ApiResult<Vec<Subject>> {
    Ok(Json(
        sqlx::query_as("select id, level, name from subjects order by level desc, name")
            .fetch_all(&s.db)
            .await?,
    ))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Topic {
    id: Uuid,
    subject_id: Uuid,
    /// The theme this topic sits under, if any.
    parent_id: Option<Uuid>,
    name: String,
    position: i32,
    /// Syllabuses that list it: WAEC, JAMB, ...
    bodies: Vec<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SubjectFilter {
    subject_id: Option<Uuid>,
}

/// Syllabus topics, optionally for one subject, in syllabus order.
#[utoipa::path(get, path = "/topics", params(SubjectFilter), responses((status = 200, body = Vec<Topic>)))]
async fn list_topics(
    State(s): State<AppState>,
    Query(f): Query<SubjectFilter>,
) -> ApiResult<Vec<Topic>> {
    Ok(Json(
        sqlx::query_as(
            "select id, subject_id, parent_id, name, position, bodies from syllabus_topics
             where $1::uuid is null or subject_id = $1
             order by subject_id, position, name",
        )
        .bind(f.subject_id)
        .fetch_all(&s.db)
        .await?,
    ))
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Exam {
    id: Uuid,
    /// WAEC, NECO, NABTEB or JAMB.
    body: String,
    /// WASSCE, SSCE, BECE, NBC or UTME.
    code: String,
    name: String,
    level: String,
}

#[utoipa::path(get, path = "/exams", responses((status = 200, body = Vec<Exam>)))]
async fn list_exams(State(s): State<AppState>) -> ApiResult<Vec<Exam>> {
    Ok(Json(
        sqlx::query_as(
            "select e.id, b.code as body, e.code, e.name, e.level
             from exams e join exam_bodies b on b.id = e.body_id order by b.code, e.code",
        )
        .fetch_all(&s.db)
        .await?,
    ))
}

// ── Sources ─────────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Source {
    id: Uuid,
    /// book or paper.
    kind: String,
    /// by_topic, by_year or single_paper.
    layout: String,
    title: String,
    publisher: Option<String>,
    edition: Option<String>,
    note: Option<String>,
    subject_id: Uuid,
    subject: String,
    level: String,
    /// Defaults for anything the pages don't print.
    exam_id: Option<Uuid>,
    year: Option<i32>,
    paper: Option<String>,
    /// may_june, june_july or nov_dec.
    sitting: Option<String>,
    page_count: i64,
    created_at: DateTime<Utc>,
}

const SOURCE_SELECT: &str =
    "select src.id, src.kind, src.layout, src.title, src.publisher, src.edition, src.note,
    src.subject_id, s.name as subject, s.level, src.exam_id, src.year, src.paper,
    src.sitting, (select count(*) from pages p where p.source_id = src.id) as page_count,
    src.created_at
  from sources src join subjects s on s.id = src.subject_id";

#[utoipa::path(get, path = "/sources", params(SubjectFilter), responses((status = 200, body = Vec<Source>)))]
async fn list_sources(
    State(s): State<AppState>,
    Query(f): Query<SubjectFilter>,
) -> ApiResult<Vec<Source>> {
    Ok(Json(
        sqlx::query_as(&format!(
            "{SOURCE_SELECT} where $1::uuid is null or src.subject_id = $1 order by src.title"
        ))
        .bind(f.subject_id)
        .fetch_all(&s.db)
        .await?,
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct NewSource {
    /// book or paper.
    kind: String,
    /// by_topic or by_year for a book; single_paper for a paper.
    layout: String,
    title: String,
    publisher: Option<String>,
    edition: Option<String>,
    note: Option<String>,
    subject_id: Uuid,
    /// Required for a paper; for a book, the exam all its questions come from, if just one.
    exam_id: Option<Uuid>,
    /// Required for a paper.
    year: Option<i32>,
    paper: Option<String>,
    /// may_june, june_july or nov_dec, as printed; leave out if the source doesn't say.
    sitting: Option<String>,
}

/// Trims text and turns blanks into none.
fn clean(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

#[utoipa::path(post, path = "/sources", request_body = NewSource,
    responses((status = 200, body = Source), (status = 400, body = ErrorBody)))]
async fn create_source(State(s): State<AppState>, Json(n): Json<NewSource>) -> ApiResult<Source> {
    let title = n.title.trim();
    if title.is_empty() {
        return Err(bad_request("a source needs a title"));
    }
    match (n.kind.as_str(), n.layout.as_str()) {
        ("book", "by_topic" | "by_year") | ("paper", "single_paper") => {}
        ("book" | "paper", _) => {
            return Err(bad_request("that layout doesn't fit that kind of source"));
        }
        _ => return Err(bad_request("kind must be book or paper")),
    }
    if n.kind == "paper" && (n.exam_id.is_none() || n.year.is_none()) {
        return Err(bad_request("a paper needs its exam and year"));
    }
    if let Some(year) = n.year
        && !(1950..=2100).contains(&year)
    {
        return Err(bad_request("that year doesn't look right"));
    }
    let sitting = clean(n.sitting);
    if let Some(sitting) = &sitting
        && !["may_june", "june_july", "nov_dec"].contains(&sitting.as_str())
    {
        return Err(bad_request(
            "sitting must be may_june, june_july or nov_dec",
        ));
    }
    let subject_level: Option<String> =
        sqlx::query_scalar("select level from subjects where id = $1")
            .bind(n.subject_id)
            .fetch_optional(&s.db)
            .await?;
    let Some(subject_level) = subject_level else {
        return Err(bad_request("unknown subject"));
    };
    if let Some(exam_id) = n.exam_id {
        let exam_level: Option<String> =
            sqlx::query_scalar("select level from exams where id = $1")
                .bind(exam_id)
                .fetch_optional(&s.db)
                .await?;
        match exam_level {
            None => return Err(bad_request("unknown exam")),
            Some(level) if level != subject_level => {
                return Err(bad_request(
                    "that exam is for a different level than the subject",
                ));
            }
            Some(_) => {}
        }
    }
    let id: Uuid = sqlx::query_scalar(
        "insert into sources (kind, layout, title, publisher, edition, note, subject_id, exam_id, year, paper, sitting)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) returning id",
    )
    .bind(&n.kind)
    .bind(&n.layout)
    .bind(title)
    .bind(clean(n.publisher))
    .bind(clean(n.edition))
    .bind(clean(n.note))
    .bind(n.subject_id)
    .bind(n.exam_id)
    .bind(n.year)
    .bind(clean(n.paper))
    .bind(sitting)
    .fetch_one(&s.db)
    .await?;
    Ok(Json(
        sqlx::query_as(&format!("{SOURCE_SELECT} where src.id = $1"))
            .bind(id)
            .fetch_one(&s.db)
            .await?,
    ))
}

#[derive(Serialize, ToSchema)]
pub struct PdfUpload {
    /// Send with the PDF's rendered pages to `POST /pages`.
    upload_id: Uuid,
}

/// Registers a PDF of one source. Its pages are rendered by the browser and sent to
/// `POST /pages` with this upload's id. Fields: `source_id`, then `pdf` (the file, kept
/// as uploaded) or just `name` for a PDF too large to send in one request.
#[utoipa::path(post, path = "/uploads/pdf", request_body(content_type = "multipart/form-data"),
    responses((status = 200, body = PdfUpload), (status = 400, body = ErrorBody)))]
async fn upload_pdf(State(s): State<AppState>, mut form: Multipart) -> ApiResult<PdfUpload> {
    let mut source_id: Option<Uuid> = None;
    let mut pdf = None;
    let mut name_only: Option<String> = None;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        match field.name() {
            Some("source_id") => source_id = Some(parse_uuid_field(field, "source_id").await?),
            Some("pdf") => {
                let name = field.file_name().map(str::to_owned);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(e.to_string()))?;
                if !bytes.starts_with(b"%PDF-") {
                    return Err(bad_request("that file isn't a PDF"));
                }
                pdf = Some((name, bytes));
            }
            Some("name") => {
                name_only = Some(field.text().await.map_err(|e| bad_request(e.to_string()))?);
            }
            _ => {}
        }
    }
    let source_id = source_id.ok_or_else(|| bad_request("source_id is required"))?;
    ensure_source(&s, source_id).await?;
    let id = Uuid::new_v4();
    // An empty key means the original wasn't kept, only its pages.
    let (name, key) = match pdf {
        Some((name, bytes)) => {
            let key = format!("uploads/{id}.pdf");
            s.storage.put(&key, bytes).await?;
            (name, key)
        }
        None => (
            Some(name_only.ok_or_else(|| bad_request("send pdf or name"))?),
            String::new(),
        ),
    };
    sqlx::query("insert into uploads (id, source_id, kind, file_key, original_name) values ($1, $2, 'pdf', $3, $4)")
        .bind(id)
        .bind(source_id)
        .bind(&key)
        .bind(name)
        .execute(&s.db)
        .await?;
    Ok(Json(PdfUpload { upload_id: id }))
}

async fn parse_uuid_field(
    field: axum::extract::multipart::Field<'_>,
    name: &str,
) -> Result<Uuid, ApiError> {
    let text = field.text().await.map_err(|e| bad_request(e.to_string()))?;
    text.trim()
        .parse()
        .map_err(|_| bad_request(format!("invalid {name}")))
}

async fn ensure_source(s: &AppState, id: Uuid) -> Result<(), ApiError> {
    let exists: bool = sqlx::query_scalar("select exists(select 1 from sources where id = $1)")
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(bad_request("unknown source"))
    }
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
    source_id: Uuid,
    source: String,
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

const PAGE_SELECT: &str = "select p.id, p.source_id, src.title as source, src.subject_id, s.name as subject, p.page_number, p.status, p.error, p.attempts,
    p.next_attempt_at, p.uploaded_at, p.processed_at,
    (select count(*) from question_appearances a where a.page_id = p.id) as question_count,
    (select count(*) from question_appearances a join questions q on q.id = a.question_id
       where a.page_id = p.id and q.needs_check and q.resolved_at is null) as flagged_count,
    (p.tail_fragment is not null and not p.tail_joined) as waiting_for_next_page
  from pages p join sources src on src.id = p.source_id join subjects s on s.id = src.subject_id";

/// Upload up to 20 JPEG photos of one source, in page order. Fields: `source_id`, then one `photos`
/// part per file. Pages rendered from a PDF also send the `upload_id` from `POST /uploads/pdf`.
#[utoipa::path(post, path = "/pages", request_body(content_type = "multipart/form-data"),
    responses((status = 200, body = Vec<PageSummary>), (status = 400, body = ErrorBody)))]
async fn upload_pages(
    State(s): State<AppState>,
    mut form: Multipart,
) -> ApiResult<Vec<PageSummary>> {
    let mut source_id: Option<Uuid> = None;
    let mut pdf_upload: Option<Uuid> = None;
    let mut photos = Vec::new();
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        match field.name() {
            Some("source_id") => source_id = Some(parse_uuid_field(field, "source_id").await?),
            Some("upload_id") => pdf_upload = Some(parse_uuid_field(field, "upload_id").await?),
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
    let source_id = source_id.ok_or_else(|| bad_request("source_id is required"))?;
    if photos.is_empty() || photos.len() > MAX_PHOTOS {
        return Err(bad_request(format!(
            "send between 1 and {MAX_PHOTOS} photos"
        )));
    }
    ensure_source(&s, source_id).await?;
    if let Some(upload_id) = pdf_upload {
        let fits: bool = sqlx::query_scalar(
            "select exists(select 1 from uploads where id = $1 and source_id = $2 and kind = 'pdf')",
        )
        .bind(upload_id)
        .bind(source_id)
        .fetch_one(&s.db)
        .await?;
        if !fits {
            return Err(bad_request("that upload_id isn't a PDF of this source"));
        }
    }

    let mut ids = Vec::new();
    for photo in photos {
        let id = Uuid::new_v4();
        let key = format!("pages/{id}.jpg");
        s.storage.put(&key, photo).await?;
        let mut tx = s.db.begin().await?;
        // Lock the source so concurrent uploads can't take the same position.
        sqlx::query("select 1 from sources where id = $1 for update")
            .bind(source_id)
            .execute(&mut *tx)
            .await?;
        let upload_id: Uuid = match pdf_upload {
            Some(id) => id,
            None => {
                sqlx::query_scalar(
                    "insert into uploads (source_id, kind, file_key) values ($1, 'photo', $2) returning id",
                )
                .bind(source_id)
                .bind(&key)
                .fetch_one(&mut *tx)
                .await?
            }
        };
        sqlx::query(
            "insert into pages (id, source_id, upload_id, photo_key, position)
             values ($1, $2, $3, $4, (select coalesce(max(position), 0) + 1 from pages where source_id = $2))",
        )
        .bind(id)
        .bind(source_id)
        .bind(upload_id)
        .bind(&key)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        ids.push(id);
    }
    Ok(Json(
        sqlx::query_as(&format!(
            "{PAGE_SELECT} where p.id = any($1) order by p.position"
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
    source_id: Option<Uuid>,
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
            "{PAGE_SELECT} where ($1::uuid is null or src.subject_id = $1) and ($2::text is null or p.status = $2)
               and ($4::uuid is null or p.source_id = $4)
             order by p.uploaded_at desc, p.position desc limit $3"
        ))
        .bind(f.subject_id)
        .bind(f.status)
        .bind(f.limit.unwrap_or(50).clamp(1, 200))
        .bind(f.source_id)
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
        "{QUESTION_SELECT} where q.id in (select question_id from question_appearances where page_id = $1)
         order by {QUESTION_ORDER}"
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
/// A question first seen here that another page also prints moves to that page instead.
async fn clear_page(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("delete from question_appearances where page_id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "update questions q set source_id = a.source_id, page_id = a.page_id,
           page_number = a.page_number, number = a.number
         from (select distinct on (question_id) * from question_appearances
               order by question_id, created_at) a
         where a.question_id = q.id and q.page_id = $1",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    // Its groups and diagrams go with it, as do any a saved question took from this page.
    sqlx::query(
        "update question_groups g set page_id = q.page_id from questions q
         where q.group_id = g.id and g.page_id = $1 and q.page_id <> $1",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "update diagrams d set page_id = q.page_id from questions q
         where d.page_id = $1 and q.page_id <> $1
           and (q.diagram_id = d.id
             or exists (select 1 from question_groups g where g.id = q.group_id and g.diagram_id = d.id)
             or exists (select 1 from options o where o.question_id = q.id and o.diagram_id = d.id)
             or exists (select 1 from question_parts p where p.question_id = q.id and p.diagram_id = d.id))",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
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
    let (key, upload_id): (String, Uuid) =
        sqlx::query_as("select photo_key, upload_id from pages where id = $1")
            .bind(id)
            .fetch_one(&s.db)
            .await?;
    let mut tx = s.db.begin().await?;
    clear_page(&mut tx, id).await?;
    sqlx::query("delete from pages where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // A PDF stays while any of its pages do; a photo upload is the page itself.
    let upload_file: Option<(String, String)> = sqlx::query_as(
        "delete from uploads u where u.id = $1 and not exists (select 1 from pages where upload_id = u.id)
         returning kind, file_key",
    )
    .bind(upload_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut keys = vec![key];
    if let Some((kind, file_key)) = upload_file
        && kind == "pdf"
        && !file_key.is_empty()
    {
        keys.push(file_key);
    }
    for key in keys {
        if let Err(e) = s.storage.delete(&key).await {
            tracing::warn!("deleting {key}: {e:#}");
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

// ── Questions ───────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct QuestionSummary {
    id: Uuid,
    /// objective or theory.
    kind: String,
    /// source (from a book or paper) or ours (written by the team).
    origin: String,
    subject_id: Uuid,
    subject: String,
    /// Where it was first seen; none for our own questions.
    source_id: Option<Uuid>,
    source: Option<String>,
    page_id: Option<Uuid>,
    page_number: Option<String>,
    number: String,
    /// Syllabus topic.
    topic_id: Option<Uuid>,
    topic: Option<String>,
    body: String,
    needs_check: bool,
    /// conflicting_answers, has_diagram, low_confidence, no_answer, refers_to_number,
    /// possible_duplicate, source_conflict.
    check_reasons: Vec<String>,
    resolved_at: Option<DateTime<Utc>>,
}

const QUESTION_SELECT: &str = "select q.id, q.kind, q.origin, q.subject_id, s.name as subject,
    q.source_id, src.title as source, q.page_id, q.page_number, q.number, q.topic_id, t.name as topic,
    q.body, q.needs_check, q.check_reasons, q.resolved_at
  from questions q join subjects s on s.id = q.subject_id
    left join sources src on src.id = q.source_id
    left join syllabus_topics t on t.id = q.topic_id";

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
    topic_id: Option<Uuid>,
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
        and ($2::uuid is null or q.topic_id = $2)
        and ($3::text is null or q.page_number = $3)
        and ($4::text is null or q.body ilike '%' || $4 || '%'
             or exists (select 1 from options o where o.question_id = q.id and o.body ilike '%' || $4 || '%'))";
    let search = f.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let total: i64 = sqlx::query_scalar(&format!("select count(*) from questions q {filter}"))
        .bind(f.subject_id)
        .bind(f.topic_id)
        .bind(&f.page_number)
        .bind(search)
        .fetch_one(&s.db)
        .await?;
    let items = sqlx::query_as(&format!(
        "{QUESTION_SELECT} {filter} order by {QUESTION_ORDER} limit $5 offset $6"
    ))
    .bind(f.subject_id)
    .bind(f.topic_id)
    .bind(&f.page_number)
    .bind(search)
    .bind(f.limit.unwrap_or(50).clamp(1, 200))
    .bind(f.offset.unwrap_or(0).max(0))
    .fetch_all(&s.db)
    .await?;
    Ok(Json(QuestionList { total, items }))
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
    /// The group this question belongs to; its questions are always shown together.
    group: Option<QuestionGroup>,
    /// The question's own diagram, or the shared one from its group.
    diagram: Option<Diagram>,
    options: Vec<OptionDetail>,
    /// Theory parts in printed order; `depth` 0 is (a), 1 is (a)(i).
    parts: Vec<PartDetail>,
    /// Where it was set. The exam's level matches the subject's.
    exam_id: Option<Uuid>,
    /// e.g. "WAEC WASSCE".
    exam: Option<String>,
    year: Option<i32>,
    paper: Option<String>,
    /// may_june, june_july or nov_dec.
    sitting: Option<String>,
    /// Its number in that exam.
    original_number: Option<String>,
    marks: Option<f64>,
    /// Every page that prints it, first seen first.
    appearances: Vec<Appearance>,
    /// Other questions with the same exam, year, number and kind (possible duplicates).
    related: Vec<RelatedQuestion>,
    /// Photo of the page it was first seen on; none for our own questions.
    photo_url: Option<String>,
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

#[derive(Serialize, ToSchema)]
pub struct PartDetail {
    id: Uuid,
    parent_id: Option<Uuid>,
    /// "a", "ii"
    label: String,
    depth: i32,
    body: Option<String>,
    closing_text: Option<String>,
    marks: Option<f64>,
    diagram: Option<Diagram>,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct Appearance {
    source_id: Uuid,
    source: String,
    page_id: Uuid,
    /// As printed.
    page_number: Option<String>,
    /// As printed in this source.
    number: String,
    /// API path of the page photo.
    #[sqlx(skip)]
    photo_url: String,
}

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct RelatedQuestion {
    id: Uuid,
    source: Option<String>,
    page_number: Option<String>,
    number: String,
    body: String,
    sitting: Option<String>,
    paper: Option<String>,
    option_count: i64,
    part_count: i64,
}

#[derive(sqlx::FromRow)]
struct QuestionExtra {
    explanation: Option<String>,
    confidence: Option<String>,
    second_answer: Option<String>,
    second_explanation: Option<String>,
    group_id: Option<Uuid>,
    diagram_id: Option<Uuid>,
    exam_id: Option<Uuid>,
    exam: Option<String>,
    year: Option<i32>,
    paper: Option<String>,
    sitting: Option<String>,
    original_number: Option<String>,
    marks: Option<f64>,
}

#[derive(sqlx::FromRow)]
struct PartRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    label: String,
    body: Option<String>,
    closing_text: Option<String>,
    marks: Option<f64>,
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
        "select q.explanation, q.confidence, q.second_answer, q.second_explanation, q.group_id,
                coalesce(q.diagram_id, g.diagram_id) as diagram_id,
                q.exam_id, b.code || ' ' || e.code as exam, q.year, q.paper, q.sitting,
                q.original_number, q.marks::float8 as marks
         from questions q left join question_groups g on g.id = q.group_id
           left join exams e on e.id = q.exam_id left join exam_bodies b on b.id = e.body_id
         where q.id = $1",
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
    let part_rows: Vec<PartRow> = sqlx::query_as(
        "select id, parent_id, label, body, closing_text, marks::float8 as marks, diagram_id
         from question_parts where question_id = $1 order by position",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    let mut depths: std::collections::HashMap<Uuid, i32> = std::collections::HashMap::new();
    let mut parts = Vec::new();
    for p in part_rows {
        let depth = p
            .parent_id
            .and_then(|parent| depths.get(&parent))
            .map_or(0, |d| d + 1);
        depths.insert(p.id, depth);
        parts.push(PartDetail {
            id: p.id,
            parent_id: p.parent_id,
            label: p.label,
            depth,
            body: p.body,
            closing_text: p.closing_text,
            marks: p.marks,
            diagram: load_diagram(s, p.diagram_id).await?,
        });
    }
    let mut appearances: Vec<Appearance> = sqlx::query_as(
        "select a.source_id, src.title as source, a.page_id, a.page_number, a.number
         from question_appearances a join sources src on src.id = a.source_id
         where a.question_id = $1 order by a.created_at",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    for a in &mut appearances {
        a.photo_url = format!("/pages/{}/photo", a.page_id);
    }
    let related: Vec<RelatedQuestion> = sqlx::query_as(
        "select o.id, src.title as source, o.page_number, o.number, o.body, o.sitting, o.paper,
                (select count(*) from options x where x.question_id = o.id) as option_count,
                (select count(*) from question_parts x where x.question_id = o.id) as part_count
         from questions q
         join questions o on o.id <> q.id and o.subject_id = q.subject_id and o.kind = q.kind
           and o.exam_id = q.exam_id and o.year = q.year and o.original_number = q.original_number
         left join sources src on src.id = o.source_id
         where q.id = $1
         order by o.created_at",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    let next_flagged_id: Option<Uuid> = sqlx::query_scalar(&format!(
        "with ordered as (
           select q.id, row_number() over (order by {QUESTION_ORDER}) as n
           from questions q join subjects s on s.id = q.subject_id
             left join sources src on src.id = q.source_id
           where (q.needs_check and q.resolved_at is null) or q.id = $1)
         select id from ordered
         where n > (select n from ordered where id = $1)
         order by n limit 1"
    ))
    .bind(id)
    .fetch_optional(&s.db)
    .await?;
    let photo_url = summary.page_id.map(|p| format!("/pages/{p}/photo"));
    Ok(QuestionDetail {
        summary,
        explanation: extra.explanation,
        confidence: extra.confidence,
        second_answer: extra.second_answer,
        second_explanation: extra.second_explanation,
        group: load_group(s, extra.group_id).await?,
        diagram: load_diagram(s, extra.diagram_id).await?,
        options,
        parts,
        exam_id: extra.exam_id,
        exam: extra.exam,
        year: extra.year,
        paper: extra.paper,
        sitting: extra.sitting,
        original_number: extra.original_number,
        marks: extra.marks,
        appearances,
        related,
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
pub struct PartEdit {
    id: Uuid,
    body: Option<String>,
    closing_text: Option<String>,
    marks: Option<f64>,
}

#[derive(Deserialize, ToSchema)]
pub struct QuestionEdit {
    /// May be empty for a theory question whose text is all in its parts.
    body: String,
    /// Syllabus topic; must belong to the question's subject.
    topic_id: Option<Uuid>,
    explanation: Option<String>,
    options: Vec<OptionEdit>,
    /// The option to mark correct; null leaves the question without an answer.
    correct_option_id: Option<Uuid>,
    /// Theory parts; parts not listed are left as they are.
    #[serde(default)]
    parts: Vec<PartEdit>,
    /// Where it was set; these replace the saved values.
    exam_id: Option<Uuid>,
    year: Option<i32>,
    paper: Option<String>,
    /// may_june, june_july or nov_dec.
    sitting: Option<String>,
    original_number: Option<String>,
    marks: Option<f64>,
    /// Also mark the question as checked.
    resolve: bool,
}

/// Trims text and turns blanks into none.
fn trimmed(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

/// Saves a reviewer's corrections.
#[utoipa::path(put, path = "/questions/{id}", request_body = QuestionEdit,
    responses((status = 200, body = QuestionDetail), (status = 400, body = ErrorBody)))]
async fn update_question(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(edit): Json<QuestionEdit>,
) -> ApiResult<QuestionDetail> {
    let kind: String = sqlx::query_scalar("select kind from questions where id = $1")
        .bind(id)
        .fetch_one(&s.db)
        .await?;
    if edit.body.trim().is_empty() && kind != "theory" {
        return Err(bad_request("the question text can't be empty"));
    }
    if let Some(year) = edit.year
        && !(1950..=2100).contains(&year)
    {
        return Err(bad_request("that year doesn't look right"));
    }
    let sitting = trimmed(edit.sitting.as_deref());
    if let Some(sitting) = sitting
        && !["may_june", "june_july", "nov_dec"].contains(&sitting)
    {
        return Err(bad_request(
            "sitting must be may_june, june_july or nov_dec",
        ));
    }
    if let Some(exam_id) = edit.exam_id {
        let fits: bool = sqlx::query_scalar(
            "select exists(select 1 from exams e join subjects s on s.level = e.level
               join questions q on q.subject_id = s.id where e.id = $1 and q.id = $2)",
        )
        .bind(exam_id)
        .bind(id)
        .fetch_one(&s.db)
        .await?;
        if !fits {
            return Err(bad_request(
                "that exam is for a different level than the subject",
            ));
        }
    }
    if let Some(topic_id) = edit.topic_id {
        let fits: bool = sqlx::query_scalar(
            "select exists(select 1 from syllabus_topics t join questions q on q.subject_id = t.subject_id
               where t.id = $1 and q.id = $2)",
        )
        .bind(topic_id)
        .bind(id)
        .fetch_one(&s.db)
        .await?;
        if !fits {
            return Err(bad_request(
                "topic_id is not a topic of this question's subject",
            ));
        }
    }
    let mut tx = s.db.begin().await?;
    sqlx::query(
        "update questions set body = $2, topic_id = $3, explanation = $4,
           resolved_at = case when $5 then now() else resolved_at end,
           exam_id = $6, year = $7, paper = $8, sitting = $9, original_number = $10,
           marks = $11::numeric
         where id = $1",
    )
    .bind(id)
    .bind(edit.body.trim())
    .bind(edit.topic_id)
    .bind(
        edit.explanation
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty()),
    )
    .bind(edit.resolve)
    .bind(edit.exam_id)
    .bind(edit.year)
    .bind(trimmed(edit.paper.as_deref()))
    .bind(sitting)
    .bind(trimmed(edit.original_number.as_deref()))
    .bind(edit.marks)
    .execute(&mut *tx)
    .await?;
    for p in &edit.parts {
        sqlx::query(
            "update question_parts set body = $3, closing_text = $4, marks = $5::numeric
             where id = $1 and question_id = $2",
        )
        .bind(p.id)
        .bind(id)
        .bind(trimmed(p.body.as_deref()))
        .bind(trimmed(p.closing_text.as_deref()))
        .bind(p.marks)
        .execute(&mut *tx)
        .await?;
    }
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

#[derive(Deserialize, ToSchema)]
pub struct MergeRequest {
    /// The question to keep.
    into: Uuid,
}

/// A reviewer decided two questions are one. This one's appearances move to `into`,
/// blanks there are filled from this one (including options it lacks, which sends it
/// back for review as its answer may change), and this one is deleted.
#[utoipa::path(post, path = "/questions/{id}/merge", request_body = MergeRequest,
    responses((status = 200, body = QuestionDetail), (status = 400, body = ErrorBody)))]
async fn merge_question(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(m): Json<MergeRequest>,
) -> ApiResult<QuestionDetail> {
    if m.into == id {
        return Err(bad_request("a question can't be merged into itself"));
    }
    let fits: bool = sqlx::query_scalar(
        "select exists(select 1 from questions a join questions b
           on b.subject_id = a.subject_id and b.kind = a.kind where a.id = $1 and b.id = $2)",
    )
    .bind(id)
    .bind(m.into)
    .fetch_one(&s.db)
    .await?;
    if !fits {
        return Err(bad_request(
            "only questions of the same subject and kind can be merged",
        ));
    }
    let mut tx = s.db.begin().await?;
    sqlx::query("update question_appearances set question_id = $2 where question_id = $1")
        .bind(id)
        .bind(m.into)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "update questions k set sitting = coalesce(k.sitting, d.sitting), paper = coalesce(k.paper, d.paper),
           topic_id = coalesce(k.topic_id, d.topic_id), marks = coalesce(k.marks, d.marks),
           diagram_id = coalesce(k.diagram_id, d.diagram_id)
         from questions d where k.id = $2 and d.id = $1",
    )
    .bind(id)
    .bind(m.into)
    .execute(&mut *tx)
    .await?;
    let added = sqlx::query(
        "insert into options (question_id, label, body, diagram_id, is_correct)
         select $2, label, body, diagram_id, false from options where question_id = $1
         on conflict (question_id, label) do nothing",
    )
    .bind(id)
    .bind(m.into)
    .execute(&mut *tx)
    .await?;
    if added.rows_affected() > 0 {
        sqlx::query(
            "update questions set needs_check = true, resolved_at = null,
               check_reasons = case when 'options_added' = any(check_reasons) then check_reasons
                                    else array_append(check_reasons, 'options_added') end
             where id = $1",
        )
        .bind(m.into)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("delete from questions where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(question_detail(&s, m.into).await?))
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

// ── Groups ──────────────────────────────────────────────────────────────────

#[derive(Serialize, ToSchema, sqlx::FromRow)]
pub struct GroupMember {
    id: Uuid,
    number: String,
    needs_check: bool,
    resolved_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
pub struct QuestionGroup {
    id: Uuid,
    /// As printed, e.g. "Study it and answer questions 31 to 33" — for reviewers.
    instruction: Option<String>,
    /// What students see: the instruction without question numbers. The app numbers
    /// the group's questions itself, since they are always shown together.
    display_instruction: Option<String>,
    /// The group's questions in book order.
    members: Vec<GroupMember>,
}

async fn load_group(s: &AppState, id: Option<Uuid>) -> sqlx::Result<Option<QuestionGroup>> {
    let Some(id) = id else { return Ok(None) };
    let (instruction, display_instruction): (Option<String>, Option<String>) = sqlx::query_as(
        "select instruction, display_instruction from question_groups where id = $1",
    )
    .bind(id)
    .fetch_one(&s.db)
    .await?;
    let members = sqlx::query_as(
        "select id, number, needs_check, resolved_at from questions where group_id = $1
         order by (case when number ~ '^[0-9]+$' then number::int end) nulls last, number",
    )
    .bind(id)
    .fetch_all(&s.db)
    .await?;
    Ok(Some(QuestionGroup {
        id,
        instruction,
        display_instruction,
        members,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct GroupEdit {
    /// Student-facing instruction; must not mention question numbers.
    display_instruction: Option<String>,
}

#[utoipa::path(put, path = "/groups/{id}", request_body = GroupEdit,
    responses((status = 200, body = QuestionGroup), (status = 404, body = ErrorBody)))]
async fn update_group(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Json(edit): Json<GroupEdit>,
) -> ApiResult<QuestionGroup> {
    let n = sqlx::query("update question_groups set display_instruction = $2 where id = $1")
        .bind(id)
        .bind(
            edit.display_instruction
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty()),
        )
        .execute(&s.db)
        .await?;
    if n.rows_affected() == 0 {
        return Err(ApiError(StatusCode::NOT_FOUND, "not found".into()));
    }
    Ok(Json(load_group(&s, Some(id)).await?.expect("group exists")))
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
