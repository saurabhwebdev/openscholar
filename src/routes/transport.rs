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
use crate::models::student::{list_students, StudentRow};
use crate::models::transport::{
    self as tr, RouteInput, RiderInput, StopInput, VehicleInput,
};
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/transport", get(list))
        .route("/vehicles", post(create_vehicle))
        .route("/vehicles/{id}/delete", post(delete_vehicle))
        .route("/transport-routes", post(create_route))
        .route("/transport-routes/{id}/delete", post(delete_route))
        .route("/transport-routes/{id}/stops", get(route_detail))
        .route("/transport-stops", post(create_stop))
        .route("/transport-stops/{id}/delete", post(delete_stop))
        .route("/transport-riders", post(create_rider))
        .route("/transport-riders/{id}/delete", post(delete_rider))
        .route("/transport/my", get(my_riders))
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

// ---- list ----

#[derive(Template)]
#[template(path = "transport/list.html")]
struct ListTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    vehicles: Vec<tr::VehicleRow>,
    routes: Vec<tr::RouteRow>,
    flash: Option<String>,
    flash_kind: String,
}

async fn list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let vehicles = tr::list_vehicles(&state.pool).await?;
    let routes = tr::list_routes(&state.pool).await?;
    let (flash, flash_kind) = flash_params(&params);
    let template = ListTemplate {
        section: "transport",
        section_label: "Transport",
        today: today_str(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        vehicles,
        routes,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- vehicles ----

#[derive(Debug, Deserialize)]
struct VehicleForm {
    #[serde(default)]
    reg_no: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    capacity: String,
    #[serde(default)]
    driver_name: String,
    #[serde(default)]
    driver_phone: String,
    #[serde(default)]
    active: Option<String>,
}

async fn create_vehicle(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<VehicleForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let capacity: i32 = form.capacity.trim().parse().unwrap_or(1);
    let input = VehicleInput {
        reg_no: form.reg_no.clone(),
        kind: form.kind.clone(),
        capacity,
        driver_name: empty_to_opt(&form.driver_name),
        driver_phone: empty_to_opt(&form.driver_phone),
        active: form.active.is_some(),
    };
    if let Err(msg) = tr::validate_vehicle(&input.reg_no, input.capacity) {
        return Ok(back_to("/transport", &msg).into_response());
    }
    match tr::create_vehicle(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to("/transport?flash=Vehicle+added&flash_kind=ok").into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_vehicle(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    match tr::delete_vehicle(&state.pool, id).await {
        Ok(_) => Ok(Redirect::to("/transport?flash=Vehicle+deleted&flash_kind=ok").into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to("/transport", &msg).into_response()),
        Err(e) => Err(e),
    }
}

// ---- routes ----

#[derive(Debug, Deserialize)]
struct RouteForm {
    #[serde(default)]
    name: String,
    #[serde(default)]
    vehicle_id: String,
    #[serde(default)]
    fee_cents: String,
}

async fn create_route(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<RouteForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let vehicle_id = if form.vehicle_id.trim().is_empty() {
        None
    } else {
        form.vehicle_id.trim().parse::<i64>().ok()
    };
    let fee_cents: i64 = form.fee_cents.trim().parse().unwrap_or(0);
    let input = RouteInput {
        name: form.name.clone(),
        vehicle_id,
        fee_cents,
    };
    if let Err(msg) = tr::validate_route(&input.name) {
        return Ok(back_to("/transport", &msg).into_response());
    }
    match tr::create_route(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to("/transport?flash=Route+added&flash_kind=ok").into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_route(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    match tr::delete_route(&state.pool, id).await {
        Ok(_) => Ok(Redirect::to("/transport?flash=Route+deleted&flash_kind=ok").into_response()),
        Err(e) => Err(e),
    }
}

// ---- route detail ----

#[derive(Template)]
#[template(path = "transport/route_detail.html")]
struct RouteDetailTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    route: tr::RouteRow,
    stops: Vec<tr::StopRow>,
    riders: Vec<tr::RiderRow>,
    students: Vec<StudentOption>,
    over_capacity: bool,
    flash: Option<String>,
    flash_kind: String,
}

#[derive(Clone)]
struct StudentOption {
    id: i64,
    name: String,
}

async fn route_detail(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let route = tr::find_route(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let stops = tr::list_stops(&state.pool, id).await?;
    let riders = tr::riders_for_route(&state.pool, id).await?;
    let over = tr::capacity_check(&state.pool, id).await.unwrap_or(false);
    let students_rows = list_students(&state.pool).await?;
    let students: Vec<StudentOption> = students_rows
        .into_iter()
        .map(|s: StudentRow| StudentOption {
            id: s.id,
            name: format!("{} {}", s.first_name, s.last_name),
        })
        .collect();
    let (flash, flash_kind) = flash_params(&params);
    let template = RouteDetailTemplate {
        section: "transport",
        section_label: "Transport",
        today: today_str(),
        user_name: user.full_name.clone(),
        user_initial: user_initial(&user),
        route,
        stops,
        riders,
        students,
        over_capacity: over,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

// ---- stops ----

#[derive(Debug, Deserialize)]
struct StopForm {
    #[serde(default)]
    route_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    sequence: String,
}

async fn create_stop(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<StopForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let route_id: i64 = form.route_id.trim().parse().unwrap_or(0);
    let sequence: i32 = form.sequence.trim().parse().unwrap_or(0);
    let input = StopInput {
        route_id,
        name: form.name.clone(),
        sequence,
    };
    if let Err(msg) = tr::validate_stop(&input.name) {
        return Ok(back_to(&format!("/transport-routes/{route_id}/stops"), &msg).into_response());
    }
    if route_id <= 0 {
        return Ok(back_to("/transport", "Route is required").into_response());
    }
    match tr::create_stop(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to(&format!(
            "/transport-routes/{route_id}/stops?flash=Stop+added&flash_kind=ok"
        ))
        .into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_stop(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    // find route for redirect
    let stop = tr::find_stop(&state.pool, id).await?;
    let route_id = stop.map(|s| s.route_id);
    match tr::delete_stop(&state.pool, id).await {
        Ok(_) => {
            let target = route_id
                .map(|r| format!("/transport-routes/{r}/stops?flash=Stop+deleted&flash_kind=ok"))
                .unwrap_or_else(|| "/transport?flash=Stop+deleted&flash_kind=ok".to_string());
            Ok(Redirect::to(&target).into_response())
        }
        Err(e) => Err(e),
    }
}

// ---- riders ----

#[derive(Debug, Deserialize)]
struct RiderForm {
    #[serde(default)]
    student_id: String,
    #[serde(default)]
    route_id: String,
    #[serde(default)]
    stop_id: String,
}

async fn create_rider(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<RiderForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let student_id: i64 = form.student_id.trim().parse().unwrap_or(0);
    let route_id: i64 = form.route_id.trim().parse().unwrap_or(0);
    let stop_id = if form.stop_id.trim().is_empty() {
        None
    } else {
        form.stop_id.trim().parse::<i64>().ok()
    };
    let input = RiderInput {
        student_id,
        route_id,
        stop_id,
    };
    if let Err(msg) = tr::validate_rider(input.student_id, input.route_id) {
        let target = if route_id > 0 {
            format!("/transport-routes/{route_id}/stops")
        } else {
            "/transport".to_string()
        };
        return Ok(back_to(&target, &msg).into_response());
    }
    match tr::add_rider(&state.pool, &input).await {
        Ok(_) => Ok(Redirect::to(&format!(
            "/transport-routes/{route_id}/stops?flash=Rider+added&flash_kind=ok"
        ))
        .into_response()),
        Err(AppError::BadRequest(msg)) => Ok(back_to(
            &format!("/transport-routes/{route_id}/stops"),
            &msg,
        )
        .into_response()),
        Err(e) => Err(e),
    }
}

async fn delete_rider(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let rider = tr::find_rider(&state.pool, id).await?;
    let route_id = rider.map(|r| r.route_id);
    match tr::delete_rider(&state.pool, id).await {
        Ok(_) => {
            let target = route_id
                .map(|r| format!("/transport-routes/{r}/stops?flash=Rider+removed&flash_kind=ok"))
                .unwrap_or_else(|| "/transport?flash=Rider+removed&flash_kind=ok".to_string());
            Ok(Redirect::to(&target).into_response())
        }
        Err(e) => Err(e),
    }
}

// ---- my riders ----

#[derive(Template)]
#[template(path = "transport/my.html")]
struct MyTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    riders: Vec<tr::RiderRow>,
    routes: Vec<tr::RouteRow>,
    flash: Option<String>,
    flash_kind: String,
}

async fn my_riders(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    // determine student ids
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
        // staff/admin/teacher: see all riders -> use all students? Instead list all riders directly.
        // We'll handle below by loading all riders.
    }

    let (riders, routes) = if user.has_role(&[Role::Admin, Role::Teacher, Role::Staff]) {
        let mut all_riders: Vec<tr::RiderRow> = Vec::new();
        let routes = tr::list_routes(&state.pool).await?;
        for rt in &routes {
            let rs = tr::riders_for_route(&state.pool, rt.id).await?;
            all_riders.extend(rs);
        }
        (all_riders, routes)
    } else {
        let mut all_riders: Vec<tr::RiderRow> = Vec::new();
        let mut route_ids: Vec<i64> = Vec::new();
        for sid in &student_ids {
            let rs = tr::riders_for_student(&state.pool, *sid).await?;
            for r in &rs {
                if !route_ids.contains(&r.route_id) {
                    route_ids.push(r.route_id);
                }
            }
            all_riders.extend(rs);
        }
        // fetch routes for those
        let all_routes = tr::list_routes(&state.pool).await?;
        let routes: Vec<tr::RouteRow> = all_routes
            .into_iter()
            .filter(|rt| route_ids.contains(&rt.id))
            .collect();
        (all_riders, routes)
    };

    let (flash, flash_kind) = flash_params(&params);
    let (user_name, user_initial) = (user.full_name.clone(), user_initial(&user));
    let template = MyTemplate {
        section: "transport",
        section_label: "Transport",
        today: today_str(),
        user_name,
        user_initial,
        riders,
        routes,
        flash,
        flash_kind,
    };
    template.render().map(Html).map_err(AppError::from)
}

fn empty_to_opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}
