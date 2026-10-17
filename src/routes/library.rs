use askama::Template;
use axum::{
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use chrono::NaiveDate;
use serde::Deserialize;
use std::collections::HashMap;

use crate::auth::middleware::require_roles;
use crate::error::AppError;
use crate::models::library::{
    self as lib, BookInput, BookRow, LoanView,
};
use crate::models::student::{list_students, StudentRow};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/library", get(catalog))
        .route("/books", post(create_book))
        .route("/books/{id}/delete", post(delete_book))
        .route("/library/borrow", post(borrow_book))
        .route("/library/loans/{id}/return", post(return_loan))
        .route("/library/loans", get(loans_page))
}

fn user_initial(user: &CurrentUser) -> String {
    user.full_name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn today_str() -> String {
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

fn back_to(base: &str, msg: &str) -> Redirect {
    Redirect::to(&format!(
        "{base}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
}

#[derive(Template)]
#[template(path = "library/catalog.html")]
struct CatalogTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    books: Vec<BookRow>,
    students: Vec<StudentOption>,
    q: String,
    can_edit: bool,
    flash: Option<String>,
    flash_kind: String,
}

#[derive(Clone)]
struct StudentOption {
    id: i64,
    name: String,
}

#[derive(Template)]
#[template(path = "library/loans.html")]
struct LoansTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    loans: Vec<LoanView>,
    overdue: Vec<LoanView>,
    can_edit: bool,
    flash: Option<String>,
    flash_kind: String,
}

async fn catalog(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let q = params.get("q").cloned().unwrap_or_default();
    let books = if q.trim().is_empty() {
        lib::list_books(&state.pool).await?
    } else {
        lib::search_books(&state.pool, &q).await?
    };
    let students_rows = list_students(&state.pool).await?;
    let students: Vec<StudentOption> = students_rows
        .into_iter()
        .map(|s: StudentRow| StudentOption {
            id: s.id,
            name: format!("{} {}", s.first_name, s.last_name),
        })
        .collect();
    let (flash, flash_kind) = flash_params(&params);
    let template = CatalogTemplate {
        section: "library",
        section_label: "Library",
        today: today_str(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        books,
        students,
        q,
        can_edit: user.has_role(&[Role::Admin, Role::Staff]),
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

#[derive(Debug, Deserialize)]
struct BookForm {
    #[serde(default)]
    isbn: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    copies: String,
    #[serde(default)]
    available: String,
}

async fn create_book(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<BookForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let copies: i32 = form.copies.trim().parse().unwrap_or(1);
    let available: i32 = form.available.trim().parse().unwrap_or(copies);
    let isbn = empty_to_opt(&form.isbn);
    let author = empty_to_opt(&form.author);
    let category = empty_to_opt(&form.category);
    let input = BookInput {
        isbn,
        title: form.title.clone(),
        author,
        category,
        copies,
        available,
    };
    if let Err(msg) = lib::validate_book(&input.title, input.copies, input.available) {
        return Ok(back_to("/library", &msg).into_response());
    }
    match lib::create_book(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to("/library?flash=Book+added&flash_kind=ok").into_response()),
        Err(e) => Err(e),
    }
}

fn empty_to_opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

async fn delete_book(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    match lib::delete_book(&state.pool, id).await {
        Ok(_) => Ok(Redirect::to("/library?flash=Book+deleted&flash_kind=ok").into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to("/library", &msg).into_response()),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Deserialize)]
struct BorrowForm {
    book_id: i64,
    student_id: i64,
    due_on: String,
}

async fn borrow_book(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<BorrowForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let due_on = NaiveDate::parse_from_str(form.due_on.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::BadRequest("Invalid due date".to_string()))?;
    let borrowed_on = chrono::Local::now().date_naive();
    if let Err(msg) = lib::validate_loan(borrowed_on, due_on) {
        return Ok(back_to("/library", &msg).into_response());
    }
    match lib::borrow_on(&state.pool, form.book_id, form.student_id, borrowed_on, due_on).await {
        Ok(_) => Ok(Redirect::to("/library?flash=Borrowed&flash_kind=ok").into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to("/library", &msg).into_response()),
        Err(e) => Err(e),
    }
}

async fn return_loan(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let target = "/library/loans".to_string();
    match lib::return_book(&state.pool, id).await {
        Ok(_) => Ok(Redirect::to(&format!(
            "/library/loans?flash={}&flash_kind=ok",
            util::url_encode("Returned")
        ))
        .into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to(&target, &msg).into_response()),
        Err(e) => Err(e),
    }
}

async fn loans_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let loans = lib::open_loans(&state.pool).await?;
    let overdue = lib::overdue_loans(&state.pool).await?;
    let (flash, flash_kind) = flash_params(&params);
    let template = LoansTemplate {
        section: "library",
        section_label: "Library",
        today: today_str(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        loans,
        overdue,
        can_edit: true,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}
