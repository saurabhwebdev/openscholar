//! Print views (Fedena parity): payment receipts, report cards, transcripts.
//!
//! Browser Print-to-PDF via `static/css/print.css`; standalone Askama pages
//! (no `base.html`). Scoping mirrors `portal.rs`: staff roles
//! (admin/teacher/staff) always pass; parents via `parent_links`; students via
//! `students.user_id`. All SQL is runtime-checked; money via
//! [`crate::util::format_cents`]; every `NUMERIC` casts to `FLOAT8` and every
//! money `SUM` casts to `BIGINT`.

use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    extract::{Extension, Path, Query, State},
    response::{Html, IntoResponse},
    routing::get,
};
use chrono::NaiveDate;

use crate::error::AppError;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/print/payments/{id}/receipt", get(receipt))
        .route("/print/students/{id}/report-card", get(report_card))
        .route("/print/students/{id}/transcript", get(transcript))
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested, no DB)
// ---------------------------------------------------------------------------

/// Receipt number for a payment id: `42` -> `"R-42"`.
pub fn receipt_no(payment_id: i64) -> String {
    format!("R-{payment_id}")
}

/// `?year=` filter: trimmed, empty/missing -> `None`.
pub fn parse_year_param(params: &HashMap<String, String>) -> Option<String> {
    params
        .get("year")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Weighted percent over graded `(score, max, weight)` triples:
/// `100 * SUM(score/max * weight) / SUM(weight)`. `None` when empty or the
/// weight total is zero. Mirrors the `class_overview` SQL.
pub fn weighted_percent(items: &[(f64, f64, f64)]) -> Option<f64> {
    let mut num = 0.0;
    let mut den = 0.0;
    for (score, max, weight) in items {
        if *max == 0.0 {
            continue;
        }
        num += score / max * weight;
        den += weight;
    }
    if den == 0.0 {
        // All graded work is zero-weight: fall back to an unweighted mean so a
        // grade still shows (matches the grades-page class_overview behaviour).
        let mut cnt = 0.0;
        let mut sum = 0.0;
        for (score, max, _w) in items {
            if *max != 0.0 {
                sum += score / max;
                cnt += 1.0;
            }
        }
        if cnt == 0.0 {
            None
        } else {
            Some(100.0 * sum / cnt)
        }
    } else {
        Some(100.0 * num / den)
    }
}

/// Attendance percent: `None` when nothing recorded.
pub fn attendance_pct(present: i64, total: i64) -> Option<f64> {
    if total <= 0 {
        None
    } else {
        Some(100.0 * present as f64 / total as f64)
    }
}

/// Compact number formatting (mirrors `grades.rs`): whole -> `"100"`.
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

pub fn fmt_pct(v: Option<f64>) -> String {
    v.map(|p| format!("{p:.1}%"))
        .unwrap_or_else(|| "—".to_string())
}

/// Pure scoping core: staff roles always pass; parents need a `parent_links`
/// row; students need to own the `students` row. Mirrors `portal::can_view_child`.
pub fn can_view_report_pure(role: &Role, is_linked_parent: bool, is_own_student: bool) -> bool {
    match role {
        Role::Admin | Role::Teacher | Role::Staff => true,
        Role::Parent => is_linked_parent,
        Role::Student => is_own_student,
    }
}

// ---------------------------------------------------------------------------
// DB-backed scoping (mirrors portal.rs)
// ---------------------------------------------------------------------------

/// Whether `user` may view print docs for `student_id`.
pub async fn can_view_report(
    user: &CurrentUser,
    student_id: i64,
    pool: &sqlx::PgPool,
) -> Result<bool, AppError> {
    let Ok(role) = user.role.parse::<Role>() else {
        return Ok(false);
    };
    match role {
        Role::Admin | Role::Teacher | Role::Staff => Ok(true),
        Role::Parent => {
            let allowed: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM parent_links WHERE parent_user_id = $1 AND student_id = $2)",
            )
            .bind(user.id)
            .bind(student_id)
            .fetch_one(pool)
            .await?;
            Ok(allowed)
        }
        Role::Student => {
            let own: Option<i64> =
                sqlx::query_scalar("SELECT id FROM students WHERE id = $1 AND user_id = $2")
                    .bind(student_id)
                    .bind(user.id)
                    .fetch_optional(pool)
                    .await?;
            Ok(own.is_some())
        }
    }
}

async fn check_scope(
    user: &CurrentUser,
    student_id: i64,
    pool: &sqlx::PgPool,
) -> Result<(), AppError> {
    if can_view_report(user, student_id, pool).await? {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn is_staff(user: &CurrentUser) -> bool {
    user.has_role(&[Role::Admin, Role::Teacher, Role::Staff])
}

fn printed_on() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}

// ---------------------------------------------------------------------------
// Shared read models (runtime SQL; NUMERIC::FLOAT8, SUM::BIGINT)
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct ClassRow {
    class_id: i64,
    class_name: String,
    year_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AssessmentLine {
    name: String,
    kind: String,
    max_score: f64,
    weight: f64,
    date: NaiveDate,
    score: Option<f64>,
    avg: Option<f64>,
}

#[derive(Debug, sqlx::FromRow)]
struct OverallRow {
    percent: Option<f64>,
    graded: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct AttendanceCounts {
    total: i64,
    present: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct YearRow {
    year_name: String,
    #[allow(dead_code)]
    start_date: NaiveDate,
}

async fn classes_for_student(
    pool: &sqlx::PgPool,
    student_id: i64,
    year_filter: Option<&str>,
) -> Result<Vec<ClassRow>, AppError> {
    let rows = sqlx::query_as::<_, ClassRow>(
        "SELECT c.id AS class_id, c.name AS class_name, y.name AS year_name
          FROM enrollments e
          JOIN classes c ON c.id = e.class_id
          JOIN school_years y ON y.id = c.school_year_id
          WHERE e.student_id = $1
            AND ($2::TEXT IS NULL OR y.name = $2 OR y.id::TEXT = $2)
          ORDER BY y.start_date, c.name",
    )
    .bind(student_id)
    .bind(year_filter)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn lines_for_class(
    pool: &sqlx::PgPool,
    student_id: i64,
    class_id: i64,
) -> Result<Vec<AssessmentLine>, AppError> {
    let rows = sqlx::query_as::<_, AssessmentLine>(
        "SELECT a.name AS name, a.kind AS kind,
                a.max_score::FLOAT8 AS max_score,
                a.weight::FLOAT8 AS weight,
                a.date AS date,
                g.score::FLOAT8 AS score,
                (SELECT AVG(g2.score)::FLOAT8 FROM grades g2 WHERE g2.assessment_id = a.id) AS avg
          FROM assessments a
          LEFT JOIN grades g ON g.assessment_id = a.id AND g.student_id = $1
          WHERE a.class_id = $2
          ORDER BY a.date, a.id",
    )
    .bind(student_id)
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

async fn overall_for_class(
    pool: &sqlx::PgPool,
    student_id: i64,
    class_id: i64,
) -> Result<OverallRow, AppError> {
    let row = sqlx::query_as::<_, OverallRow>(
        "SELECT (CASE
                    WHEN SUM(CASE WHEN g.score IS NOT NULL THEN a.weight::FLOAT8 ELSE 0 END) > 0
                        THEN 100 * SUM(g.score::FLOAT8 / NULLIF(a.max_score::FLOAT8, 0) * a.weight::FLOAT8)
                             / SUM(CASE WHEN g.score IS NOT NULL THEN a.weight::FLOAT8 ELSE 0 END)
                    WHEN COUNT(g.score) > 0
                        THEN 100 * AVG(g.score::FLOAT8 / NULLIF(a.max_score::FLOAT8, 0))
                    ELSE NULL
                 END)::FLOAT8 AS percent,
                COUNT(g.score)::BIGINT AS graded
          FROM assessments a
          LEFT JOIN grades g ON g.assessment_id = a.id AND g.student_id = $1
          WHERE a.class_id = $2",
    )
    .bind(student_id)
    .bind(class_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

async fn attendance_for_class(
    pool: &sqlx::PgPool,
    student_id: i64,
    class_id: i64,
) -> Result<AttendanceCounts, AppError> {
    let row = sqlx::query_as::<_, AttendanceCounts>(
        "SELECT COUNT(*)::BIGINT AS total,
                COUNT(*) FILTER (WHERE status = 'present')::BIGINT AS present
          FROM attendance WHERE student_id = $1 AND class_id = $2",
    )
    .bind(student_id)
    .bind(class_id)
    .fetch_all(pool)
    .await
    .map(|mut v| {
        v.pop().unwrap_or(AttendanceCounts {
            total: 0,
            present: 0,
        })
    })?;
    Ok(row)
}

async fn current_year_name(pool: &sqlx::PgPool) -> Option<String> {
    sqlx::query_scalar("SELECT name FROM school_years WHERE is_current LIMIT 1")
        .fetch_optional(pool)
        .await
        .unwrap_or(None)
}

// ---------------------------------------------------------------------------
// View structs (Askama gets preformatted strings only)
// ---------------------------------------------------------------------------

struct LineView {
    name: String,
    kind: String,
    date: String,
    score_display: String,
    max_display: String,
    avg_display: String,
}

struct ClassReportView {
    class_name: String,
    year_name: String,
    lines: Vec<LineView>,
    graded: i64,
    overall_display: String,
    attendance_display: String,
}

struct ClassReportData {
    view: ClassReportView,
    triples: Vec<(f64, f64, f64)>,
    present: i64,
    total: i64,
}

fn line_view(l: &AssessmentLine) -> LineView {
    LineView {
        name: l.name.clone(),
        kind: l.kind.clone(),
        date: l.date.to_string(),
        score_display: l.score.map(fmt_num).unwrap_or_else(|| "—".to_string()),
        max_display: fmt_num(l.max_score),
        avg_display: l
            .avg
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".to_string()),
    }
}

async fn class_report(
    pool: &sqlx::PgPool,
    student_id: i64,
    class: &ClassRow,
) -> Result<ClassReportData, AppError> {
    let lines = lines_for_class(pool, student_id, class.class_id).await?;
    let overall = overall_for_class(pool, student_id, class.class_id).await?;
    let att = attendance_for_class(pool, student_id, class.class_id).await?;
    let triples: Vec<(f64, f64, f64)> = lines
        .iter()
        .filter_map(|l| l.score.map(|s| (s, l.max_score, l.weight)))
        .collect();
    Ok(ClassReportData {
        view: ClassReportView {
            class_name: class.class_name.clone(),
            year_name: class.year_name.clone(),
            lines: lines.iter().map(line_view).collect(),
            graded: overall.graded,
            overall_display: fmt_pct(overall.percent),
            attendance_display: fmt_pct(attendance_pct(att.present, att.total)),
        },
        triples,
        present: att.present,
        total: att.total,
    })
}

// ---------------------------------------------------------------------------
// GET /print/payments/:id/receipt
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct PaymentReceiptRow {
    id: i64,
    invoice_id: i64,
    amount_cents: i64,
    method: String,
    reference: Option<String>,
    paid_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Template)]
#[template(path = "print/receipt.html")]
struct ReceiptTemplate {
    receipt_no: String,
    payer: String,
    admission_no: String,
    invoice_title: String,
    invoice_id: i64,
    amount: String,
    method: String,
    reference: String,
    paid_at: String,
    invoice_amount: String,
    invoice_fines: String,
    invoice_paid: String,
    balance_after: String,
    printed_on: String,
    back_url: String,
}

async fn receipt(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let payment = sqlx::query_as::<_, PaymentReceiptRow>(
        "SELECT id, invoice_id, amount_cents, method, reference, paid_at
          FROM payments WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let receipt_number: Option<String> =
        sqlx::query_scalar("SELECT number FROM receipts WHERE payment_id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;

    let (invoice, _) = crate::models::fees::invoice_detail(&state.pool, payment.invoice_id).await?;
    // Don't leak payment-id existence to families: a scope failure looks the
    // same as "no such payment" for non-staff.
    if let Err(e) = check_scope(&user, invoice.student_id, &state.pool).await {
        return Err(if is_staff(&user) {
            e
        } else {
            AppError::NotFound
        });
    }

    let back_url = if is_staff(&user) {
        format!("/fees/{}", invoice.id)
    } else {
        format!("/portal/{}", invoice.student_id)
    };
    let t = ReceiptTemplate {
        receipt_no: receipt_number.unwrap_or_else(|| receipt_no(payment.id)),
        payer: invoice.student_name.clone(),
        admission_no: invoice.admission_no.clone(),
        invoice_title: invoice.title.clone(),
        invoice_id: invoice.id,
        amount: util::format_cents(payment.amount_cents),
        method: payment.method.clone(),
        reference: payment.reference.clone().unwrap_or_default(),
        paid_at: payment.paid_at.format("%Y-%m-%d %H:%M").to_string(),
        invoice_amount: util::format_cents(invoice.amount_cents),
        invoice_fines: util::format_cents(invoice.fines),
        invoice_paid: util::format_cents(invoice.paid),
        balance_after: util::format_cents(invoice.balance),
        printed_on: printed_on(),
        back_url,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// GET /print/students/:id/report-card?year=
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "print/report_card.html")]
struct ReportCardTemplate {
    student_name: String,
    admission_no: String,
    enrolled_on: String,
    status: String,
    year_label: String,
    classes: Vec<ClassReportView>,
    overall_display: String,
    attendance_display: String,
    printed_on: String,
    back_url: String,
}

async fn report_card(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    let year = parse_year_param(&params);
    let row = crate::models::student::find_student(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    check_scope(&user, id, &state.pool).await?;

    let classes = classes_for_student(&state.pool, id, year.as_deref()).await?;
    let mut views = Vec::with_capacity(classes.len());
    let mut triples: Vec<(f64, f64, f64)> = Vec::new();
    let (mut present, mut total) = (0i64, 0i64);
    for c in &classes {
        let rep = class_report(&state.pool, id, c).await?;
        triples.extend(rep.triples);
        present += rep.present;
        total += rep.total;
        views.push(rep.view);
    }

    let year_label = match year {
        Some(y) => y,
        None => current_year_name(&state.pool)
            .await
            .unwrap_or_else(|| "Current year".to_string()),
    };
    let back_url = if is_staff(&user) {
        format!("/students/{id}")
    } else {
        format!("/portal/{id}")
    };
    let full_name = format!("{} {}", row.first_name, row.last_name)
        .trim()
        .to_string();
    let t = ReportCardTemplate {
        student_name: full_name,
        admission_no: row.admission_no,
        enrolled_on: row.enrolled_on.to_string(),
        status: row.status,
        year_label,
        classes: views,
        overall_display: fmt_pct(weighted_percent(&triples)),
        attendance_display: fmt_pct(attendance_pct(present, total)),
        printed_on: printed_on(),
        back_url,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// GET /print/students/:id/transcript
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "print/transcript.html")]
struct TranscriptTemplate {
    student_name: String,
    admission_no: String,
    enrolled_on: String,
    status: String,
    years: Vec<YearGroupView>,
    cumulative_display: String,
    attendance_display: String,
    printed_on: String,
    back_url: String,
}

struct YearGroupView {
    year_name: String,
    classes: Vec<ClassReportView>,
}

async fn transcript(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let row = crate::models::student::find_student(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    check_scope(&user, id, &state.pool).await?;

    let years = sqlx::query_as::<_, YearRow>(
        "SELECT DISTINCT y.name AS year_name, y.start_date AS start_date
          FROM enrollments e
          JOIN classes c ON c.id = e.class_id
          JOIN school_years y ON y.id = c.school_year_id
          WHERE e.student_id = $1
          ORDER BY y.start_date",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    let mut groups = Vec::with_capacity(years.len());
    let mut triples: Vec<(f64, f64, f64)> = Vec::new();
    let (mut present, mut total) = (0i64, 0i64);
    for y in &years {
        let classes = classes_for_student(&state.pool, id, Some(y.year_name.as_str())).await?;
        let mut views = Vec::with_capacity(classes.len());
        for c in &classes {
            let rep = class_report(&state.pool, id, c).await?;
            triples.extend(rep.triples);
            present += rep.present;
            total += rep.total;
            views.push(rep.view);
        }
        groups.push(YearGroupView {
            year_name: y.year_name.clone(),
            classes: views,
        });
    }

    let back_url = if is_staff(&user) {
        format!("/students/{id}")
    } else {
        format!("/portal/{id}")
    };
    let full_name = format!("{} {}", row.first_name, row.last_name)
        .trim()
        .to_string();
    let t = TranscriptTemplate {
        student_name: full_name,
        admission_no: row.admission_no,
        enrolled_on: row.enrolled_on.to_string(),
        status: row.status,
        years: groups,
        cumulative_display: fmt_pct(weighted_percent(&triples)),
        attendance_display: fmt_pct(attendance_pct(present, total)),
        printed_on: printed_on(),
        back_url,
    };
    t.render().map(Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// Tests: pure parts + forged-style scoping (no DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::user::Role;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn receipt_numbers_use_r_prefix() {
        assert_eq!(receipt_no(1), "R-1");
        assert_eq!(receipt_no(42), "R-42");
    }

    #[test]
    fn year_param_trims_and_treats_blank_as_missing() {
        assert_eq!(
            parse_year_param(&params(&[("year", " 2024-25 ")])),
            Some("2024-25".to_string())
        );
        assert_eq!(parse_year_param(&params(&[("year", "   ")])), None);
        assert_eq!(parse_year_param(&params(&[])), None);
    }

    #[test]
    fn weighted_percent_matches_sql_formula() {
        // 80/100 w1 + 90/100 w1 -> 85%.
        let items = vec![(80.0, 100.0, 1.0), (90.0, 100.0, 1.0)];
        let got = weighted_percent(&items).unwrap();
        assert!((got - 85.0).abs() < 1e-9, "got {got}");
    }

    #[test]
    fn weighted_percent_respects_weights() {
        // 50/100 w3 + 100/100 w1 -> 62.5%.
        let items = vec![(50.0, 100.0, 3.0), (100.0, 100.0, 1.0)];
        assert_eq!(weighted_percent(&items), Some(62.5));
    }

    #[test]
    fn weighted_percent_none_when_no_graded_work() {
        // No graded items at all → None.
        assert_eq!(weighted_percent(&[]), None);
        // All graded work is zero-weight → fall back to the unweighted mean
        // (mirrors the grades-page class_overview behaviour), not None.
        assert!((weighted_percent(&[(80.0, 100.0, 0.0)]).unwrap() - 80.0).abs() < 1e-9);
        assert!(
            (weighted_percent(&[(80.0, 100.0, 0.0), (90.0, 100.0, 0.0)]).unwrap() - 85.0).abs()
                < 1e-9
        );
    }

    #[test]
    fn weighted_percent_skips_zero_max() {
        assert_eq!(weighted_percent(&[(80.0, 0.0, 1.0)]), None);
        assert_eq!(
            weighted_percent(&[(80.0, 0.0, 1.0), (90.0, 100.0, 1.0)]),
            Some(90.0)
        );
    }

    #[test]
    fn attendance_pct_computes_or_none() {
        assert_eq!(attendance_pct(9, 10), Some(90.0));
        assert_eq!(attendance_pct(0, 0), None);
        assert_eq!(attendance_pct(0, -3), None);
    }

    #[test]
    fn fmt_num_drops_trailing_zeros_for_whole() {
        assert_eq!(fmt_num(100.0), "100");
        assert_eq!(fmt_num(87.5), "87.50");
    }

    #[test]
    fn fmt_pct_dashes_on_none() {
        assert_eq!(fmt_pct(Some(85.0)), "85.0%");
        assert_eq!(fmt_pct(None), "—");
    }

    #[test]
    fn staff_roles_always_pass_pure_scope() {
        for role in [Role::Admin, Role::Teacher, Role::Staff] {
            assert!(can_view_report_pure(&role, false, false));
        }
    }

    #[test]
    fn parent_passes_only_when_linked() {
        assert!(can_view_report_pure(&Role::Parent, true, false));
        assert!(!can_view_report_pure(&Role::Parent, false, false));
        assert!(!can_view_report_pure(&Role::Parent, false, true));
    }

    #[test]
    fn student_passes_only_for_own_row() {
        assert!(can_view_report_pure(&Role::Student, false, true));
        assert!(!can_view_report_pure(&Role::Student, true, false));
        assert!(!can_view_report_pure(&Role::Student, false, false));
    }

    #[test]
    fn forged_users_match_pure_scope_matrix() {
        // Forged CurrentUser values (no DB): role string drives the gate.
        let mk = |id: i64, role: &str| CurrentUser {
            id,
            email: format!("u{id}@example.com"),
            full_name: "U".to_string(),
            role: role.to_string(),
        };
        let admin = mk(1, "admin");
        let parent = mk(9, "parent");
        let student = mk(5, "student");
        assert!(is_staff(&admin));
        assert!(!is_staff(&parent));
        assert!(!is_staff(&student));
        assert!(can_view_report_pure(
            &parent.role.parse().unwrap(),
            true,
            false
        ));
        assert!(!can_view_report_pure(
            &parent.role.parse().unwrap(),
            false,
            false
        ));
        assert!(can_view_report_pure(
            &student.role.parse().unwrap(),
            false,
            true
        ));
        assert!(!can_view_report_pure(
            &student.role.parse().unwrap(),
            false,
            false
        ));
        assert!("superadmin".parse::<Role>().is_err());
    }
}
