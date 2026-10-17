use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use serde::Deserialize;

use crate::error::AppError;
use crate::models::audit as audit_model;
use crate::models::fees as fee_model;
use crate::models::fees_ext as fee_ext_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/fees", get(list_fees))
        .route("/invoices", post(create_invoice_submit))
        .route("/fees/{id}", get(fee_detail))
        .route("/fees/{id}/payments", post(record_payment_submit))
        .route("/fees/{id}/delete", post(delete_invoice_submit))
}

// ---------------------------------------------------------------------------
// Shared presentation helpers
// ---------------------------------------------------------------------------

fn user_ctx(user: &CurrentUser) -> (String, String) {
    let initial = user
        .full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    (user.full_name.clone(), initial)
}

fn today_display() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

fn today_naive() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

fn can_mutate(user: &CurrentUser) -> bool {
    user.has_role(&[Role::Admin, Role::Staff])
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
// GET /fees
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListParams {
    q: Option<String>,
    category_id: Option<i64>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "fees/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    q: String,
    category_id: Option<i64>,
    categories: Vec<fee_ext_model::CategoryRow>,
    invoices: Vec<fee_model::InvoiceView>,
    can_mutate: bool,
}

async fn list_fees(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    let q = params.q.unwrap_or_default();
    let rows =
        fee_model::outstanding_invoices_filtered(&state.pool, &q, params.category_id).await?;
    let categories = fee_ext_model::list_categories(&state.pool)
        .await
        .unwrap_or_default();
    let today = today_naive();
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "fees",
        section_label: "Fees",
        today: today_display(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        q,
        category_id: params.category_id,
        categories,
        invoices: rows.iter().map(|r| fee_model::to_view(r, today)).collect(),
        can_mutate: can_mutate(&user),
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /invoices
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct InvoiceForm {
    #[serde(default)]
    admission_no: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    amount: String,
    #[serde(default)]
    due_date: String,
}

async fn create_invoice_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<InvoiceForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let student = fee_model::find_student_by_admission(&state.pool, &form.admission_no).await?;
    let Some(student) = student else {
        return Ok(flash_redirect("/fees", "Unknown admission number", "error"));
    };
    let amount_cents = match fee_model::parse_amount_cents(&form.amount) {
        Ok(c) => c,
        Err(msg) => return Ok(flash_redirect("/fees", &msg, "error")),
    };
    let due = match fee_model::validate_invoice(&form.title, amount_cents, &form.due_date) {
        Ok(d) => d,
        Err(msg) => return Ok(flash_redirect("/fees", &msg, "error")),
    };
    match fee_model::create_invoice(
        &state.pool,
        student.id,
        form.title.trim(),
        amount_cents,
        due,
        Some(user.id),
    )
    .await
    {
        Ok(row) => {
            if let Err(e) = audit_model::log_action(
                &state.pool,
                Some(user.id),
                "create",
                "invoice",
                Some(row.id),
                Some("invoice created"),
            )
            .await
            {
                tracing::warn!(error = %e, "audit log failed");
            }
            Ok(Redirect::to(&format!("/fees/{}", row.id)))
        }
        Err(e) => {
            tracing::error!(error = %e, "create invoice failed");
            Ok(flash_redirect("/fees", "Could not create invoice", "error"))
        }
    }
}

// ---------------------------------------------------------------------------
// GET /fees/:id
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DetailParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct PaymentView {
    id: i64,
    amount: String,
    method: String,
    reference: String,
    paid_at: String,
}

struct FineView {
    id: i64,
    amount: String,
    reason: String,
}

#[derive(Template)]
#[template(path = "fees/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    invoice: fee_model::InvoiceView,
    payments: Vec<PaymentView>,
    category_name: Option<String>,
    discount: String,
    fines: Vec<FineView>,
    can_mutate: bool,
}

async fn fee_detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<DetailParams>,
) -> Result<impl IntoResponse, AppError> {
    let (row, payments) = fee_model::invoice_detail(&state.pool, id).await?;
    let today = today_naive();
    let category_name = match row.category_id {
        Some(cid) => fee_ext_model::find_category(&state.pool, cid)
            .await
            .ok()
            .flatten()
            .map(|c| c.name),
        None => None,
    };
    let fines = fee_ext_model::list_fines(&state.pool, id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|f| FineView {
            id: f.id,
            amount: util::format_cents(f.amount_cents),
            reason: f.reason,
        })
        .collect();
    let (user_name, user_initial) = user_ctx(&user);
    let t = DetailTemplate {
        section: "fees",
        section_label: "Fees",
        today: today_display(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        invoice: fee_model::to_view(&row, today),
        payments: payments
            .into_iter()
            .map(|p| PaymentView {
                id: p.id,
                amount: util::format_cents(p.amount_cents),
                method: p.method,
                reference: p.reference.unwrap_or_default(),
                paid_at: p.paid_at.format("%Y-%m-%d %H:%M").to_string(),
            })
            .collect(),
        category_name,
        discount: util::format_cents(row.discount_cents),
        fines,
        can_mutate: can_mutate(&user),
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /fees/:id/payments
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PaymentForm {
    #[serde(default)]
    amount: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    reference: String,
}

async fn record_payment_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<PaymentForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let target = format!("/fees/{id}");
    let amount_cents = match fee_model::parse_amount_cents(&form.amount) {
        Ok(c) => c,
        Err(msg) => return Ok(flash_redirect(&target, &msg, "error")),
    };
    let reference = form.reference.trim();
    match fee_model::record_payment(
        &state.pool,
        id,
        amount_cents,
        form.method.trim(),
        if reference.is_empty() {
            None
        } else {
            Some(reference)
        },
        Some(user.id),
    )
    .await
    {
        Ok(_) => Ok(flash_redirect(&target, "Payment recorded", "ok")),
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect(&target, &msg, "error")),
        Err(AppError::NotFound) => Ok(flash_redirect("/fees", "Invoice not found", "error")),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// POST /fees/:id/delete
// ---------------------------------------------------------------------------

async fn delete_invoice_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let (row, _) = match fee_model::invoice_detail(&state.pool, id).await {
        Ok(detail) => detail,
        Err(AppError::NotFound) => {
            return Ok(flash_redirect("/fees", "Invoice not found", "error"));
        }
        Err(e) => return Err(e),
    };
    if !fee_model::can_delete_invoice(row.paid) {
        return Ok(flash_redirect(
            &format!("/fees/{id}"),
            "Cannot delete an invoice with payments — record adjustments instead",
            "error",
        ));
    }
    if fee_model::delete_invoice(&state.pool, id).await? {
        if let Err(e) = audit_model::log_action(
            &state.pool,
            Some(user.id),
            "delete",
            "invoice",
            Some(id),
            Some("invoice deleted"),
        )
        .await
        {
            tracing::warn!(error = %e, "audit log failed");
        }
        Ok(flash_redirect("/fees", "Invoice deleted", "ok"))
    } else {
        Ok(flash_redirect("/fees", "Invoice not found", "error"))
    }
}
