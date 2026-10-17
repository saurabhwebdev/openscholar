use std::time::Duration;

use axum::{extract::DefaultBodyLimit, middleware, response::Redirect, routing::get, Router};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::services::ServeDir;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::auth;
use crate::state::AppState;

pub mod activities;
pub mod admissions;
pub mod analytics;
pub mod api;
pub mod attachments;
pub mod audit;
pub mod attendance;
pub mod classes;
pub mod dashboard;
pub mod fees;
pub mod fees_ext;
pub mod grades;
pub mod hostel;
pub mod hr;
pub mod lessons;
pub mod library;
pub mod notices;
pub mod portal;
pub mod print;
pub mod settings;
pub mod staff;
pub mod students;
pub mod timetable;
pub mod transport;
pub mod users;
pub mod welfare;

pub fn app_router(state: AppState) -> Router {
    // Staff-side workspace: parents/students are redirected to /portal.
    let staff = Router::new()
        .merge(dashboard::router())
        .merge(students::router())
        .merge(staff::router())
        .merge(classes::router())
        .merge(attendance::router())
        .merge(grades::router())
        .merge(fees::router())
        .merge(fees_ext::router())
        .merge(timetable::router())
        .merge(admissions::router())
        .merge(library::router())
        .merge(welfare::router())
        .merge(audit::router())
        .merge(settings::router())
        .merge(attachments::router())
        .merge(analytics::router())
        .merge(crate::comms::routes::router())
        .merge(users::router())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_staff_side,
        ));

    // Family workspace: every authenticated role.
    // Modules with both staff pages and `/…/my` views live here; their handlers
    // gate the staff-only pages with `require_roles`.
    let family = Router::new()
        .route("/nav/role", get(auth::routes::nav_role))
        .merge(portal::router())
        .merge(print::router())
        .merge(notices::router())
        .merge(crate::notifications::routes::router())
        .merge(transport::router())
        .merge(hostel::router())
        .merge(hr::router())
        .merge(lessons::router())
        .merge(activities::router())
        .merge(api::router());

    let protected = Router::new().merge(staff).merge(family).route_layer(
        middleware::from_fn_with_state(state.clone(), auth::middleware::require_auth),
    );

    Router::new()
        .route("/", get(|| async { Redirect::to("/dashboard") }))
        .merge(auth::routes::router())
        .merge(api::health_router())
        .merge(protected)
        .nest_service("/static", ServeDir::new("static"))
        // NOTE: `uploads/` is intentionally NOT served as a static mount. Uploaded
        // documents (incl. student welfare/medical) are delivered only through the
        // authenticated, role-gated `/attachments/{id}/download` handler.
        .with_state(state)
        // Cross-cutting layers (last = outermost). CatchPanic is outermost so a
        // panic in any inner layer/handler becomes a clean 500 instead of a
        // dropped connection; Trace logs each request; Timeout sheds slow ones;
        // security headers + CSRF-origin check guard every response/mutation.
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(middleware::from_fn(auth::middleware::csrf_protect))
        .layer(middleware::from_fn(auth::middleware::security_headers))
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::new())
}
