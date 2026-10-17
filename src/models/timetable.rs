use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SubjectRow {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub department: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PeriodRow {
    pub id: i64,
    pub weekday: i16,
    pub start: String,
    pub end: String,
    pub label: String,
}

/// One booked lesson with everything the grid tiles need pre-joined.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SlotView {
    pub id: i64,
    pub class_id: i64,
    pub class_name: String,
    pub period_id: i64,
    pub weekday: i16,
    pub period_label: String,
    pub start: String,
    pub end: String,
    pub subject_id: i64,
    pub subject_code: String,
    pub subject_name: String,
    pub teacher_id: Option<i64>,
    pub teacher_name: Option<String>,
    pub room: Option<String>,
}

/// A subject linked to a class (the assign table behind the slot form).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClassSubjectView {
    pub id: i64,
    pub subject_id: i64,
    pub code: String,
    pub name: String,
    pub teacher_id: Option<i64>,
    pub teacher_name: Option<String>,
}

/// One grid row: a period label + time range with Mon-Fri cells.
#[derive(Debug, Clone)]
pub struct GridRow {
    pub label: String,
    pub time_range: String,
    pub cells: Vec<Option<SlotView>>,
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------

/// `code` and `name` are required; department is optional.
pub fn validate_subject(code: &str, name: &str) -> Result<(), String> {
    if code.trim().is_empty() {
        return Err("Subject code is required".to_string());
    }
    if name.trim().is_empty() {
        return Err("Subject name is required".to_string());
    }
    Ok(())
}

pub fn weekday_name(weekday: i16) -> &'static str {
    match weekday {
        0 => "Monday",
        1 => "Tuesday",
        2 => "Wednesday",
        3 => "Thursday",
        4 => "Friday",
        _ => "—",
    }
}

pub fn weekday_short(weekday: i16) -> &'static str {
    match weekday {
        0 => "Mon",
        1 => "Tue",
        2 => "Wed",
        3 => "Thu",
        4 => "Fri",
        _ => "?",
    }
}

/// Flash/400 message used when a teacher is already booked at a period.
pub fn clash_message(teacher_name: &str) -> String {
    format!("{teacher_name} is already booked in another class at that time")
}

/// Find the slot for one grid cell.
pub fn grid_lookup<'a>(
    slots: &'a [SlotView],
    weekday: i16,
    period_label: &str,
) -> Option<&'a SlotView> {
    slots
        .iter()
        .find(|s| s.weekday == weekday && s.period_label == period_label)
}

/// Build Mon-Fri rows from the weekday-0 canonical periods; cells resolve
/// via [`grid_lookup`]. Empty when no periods exist (fresh DB, no migrate).
pub fn build_grid(periods: &[PeriodRow], slots: &[SlotView]) -> Vec<GridRow> {
    let mut day0: Vec<&PeriodRow> = periods.iter().filter(|p| p.weekday == 0).collect();
    day0.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| a.label.cmp(&b.label))
    });
    day0
        .into_iter()
        .map(|p| GridRow {
            label: p.label.clone(),
            time_range: format!("{}–{}", p.start, p.end),
            cells: (0..5)
                .map(|w| grid_lookup(slots, w, &p.label).cloned())
                .collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Queries (runtime SQL only)
// ---------------------------------------------------------------------------

pub async fn list_subjects(pool: &PgPool) -> Result<Vec<SubjectRow>, AppError> {
    let rows = sqlx::query_as::<_, SubjectRow>(
        "SELECT id, code, name, department FROM subjects ORDER BY code LIMIT 500",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Ids of subjects referenced by any class link or slot (delete guard +
/// "In use" pills). Empty when the timetable tables do not exist yet.
pub async fn assigned_subject_ids(pool: &PgPool) -> Result<Vec<i64>, AppError> {
    let rows: Vec<i64> = sqlx::query_scalar(
        "SELECT subject_id FROM class_subjects
          UNION SELECT subject_id FROM slots",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_subject(
    pool: &PgPool,
    code: &str,
    name: &str,
    department: Option<&str>,
) -> Result<SubjectRow, AppError> {
    validate_subject(code, name).map_err(AppError::BadRequest)?;
    let row = sqlx::query_as::<_, SubjectRow>(
        "INSERT INTO subjects (code, name, department)
          VALUES (trim($1), trim($2), NULLIF(trim($3), ''))
          RETURNING id, code, name, department",
    )
    .bind(code.trim())
    .bind(name.trim())
    .bind(department.unwrap_or("").trim())
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Delete a subject; refuses when it is linked to classes or booked in the
/// timetable so routes can flash instead of 500ing. 404 when missing.
pub async fn delete_subject(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let row = sqlx::query_as::<_, SubjectRow>(
        "SELECT id, code, name, department FROM subjects WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let used: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM class_subjects WHERE subject_id = $1)
              + (SELECT COUNT(*) FROM slots WHERE subject_id = $1)",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if used > 0 {
        return Err(AppError::BadRequest(format!(
            "Cannot delete {} — it is assigned to classes or the timetable",
            row.code
        )));
    }
    sqlx::query("DELETE FROM subjects WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Link a subject to a class (upsert: re-assigning updates the teacher).
pub async fn assign_subject(
    pool: &PgPool,
    class_id: i64,
    subject_id: i64,
    teacher_id: Option<i64>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO class_subjects (class_id, subject_id, teacher_id)
          VALUES ($1, $2, $3)
          ON CONFLICT (class_id, subject_id)
          DO UPDATE SET teacher_id = EXCLUDED.teacher_id",
    )
    .bind(class_id)
    .bind(subject_id)
    .bind(teacher_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Unlink a subject from a class, dropping its booked slots with it so the
/// grid never shows stale tiles.
pub async fn unassign_subject(
    pool: &PgPool,
    class_id: i64,
    subject_id: i64,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM slots WHERE class_id = $1 AND subject_id = $2")
        .bind(class_id)
        .bind(subject_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM class_subjects WHERE class_id = $1 AND subject_id = $2")
        .bind(class_id)
        .bind(subject_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_class_subjects(
    pool: &PgPool,
    class_id: i64,
) -> Result<Vec<ClassSubjectView>, AppError> {
    let rows = sqlx::query_as::<_, ClassSubjectView>(
        "SELECT cs.id, cs.subject_id, s.code, s.name,
                cs.teacher_id, (t.first_name || ' ' || t.last_name) AS teacher_name
          FROM class_subjects cs
          JOIN subjects s ON s.id = cs.subject_id
          LEFT JOIN staff t ON t.id = cs.teacher_id
          WHERE cs.class_id = $1
          ORDER BY s.code",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_periods(pool: &PgPool) -> Result<Vec<PeriodRow>, AppError> {
    let rows = sqlx::query_as::<_, PeriodRow>(
        "SELECT id, weekday,
                to_char(start_time, 'HH24:MI') AS \"start\",
                to_char(end_time, 'HH24:MI') AS \"end\",
                label
          FROM periods ORDER BY weekday, start_time",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every booked lesson for one class, ordered period-first for grid order.
pub async fn class_grid(pool: &PgPool, class_id: i64) -> Result<Vec<SlotView>, AppError> {
    // NOTE: kept as two literal queries (not format!-built) because sqlx 0.9
    // requires static SQL strings.
    let rows = sqlx::query_as::<_, SlotView>(
        "SELECT sl.id, sl.class_id, c.name AS class_name,
        sl.period_id, p.weekday, p.label AS period_label,
        to_char(p.start_time, 'HH24:MI') AS \"start\",
        to_char(p.end_time, 'HH24:MI') AS \"end\",
        sl.subject_id, s.code AS subject_code, s.name AS subject_name,
        sl.teacher_id, (t.first_name || ' ' || t.last_name) AS teacher_name, sl.room
      FROM slots sl
      JOIN classes c ON c.id = sl.class_id
      JOIN periods p ON p.id = sl.period_id
      JOIN subjects s ON s.id = sl.subject_id
      LEFT JOIN staff t ON t.id = sl.teacher_id
      WHERE sl.class_id = $1 ORDER BY p.start_time, p.weekday",
    )
    .bind(class_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every booked lesson for one teacher, ordered for the weekly view.
pub async fn teacher_grid(pool: &PgPool, staff_id: i64) -> Result<Vec<SlotView>, AppError> {
    let rows = sqlx::query_as::<_, SlotView>(
        "SELECT sl.id, sl.class_id, c.name AS class_name,
        sl.period_id, p.weekday, p.label AS period_label,
        to_char(p.start_time, 'HH24:MI') AS \"start\",
        to_char(p.end_time, 'HH24:MI') AS \"end\",
        sl.subject_id, s.code AS subject_code, s.name AS subject_name,
        sl.teacher_id, (t.first_name || ' ' || t.last_name) AS teacher_name, sl.room
      FROM slots sl
      JOIN classes c ON c.id = sl.class_id
      JOIN periods p ON p.id = sl.period_id
      JOIN subjects s ON s.id = sl.subject_id
      LEFT JOIN staff t ON t.id = sl.teacher_id
      WHERE sl.teacher_id = $1 ORDER BY p.start_time, p.weekday",
    )
    .bind(staff_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// True when `teacher_id` already teaches another class at `period_id`.
/// `ignore_slot` excludes one slot (slot-edit flows); `None` for creates.
pub async fn teacher_clash(
    pool: &PgPool,
    teacher_id: i64,
    period_id: i64,
    ignore_slot: Option<i64>,
) -> Result<bool, AppError> {
    let clash: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM slots
          WHERE teacher_id = $1 AND period_id = $2
            AND ($3::BIGINT IS NULL OR id != $3))",
    )
    .bind(teacher_id)
    .bind(period_id)
    .bind(ignore_slot)
    .fetch_one(pool)
    .await?;
    Ok(clash)
}

/// Book a lesson: teacher double-booking becomes a named `BadRequest` and a
/// duplicate class/period becomes a friendly `BadRequest` (routes flash
/// both). Returns the new slot id.
pub async fn save_slot(
    pool: &PgPool,
    class_id: i64,
    subject_id: i64,
    period_id: i64,
    teacher_id: Option<i64>,
    room: Option<&str>,
) -> Result<i64, AppError> {
    // Validate referenced rows up front so a bad/stale id is a friendly error
    // rather than a raw 500 from a foreign-key violation.
    let class_ok: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM classes WHERE id = $1)")
            .bind(class_id)
            .fetch_one(pool)
            .await?;
    if !class_ok {
        return Err(AppError::BadRequest("That class no longer exists".to_string()));
    }
    let subject_ok: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM subjects WHERE id = $1)")
            .bind(subject_id)
            .fetch_one(pool)
            .await?;
    if !subject_ok {
        return Err(AppError::BadRequest("Unknown subject".to_string()));
    }
    let period_ok: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM periods WHERE id = $1)")
            .bind(period_id)
            .fetch_one(pool)
            .await?;
    if !period_ok {
        return Err(AppError::BadRequest("Unknown period".to_string()));
    }
    if let Some(tid) = teacher_id {
        let teacher_ok: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM staff WHERE id = $1)")
                .bind(tid)
                .fetch_one(pool)
                .await?;
        if !teacher_ok {
            return Err(AppError::BadRequest("Unknown teacher".to_string()));
        }
    }
    if let Some(tid) = teacher_id {
        if teacher_clash(pool, tid, period_id, None).await? {
            let name: Option<String> = sqlx::query_scalar(
                "SELECT first_name || ' ' || last_name FROM staff WHERE id = $1",
            )
            .bind(tid)
            .fetch_optional(pool)
            .await?;
            return Err(AppError::BadRequest(clash_message(
                name.as_deref().unwrap_or("That teacher"),
            )));
        }
    }
    // Reject booking a room that is already occupied in the same period.
    if let Some(r) = room {
        let r = r.trim();
        if !r.is_empty() {
            let room_clash: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM slots
                  WHERE period_id = $1 AND room IS NOT NULL AND lower(trim(room)) = lower($2))",
            )
            .bind(period_id)
            .bind(r)
            .fetch_one(pool)
            .await?;
            if room_clash {
                return Err(AppError::BadRequest(format!(
                    "Room {r} is already booked in this period"
                )));
            }
        }
    }
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO slots (class_id, subject_id, period_id, teacher_id, room)
          VALUES ($1, $2, $3, $4, NULLIF(trim($5), ''))
          RETURNING id",
    )
    .bind(class_id)
    .bind(subject_id)
    .bind(period_id)
    .bind(teacher_id)
    .bind(room.unwrap_or("").trim())
    .fetch_one(pool)
    .await;
    match id {
        Ok(id) => Ok(id),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            // Distinguish the teacher-period backstop (race) from the class-period key.
            let msg = match db.constraint() {
                Some("slots_teacher_period_uniq") => {
                    "That teacher is already booked in this period".to_string()
                }
                _ => "That class already has a lesson in this period".to_string(),
            };
            Err(AppError::BadRequest(msg))
        }
        Err(e) => Err(AppError::from(e)),
    }
}

/// Delete a slot; returns the owning class id for the redirect. 404 when
/// the slot is already gone.
pub async fn delete_slot(pool: &PgPool, id: i64) -> Result<i64, AppError> {
    let class_id: Option<i64> =
        sqlx::query_scalar("DELETE FROM slots WHERE id = $1 RETURNING class_id")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    class_id.ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(weekday: i16, label: &str) -> SlotView {
        SlotView {
            id: 1,
            class_id: 1,
            class_name: "Grade 1A".to_string(),
            period_id: 1,
            weekday,
            period_label: label.to_string(),
            start: "08:00".to_string(),
            end: "08:45".to_string(),
            subject_id: 1,
            subject_code: "MAT".to_string(),
            subject_name: "Mathematics".to_string(),
            teacher_id: None,
            teacher_name: None,
            room: None,
        }
    }

    fn period(weekday: i16, label: &str, start: &str, end: &str) -> PeriodRow {
        PeriodRow {
            id: 1,
            weekday,
            start: start.to_string(),
            end: end.to_string(),
            label: label.to_string(),
        }
    }

    #[test]
    fn rejects_blank_code() {
        assert_eq!(
            validate_subject("", "Mathematics"),
            Err("Subject code is required".to_string())
        );
        assert_eq!(
            validate_subject("   ", "Mathematics"),
            Err("Subject code is required".to_string())
        );
    }

    #[test]
    fn rejects_blank_name() {
        assert_eq!(
            validate_subject("MAT", ""),
            Err("Subject name is required".to_string())
        );
        assert_eq!(
            validate_subject("MAT", "  "),
            Err("Subject name is required".to_string())
        );
    }

    #[test]
    fn accepts_valid_subject() {
        assert!(validate_subject("MAT", "Mathematics").is_ok());
    }

    #[test]
    fn trims_before_checking() {
        assert!(validate_subject("  MAT  ", "  Mathematics  ").is_ok());
    }

    #[test]
    fn weekday_names_cover_mon_fri() {
        assert_eq!(
            ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]
                .iter()
                .enumerate()
                .map(|(i, _)| weekday_name(i as i16))
                .collect::<Vec<_>>(),
            ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"]
        );
        assert_eq!(
            (0..5).map(weekday_short).collect::<Vec<_>>(),
            ["Mon", "Tue", "Wed", "Thu", "Fri"]
        );
    }

    #[test]
    fn weekday_name_falls_back_off_grid() {
        assert_eq!(weekday_name(5), "—");
        assert_eq!(weekday_name(-1), "—");
    }

    #[test]
    fn clash_message_names_the_teacher() {
        let msg = clash_message("Ada Lovelace");
        assert!(msg.contains("Ada Lovelace"), "got: {msg}");
        assert!(msg.contains("already booked"), "got: {msg}");
    }

    #[test]
    fn grid_lookup_hits_exact_cell() {
        let slots = vec![slot(2, "P3")];
        let hit = grid_lookup(&slots, 2, "P3");
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().subject_code, "MAT");
    }

    #[test]
    fn grid_lookup_misses_wrong_day_or_period() {
        let slots = vec![slot(2, "P3")];
        assert!(grid_lookup(&slots, 3, "P3").is_none());
        assert!(grid_lookup(&slots, 2, "P4").is_none());
        assert!(grid_lookup(&[], 2, "P3").is_none());
    }

    #[test]
    fn build_grid_places_slots_in_cells() {
        let periods = vec![
            period(0, "P1", "08:00", "08:45"),
            period(0, "P2", "08:45", "09:30"),
            period(1, "P1", "08:00", "08:45"),
        ];
        let slots = vec![slot(2, "P1")];
        let rows = build_grid(&periods, &slots);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "P1");
        assert_eq!(rows[0].time_range, "08:00–08:45");
        // Wednesday cell filled, everything else empty.
        assert!(rows[0].cells[0].is_none());
        assert!(rows[0].cells[2].is_some());
        assert!(rows[1].cells.iter().all(|c| c.is_none()));
    }

    #[test]
    fn build_grid_orders_by_start_time() {
        let periods = vec![
            period(0, "P2", "08:45", "09:30"),
            period(0, "P1", "08:00", "08:45"),
        ];
        let rows = build_grid(&periods, &[]);
        assert_eq!(rows[0].label, "P1");
        assert_eq!(rows[1].label, "P2");
    }

    #[test]
    fn build_grid_is_empty_without_periods() {
        assert!(build_grid(&[], &[slot(0, "P1")]).is_empty());
    }
}
