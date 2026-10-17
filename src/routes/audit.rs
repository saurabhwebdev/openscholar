use askama::Template;
use axum::{
    extract::{Extension, Form, Query, State},
    http::header,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use serde::Deserialize;

use crate::auth::middleware::require_roles;
use crate::error::AppError;
use crate::models::audit as audit_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audit", get(list_audit))
        .route("/audit/export/students", get(export_students))
        .route("/audit/import/students", post(import_students))
}

fn user_ctx(user: &CurrentUser) -> (String, String) {
    let initial = user
        .full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    (user.full_name.clone(), initial)
}

fn today() -> String {
    chrono::Local::now().format("%A %-d %B %Y").to_string()
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

#[derive(Debug, Deserialize)]
struct ListParams {
    entity: Option<String>,
    id: Option<i64>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct AuditView {
    id: i64,
    user_id: String,
    action: String,
    entity: String,
    entity_id: String,
    detail: String,
    created_at: String,
}

#[derive(Template)]
#[template(path = "audit/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    rows: Vec<AuditView>,
}

async fn list_audit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin])?;
    let rows = if let (Some(entity), Some(id)) = (params.entity.as_deref(), params.id) {
        audit_model::list_for(&state.pool, entity, id).await?
    } else if let Some(entity) = params.entity.as_deref() {
        let all = audit_model::list_recent(&state.pool).await?;
        all.into_iter().filter(|r| r.entity == entity).collect()
    } else {
        audit_model::list_recent(&state.pool).await?
    };
    let views: Vec<AuditView> = rows
        .into_iter()
        .map(|r| AuditView {
            id: r.id,
            user_id: r.user_id.map(|v| v.to_string()).unwrap_or_default(),
            action: r.action,
            entity: r.entity,
            entity_id: r.entity_id.map(|v| v.to_string()).unwrap_or_default(),
            detail: r.detail.unwrap_or_default(),
            created_at: r.created_at.format("%-d %b %Y %H:%M").to_string(),
        })
        .collect();
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "audit",
        section_label: "Audit log",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        rows: views,
    };
    t.render().map(Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct ImportForm {
    #[serde(default)]
    csv: String,
}

async fn export_students(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin])?;
    let rows = audit_model::csv_export_students(&state.pool).await?;
    let mut body = String::new();
    for row in rows {
        let line: Vec<String> = row.iter().map(|f| audit_model::csv_quote(f)).collect();
        body.push_str(&line.join(","));
        body.push('\n');
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"students.csv\"",
            ),
        ],
        body,
    ))
}

async fn import_students(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ImportForm>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin])?;
    let report = audit_model::csv_import_students(&state.pool, &form.csv).await?;
    let msg = format!(
        "Imported {}, skipped {}, {} errors",
        report.inserted,
        report.skipped,
        report.errors.len()
    );
    Ok(flash_redirect("/audit", &msg, "ok").into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_builds() {
        let _ = router();
    }
}
