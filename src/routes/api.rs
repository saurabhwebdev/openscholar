use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::models::attendance as att_model;
use crate::models::fees as fee_model;
use crate::models::grades as grade_model;
use crate::models::student::{find_student, list_students};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;

pub fn health_router() -> Router<AppState> {
    Router::new().route("/api/v1/health", get(health))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/students", get(api_students))
        .route("/api/v1/students/{id}", get(api_student))
        .route("/api/v1/classes", get(api_classes))
        .route("/api/v1/attendance", get(api_attendance))
        .route("/api/v1/grades", get(api_grades))
        .route("/api/v1/fees/outstanding", get(api_fees_outstanding))
}

fn require_api_role(user: &CurrentUser) -> Result<(), AppError> {
    if user.has_role(&[Role::Admin, Role::Staff]) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(json!({"status": "ok"})))
}

async fn api_students(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    require_api_role(&user)?;
    let rows = list_students(&state.pool).await?;
    let body: Vec<_> = rows
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "admission_no": s.admission_no,
                "full_name": format!("{} {}", s.first_name, s.last_name),
                "status": s.status,
            })
        })
        .collect();
    Ok((StatusCode::OK, Json(json!(body))))
}

async fn api_student(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    require_api_role(&user)?;
    let row = find_student(&state.pool, id).await?.ok_or(AppError::NotFound)?;
    let body = json!({
        "id": row.id,
        "admission_no": row.admission_no,
        "full_name": format!("{} {}", row.first_name, row.last_name),
        "status": row.status,
    });
    Ok((StatusCode::OK, Json(body)))
}

async fn api_classes(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    require_api_role(&user)?;
    let opts = grade_model::list_class_options(&state.pool).await?;
    let body: Vec<_> = opts.into_iter().map(|c| json!({"id": c.id, "name": c.name})).collect();
    Ok((StatusCode::OK, Json(json!(body))))
}

#[derive(Debug, Deserialize)]
struct AttParams {
    class_id: Option<i64>,
    date: Option<String>,
}

async fn api_attendance(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<AttParams>,
) -> Result<impl IntoResponse, AppError> {
    require_api_role(&user)?;
    let class_id = params.class_id.unwrap_or(0);
    let date_str = params.date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let date = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("bad date".to_string()))?;
    let roster = att_model::roster_for(&state.pool, class_id, date).await?;
    let body: Vec<_> = roster
        .into_iter()
        .map(|r| {
            json!({
                "student_id": r.student_id,
                "admission_no": r.admission_no,
                "full_name": r.full_name,
                "status": r.status,
                "note": r.note,
            })
        })
        .collect();
    Ok((StatusCode::OK, Json(json!(body))))
}

#[derive(Debug, Deserialize)]
struct GradeParams {
    class_id: Option<i64>,
}

async fn api_grades(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<GradeParams>,
) -> Result<impl IntoResponse, AppError> {
    require_api_role(&user)?;
    let class_id = params.class_id.unwrap_or(0);
    let assessments = grade_model::assessments_for_class(&state.pool, class_id).await?;
    let body: Vec<_> = assessments
        .into_iter()
        .map(|a| {
            json!({
                "id": a.id,
                "name": a.name,
                "kind": a.kind,
                "max_score": a.max_score,
                "weight": a.weight,
                "date": a.date.to_string(),
                "avg": a.avg,
                "graded_count": a.graded_count,
            })
        })
        .collect();
    Ok((StatusCode::OK, Json(json!(body))))
}

async fn api_fees_outstanding(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    require_api_role(&user)?;
    let rows = fee_model::outstanding_invoices_filtered(&state.pool, "", None).await.unwrap_or_default();
    let today = chrono::Local::now().date_naive();
    let body: Vec<_> = rows
        .iter()
        .map(|r| {
            let v = fee_model::to_view(r, today);
            json!({
                "invoice_id": v.id,
                "student_id": r.student_id,
                "title": v.title,
                "amount": v.amount,
                "balance": v.balance,
                "status": v.status,
            })
        })
        .collect();
    Ok((StatusCode::OK, Json(json!(body))))
}