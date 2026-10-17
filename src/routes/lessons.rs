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

use crate::error::AppError;
use crate::models::grades as grades_model;
use crate::models::lesson::{self as lm, LessonInput};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/lessons", get(lessons_list).post(create_lesson_submit))
        .route("/lessons/my", get(my_lessons))
        .route("/lessons/{id}", get(lesson_detail))
        .route("/lessons/{id}/materials", post(add_material_submit))
        .route("/materials/{id}/delete", post(delete_material_submit))
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

fn today_display() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

fn flash_params(params: &HashMap<String, String>) -> (Option<String>, String) {
    (
        params.get("flash").cloned(),
        params
            .get("flash_kind")
            .cloned()
            .unwrap_or_else(|| "info".to_string()),
    )
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

fn check_mutate(user: &CurrentUser) -> Result<(), AppError> {
    crate::auth::middleware::require_roles(user, &[Role::Admin, Role::Staff, Role::Teacher])
}

// ---- list ----

#[derive(Debug, Deserialize)]
struct ListParams {
    class_id: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct ClassOptionView {
    id: i64,
    name: String,
    selected: bool,
}

#[derive(Template)]
#[template(path = "lessons/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    classes: Vec<ClassOptionView>,
    selected_class: Option<i64>,
    lessons: Vec<lm::LessonRow>,
    can_mutate: bool,
    today_date: String,
}

async fn lessons_list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let class_options = grades_model::list_class_options(&state.pool).await.unwrap_or_default();
    let selected = params
        .class_id
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .and_then(|id| class_options.iter().find(|c| c.id == id).map(|c| c.id))
        .or_else(|| class_options.first().map(|c| c.id));
    let classes = class_options;
    let lessons = match selected {
        Some(cid) => lm::list_for_class(&state.pool, cid).await?,
        None => Vec::new(),
    };
    let (user_name, user_initial) = user_ctx(&user);
    let (flash, flash_kind) = (params.flash, params.flash_kind.unwrap_or_else(|| "info".to_string()));
    let t = ListTemplate {
        section: "lessons",
        section_label: "Lessons",
        today: today_display(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        classes: classes
            .into_iter()
            .map(|c| ClassOptionView {
                id: c.id,
                name: c.name.clone(),
                selected: Some(c.id) == selected,
            })
            .collect(),
        selected_class: selected,
        lessons,
        can_mutate: true,
        today_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---- create ----

#[derive(Debug, Deserialize)]
struct LessonForm {
    #[serde(default)]
    class_id: String,
    #[serde(default)]
    subject_id: String,
    #[serde(default)]
    teacher_id: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    homework: String,
}

async fn create_lesson_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<LessonForm>,
) -> Result<Response, AppError> {
    check_mutate(&user)?;
    let class_id: i64 = match form.class_id.trim().parse() {
        Ok(v) => v,
        Err(_) => return Ok(flash_redirect("/lessons", "Invalid class", "error").into_response()),
    };
    if let Err(msg) = lm::validate_lesson(&form.title, &form.date) {
        return Ok(flash_redirect(&format!("/lessons?class_id={class_id}"), &msg, "error").into_response());
    }
    let date = NaiveDate::parse_from_str(form.date.trim(), "%Y-%m-%d").unwrap();
    let subject_id = form.subject_id.trim().parse::<i64>().ok();
    let teacher_id = form.teacher_id.trim().parse::<i64>().ok();
    let body = if form.body.trim().is_empty() { None } else { Some(form.body.trim().to_string()) };
    let homework = if form.homework.trim().is_empty() { None } else { Some(form.homework.trim().to_string()) };
    let input = LessonInput {
        class_id,
        subject_id,
        teacher_id,
        date,
        title: form.title.clone(),
        body,
        homework,
    };
    match lm::create_lesson(&state.pool, &input).await {
        Ok(row) => Ok(Redirect::to(&format!("/lessons/{}", row.id)).into_response()),
        Err(e) => Err(e),
    }
}

// ---- detail ----

#[derive(Template)]
#[template(path = "lessons/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    lesson: lm::LessonRow,
    materials: Vec<lm::MaterialRow>,
    can_mutate: bool,
    flash: Option<String>,
    flash_kind: String,
}

async fn lesson_detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    // Staff-side only: the family workspace uses the scoped `/lessons/my` list.
    // Without this gate any authenticated parent/student could read any lesson
    // (body/homework + material keys) by id (IDOR).
    check_mutate(&user)?;
    let lesson = lm::find_lesson(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let materials = lm::list_materials(&state.pool, id).await.unwrap_or_default();
    let (flash, flash_kind) = flash_params(&params);
    let (user_name, user_initial) = user_ctx(&user);
    let t = DetailTemplate {
        section: "lessons",
        section_label: "Lessons",
        today: today_display(),
        user_name,
        user_initial,
        lesson,
        materials,
        can_mutate: user.has_role(&[Role::Admin, Role::Staff, Role::Teacher]),
        flash,
        flash_kind,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---- materials ----

#[derive(Debug, Deserialize)]
struct MaterialForm {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    storage_key: String,
    #[serde(default)]
    size_bytes: String,
}

async fn add_material_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<MaterialForm>,
) -> Result<Response, AppError> {
    check_mutate(&user)?;
    let size: i64 = form.size_bytes.trim().parse().unwrap_or(0);
    let storage_key = if form.storage_key.trim().is_empty() {
        form.filename.trim().to_string()
    } else {
        form.storage_key.trim().to_string()
    };
    match lm::add_material(&state.pool, id, &form.filename, &storage_key, size).await {
        Ok(_) => Ok(Redirect::to(&format!("/lessons/{id}?flash=Material+added&flash_kind=ok")).into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_material_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    check_mutate(&user)?;
    let mat = lm::find_material(&state.pool, id).await?;
    let lesson_id = mat.map(|m| m.lesson_id);
    match lm::delete_material(&state.pool, id).await {
        Ok(_) => {
            let target = lesson_id
                .map(|l| format!("/lessons/{l}?flash=Material+deleted&flash_kind=ok"))
                .unwrap_or_else(|| "/lessons?flash=Material+deleted&flash_kind=ok".to_string());
            Ok(Redirect::to(&target).into_response())
        }
        Err(e) => Err(e),
    }
}

// ---- my lessons ----

#[derive(Template)]
#[template(path = "lessons/list.html")]
struct MyLessonsTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    classes: Vec<ClassOptionView>,
    selected_class: Option<i64>,
    lessons: Vec<lm::LessonRow>,
    can_mutate: bool,
    today_date: String,
}

async fn my_lessons(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let mut student_ids: Vec<i64> = Vec::new();
    if user.has_role(&[Role::Parent]) {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT student_id FROM parent_links WHERE parent_user_id = $1",
        )
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;
        student_ids = ids;
    } else if user.has_role(&[Role::Student]) {
        let own: Option<i64> = sqlx::query_scalar("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
            .bind(user.id)
            .fetch_optional(&state.pool)
            .await?;
        if let Some(id) = own {
            student_ids.push(id);
        }
    }

    let lessons = if user.has_role(&[Role::Admin, Role::Teacher, Role::Staff]) {
        lm::list_lessons(&state.pool).await?
    } else {
        let mut all = Vec::new();
        for sid in &student_ids {
            let ls = lm::list_for_student(&state.pool, *sid).await?;
            all.extend(ls);
        }
        all
    };

    let (flash, flash_kind) = flash_params(&params);
    let (user_name, user_initial) = user_ctx(&user);
    let t = MyLessonsTemplate {
        section: "lessons",
        section_label: "Lessons",
        today: today_display(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        classes: Vec::new(),
        selected_class: None,
        lessons,
        can_mutate: false,
        today_date: chrono::Local::now().format("%Y-%m-%d").to_string(),
    };
    t.render().map(Html).map_err(AppError::from)
}