use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;

use crate::auth::middleware::require_roles;
use crate::auth::password;
use crate::error::AppError;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(users_list).post(users_create))
        .route("/users/{id}/toggle-active", post(users_toggle_active))
        .route("/users/{id}/delete", post(users_delete))
}

fn today() -> String {
    chrono::Local::now().format("%A %-d %B %Y").to_string()
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

fn flash_ctx(params: &HashMap<String, String>) -> (Option<String>, String) {
    (
        params.get("flash").cloned(),
        params
            .get("flash_kind")
            .cloned()
            .unwrap_or_else(|| "info".to_string()),
    )
}

fn back_to_list(msg: &str, kind: &str) -> Response {
    Redirect::to(&format!(
        "/users?flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
    .into_response()
}

fn fail_redirect(msg: &str) -> Response {
    back_to_list(msg, "error")
}

fn ok_redirect(msg: &str) -> Response {
    back_to_list(msg, "ok")
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// Pure validation for the inline create form.
/// Returns the parsed role on success so handlers can insert directly.
pub fn validate_new_user(email: &str, full_name: &str, role_raw: &str) -> Result<Role, AppError> {
    let email = email.trim();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::BadRequest(
            "Email must contain @".to_string(),
        ));
    }
    if full_name.trim().is_empty() {
        return Err(AppError::BadRequest("Full name is required".to_string()));
    }
    role_raw.trim().parse::<Role>().map_err(AppError::BadRequest)
}

/// True when an admin targets their own row (toggle/delete must refuse this).
pub fn is_self(current_id: i64, target_id: i64) -> bool {
    current_id == target_id
}

#[derive(Debug, sqlx::FromRow)]
struct UserListRow {
    id: i64,
    email: String,
    full_name: String,
    role: String,
    active: bool,
    created: String,
}

async fn list_users(pool: &sqlx::PgPool) -> Result<Vec<UserListRow>, AppError> {
    let rows = sqlx::query_as::<_, UserListRow>(
        "SELECT id, email, full_name, role, active,
                to_char(created_at, 'YYYY-MM-DD') AS created
         FROM users ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------- list ----------

#[derive(Template)]
#[template(path = "users/list.html")]
struct UsersListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    users: Vec<UserListRow>,
    current_user_id: i64,
}

async fn users_list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin])?;
    let rows = list_users(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let (flash, flash_kind) = flash_ctx(&params);
    let template = UsersListTemplate {
        section: "users",
        section_label: "Users",
        today: today(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        users: rows,
        current_user_id: user.id,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---------- create ----------

#[derive(Debug, Deserialize)]
struct UserForm {
    #[serde(default)]
    email: String,
    #[serde(default)]
    full_name: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    password: String,
}

async fn users_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<UserForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?;
    let role = match validate_new_user(&form.email, &form.full_name, &form.role) {
        Ok(role) => role,
        Err(e) => return Ok(fail_redirect(&e.to_string())),
    };
    let hash = match password::hash_password(&form.password) {
        Ok(hash) => hash,
        Err(e) => return Ok(fail_redirect(&e.to_string())),
    };
    match sqlx::query(
        "INSERT INTO users (email, password_hash, role, full_name)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(form.email.trim())
    .bind(&hash)
    .bind(role.as_str())
    .bind(form.full_name.trim())
    .execute(&state.pool)
    .await
    {
        Ok(_) => Ok(ok_redirect("User created")),
        Err(e) if is_unique_violation(&e) => Ok(fail_redirect("Email already exists")),
        Err(e) => Err(AppError::from(e)),
    }
}

// ---------- toggle-active ----------

async fn users_toggle_active(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?;
    if is_self(user.id, id) {
        return Ok(fail_redirect("You cannot deactivate your own account"));
    }
    let done = sqlx::query("UPDATE users SET active = NOT active WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(ok_redirect("User updated"))
}

// ---------- delete ----------

async fn users_delete(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin])?;
    if is_self(user.id, id) {
        return Ok(fail_redirect("You cannot delete your own account"));
    }
    // Sessions for the deleted user cascade via the sessions.user_id FK.
    let done = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(ok_redirect("User deleted"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_input() {
        assert_eq!(
            validate_new_user("ada@example.com", "Ada Lovelace", "admin").unwrap(),
            Role::Admin
        );
        assert_eq!(
            validate_new_user("t@example.com", "T", "teacher").unwrap(),
            Role::Teacher
        );
        assert_eq!(
            validate_new_user("s@example.com", "S", "staff").unwrap(),
            Role::Staff
        );
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(
            validate_new_user("  a@example.com  ", "  Ada  ", "  admin  ").unwrap(),
            Role::Admin
        );
    }

    #[test]
    fn rejects_email_without_at() {
        assert!(matches!(
            validate_new_user("not-an-email", "Ada", "admin"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_empty_email() {
        assert!(matches!(
            validate_new_user("", "Ada", "admin"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_new_user("   ", "Ada", "admin"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_empty_full_name() {
        assert!(matches!(
            validate_new_user("a@example.com", "", "admin"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_new_user("a@example.com", "   ", "teacher"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn rejects_unknown_role() {
        assert!(matches!(
            validate_new_user("a@example.com", "Ada", "superadmin"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_new_user("a@example.com", "Ada", ""),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn detects_self_targeting() {
        assert!(is_self(7, 7));
        assert!(!is_self(7, 8));
    }
}
