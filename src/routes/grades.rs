use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    extract::{Extension, Form, Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::AppError;
use crate::models::audit as audit_model;
use crate::models::grades as grades_model;
use crate::models::user::{CurrentUser, Role};
use crate::state::AppState;
use crate::util;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/grades", get(grades_list))
        .route("/assessments", post(create_assessment_submit))
        .route("/grades/assessments/{id}", get(assessment_scores_page))
        .route("/grades/assessments/{id}/scores", post(save_scores_submit))
        .route(
            "/grades/assessments/{id}/delete",
            post(delete_assessment_submit),
        )
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

fn today() -> String {
    chrono::Local::now().format("%A, %-d %B %Y").to_string()
}

fn today_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn can_mutate(user: &CurrentUser) -> bool {
    user.has_role(&[Role::Admin, Role::Teacher])
}

fn check_mutate(user: &CurrentUser) -> Result<(), AppError> {
    crate::auth::middleware::require_roles(user, &[Role::Admin, Role::Teacher])
}

fn flash_redirect(base: &str, msg: &str, kind: &str) -> Redirect {
    let sep = if base.contains('?') { "&" } else { "?" };
    Redirect::to(&format!(
        "{base}{sep}flash={}&flash_kind={kind}",
        util::url_encode(msg)
    ))
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

// ---------------------------------------------------------------------------
// GET /grades
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ListParams {
    // Lenient string: garbage falls back to the default class instead of
    // Axum's bare 400 rejection page.
    class_id: Option<String>,
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct ClassOptionView {
    id: i64,
    name: String,
    selected: bool,
}

struct AssessmentListView {
    id: i64,
    name: String,
    kind: String,
    max_display: String,
    weight_display: String,
    date: String,
    avg: Option<f64>,
    avg_display: String,
    avg_ok: bool,
    graded_count: i64,
}

struct OverviewView {
    full_name: String,
    percent: Option<f64>,
    percent_display: String,
    percent_ok: bool,
    graded: i64,
}

#[derive(Template)]
#[template(path = "grades/list.html")]
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
    selected_class_name: Option<String>,
    assessments: Vec<AssessmentListView>,
    overview: Vec<OverviewView>,
    can_mutate: bool,
    today_date: String,
}

async fn grades_list(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(params): Query<ListParams>,
) -> Result<impl IntoResponse, AppError> {
    let options = grades_model::list_class_options(&state.pool).await?;
    let selected = params
        .class_id
        .as_deref()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .and_then(|id| options.iter().find(|c| c.id == id).map(|c| c.id))
        .or_else(|| options.first().map(|c| c.id));
    let (assessments, overview) = match selected {
        Some(cid) => (
            grades_model::assessments_for_class(&state.pool, cid).await?,
            grades_model::class_overview(&state.pool, cid).await?,
        ),
        None => (Vec::new(), Vec::new()),
    };
    let (user_name, user_initial) = user_ctx(&user);
    let t = ListTemplate {
        section: "grades",
        section_label: "Grades",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        classes: options
            .iter()
            .map(|c| ClassOptionView {
                id: c.id,
                name: c.name.clone(),
                selected: Some(c.id) == selected,
            })
            .collect(),
        selected_class: selected,
        selected_class_name: selected
            .and_then(|id| options.iter().find(|c| c.id == id).map(|c| c.name.clone())),
        assessments: assessments
            .into_iter()
            .map(|a| AssessmentListView {
                id: a.id,
                name: a.name.clone(),
                kind: a.kind.clone(),
                max_display: fmt_num(a.max_score),
                weight_display: fmt_num(a.weight),
                date: a.date.to_string(),
                avg: a.avg,
                // Show the mean out of the assessment's max, and base the
                // pass/fail pill on the percentage (not a raw score vs 50).
                avg_display: a
                    .avg
                    .map(|v| format!("{v:.1} / {:.0}", a.max_score))
                    .unwrap_or_default(),
                avg_ok: a
                    .avg
                    .is_some_and(|v| a.max_score > 0.0 && (v / a.max_score * 100.0) >= 50.0),
                graded_count: a.graded_count,
            })
            .collect(),
        overview: overview
            .into_iter()
            .map(|s| OverviewView {
                full_name: s.full_name.clone(),
                percent: s.percent,
                percent_display: s
                    .percent
                    .map(|v| format!("{v:.1}%"))
                    .unwrap_or_else(|| "—".to_string()),
                percent_ok: s.percent.is_some_and(|v| v >= 50.0),
                graded: s.graded,
            })
            .collect(),
        can_mutate: can_mutate(&user),
        today_date: today_date(),
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /assessments
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AssessmentForm {
    class_id: String,
    name: String,
    kind: String,
    max_score: String,
    weight: String,
    date: String,
}

async fn create_assessment_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<AssessmentForm>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let class_id: i64 = match form.class_id.trim().parse() {
        Ok(id) => id,
        Err(_) => {
            return Ok(flash_redirect(
                "/grades",
                "Invalid class selection",
                "error",
            ));
        }
    };
    let max_score: f64 = match form.max_score.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(flash_redirect(
                &format!("/grades?class_id={}", form.class_id.trim()),
                "Max score must be a number",
                "error",
            ));
        }
    };
    let weight: f64 = match form.weight.trim().parse() {
        Ok(v) => v,
        Err(_) => {
            return Ok(flash_redirect(
                &format!("/grades?class_id={}", form.class_id.trim()),
                "Weight must be a number",
                "error",
            ));
        }
    };
    let date = match NaiveDate::parse_from_str(form.date.trim(), "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            return Ok(flash_redirect(
                &format!("/grades?class_id={}", form.class_id.trim()),
                "Date must be YYYY-MM-DD",
                "error",
            ));
        }
    };
    let kind = form.kind.trim();
    if let Err(msg) = grades_model::validate_assessment(&form.name, kind, max_score, weight) {
        return Ok(flash_redirect(
            &format!("/grades?class_id={class_id}"),
            &msg,
            "error",
        ));
    }
    match grades_model::create_assessment(
        &state.pool,
        class_id,
        form.name.trim(),
        kind,
        max_score,
        weight,
        date,
    )
    .await
    {
        Ok(row) => {
            if let Err(e) = audit_model::log_action(
                &state.pool,
                Some(user.id),
                "create",
                "assessment",
                Some(row.id),
                Some("assessment created"),
            )
            .await
            {
                tracing::warn!(error = %e, "audit log failed");
            }
            Ok(flash_redirect(
                &format!("/grades?class_id={class_id}"),
                "Assessment created",
                "ok",
            ))
        }
        Err(AppError::Db(e))
            if e.as_database_error()
                .is_some_and(|db| db.is_foreign_key_violation()) =>
        {
            Ok(flash_redirect(
                "/grades",
                "Selected class no longer exists",
                "error",
            ))
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// GET /grades/assessments/:id
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ScoresParams {
    flash: Option<String>,
    flash_kind: Option<String>,
}

struct ScoreEntryView {
    student_id: i64,
    full_name: String,
    score_display: String,
}

#[derive(Template)]
#[template(path = "grades/scores.html")]
struct ScoresTemplate {
    section: &'static str,
    section_label: &'static str,
    today: String,
    user_name: String,
    user_initial: String,
    flash: Option<String>,
    flash_kind: String,
    assessment_id: i64,
    assessment_name: String,
    kind: String,
    class_id: i64,
    class_name: String,
    max_display: String,
    weight_display: String,
    date: String,
    entries: Vec<ScoreEntryView>,
    can_mutate: bool,
}

async fn assessment_scores_page(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Query(params): Query<ScoresParams>,
) -> Result<impl IntoResponse, AppError> {
    let detail = grades_model::assessment_detail(&state.pool, id).await?;
    let (user_name, user_initial) = user_ctx(&user);
    let t = ScoresTemplate {
        section: "grades",
        section_label: "Grades",
        today: today(),
        user_name,
        user_initial,
        flash: params.flash,
        flash_kind: params.flash_kind.unwrap_or_else(|| "info".to_string()),
        assessment_id: detail.assessment.id,
        assessment_name: detail.assessment.name.clone(),
        kind: detail.assessment.kind.clone(),
        class_id: detail.assessment.class_id,
        class_name: detail.class_name.clone(),
        max_display: fmt_num(detail.assessment.max_score),
        weight_display: fmt_num(detail.assessment.weight),
        date: detail.assessment.date.to_string(),
        entries: detail
            .entries
            .into_iter()
            .map(|e| ScoreEntryView {
                student_id: e.student_id,
                full_name: e.full_name.clone(),
                score_display: e.score.map(fmt_num).unwrap_or_default(),
            })
            .collect(),
        can_mutate: can_mutate(&user),
    };
    t.render().map(axum::response::Html).map_err(AppError::from)
}

// ---------------------------------------------------------------------------
// POST /grades/assessments/:id/scores
// ---------------------------------------------------------------------------

async fn save_scores_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<HashMap<String, String>>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let detail = match grades_model::assessment_detail(&state.pool, id).await {
        Ok(d) => d,
        Err(AppError::NotFound) => {
            return Ok(flash_redirect("/grades", "Assessment not found", "error"));
        }
        Err(e) => return Err(e),
    };
    let max = detail.assessment.max_score;
    let back = format!("/grades/assessments/{id}");
    let mut scores: Vec<(i64, Option<f64>)> = Vec::with_capacity(detail.entries.len());
    for entry in &detail.entries {
        let key = format!("score_{}", entry.student_id);
        let raw = form
            .get(&key)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if raw.is_empty() {
            scores.push((entry.student_id, None));
            continue;
        }
        let value: f64 = match raw.parse() {
            Ok(v) => v,
            Err(_) => {
                return Ok(flash_redirect(
                    &back,
                    &format!(
                        "{}: enter a number between 0 and {}",
                        entry.full_name,
                        fmt_num(max)
                    ),
                    "error",
                ));
            }
        };
        if let Err(msg) = grades_model::validate_score(Some(value), max) {
            return Ok(flash_redirect(
                &back,
                &format!("{}: {msg}", entry.full_name),
                "error",
            ));
        }
        scores.push((entry.student_id, Some(value)));
    }
    grades_model::save_scores(&state.pool, id, &scores).await?;
    Ok(flash_redirect(&back, "Scores saved", "ok"))
}

// ---------------------------------------------------------------------------
// POST /grades/assessments/:id/delete
// ---------------------------------------------------------------------------

async fn delete_assessment_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    check_mutate(&user)?;
    let class_id = match grades_model::delete_assessment(&state.pool, id).await {
        Ok(cid) => {
            if let Err(e) = audit_model::log_action(
                &state.pool,
                Some(user.id),
                "delete",
                "assessment",
                Some(id),
                Some("assessment deleted"),
            )
            .await
            {
                tracing::warn!(error = %e, "audit log failed");
            }
            cid
        }
        Err(AppError::NotFound) => {
            return Ok(flash_redirect("/grades", "Assessment not found", "error"));
        }
        Err(e) => return Err(e),
    };
    Ok(flash_redirect(
        &format!("/grades?class_id={class_id}"),
        "Assessment deleted",
        "ok",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn location(r: Redirect) -> (StatusCode, String) {
        let resp = r.into_response();
        let status = resp.status();
        let loc = resp
            .headers()
            .get("location")
            .expect("redirect has location")
            .to_str()
            .expect("location is ascii")
            .to_string();
        (status, loc)
    }

    #[test]
    fn flash_redirect_targets_class_on_assessment_delete() {
        let (status, loc) = location(flash_redirect(
            "/grades?class_id=3",
            "Assessment deleted",
            "ok",
        ));
        assert_eq!(status, StatusCode::SEE_OTHER);
        assert_eq!(
            loc,
            "/grades?class_id=3&flash=Assessment+deleted&flash_kind=ok"
        );
    }

    #[test]
    fn flash_redirect_carries_error_kind_for_missing_assessment() {
        let (_, loc) = location(flash_redirect("/grades", "Assessment not found", "error"));
        assert_eq!(loc, "/grades?flash=Assessment+not+found&flash_kind=error");
    }
}
