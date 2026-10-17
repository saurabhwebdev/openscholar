use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::auth::middleware::require_roles;
use crate::error::AppError;
use crate::models::activity as activity_model;
use crate::models::hostel as hostel_model;
use crate::models::lesson as lesson_model;
use crate::models::transport as transport_model;
use crate::models::user::{CurrentUser, Role};
use crate::models::welfare as welfare_model;
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/portal", get(portal_home))
        .route("/portal/{id}", get(portal_child))
        .route("/students/{id}/link-parent", post(link_parent_submit))
        .route("/parent-links/{id}/delete", post(delete_parent_link_submit))
        .route("/students/{id}/link-user", post(link_student_user_submit))
}

// ---------------------------------------------------------------------------
// Pure scoping + validation helpers (unit-tested, no DB)
// ---------------------------------------------------------------------------

/// One parent->student edge used for pure scoping decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalLink {
    pub parent_user_id: i64,
    pub student_id: i64,
}

/// Student ids visible in the portal for `(role, user_id)`.
///
/// - Parent: distinct `student_id`s linked to this `user_id`, sorted.
/// - Student: the single owned `students` row, if the login has one.
/// - Staff roles (admin/teacher/staff): empty — they are redirected to `/dashboard`.
pub fn child_ids_for(
    role: &Role,
    user_id: i64,
    links: &[PortalLink],
    own_student_id: Option<i64>,
) -> Vec<i64> {
    match role {
        Role::Parent => {
            let mut ids: Vec<i64> = links
                .iter()
                .filter(|l| l.parent_user_id == user_id)
                .map(|l| l.student_id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        }
        Role::Student => own_student_id.into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Whether `(role, user_id)` may view `target_student_id` in the portal.
/// Staff roles always return false here (they use the staff UI, not `/portal`).
pub fn can_view_child(
    role: &Role,
    user_id: i64,
    links: &[PortalLink],
    own_student_id: Option<i64>,
    target_student_id: i64,
) -> bool {
    match role {
        Role::Parent => links
            .iter()
            .any(|l| l.parent_user_id == user_id && l.student_id == target_student_id),
        Role::Student => own_student_id == Some(target_student_id),
        _ => false,
    }
}

/// Shared email check for the family-access forms: trimmed, one `@`,
/// domain contains a dot, no spaces, max 254 chars. Returns the trimmed value.
pub fn validate_portal_email(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Err("Email is required".to_string());
    }
    if t.len() > 254 {
        return Err("Email is too long".to_string());
    }
    if t.contains(' ') {
        return Err("Enter a valid email address".to_string());
    }
    let mut parts = t.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err("Enter a valid email address".to_string());
    };
    if local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err("Enter a valid email address".to_string());
    }
    Ok(t.to_string())
}

/// Relation for `parent_links.relation`: blank defaults to `guardian`,
/// otherwise the trimmed value (max 50 chars).
pub fn validate_relation(raw: &str) -> Result<String, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok("guardian".to_string());
    }
    if t.len() > 50 {
        return Err("Relation is too long".to_string());
    }
    Ok(t.to_string())
}

/// `(present, total)` over a slice of attendance statuses.
/// Only `"present"` counts as present; every row counts toward total.
pub fn summarize_attendance(statuses: &[&str]) -> (i64, i64) {
    let total = statuses.len() as i64;
    let present = statuses.iter().filter(|s| **s == "present").count() as i64;
    (present, total)
}

/// True when `err` is a Postgres unique-violation (duplicate `parent_links` pair).
pub fn is_duplicate_link(err: &AppError) -> bool {
    match err {
        AppError::Db(sqlx::Error::Database(db)) => db.code().as_deref() == Some("23505"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Shared handler helpers
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

fn is_staff_side(user: &CurrentUser) -> bool {
    user.has_role(&[Role::Admin, Role::Teacher, Role::Staff])
}

// ---------------------------------------------------------------------------
// Portal read models (runtime SQL; NUMERIC::FLOAT8, money SUM::BIGINT)
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct EnrollmentRow {
    class_name: String,
    enrolled_on: NaiveDate,
}

#[derive(Debug, sqlx::FromRow)]
struct AttendanceRow {
    date: NaiveDate,
    status: String,
    class_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AttendanceCounts {
    total: i64,
    present: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ScoreRow {
    assessment_name: String,
    class_name: String,
    score: f64,
    max_score: f64,
    date: NaiveDate,
}

#[derive(Debug, sqlx::FromRow)]
struct OpenInvoiceRow {
    id: i64,
    title: String,
    amount_cents: i64,
    paid: i64,
    balance: i64,
    due_date: NaiveDate,
}

struct EnrollmentView {
    class_name: String,
    enrolled_on: String,
}

struct AttendanceView {
    date: String,
    status: String,
    class_name: String,
}

struct ScoreView {
    name: String,
    class_name: String,
    score: String,
    max_score: String,
    date: String,
}

struct InvoiceLiteView {
    id: i64,
    title: String,
    amount: String,
    paid: String,
    balance: String,
    due: String,
}

#[derive(Clone)]
struct IncidentLiteView {
    date: String,
    kind: String,
    description: String,
}

struct ChildCardView {
    id: i64,
    full_name: String,
    admission_no: String,
    status: String,
    enrolled_on: String,
    attendance_present: i64,
    attendance_total: i64,
    balance: String,
    enrollments: Vec<EnrollmentView>,
    recent_attendance: Vec<AttendanceView>,
    assessments: Vec<ScoreView>,
    invoices: Vec<InvoiceLiteView>,
    recent_incidents: Vec<IncidentLiteView>,
    medical_count: i64,
    transport_routes: Vec<transport_model::RouteRow>,
    hostel_allocation: Option<hostel_model::AllocationRow>,
    lessons: Vec<lesson_model::LessonRow>,
    activities: Vec<activity_model::ActivityRow>,
}

async fn load_child(pool: &sqlx::PgPool, student_id: i64) -> Result<ChildCardView, AppError> {
    let row = crate::models::student::find_student(pool, student_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let full_name = format!("{} {}", row.first_name, row.last_name)
        .trim()
        .to_string();

    let enrollments = sqlx::query_as::<_, EnrollmentRow>(
        "SELECT c.name AS class_name, e.enrolled_on AS enrolled_on
         FROM enrollments e JOIN classes c ON c.id = e.class_id
         WHERE e.student_id = $1 ORDER BY c.name",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;

    let counts = sqlx::query_as::<_, AttendanceCounts>(
        "SELECT COUNT(*)::BIGINT AS total,
                COUNT(*) FILTER (WHERE status = 'present')::BIGINT AS present
         FROM attendance
         WHERE student_id = $1 AND date >= CURRENT_DATE - INTERVAL '14 days'",
    )
    .bind(student_id)
    .fetch_one(pool)
    .await?;

    let recent = sqlx::query_as::<_, AttendanceRow>(
        "SELECT a.date AS date, a.status AS status, c.name AS class_name
         FROM attendance a JOIN classes c ON c.id = a.class_id
         WHERE a.student_id = $1 AND a.date >= CURRENT_DATE - INTERVAL '14 days'
         ORDER BY a.date DESC LIMIT 20",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;

    let scores = sqlx::query_as::<_, ScoreRow>(
        "SELECT a.name AS assessment_name, c.name AS class_name,
                g.score::FLOAT8 AS score, a.max_score::FLOAT8 AS max_score, a.date AS date
         FROM grades g
         JOIN assessments a ON a.id = g.assessment_id
         JOIN classes c ON c.id = a.class_id
         WHERE g.student_id = $1 AND g.score IS NOT NULL
         ORDER BY a.date DESC, a.id DESC LIMIT 5",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;

    // Postgres SUM(bigint) returns NUMERIC, so cast the difference to BIGINT.
    let balance: i64 = sqlx::query_scalar(
        "SELECT (COALESCE((SELECT SUM(amount_cents) FROM invoices WHERE student_id = $1), 0)
                - COALESCE((SELECT SUM(p.amount_cents) FROM payments p
                            JOIN invoices i ON i.id = p.invoice_id
                            WHERE i.student_id = $1), 0))::BIGINT",
    )
    .bind(student_id)
    .fetch_one(pool)
    .await?;

    let open = sqlx::query_as::<_, OpenInvoiceRow>(
        "SELECT i.id AS id, i.title AS title, i.amount_cents AS amount_cents,
                (COALESCE(SUM(p.amount_cents), 0))::BIGINT AS paid,
                (i.amount_cents - COALESCE(SUM(p.amount_cents), 0))::BIGINT AS balance,
                i.due_date AS due_date
         FROM invoices i LEFT JOIN payments p ON p.invoice_id = i.id
         WHERE i.student_id = $1
         GROUP BY i.id, i.title, i.amount_cents, i.due_date
         HAVING (i.amount_cents - COALESCE(SUM(p.amount_cents), 0)) > 0
         ORDER BY i.due_date ASC LIMIT 20",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;

    // Welfare (read-only for families): latest 5 incidents + medical count.
    let recent_incidents: Vec<IncidentLiteView> = welfare_model::list_incidents(pool, student_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .take(5)
        .map(|r| IncidentLiteView {
            date: r.incident_date.to_string(),
            kind: r.kind,
            description: r.description,
        })
        .collect();
    // list_incidents already newest-first; ensure.
    let medical_count = welfare_model::list_medical(pool, student_id)
        .await
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    let transport_routes = transport_model::routes_for_student(pool, student_id)
        .await
        .unwrap_or_default();
    let hostel_allocation = hostel_model::open_allocation_for_student(pool, student_id)
        .await
        .unwrap_or(None);
    let lessons = lesson_model::list_for_student(pool, student_id)
        .await
        .unwrap_or_default();
    let activities = activity_model::activities_for_student(pool, student_id)
        .await
        .unwrap_or_default();

    Ok(ChildCardView {
        id: row.id,
        full_name,
        admission_no: row.admission_no,
        status: row.status,
        enrolled_on: row.enrolled_on.to_string(),
        attendance_present: counts.present,
        attendance_total: counts.total,
        balance: util::format_cents(balance),
        enrollments: enrollments
            .into_iter()
            .map(|e| EnrollmentView {
                class_name: e.class_name,
                enrolled_on: e.enrolled_on.to_string(),
            })
            .collect(),
        recent_attendance: recent
            .into_iter()
            .map(|a| AttendanceView {
                date: a.date.to_string(),
                status: a.status,
                class_name: a.class_name,
            })
            .collect(),
        assessments: scores
            .into_iter()
            .map(|s| ScoreView {
                name: s.assessment_name,
                class_name: s.class_name,
                score: format!("{:.2}", s.score),
                max_score: format!("{:.2}", s.max_score),
                date: s.date.to_string(),
            })
            .collect(),
        invoices: open
            .into_iter()
            .map(|i| InvoiceLiteView {
                id: i.id,
                title: i.title,
                amount: util::format_cents(i.amount_cents),
                paid: util::format_cents(i.paid),
                balance: util::format_cents(i.balance),
                due: i.due_date.to_string(),
            })
            .collect(),
        recent_incidents,
        medical_count,
        transport_routes,
        hostel_allocation,
        lessons,
        activities,
    })
}

// ---------------------------------------------------------------------------
// Templates (section="dashboard": sidebar has no portal entry)
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "portal/home.html")]
struct HomeTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    children: Vec<ChildCardView>,
}

#[derive(Template)]
#[template(path = "portal/child.html")]
struct ChildTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    child: ChildCardView,
}

fn home_context(
    user: &CurrentUser,
    params: &HashMap<String, String>,
    children: Vec<ChildCardView>,
) -> HomeTemplate {
    let (flash, flash_kind) = flash_params(params);
    let (user_name, user_initial) = user_ctx(user);
    HomeTemplate {
        section: "portal",
        section_label: "Family portal",
        today: today_display(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        children,
    }
}

fn child_context(
    user: &CurrentUser,
    params: &HashMap<String, String>,
    child: ChildCardView,
) -> ChildTemplate {
    let (flash, flash_kind) = flash_params(params);
    let (user_name, user_initial) = user_ctx(user);
    ChildTemplate {
        section: "portal",
        section_label: "Family portal",
        today: today_display(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        child,
    }
}

// ---------------------------------------------------------------------------
// GET /portal + GET /portal/{id}
// ---------------------------------------------------------------------------

async fn portal_home(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    if is_staff_side(&user) {
        return Ok(Redirect::to("/dashboard").into_response());
    }
    if user.has_role(&[Role::Parent]) {
        // Scoped in SQL: only rows linked via parent_links.
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT s.id FROM students s
             WHERE s.id IN (SELECT student_id FROM parent_links WHERE parent_user_id = $1)
             ORDER BY s.last_name, s.first_name",
        )
        .bind(user.id)
        .fetch_all(&state.pool)
        .await?;
        let mut children = Vec::with_capacity(ids.len());
        for sid in ids {
            children.push(load_child(&state.pool, sid).await?);
        }
        let t = home_context(&user, &params, children);
        return t
            .render()
            .map(Html)
            .map_err(AppError::from)
            .map(|h| h.into_response());
    }
    if user.has_role(&[Role::Student]) {
        let own: Option<i64> =
            sqlx::query_scalar("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
                .bind(user.id)
                .fetch_optional(&state.pool)
                .await?;
        let mut children = Vec::new();
        if let Some(sid) = own {
            children.push(load_child(&state.pool, sid).await?);
        }
        let t = home_context(&user, &params, children);
        return t
            .render()
            .map(Html)
            .map_err(AppError::from)
            .map(|h| h.into_response());
    }
    Err(AppError::Forbidden)
}

async fn portal_child(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, AppError> {
    if is_staff_side(&user) {
        return Ok(Redirect::to("/dashboard").into_response());
    }
    if user.has_role(&[Role::Parent]) {
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM parent_links WHERE parent_user_id = $1 AND student_id = $2)",
        )
        .bind(user.id)
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
        if !allowed {
            return Err(AppError::Forbidden);
        }
    } else if user.has_role(&[Role::Student]) {
        let own: Option<i64> =
            sqlx::query_scalar("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
                .bind(user.id)
                .fetch_optional(&state.pool)
                .await?;
        if own != Some(id) {
            return Err(AppError::Forbidden);
        }
    } else {
        return Err(AppError::Forbidden);
    }
    let child = load_child(&state.pool, id).await?;
    let t = child_context(&user, &params, child);
    t.render()
        .map(Html)
        .map_err(AppError::from)
        .map(|h| h.into_response())
}

// ---------------------------------------------------------------------------
// Family-access management (admin + staff only)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LinkParentForm {
    #[serde(default)]
    parent_email: String,
    #[serde(default)]
    relation: String,
}

#[derive(Debug, Deserialize)]
struct LinkUserForm {
    #[serde(default)]
    email: String,
}

async fn link_parent_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<LinkParentForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let back = format!("/students/{id}");
    let email = match validate_portal_email(&form.parent_email) {
        Ok(e) => e,
        Err(msg) => return Ok(flash_redirect(&back, &msg, "error").into_response()),
    };
    let relation = match validate_relation(&form.relation) {
        Ok(r) => r,
        Err(msg) => return Ok(flash_redirect(&back, &msg, "error").into_response()),
    };
    if crate::models::student::find_student(&state.pool, id)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound);
    }
    let parent: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM users WHERE lower(email) = lower($1) AND role = 'parent' AND active = TRUE",
    )
    .bind(email.trim())
    .fetch_optional(&state.pool)
    .await?;
    let Some((parent_id,)) = parent else {
        return Ok(
            flash_redirect(&back, "No active parent account with that email", "error")
                .into_response(),
        );
    };
    let done = sqlx::query(
        "INSERT INTO parent_links (parent_user_id, student_id, relation)
         VALUES ($1, $2, $3) ON CONFLICT (parent_user_id, student_id) DO NOTHING",
    )
    .bind(parent_id)
    .bind(id)
    .bind(relation)
    .execute(&state.pool)
    .await;
    match done {
        Ok(r) if r.rows_affected() == 0 => {
            Ok(flash_redirect(&back, "That parent is already linked", "info").into_response())
        }
        Ok(_) => Ok(flash_redirect(&back, "Parent linked", "ok").into_response()),
        Err(e) => {
            let dup =
                matches!(&e, sqlx::Error::Database(db) if db.code().as_deref() == Some("23505"));
            if dup {
                Ok(flash_redirect(&back, "That parent is already linked", "info").into_response())
            } else {
                Err(AppError::Db(e))
            }
        }
    }
}

async fn delete_parent_link_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let student: Option<i64> =
        sqlx::query_scalar("SELECT student_id FROM parent_links WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some(sid) = student else {
        return Ok(flash_redirect("/students", "Link not found", "error").into_response());
    };
    sqlx::query("DELETE FROM parent_links WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(flash_redirect(&format!("/students/{sid}"), "Link removed", "ok").into_response())
}

async fn link_student_user_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<LinkUserForm>,
) -> Result<Response, AppError> {
    require_roles(&user, &[Role::Admin, Role::Staff])?;
    let back = format!("/students/{id}");
    let email = match validate_portal_email(&form.email) {
        Ok(e) => e,
        Err(msg) => return Ok(flash_redirect(&back, &msg, "error").into_response()),
    };
    if crate::models::student::find_student(&state.pool, id)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound);
    }
    let student_user: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM users WHERE lower(email) = lower($1) AND role = 'student' AND active = TRUE",
    )
    .bind(email.trim())
    .fetch_optional(&state.pool)
    .await?;
    let Some((uid,)) = student_user else {
        return Ok(
            flash_redirect(&back, "No active student account with that email", "error")
                .into_response(),
        );
    };
    sqlx::query("UPDATE students SET user_id = $1 WHERE id = $2")
        .bind(uid)
        .bind(id)
        .execute(&state.pool)
        .await?;
    Ok(flash_redirect(&back, "Student login linked", "ok").into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(pairs: &[(i64, i64)]) -> Vec<PortalLink> {
        pairs
            .iter()
            .map(|(p, s)| PortalLink {
                parent_user_id: *p,
                student_id: *s,
            })
            .collect()
    }

    #[test]
    fn parent_sees_only_own_links_sorted_deduped() {
        let all = links(&[(9, 30), (9, 10), (9, 30), (8, 99)]);
        assert_eq!(child_ids_for(&Role::Parent, 9, &all, None), vec![10, 30]);
    }

    #[test]
    fn parent_with_no_links_sees_none() {
        let all = links(&[(8, 99)]);
        assert!(child_ids_for(&Role::Parent, 9, &all, None).is_empty());
    }

    #[test]
    fn student_sees_only_own_row() {
        assert_eq!(child_ids_for(&Role::Student, 5, &[], Some(42)), vec![42]);
        assert!(child_ids_for(&Role::Student, 5, &[], None).is_empty());
    }

    #[test]
    fn staff_roles_see_nothing_in_portal() {
        let all = links(&[(1, 10)]);
        for role in [Role::Admin, Role::Teacher, Role::Staff] {
            assert!(child_ids_for(&role, 1, &all, Some(10)).is_empty());
        }
    }

    #[test]
    fn parent_can_view_only_linked_child() {
        let all = links(&[(9, 10)]);
        assert!(can_view_child(&Role::Parent, 9, &all, None, 10));
        assert!(!can_view_child(&Role::Parent, 9, &all, None, 11));
        assert!(!can_view_child(&Role::Parent, 8, &all, None, 10));
    }

    #[test]
    fn student_can_view_only_own_row() {
        assert!(can_view_child(&Role::Student, 5, &[], Some(42), 42));
        assert!(!can_view_child(&Role::Student, 5, &[], Some(42), 43));
        assert!(!can_view_child(&Role::Student, 5, &[], None, 42));
    }

    #[test]
    fn staff_never_views_via_portal() {
        let all = links(&[(1, 10)]);
        for role in [Role::Admin, Role::Teacher, Role::Staff] {
            assert!(!can_view_child(&role, 1, &all, Some(10), 10));
        }
    }

    #[test]
    fn accepts_valid_email_trimmed() {
        assert_eq!(
            validate_portal_email("  parent@example.com  "),
            Ok("parent@example.com".to_string())
        );
    }

    #[test]
    fn rejects_bad_emails() {
        assert!(validate_portal_email("").is_err());
        assert!(validate_portal_email("   ").is_err());
        assert!(validate_portal_email("no-at-sign").is_err());
        assert!(validate_portal_email("a@b").is_err());
        assert!(validate_portal_email("a @b.com").is_err());
        assert!(validate_portal_email("a@@b.com").is_err());
    }

    #[test]
    fn relation_defaults_to_guardian() {
        assert_eq!(validate_relation(""), Ok("guardian".to_string()));
        assert_eq!(validate_relation("   "), Ok("guardian".to_string()));
        assert_eq!(validate_relation(" mother "), Ok("mother".to_string()));
    }

    #[test]
    fn rejects_long_relation() {
        assert!(validate_relation(&"x".repeat(51)).is_err());
    }

    #[test]
    fn summarizes_attendance_counts() {
        assert_eq!(
            summarize_attendance(&["present", "absent", "present"]),
            (2, 3)
        );
        assert_eq!(summarize_attendance(&[]), (0, 0));
        assert_eq!(summarize_attendance(&["late", "excused"]), (0, 2));
    }
}
