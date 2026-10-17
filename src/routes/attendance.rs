use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Extension, Form, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::AppError;
use crate::models::attendance as att;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/attendance", get(mark_page).post(save_submit))
        .route("/attendance/clear", post(clear_day_submit))
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

fn today_pretty() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

fn today_iso() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

fn back_to(class_id: i64, date: &str) -> String {
    format!("/attendance?class_id={class_id}&date={date}")
}

/// Class picker option with the selected flag precomputed for Askama.
struct ClassOptionView {
    id: i64,
    name: String,
    year_name: String,
    is_current: bool,
    selected: bool,
}

/// Roster line with template-ready defaults (existing value or present/"").
struct RosterView {
    student_id: i64,
    admission_no: String,
    full_name: String,
    status: String,
    note: String,
}

fn roster_views(rows: Vec<att::RosterEntry>) -> Vec<RosterView> {
    rows.into_iter()
        .map(|r| RosterView {
            student_id: r.student_id,
            admission_no: r.admission_no,
            full_name: r.full_name,
            status: r.status.unwrap_or_else(|| "present".to_string()),
            note: r.note.unwrap_or_default(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// GET /attendance
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MarkParams {
    // Lenient string: garbage falls back to the default class instead of
    // Axum's bare 400 rejection page.
    class_id: Option<String>,
    date: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

#[derive(Template)]
#[template(path = "attendance/mark.html")]
struct MarkTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    classes: Vec<ClassOptionView>,
    selected_class: Option<i64>,
    date_str: String,
    roster: Vec<RosterView>,
    recent: Vec<att::DaySummary>,
}

async fn mark_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<MarkParams>,
) -> Result<impl IntoResponse, AppError> {
    let mut flash = params.flash;
    let mut flash_kind = params.flash_kind.unwrap_or_else(|| "info".to_string());

    let date_str = params.date.unwrap_or_else(today_iso);
    let date: NaiveDate = match att::validate_date(&date_str) {
        Ok(d) => d,
        Err(msg) => {
            flash = Some(msg);
            flash_kind = "error".to_string();
            att::validate_date(&today_iso()).expect("today is always valid")
        }
    };
    let date_str = date.format("%Y-%m-%d").to_string();

    let picker = att::classes_for_picker(&state.pool).await?;
    let selected_class = params
        .class_id
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .or_else(|| {
        picker
            .iter()
            .find(|c| c.is_current)
            .map(|c| c.id)
            .or_else(|| picker.first().map(|c| c.id))
    });

    let (roster, recent) = match selected_class {
        Some(id) => {
            if !picker.iter().any(|c| c.id == id) {
                return Err(AppError::NotFound);
            }
            let roster = roster_views(att::roster_for(&state.pool, id, date).await?);
            let recent = att::recent_days(&state.pool, id, 14).await?;
            (roster, recent)
        }
        None => (Vec::new(), Vec::new()),
    };

    let (user_name, user_initial) = user_ctx(&user);
    let t = MarkTemplate {
        section: "attendance",
        section_label: "Attendance",
        today: today_pretty(),
        user_name,
        user_initial,
        flash,
        flash_kind,
        classes: picker
            .into_iter()
            .map(|c| ClassOptionView {
                id: c.id,
                name: c.name,
                year_name: c.year_name,
                is_current: c.is_current,
                selected: Some(c.id) == selected_class,
            })
            .collect(),
        selected_class,
        date_str,
        roster,
        recent,
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /attendance
// ---------------------------------------------------------------------------

/// Parse the per-student `status_<id>` / `note_<id>` pairs out of the raw
/// form map. Unknown/invalid statuses are returned as an error message so
/// the handler can flash it instead of failing with a 500.
fn parse_entries(form: &HashMap<String, String>) -> Result<Vec<att::AttendanceEntry>, String> {
    let mut entries = Vec::new();
    for (key, value) in form {
        let Some(rest) = key.strip_prefix("status_") else {
            continue;
        };
        let student_id: i64 = rest
            .parse()
            .map_err(|_| "Invalid student in attendance form".to_string())?;
        if student_id <= 0 {
            continue;
        }
        att::validate_status(value)?;
        let note_key = format!("note_{student_id}");
        let note = form
            .get(&note_key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        entries.push(att::AttendanceEntry {
            student_id,
            status: value.trim().to_string(),
            note,
        });
    }
    Ok(entries)
}

async fn save_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    crate::auth::middleware::require_roles(
        &user,
        &[Role::Admin, Role::Teacher, Role::Staff],
    )?;

    let class_id: i64 = form
        .get("class_id")
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| AppError::BadRequest("Choose a class first".to_string()))?;
    let date_raw = form.get("date").cloned().unwrap_or_else(today_iso);
    let date = match att::validate_date(&date_raw) {
        Ok(d) => d,
        Err(msg) => {
            return Ok(flash_redirect(
                &back_to(class_id, &today_iso()),
                &msg,
                "error",
            ));
        }
    };
    let date_str = date.format("%Y-%m-%d").to_string();

    let entries = match parse_entries(&form) {
        Ok(e) => e,
        Err(msg) => return Ok(flash_redirect(&back_to(class_id, &date_str), &msg, "error")),
    };

    match att::save_attendance(&state.pool, class_id, date, &entries, user.id).await {
        Ok(()) => {
            // Best-effort: alert failures are logged inside, never fail the save.
            send_absence_alerts(&state, class_id, &date_str, &entries).await;
            Ok(flash_redirect(
                &back_to(class_id, &date_str),
                "Saved",
                "ok",
            ))
        }
        Err(AppError::BadRequest(msg)) => Ok(flash_redirect(
            &back_to(class_id, &date_str),
            &msg,
            "error",
        )),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Absence alerts (best-effort: hook failures are logged, never fail the save)
// ---------------------------------------------------------------------------

/// Subject line shared by the in-app notification and the queued email.
fn absence_subject() -> &'static str {
    "Absence alert"
}

/// Body naming the child, class, and date.
fn absence_body(child: &str, class: &str, date: &str) -> String {
    format!("{child} was marked absent from {class} on {date}.")
}

/// Student ids in this submission marked absent.
fn absent_student_ids(entries: &[att::AttendanceEntry]) -> Vec<i64> {
    entries
        .iter()
        .filter(|e| e.status.trim() == "absent")
        .map(|e| e.student_id)
        .collect()
}

/// After a successful save, notify linked parents of absent students
/// in-app and queue them an email. Best-effort throughout.
async fn send_absence_alerts(
    state: &AppState,
    class_id: i64,
    date_str: &str,
    entries: &[att::AttendanceEntry],
) {
    let absent = absent_student_ids(entries);
    if absent.is_empty() {
        return;
    }
    let class_name: String = sqlx::query_scalar("SELECT name FROM classes WHERE id = $1")
        .bind(class_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, class_id, "absence alert: class lookup failed");
            None
        })
        .unwrap_or_else(|| "their class".to_string());
    for student_id in absent {
        alert_for_student(state, class_id, &class_name, date_str, student_id).await;
    }
}

#[derive(Debug, sqlx::FromRow)]
struct ParentContact {
    id: i64,
    email: String,
}

async fn alert_for_student(
    state: &AppState,
    class_id: i64,
    class_name: &str,
    date_str: &str,
    student_id: i64,
) {
    let child: Option<String> = match sqlx::query_scalar(
        "SELECT first_name || ' ' || last_name FROM students WHERE id = $1",
    )
    .bind(student_id)
    .fetch_optional(&state.pool)
    .await
    {
        Ok(name) => name,
        Err(e) => {
            tracing::warn!(error = %e, student_id, "absence alert: student lookup failed");
            return;
        }
    };
    let Some(child) = child else { return };

    let parents: Vec<ParentContact> = match sqlx::query_as::<_, ParentContact>(
        "SELECT u.id, u.email
         FROM parent_links pl
         JOIN users u ON u.id = pl.parent_user_id
         WHERE pl.student_id = $1 AND u.active = TRUE",
    )
    .bind(student_id)
    .fetch_all(&state.pool)
    .await
    {
        Ok(parents) => parents,
        Err(e) => {
            tracing::warn!(error = %e, student_id, "absence alert: parent lookup failed");
            return;
        }
    };
    if parents.is_empty() {
        return;
    }

    let body = absence_body(&child, class_name, date_str);
    let link = format!("/attendance?class_id={class_id}&date={date_str}");

    // (a) In-app inbox for every linked parent.
    let ids: Vec<i64> = parents.iter().map(|p| p.id).collect();
    let registry = crate::notifications::routes::registry_for(state);
    if let Err(e) = crate::notifications::channels::notify(
        &state.pool,
        &registry,
        &ids,
        "attendance",
        absence_subject(),
        Some(body.clone()),
        Some(link),
    )
    .await
    {
        tracing::warn!(error = %e, student_id, "absence alert: notify failed");
    }

    // (b) One queued email per parent address.
    for parent in &parents {
        if parent.email.trim().is_empty() {
            continue;
        }
        if let Err(e) = sqlx::query(
            "INSERT INTO email_outbox (to_addr, subject, body) VALUES ($1, $2, $3)",
        )
        .bind(parent.email.trim())
        .bind(absence_subject())
        .bind(&body)
        .execute(&state.pool)
        .await
        {
            tracing::warn!(error = %e, student_id, "absence alert: outbox insert failed");
        }
    }
}

// ---------------------------------------------------------------------------
// POST /attendance/clear
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ClearForm {
    #[serde(default)]
    class_id: String,
    #[serde(default)]
    date: String,
}

async fn clear_day_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ClearForm>,
) -> Result<impl IntoResponse, AppError> {
    crate::auth::middleware::require_roles(
        &user,
        &[Role::Admin, Role::Teacher, Role::Staff],
    )?;

    let class_id: i64 = match form.class_id.trim().parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(flash_redirect(
                "/attendance",
                "Choose a class first",
                "error",
            ));
        }
    };
    let date = match att::validate_clear_day(class_id, &form.date) {
        Ok(d) => d,
        Err(msg) => {
            let fallback = if class_id > 0 {
                back_to(class_id, &today_iso())
            } else {
                "/attendance".to_string()
            };
            return Ok(flash_redirect(&fallback, &msg, "error"));
        }
    };
    let date_str = date.format("%Y-%m-%d").to_string();
    let deleted = att::clear_day(&state.pool, class_id, date).await?;
    let msg = if deleted == 1 {
        "Cleared 1 attendance record".to_string()
    } else {
        format!("Cleared {deleted} attendance records")
    };
    Ok(flash_redirect(&back_to(class_id, &date_str), &msg, "ok"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_status_and_note_pairs() {
        let f = form(&[
            ("class_id", "3"),
            ("date", "2026-10-16"),
            ("status_11", "present"),
            ("note_11", "  arrived early  "),
            ("status_12", "late"),
        ]);
        let entries = parse_entries(&f).unwrap();
        assert_eq!(entries.len(), 2);
        let e11 = entries.iter().find(|e| e.student_id == 11).unwrap();
        assert_eq!(e11.status, "present");
        assert_eq!(e11.note.as_deref(), Some("arrived early"));
        let e12 = entries.iter().find(|e| e.student_id == 12).unwrap();
        assert_eq!(e12.status, "late");
        assert_eq!(e12.note, None);
    }

    #[test]
    fn skips_non_positive_student_ids() {
        let f = form(&[("status_0", "present"), ("status_-4", "absent")]);
        assert!(parse_entries(&f).unwrap().is_empty());
    }

    #[test]
    fn ignores_non_status_keys() {
        let f = form(&[("class_id", "3"), ("date", "2026-10-16")]);
        assert!(parse_entries(&f).unwrap().is_empty());
    }

    #[test]
    fn rejects_invalid_status() {
        let f = form(&[("status_7", "sick")]);
        assert_eq!(
            parse_entries(&f),
            Err("Status must be present, absent, late, or excused".to_string())
        );
    }

    #[test]
    fn rejects_malformed_student_key() {
        let f = form(&[("status_abc", "present")]);
        assert!(parse_entries(&f).is_err());
    }

    #[test]
    fn back_to_builds_get_url() {
        assert_eq!(
            back_to(3, "2026-10-16"),
            "/attendance?class_id=3&date=2026-10-16"
        );
    }

    #[test]
    fn absence_subject_is_stable() {
        assert_eq!(absence_subject(), "Absence alert");
    }

    #[test]
    fn absence_body_names_child_class_and_date() {
        assert_eq!(
            absence_body("Ada Lovelace", "Grade 3", "2026-10-16"),
            "Ada Lovelace was marked absent from Grade 3 on 2026-10-16."
        );
    }

    #[test]
    fn collects_only_absent_ids() {
        let entries = vec![
            att::AttendanceEntry {
                student_id: 1,
                status: "present".to_string(),
                note: None,
            },
            att::AttendanceEntry {
                student_id: 2,
                status: "absent".to_string(),
                note: None,
            },
            att::AttendanceEntry {
                student_id: 3,
                status: " absent ".to_string(),
                note: None,
            },
            att::AttendanceEntry {
                student_id: 4,
                status: "late".to_string(),
                note: None,
            },
        ];
        assert_eq!(absent_student_ids(&entries), vec![2, 3]);
    }

    #[test]
    fn no_absent_means_no_alerts() {
        let entries = vec![att::AttendanceEntry {
            student_id: 1,
            status: "present".to_string(),
            note: None,
        }];
        assert!(absent_student_ids(&entries).is_empty());
    }
}
