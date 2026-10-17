use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    body::Bytes,
    extract::{Extension, Form, Path, Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::auth::middleware::require_roles;
use crate::error::AppError;
use crate::models::attachment::{self, AttachmentView};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::storage;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/attachments", get(list).post(create))
        .route("/attachments/{id}/delete", post(delete_attachment))
        .route("/attachments/{id}/download", get(download))
        // Base64 inflates ~33%; allow headroom above the 10 MB decoded cap so a
        // legitimate document isn't rejected by axum's 2 MB default body limit.
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
}

fn user_initial(user: &CurrentUser) -> String {
    user.full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn today() -> String {
    chrono::Local::now().format("%A %-d %B %Y").to_string()
}

fn flash_params(params: &HashMap<String, String>) -> (Option<String>, String) {
    let flash = params.get("flash").cloned();
    let flash_kind = params
        .get("flash_kind")
        .cloned()
        .unwrap_or_else(|| "info".to_string());
    (flash, flash_kind)
}

fn back_to_form(base: &str, msg: &str) -> Redirect {
    Redirect::to(&format!(
        "{base}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
}

#[derive(Debug, Deserialize)]
struct AttachmentForm {
    #[serde(default)]
    entity: String,
    #[serde(default)]
    entity_id: i64,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    content_type: String,
    #[serde(default)]
    size_bytes: i64,
    #[serde(default, alias = "data_base64")]
    data: String,
}

#[derive(Template)]
#[template(path = "attachments/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    attachments: Vec<AttachmentView>,
    entity: String,
    entity_id: i64,
    can_edit: bool,
    flash: Option<String>,
    flash_kind: String,
}

async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let entity = params.get("entity").cloned().unwrap_or_default();
    let entity_id: i64 = params
        .get("entity_id")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let rows = if !entity.trim().is_empty() && entity_id > 0 {
        attachment::list_for(&state.pool, &entity, entity_id).await?
    } else {
        Vec::new()
    };
    let (flash, flash_kind) = flash_params(&params);
    let template = ListTemplate {
        section: "attachments",
        section_label: "Attachments",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        attachments: rows.into_iter().map(AttachmentView::from).collect(),
        entity,
        entity_id,
        can_edit: user.has_role(&[Role::Admin, Role::Staff]),
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

async fn create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<AttachmentForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;

    let entity = form.entity.trim().to_string();
    let entity_id = form.entity_id;
    let filename = form.filename.trim().to_string();
    if entity.is_empty() {
        return Ok(back_to_form("/attachments", "Entity is required").into_response());
    }
    if entity_id <= 0 {
        return Ok(back_to_form("/attachments", "Entity id is required").into_response());
    }

    let bytes = match util::decode_base64(&form.data) {
        Ok(b) => b,
        Err(msg) => return Ok(back_to_form("/attachments", &msg).into_response()),
    };

    let size_bytes = bytes.len() as i64;
    if let Err(msg) = attachment::validate_attachment(&entity, &filename, size_bytes) {
        return Ok(back_to_form("/attachments", &msg).into_response());
    }

    let content_type = if form.content_type.trim().is_empty() {
        None
    } else {
        Some(form.content_type.trim().to_string())
    };

    // Build a safe storage key.
    let safe_entity: String = entity
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let key = format!("{}/{}/{}", safe_entity, entity_id, uuid::Uuid::new_v4());

    let backend = storage::StorageConfig::from_env().backend();
    if let Err(e) = storage::store_bytes(&backend, &key, &bytes, content_type.as_deref()).await {
        return Err(AppError::Internal(e.to_string()));
    }

    let row = attachment::add(
        &state.pool,
        &entity,
        entity_id,
        &filename,
        content_type,
        size_bytes,
        &key,
        Some(user.id),
    )
    .await?;

    if let Err(e) = crate::models::audit::log_action(
        &state.pool,
        Some(user.id),
        "upload",
        "attachment",
        Some(row.id),
        Some(format!("{entity}#{entity_id}: {filename}")),
    )
    .await
    {
        tracing::warn!(error = %e, "audit log failed");
    }

    let redirect = redirect_after(&entity, entity_id, "Uploaded");
    Ok(Redirect::to(&redirect).into_response())
}

fn redirect_after(entity: &str, entity_id: i64, msg: &str) -> String {
    let base = match entity {
        "student" => format!("/students/{}", entity_id),
        _ => "/attachments".to_string(),
    };
    format!("{}?flash={}&flash_kind=ok", base, util::url_encode(msg))
}

async fn delete_attachment(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = attachment::delete(&state.pool, id).await?;
    let (entity, entity_id) = match row {
        Some(r) => (r.entity, r.entity_id),
        None => return Err(AppError::NotFound),
    };
    if let Err(e) = crate::models::audit::log_action(
        &state.pool,
        Some(user.id),
        "delete",
        "attachment",
        Some(id),
        Some(format!("{entity}#{entity_id}")),
    )
    .await
    {
        tracing::warn!(error = %e, "audit log failed");
    }
    let base = match entity.as_str() {
        "student" => format!("/students/{}", entity_id),
        _ => "/attachments".to_string(),
    };
    Ok(Redirect::to(&format!(
        "{}?flash={}&flash_kind=ok",
        base,
        util::url_encode("Attachment deleted")
    ))
    .into_response())
}

async fn download(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    // Only staff-side roles may download; the handler is otherwise an IDOR over
    // sequential ids (any logged-in user could enumerate every student's docs).
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let row = attachment::find(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let path = std::path::Path::new("uploads").join(&row.storage_key);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    let content_type = row
        .content_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());
    // Strip quotes and control chars so the filename can't break the header
    // (CRLF would otherwise 500 the download).
    let safe_name: String = row
        .filename
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect();
    let disposition = format!("attachment; filename=\"{safe_name}\"");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        Bytes::from(bytes),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_after_student() {
        assert_eq!(
            redirect_after("student", 5, "Uploaded"),
            "/students/5?flash=Uploaded&flash_kind=ok"
        );
    }
}
