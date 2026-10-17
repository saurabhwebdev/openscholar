use askama::Template;
use axum::{
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::fees as fee_model;
use crate::models::fees_ext as fee_ext;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/fees/categories", get(categories_page).post(create_category_submit))
        .route("/fees/categories/{id}/delete", post(delete_category_submit))
        .route("/fees/{id}/discount", post(apply_discount_submit))
        .route("/fees/{id}/fines", post(create_fine_submit))
        .route("/fines/{id}/delete", post(delete_fine_submit))
        .route("/payments/{id}/receipt", post(issue_receipt_submit))
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

fn check_mutate(user: &CurrentUser) -> Result<(), AppError> {
    crate::auth::middleware::require_roles(user, &[Role::Admin, Role::Staff])
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

// ---------------------------------------------------------------------------
// GET /fees/categories
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CategoriesParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "fees/categories.html")]
struct CategoriesTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    categories: Vec<fee_ext::CategoryRow>,
    can_mutate: bool,
}

async fn categories_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<CategoriesParams>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let categories = fee_ext::list_categories(&state.pool).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = CategoriesTemplate {
        section: "fees",
        section_label: "Fees",
        today: chrono::Local::now().format("%A, %-d %B %Y").to_string(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        categories,
        can_mutate: true,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /fees/categories
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CategoryForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

async fn create_category_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<CategoryForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    if let Err(msg) = fee_ext::validate_category(&form.name) {
        return Ok(flash_redirect("/fees/categories", &msg, "error"));
    }
    let desc = if form.description.trim().is_empty() {
        None
    } else {
        Some(form.description.as_str())
    };
    match fee_ext::create_category(&state.pool, form.name.trim(), desc).await {
        Ok(_) => Ok(flash_redirect("/fees/categories", "Category created", "ok")),
        Err(AppError::Db(e)) if e.to_string().contains("unique") => {
            Ok(flash_redirect("/fees/categories", "Category already exists", "error"))
        }
        Err(_) => Ok(flash_redirect("/fees/categories", "Could not create category", "error")),
    }
}

// ---------------------------------------------------------------------------
// POST /fees/categories/:id/delete
// ---------------------------------------------------------------------------

async fn delete_category_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    match fee_ext::delete_category(&state.pool, id).await {
        Ok(true) => Ok(flash_redirect("/fees/categories", "Category deleted", "ok")),
        Ok(false) => Ok(flash_redirect("/fees/categories", "Category not found", "error")),
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect("/fees/categories", &msg, "error")),
        Err(_) => Ok(flash_redirect("/fees/categories", "Could not delete category", "error")),
    }
}

// ---------------------------------------------------------------------------
// POST /fees/:id/discount
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DiscountForm {
    #[serde(default)]
    discount_id: String,
    #[serde(default)]
    percent: String,
}

async fn apply_discount_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<DiscountForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let target = format!("/fees/{id}");
    let did = form.discount_id.trim();
    let pct = form.percent.trim();
    let res = if !did.is_empty() {
        if let Ok(did) = did.parse::<i64>() {
            fee_ext::apply_discount_by_id(&state.pool, id, did).await
        } else {
            Err(AppError::BadRequest("Invalid discount".to_string()))
        }
    } else if !pct.is_empty() {
        let p: f64 = match pct.parse() {
            Ok(v) => v,
            Err(_) => return Ok(flash_redirect(&target, "Invalid percent", "error")),
        };
        if let Err(msg) = fee_ext::validate_discount("x", p) {
            return Ok(flash_redirect(&target, &msg, "error"));
        }
        fee_ext::apply_discount_to_invoice_percent(&state.pool, id, p).await
    } else {
        return Ok(flash_redirect(&target, "Provide discount or percent", "error"));
    };
    match res {
        Ok(_) => Ok(flash_redirect(&target, "Discount applied", "ok")),
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect(&target, &msg, "error")),
        Err(AppError::NotFound) => Ok(flash_redirect(&target, "Invoice not found", "error")),
        Err(_) => Ok(flash_redirect(&target, "Could not apply discount", "error")),
    }
}

// ---------------------------------------------------------------------------
// POST /fees/:id/fines
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FineForm {
    #[serde(default)]
    amount: String,
    #[serde(default)]
    reason: String,
}

async fn create_fine_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<FineForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let target = format!("/fees/{id}");
    let amount_cents = match fee_model::parse_amount_cents(&form.amount) {
        Ok(c) => c,
        Err(msg) => return Ok(flash_redirect(&target, &msg, "error")),
    };
    if let Err(msg) = fee_ext::validate_fine(amount_cents, &form.reason) {
        return Ok(flash_redirect(&target, &msg, "error"));
    }
    match fee_ext::create_fine(&state.pool, id, amount_cents, &form.reason).await {
        Ok(_) => Ok(flash_redirect(&target, "Fine added", "ok")),
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect(&target, &msg, "error")),
        Err(AppError::NotFound) => Ok(flash_redirect(&target, "Invoice not found", "error")),
        Err(_) => Ok(flash_redirect(&target, "Could not add fine", "error")),
    }
}

// ---------------------------------------------------------------------------
// POST /fines/:id/delete
// ---------------------------------------------------------------------------

async fn delete_fine_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    // find invoice for redirect
    let fine = fee_ext::find_fine(&state.pool, id).await?;
    let invoice_id = fine.map(|f| f.invoice_id);
    let target = invoice_id.map(|i| format!("/fees/{i}")).unwrap_or_else(|| "/fees".to_string());
    match fee_ext::delete_fine(&state.pool, id).await {
        Ok(true) => Ok(flash_redirect(&target, "Fine deleted", "ok")),
        Ok(false) => Ok(flash_redirect(&target, "Fine not found", "error")),
        Err(_) => Ok(flash_redirect(&target, "Could not delete fine", "error")),
    }
}

// ---------------------------------------------------------------------------
// POST /payments/:id/receipt
// ---------------------------------------------------------------------------

async fn issue_receipt_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    fee_ext::ensure_receipt(&state.pool, id, user.id).await?;
    Ok(Redirect::to(&format!("/print/payments/{id}/receipt")))
}
