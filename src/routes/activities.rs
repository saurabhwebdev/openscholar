use askama::Template;
use axum::{
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::collections::HashMap;

use crate::error::AppError;
use crate::models::activity::{self as am, ActivityInput};
use crate::models::student::list_students;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/activities", get(activities_list).post(create_activity_submit))
        .route("/activities/my", get(my_activities))
        .route("/activities/{id}", get(activity_detail))
        .route("/activities/{id}/members", post(add_member_submit))
        .route("/activity-members/{id}/delete", post(delete_member_submit))
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
    crate::auth::middleware::require_roles(user, &[Role::Admin, Role::Staff])
}

// ---- list ----

#[derive(Template)]
#[template(path = "activities/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    activities: Vec<am::ActivityRow>,
    can_mutate: bool,
}

async fn activities_list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let activities = am::list_activities(&state.pool).await?;
    let (flash, flash_kind) = flash_params(&params);
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "activities",
        section_label: "Activities",
        today: today_display(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        activities,
        can_mutate: true,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---- create ----

#[derive(Debug, Deserialize)]
struct ActivityForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    supervisor_id: String,
}

async fn create_activity_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ActivityForm>,
) -> Result<Response, AppError> {
    check_mutate(&user)?;
    let supervisor_id = form.supervisor_id.trim().parse::<i64>().ok();
    let input = ActivityInput {
        name: form.name.clone(),
        kind: form.kind.clone(),
        description: if form.description.trim().is_empty() {
            None
        } else {
            Some(form.description.trim().to_string())
        },
        supervisor_id,
    };
    if let Err(msg) = am::validate_activity(&input.name) {
        return Ok(flash_redirect("/activities", &msg, "error").into_response());
    }
    match am::create_activity(&state.pool, &input).await {
        Ok(row) => Ok(Redirect::to(&format!("/activities/{}", row.id)).into_response()),
        Err(e) => Err(e),
    }
}

// ---- detail ----

struct MemberView {
    id: i64,
    student_id: i64,
    student_name: String,
    joined_on: String,
}

#[derive(Template)]
#[template(path = "activities/detail.html")]
struct DetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    activity: am::ActivityRow,
    members: Vec<MemberView>,
    students: Vec<(i64, String)>,
    can_mutate: bool,
    flash: Option<String>,
    flash_kind: String,
}

async fn activity_detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    // Staff-side only: the family workspace uses the scoped `/activities/my`
    // list. Without this gate any authenticated parent/student could read any
    // activity's member roster (student names) by id (IDOR).
    crate::auth::middleware::require_roles(&user, &[Role::Admin, Role::Staff, Role::Teacher])?;
    let activity = am::find_activity(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let members_rows = am::members_for_activity(&state.pool, id).await.unwrap_or_default();
    // fetch student names
    let mut members = Vec::new();
    for m in members_rows {
        let name: Option<String> = sqlx::query_scalar(
            "SELECT first_name || ' ' || last_name FROM students WHERE id = $1",
        )
        .bind(m.student_id)
        .fetch_optional(&state.pool)
        .await?;
        members.push(MemberView {
            id: m.id,
            student_id: m.student_id,
            student_name: name.unwrap_or_else(|| format!("#{}", m.student_id)),
            joined_on: m.joined_on.to_string(),
        });
    }
    let students_rows = list_students(&state.pool).await.unwrap_or_default();
    let students: Vec<(i64, String)> = students_rows
        .into_iter()
        .map(|s| (s.id, format!("{} {}", s.first_name, s.last_name)))
        .collect();
    let (flash, flash_kind) = flash_params(&params);
    let (user_name, user_initial) = user_ctx(&user);
    let t = DetailTemplate {
        section: "activities",
        section_label: "Activities",
        today: today_display(),
        user_name,
        user_initial,
        activity,
        members,
        students,
        can_mutate: user.has_role(&[Role::Admin, Role::Staff]),
        flash,
        flash_kind,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---- add member ----

#[derive(Debug, Deserialize)]
struct MemberForm {
    #[serde(default)]
    student_id: String,
}

async fn add_member_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<MemberForm>,
) -> Result<Response, AppError> {
    check_mutate(&user)?;
    let student_id: i64 = match form.student_id.trim().parse() {
        Ok(v) => v,
        Err(_) => return Ok(flash_redirect(&format!("/activities/{id}"), "Invalid student", "error").into_response()),
    };
    match am::add_member(&state.pool, id, student_id, None).await {
        Ok(_) => Ok(Redirect::to(&format!("/activities/{id}?flash=Member+added&flash_kind=ok")).into_response()),
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect(&format!("/activities/{id}"), &msg, "error").into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_member_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    check_mutate(&user)?;
    // find activity for redirect
    let member = sqlx::query_scalar::<_, i64>("SELECT activity_id FROM activity_members WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;
    match am::remove_member(&state.pool, id).await {
        Ok(_) => {
            let target = member
                .map(|a| format!("/activities/{a}?flash=Member+removed&flash_kind=ok"))
                .unwrap_or_else(|| "/activities?flash=Member+removed&flash_kind=ok".to_string());
            Ok(Redirect::to(&target).into_response())
        }
        Err(e) => Err(e),
    }
}

// ---- my activities ----

#[derive(Template)]
#[template(path = "activities/list.html")]
struct MyActivitiesTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    activities: Vec<am::ActivityRow>,
    can_mutate: bool,
}

async fn my_activities(
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

    let activities = if user.has_role(&[Role::Admin, Role::Teacher, Role::Staff]) {
        am::list_activities(&state.pool).await?
    } else {
        let mut all = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sid in &student_ids {
            let acts = am::activities_for_student(&state.pool, *sid).await?;
            for a in acts {
                if seen.insert(a.id) {
                    all.push(a);
                }
            }
        }
        all
    };

    let (flash, flash_kind) = flash_params(&params);
    let (user_name, user_initial) = user_ctx(&user);
    let t = MyActivitiesTemplate {
        section: "activities",
        section_label: "Activities",
        today: today_display(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        activities,
        can_mutate: false,
    };
    t.render().map(Html).map_err(AppError::from)
}