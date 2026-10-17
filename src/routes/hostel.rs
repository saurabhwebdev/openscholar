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
use crate::error::AppError;
use crate::models::hostel as hm;
use crate::models::student::{list_students, StudentRow};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/hostel", get(list))
        .route("/hostel/rooms", post(create_room))
        .route("/hostel/rooms/{id}/delete", post(delete_room))
        .route("/hostel/allocations", post(create_allocation))
        .route("/hostel/allocations/{id}/release", post(release_allocation))
        .route("/hostel/my", get(my_allocations))
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

fn back_to(base: &str, msg: &str) -> Redirect {
    Redirect::to(&format!(
        "{base}?flash={}&flash_kind=error",
        util::url_encode(msg)
    ))
}

// ---- list ----

#[derive(Template)]
#[template(path = "hostel/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    rooms: Vec<hm::RoomRow>,
    allocations: Vec<hm::AllocationRow>,
    students: Vec<StudentOption>,
    flash: Option<String>,
    flash_kind: String,
}

#[derive(Clone)]
struct StudentOption {
    id: i64,
    name: String,
}

async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let rooms = hm::room_occupancy(&state.pool).await?;
    let allocations = hm::allocations_list(&state.pool).await?;
    let student_rows = list_students(&state.pool).await.unwrap_or_default();
    let students: Vec<StudentOption> = student_rows
        .into_iter()
        .map(|s: StudentRow| StudentOption {
            id: s.id,
            name: format!("{} {}", s.first_name, s.last_name),
        })
        .collect();
    let (flash, flash_kind) = flash_params(&params);
    let template = ListTemplate {
        section: "hostel",
        section_label: "Hostel",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        rooms,
        allocations,
        students,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- rooms ----

#[derive(Debug, Deserialize)]
struct RoomForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    block: String,
    #[serde(default)]
    capacity: String,
}

async fn create_room(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<RoomForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let capacity: i32 = form.capacity.trim().parse().unwrap_or(1);
    let block = if form.block.trim().is_empty() {
        None
    } else {
        Some(form.block.trim().to_string())
    };
    match hm::create_room(&state.pool, &form.name, block, capacity).await {
        Ok(_) => Ok(Redirect::to("/hostel?flash=Room+added&flash_kind=ok").into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to("/hostel", &msg).into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_room(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    match hm::delete_room(&state.pool, id).await {
        Ok(_) => Ok(Redirect::to("/hostel?flash=Room+deleted&flash_kind=ok").into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to("/hostel", &msg).into_response()),
        Err(e) => Err(e),
    }
}

// ---- allocations ----

#[derive(Debug, Deserialize)]
struct AllocationForm {
    #[serde(default)]
    student_id: String,
    #[serde(default)]
    room_id: String,
    #[serde(default)]
    bed_label: String,
}

async fn create_allocation(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<AllocationForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let student_id: i64 = form.student_id.trim().parse().unwrap_or(0);
    let room_id: i64 = form.room_id.trim().parse().unwrap_or(0);
    let bed_label = if form.bed_label.trim().is_empty() {
        None
    } else {
        Some(form.bed_label.trim().to_string())
    };
    if let Err(msg) = hm::validate_allocation(student_id, room_id, bed_label.as_deref()) {
        return Ok(back_to("/hostel", &msg).into_response());
    }
    match hm::add_allocation(&state.pool, student_id, room_id, bed_label).await {
        Ok(_) => Ok(Redirect::to("/hostel?flash=Allocation+added&flash_kind=ok").into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to("/hostel", &msg).into_response()),
        Err(e) => Err(e),
    }
}

async fn release_allocation(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    match hm::release_allocation(&state.pool, id).await {
        Ok(_) => Ok(Redirect::to("/hostel?flash=Allocation+released&flash_kind=ok").into_response()),
        Err(AppError::NotFound) => Ok(back_to("/hostel", "Allocation not found").into_response()),
        Err(e) => Err(e),
    }
}

// ---- my allocations ----

#[derive(Template)]
#[template(path = "hostel/my.html")]
struct MyTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    allocations: Vec<hm::AllocationRow>,
    flash: Option<String>,
    flash_kind: String,
}

async fn my_allocations(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let mut student_ids: Vec<i64> = Vec::new();
    if user.has_role(&[Role::Parent]) {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT s.id FROM students s WHERE s.id IN (SELECT student_id FROM parent_links WHERE parent_user_id = $1)",
        )
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;
        student_ids = ids;
    } else if user.has_role(&[Role::Student]) {
        let own: Option<i64> =
            sqlx::query_scalar("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
                .bind(user.id)
                .fetch_optional(&state.pool)
                .await?;
        if let Some(id) = own {
            student_ids.push(id);
        }
    } else {
        // staff: show all allocations via allocations_list
    }

    let allocations = if user.has_role(&[Role::Admin, Role::Teacher, Role::Staff]) {
        hm::allocations_list(&state.pool).await?
    } else {
        let mut all: Vec<hm::AllocationRow> = Vec::new();
        for sid in &student_ids {
            let rows = hm::list_for_student(&state.pool, *sid).await?;
            all.extend(rows);
        }
        all
    };

    let (flash, flash_kind) = flash_params(&params);
    let template = MyTemplate {
        section: "hostel",
        section_label: "Hostel",
        today: today(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        allocations,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}
