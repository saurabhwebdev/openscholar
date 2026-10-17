//! HR module: staff attendance, leave requests, payroll basics.
//! Runtime SQL only. Money in BIGINT cents.

use chrono::{NaiveDate, Utc};
use sqlx::PgPool;

use crate::error::AppError;
use crate::models::staff::StaffRow;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StaffAttendanceRow {
    pub id: i64,
    pub staff_id: i64,
    pub date: NaiveDate,
    pub status: String,
    pub note: Option<String>,
    pub recorded_by: Option<i64>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LeaveRequestRow {
    pub id: i64,
    pub staff_id: i64,
    pub leave_type: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub reason: Option<String>,
    pub status: String,
    pub approved_by: Option<i64>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayrollRow {
    pub id: i64,
    pub staff_id: i64,
    pub period_label: String,
    pub base_cents: i64,
    pub allowance_cents: i64,
    pub deduction_cents: i64,
    pub net_cents: i64,
    pub paid_on: Option<NaiveDate>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MonthlySummary {
    pub staff_id: i64,
    pub staff_name: String,
    pub present: i64,
    pub absent: i64,
    pub late: i64,
    pub leave: i64,
    pub total: i64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const ATTENDANCE_STATUSES: &[&str] = &["present", "absent", "late", "leave"];
pub const LEAVE_TYPES: &[&str] = &["annual", "sick", "personal", "unpaid"];
pub const LEAVE_STATUSES: &[&str] = &["pending", "approved", "rejected"];

// ---------------------------------------------------------------------------
// Input traits (flexible args)
// ---------------------------------------------------------------------------

pub trait ToNaiveDate {
    fn to_naive_date(&self) -> Result<NaiveDate, String>;
}

impl ToNaiveDate for NaiveDate {
    fn to_naive_date(&self) -> Result<NaiveDate, String> {
        Ok(*self)
    }
}

impl ToNaiveDate for &NaiveDate {
    fn to_naive_date(&self) -> Result<NaiveDate, String> {
        Ok(**self)
    }
}

impl ToNaiveDate for String {
    fn to_naive_date(&self) -> Result<NaiveDate, String> {
        NaiveDate::parse_from_str(self.trim(), "%Y-%m-%d")
            .map_err(|_| "Date must be YYYY-MM-DD".to_string())
    }
}

impl ToNaiveDate for &str {
    fn to_naive_date(&self) -> Result<NaiveDate, String> {
        NaiveDate::parse_from_str(self.trim(), "%Y-%m-%d")
            .map_err(|_| "Date must be YYYY-MM-DD".to_string())
    }
}

impl ToNaiveDate for &String {
    fn to_naive_date(&self) -> Result<NaiveDate, String> {
        NaiveDate::parse_from_str(self.trim(), "%Y-%m-%d")
            .map_err(|_| "Date must be YYYY-MM-DD".to_string())
    }
}

pub trait NoteArg {
    fn to_note(&self) -> Option<String>;
}

impl NoteArg for Option<&str> {
    fn to_note(&self) -> Option<String> {
        self.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
    }
}

impl NoteArg for Option<String> {
    fn to_note(&self) -> Option<String> {
        self.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }
}

impl NoteArg for Option<&String> {
    fn to_note(&self) -> Option<String> {
        self.map(|s| s.trim()).filter(|s| !s.is_empty()).map(str::to_string)
    }
}

impl NoteArg for &str {
    fn to_note(&self) -> Option<String> {
        let t = self.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

impl NoteArg for String {
    fn to_note(&self) -> Option<String> {
        let t = self.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

impl NoteArg for &String {
    fn to_note(&self) -> Option<String> {
        let t = self.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

pub trait ApproverArg {
    fn to_option(self) -> Option<i64>;
}

impl ApproverArg for i64 {
    fn to_option(self) -> Option<i64> {
        Some(self)
    }
}

impl ApproverArg for Option<i64> {
    fn to_option(self) -> Option<i64> {
        self
    }
}

pub trait OptionalDate {
    fn to_option(self) -> Option<NaiveDate>;
}

impl OptionalDate for Option<NaiveDate> {
    fn to_option(self) -> Option<NaiveDate> {
        self
    }
}

impl OptionalDate for Option<&str> {
    fn to_option(self) -> Option<NaiveDate> {
        self.and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
    }
}

impl OptionalDate for Option<String> {
    fn to_option(self) -> Option<NaiveDate> {
        self.as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
    }
}

impl OptionalDate for Option<&String> {
    fn to_option(self) -> Option<NaiveDate> {
        self.and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
    }
}

pub trait MonthInput {
    fn month_range(&self) -> Result<(NaiveDate, NaiveDate), String>;
}

impl MonthInput for NaiveDate {
    fn month_range(&self) -> Result<(NaiveDate, NaiveDate), String> {
        let y = self.year();
        let m = self.month();
        month_range_ym(y as i32, m)
    }
}

impl MonthInput for &NaiveDate {
    fn month_range(&self) -> Result<(NaiveDate, NaiveDate), String> {
        (**self).month_range()
    }
}

impl MonthInput for String {
    fn month_range(&self) -> Result<(NaiveDate, NaiveDate), String> {
        parse_month_str(self.trim())
    }
}

impl MonthInput for &str {
    fn month_range(&self) -> Result<(NaiveDate, NaiveDate), String> {
        parse_month_str(self.trim())
    }
}

impl MonthInput for &String {
    fn month_range(&self) -> Result<(NaiveDate, NaiveDate), String> {
        parse_month_str(self.trim())
    }
}

fn parse_month_str(s: &str) -> Result<(NaiveDate, NaiveDate), String> {
    if s.is_empty() {
        return Err("Month is required".to_string());
    }
    // Try YYYY-MM
    if let Ok(d) = NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d") {
        return month_range_ym(d.year(), d.month());
    }
    // Try full date -> use its month
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return month_range_ym(d.year(), d.month());
    }
    Err("Month must be YYYY-MM".to_string())
}

use chrono::Datelike;

fn month_range_ym(y: i32, m: u32) -> Result<(NaiveDate, NaiveDate), String> {
    let first = NaiveDate::from_ymd_opt(y, m, 1).ok_or("Invalid month".to_string())?;
    let (ny, nm) = if m == 12 {
        (y + 1, 1u32)
    } else {
        (y, m + 1)
    };
    let last = NaiveDate::from_ymd_opt(ny, nm, 1)
        .ok_or("Invalid month".to_string())?
        .pred_opt()
        .ok_or("Invalid month".to_string())?;
    Ok((first, last))
}

// ---------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------

pub fn validate_attendance_status(status: &str) -> Result<(), String> {
    let s = status.trim();
    if ATTENDANCE_STATUSES.contains(&s) {
        Ok(())
    } else {
        Err("Status must be present, absent, late, or leave".to_string())
    }
}

pub fn validate_leave<L, E>(leave_type: &str, start: L, end: E) -> Result<(), String>
where
    L: ToNaiveDate,
    E: ToNaiveDate,
{
    let lt = leave_type.trim();
    if !LEAVE_TYPES.contains(&lt) {
        return Err("Leave type must be annual, sick, personal, or unpaid".to_string());
    }
    let s = start.to_naive_date()?;
    let e = end.to_naive_date()?;
    if e < s {
        return Err("End date must be on or after start date".to_string());
    }
    Ok(())
}

pub fn validate_leave_dates<L, E>(leave_type: &str, start: L, end: E) -> Result<(), String>
where
    L: ToNaiveDate,
    E: ToNaiveDate,
{
    validate_leave(leave_type, start, end)
}

pub fn validate_leave_request<L, E>(leave_type: &str, start: L, end: E) -> Result<(), String>
where
    L: ToNaiveDate,
    E: ToNaiveDate,
{
    validate_leave(leave_type, start, end)
}

pub fn calculate_net(base_cents: i64, allowance_cents: i64, deduction_cents: i64) -> i64 {
    (base_cents + allowance_cents - deduction_cents).max(0)
}

pub fn validate_payroll(
    period_label: &str,
    base_cents: i64,
    allowance_cents: i64,
    deduction_cents: i64,
) -> Result<(), String> {
    if period_label.trim().is_empty() {
        return Err("Period label is required".to_string());
    }
    if base_cents < 0 || allowance_cents < 0 || deduction_cents < 0 {
        return Err("Amounts must be non-negative".to_string());
    }
    let raw = base_cents + allowance_cents - deduction_cents;
    if raw < 0 {
        return Err("Net pay cannot be negative".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn upsert_attendance<D, N>(
    pool: &PgPool,
    staff_id: i64,
    date: D,
    status: &str,
    note: N,
    recorded_by: Option<i64>,
) -> Result<StaffAttendanceRow, AppError>
where
    D: ToNaiveDate,
    N: NoteArg,
{
    let d = date.to_naive_date().map_err(AppError::BadRequest)?;
    let st = status.trim();
    validate_attendance_status(st).map_err(AppError::BadRequest)?;
    let n = note.to_note();
    let row = sqlx::query_as::<_, StaffAttendanceRow>(
        "INSERT INTO staff_attendance (staff_id, date, status, note, recorded_by) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (staff_id, date) DO UPDATE SET status = EXCLUDED.status, \
         note = EXCLUDED.note, recorded_by = EXCLUDED.recorded_by \
         RETURNING id, staff_id, date, status, note, recorded_by, created_at",
    )
    .bind(staff_id)
    .bind(d)
    .bind(st)
    .bind(n)
    .bind(recorded_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn save_attendance<D, N>(
    pool: &PgPool,
    staff_id: i64,
    date: D,
    status: &str,
    note: N,
    recorded_by: Option<i64>,
) -> Result<StaffAttendanceRow, AppError>
where
    D: ToNaiveDate,
    N: NoteArg,
{
    upsert_attendance(pool, staff_id, date, status, note, recorded_by).await
}

pub async fn record_attendance<D, N>(
    pool: &PgPool,
    staff_id: i64,
    date: D,
    status: &str,
    note: N,
    recorded_by: Option<i64>,
) -> Result<StaffAttendanceRow, AppError>
where
    D: ToNaiveDate,
    N: NoteArg,
{
    upsert_attendance(pool, staff_id, date, status, note, recorded_by).await
}

pub async fn mark_attendance<D, N>(
    pool: &PgPool,
    staff_id: i64,
    date: D,
    status: &str,
    note: N,
    recorded_by: Option<i64>,
) -> Result<StaffAttendanceRow, AppError>
where
    D: ToNaiveDate,
    N: NoteArg,
{
    upsert_attendance(pool, staff_id, date, status, note, recorded_by).await
}

pub async fn list_attendance<M>(
    pool: &PgPool,
    staff_id: i64,
    month: M,
) -> Result<Vec<StaffAttendanceRow>, AppError>
where
    M: MonthInput,
{
    let (start, end) = month.month_range().map_err(AppError::BadRequest)?;
    let rows = sqlx::query_as::<_, StaffAttendanceRow>(
        "SELECT id, staff_id, date, status, note, recorded_by, created_at \
         FROM staff_attendance WHERE staff_id = $1 AND date >= $2 AND date <= $3 \
         ORDER BY date",
    )
    .bind(staff_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_staff_attendance<M>(
    pool: &PgPool,
    staff_id: i64,
    month: M,
) -> Result<Vec<StaffAttendanceRow>, AppError>
where
    M: MonthInput,
{
    list_attendance(pool, staff_id, month).await
}

// Attendance for a single date (upsert shorthand already). Find helpers:
pub async fn find_attendance(pool: &PgPool, id: i64) -> Result<Option<StaffAttendanceRow>, AppError> {
    let row = sqlx::query_as::<_, StaffAttendanceRow>(
        "SELECT id, staff_id, date, status, note, recorded_by, created_at \
         FROM staff_attendance WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_attendance(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM staff_attendance WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Leave CRUD
// ---------------------------------------------------------------------------

pub async fn create_leave<D1, D2, N>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
    reason: N,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
    N: NoteArg,
{
    let lt = leave_type.trim();
    validate_leave(lt, &start_date.to_naive_date().map_err(AppError::BadRequest)?, &end_date.to_naive_date().map_err(AppError::BadRequest)?)
        .map_err(AppError::BadRequest)?;
    let s = start_date.to_naive_date().map_err(AppError::BadRequest)?;
    let e = end_date.to_naive_date().map_err(AppError::BadRequest)?;
    let r = reason.to_note();
    let row = sqlx::query_as::<_, LeaveRequestRow>(
        "INSERT INTO leave_requests (staff_id, leave_type, start_date, end_date, reason, status) \
         VALUES ($1, $2, $3, $4, $5, 'pending') \
         RETURNING id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at",
    )
    .bind(staff_id)
    .bind(lt)
    .bind(s)
    .bind(e)
    .bind(r)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_leave_simple<D1, D2>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
{
    create_leave(pool, staff_id, leave_type, start_date, end_date, Option::<&str>::None).await
}

pub async fn create_leave_with_reason<D1, D2, N>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
    reason: N,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
    N: NoteArg,
{
    create_leave(pool, staff_id, leave_type, start_date, end_date, reason).await
}

// aliases
pub async fn request_leave<D1, D2, N>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
    reason: N,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
    N: NoteArg,
{
    create_leave(pool, staff_id, leave_type, start_date, end_date, reason).await
}

pub async fn add_leave<D1, D2, N>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
    reason: N,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
    N: NoteArg,
{
    create_leave(pool, staff_id, leave_type, start_date, end_date, reason).await
}

pub async fn create_leave_request<D1, D2, N>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
    reason: N,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
    N: NoteArg,
{
    create_leave(pool, staff_id, leave_type, start_date, end_date, reason).await
}

pub async fn create_leave_full<D1, D2, N>(
    pool: &PgPool,
    staff_id: i64,
    leave_type: &str,
    start_date: D1,
    end_date: D2,
    reason: N,
    status: &str,
    approved_by: Option<i64>,
) -> Result<LeaveRequestRow, AppError>
where
    D1: ToNaiveDate,
    D2: ToNaiveDate,
    N: NoteArg,
{
    let lt = leave_type.trim();
    let s = start_date.to_naive_date().map_err(AppError::BadRequest)?;
    let e = end_date.to_naive_date().map_err(AppError::BadRequest)?;
    validate_leave(lt, &s, &e).map_err(AppError::BadRequest)?;
    let st = status.trim();
    if !LEAVE_STATUSES.contains(&st) {
        return Err(AppError::BadRequest("Invalid leave status".to_string()));
    }
    let r = reason.to_note();
    let row = sqlx::query_as::<_, LeaveRequestRow>(
        "INSERT INTO leave_requests (staff_id, leave_type, start_date, end_date, reason, status, approved_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at",
    )
    .bind(staff_id)
    .bind(lt)
    .bind(s)
    .bind(e)
    .bind(r)
    .bind(st)
    .bind(approved_by)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn find_leave(pool: &PgPool, id: i64) -> Result<Option<LeaveRequestRow>, AppError> {
    let row = sqlx::query_as::<_, LeaveRequestRow>(
        "SELECT id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at \
         FROM leave_requests WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_leave(pool: &PgPool, id: i64) -> Result<Option<LeaveRequestRow>, AppError> {
    find_leave(pool, id).await
}

pub async fn list_leaves(pool: &PgPool) -> Result<Vec<LeaveRequestRow>, AppError> {
    let rows = sqlx::query_as::<_, LeaveRequestRow>(
        "SELECT id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at \
         FROM leave_requests ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_leave_requests(pool: &PgPool) -> Result<Vec<LeaveRequestRow>, AppError> {
    list_leaves(pool).await
}

pub async fn list_leaves_for_staff(pool: &PgPool, staff_id: i64) -> Result<Vec<LeaveRequestRow>, AppError> {
    let rows = sqlx::query_as::<_, LeaveRequestRow>(
        "SELECT id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at \
         FROM leave_requests WHERE staff_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(staff_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_leaves_by_staff(pool: &PgPool, staff_id: i64) -> Result<Vec<LeaveRequestRow>, AppError> {
    list_leaves_for_staff(pool, staff_id).await
}

pub async fn list_pending_leaves(pool: &PgPool) -> Result<Vec<LeaveRequestRow>, AppError> {
    let rows = sqlx::query_as::<_, LeaveRequestRow>(
        "SELECT id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at \
         FROM leave_requests WHERE status = 'pending' ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn update_leave_status<A>(
    pool: &PgPool,
    id: i64,
    status: &str,
    approved_by: A,
) -> Result<LeaveRequestRow, AppError>
where
    A: ApproverArg,
{
    let st = status.trim();
    if !LEAVE_STATUSES.contains(&st) {
        return Err(AppError::BadRequest("Invalid leave status".to_string()));
    }
    let ab = approved_by.to_option();
    // For pending, clear approved_by? keep provided.
    let row = sqlx::query_as::<_, LeaveRequestRow>(
        "UPDATE leave_requests SET status = $1, approved_by = $2 WHERE id = $3 \
         RETURNING id, staff_id, leave_type, start_date, end_date, reason, status, approved_by, created_at",
    )
    .bind(st)
    .bind(ab)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn set_leave_status<A>(
    pool: &PgPool,
    id: i64,
    status: &str,
    approved_by: A,
) -> Result<LeaveRequestRow, AppError>
where
    A: ApproverArg,
{
    update_leave_status(pool, id, status, approved_by).await
}

pub async fn approve_leave<A>(pool: &PgPool, id: i64, approved_by: A) -> Result<LeaveRequestRow, AppError>
where
    A: ApproverArg,
{
    update_leave_status(pool, id, "approved", approved_by).await
}

pub async fn approve_leave_simple(pool: &PgPool, id: i64) -> Result<LeaveRequestRow, AppError> {
    approve_leave(pool, id, Option::<i64>::None).await
}

pub async fn reject_leave<A>(pool: &PgPool, id: i64, approved_by: A) -> Result<LeaveRequestRow, AppError>
where
    A: ApproverArg,
{
    update_leave_status(pool, id, "rejected", approved_by).await
}

pub async fn reject_leave_simple(pool: &PgPool, id: i64) -> Result<LeaveRequestRow, AppError> {
    reject_leave(pool, id, Option::<i64>::None).await
}

pub async fn delete_leave(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM leave_requests WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_leave(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_leave(pool, id).await
}

// ---------------------------------------------------------------------------
// Payroll CRUD
// ---------------------------------------------------------------------------

pub async fn create_payroll(
    pool: &PgPool,
    staff_id: i64,
    period_label: &str,
    base_cents: i64,
    allowance_cents: i64,
    deduction_cents: i64,
) -> Result<PayrollRow, AppError> {
    create_payroll_with_paid_on(
        pool,
        staff_id,
        period_label,
        base_cents,
        allowance_cents,
        deduction_cents,
        Option::<NaiveDate>::None,
    )
    .await
}

pub async fn create_payroll_with_paid_on<P>(
    pool: &PgPool,
    staff_id: i64,
    period_label: &str,
    base_cents: i64,
    allowance_cents: i64,
    deduction_cents: i64,
    paid_on: P,
) -> Result<PayrollRow, AppError>
where
    P: OptionalDate,
{
    validate_payroll(period_label, base_cents, allowance_cents, deduction_cents)
        .map_err(AppError::BadRequest)?;
    let net = calculate_net(base_cents, allowance_cents, deduction_cents);
    let paid = paid_on.to_option();
    let row = sqlx::query_as::<_, PayrollRow>(
        "INSERT INTO payroll_runs (staff_id, period_label, base_cents, allowance_cents, deduction_cents, net_cents, paid_on) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id, staff_id, period_label, base_cents, allowance_cents, deduction_cents, net_cents, paid_on, created_at",
    )
    .bind(staff_id)
    .bind(period_label.trim())
    .bind(base_cents)
    .bind(allowance_cents)
    .bind(deduction_cents)
    .bind(net)
    .bind(paid)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn add_payroll(
    pool: &PgPool,
    staff_id: i64,
    period_label: &str,
    base_cents: i64,
    allowance_cents: i64,
    deduction_cents: i64,
) -> Result<PayrollRow, AppError> {
    create_payroll(pool, staff_id, period_label, base_cents, allowance_cents, deduction_cents).await
}

pub async fn create_payroll_run<P>(
    pool: &PgPool,
    staff_id: i64,
    period_label: &str,
    base_cents: i64,
    allowance_cents: i64,
    deduction_cents: i64,
    paid_on: P,
) -> Result<PayrollRow, AppError>
where
    P: OptionalDate,
{
    create_payroll_with_paid_on(
        pool,
        staff_id,
        period_label,
        base_cents,
        allowance_cents,
        deduction_cents,
        paid_on,
    )
    .await
}

pub async fn find_payroll(pool: &PgPool, id: i64) -> Result<Option<PayrollRow>, AppError> {
    let row = sqlx::query_as::<_, PayrollRow>(
        "SELECT id, staff_id, period_label, base_cents, allowance_cents, deduction_cents, net_cents, paid_on, created_at \
         FROM payroll_runs WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_payroll(pool: &PgPool, id: i64) -> Result<Option<PayrollRow>, AppError> {
    find_payroll(pool, id).await
}

pub async fn list_payroll(pool: &PgPool) -> Result<Vec<PayrollRow>, AppError> {
    let rows = sqlx::query_as::<_, PayrollRow>(
        "SELECT id, staff_id, period_label, base_cents, allowance_cents, deduction_cents, net_cents, paid_on, created_at \
         FROM payroll_runs ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_payrolls(pool: &PgPool) -> Result<Vec<PayrollRow>, AppError> {
    list_payroll(pool).await
}

pub async fn list_payroll_for_staff(pool: &PgPool, staff_id: i64) -> Result<Vec<PayrollRow>, AppError> {
    let rows = sqlx::query_as::<_, PayrollRow>(
        "SELECT id, staff_id, period_label, base_cents, allowance_cents, deduction_cents, net_cents, paid_on, created_at \
         FROM payroll_runs WHERE staff_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(staff_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_payroll_by_staff(pool: &PgPool, staff_id: i64) -> Result<Vec<PayrollRow>, AppError> {
    list_payroll_for_staff(pool, staff_id).await
}

pub async fn delete_payroll(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM payroll_runs WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_payroll(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_payroll(pool, id).await
}

// ---------------------------------------------------------------------------
// Monthly summary
// ---------------------------------------------------------------------------

pub async fn monthly_summary<M>(
    pool: &PgPool,
    month: M,
) -> Result<Vec<MonthlySummary>, AppError>
where
    M: MonthInput,
{
    let (start, end) = month.month_range().map_err(AppError::BadRequest)?;
    let rows = sqlx::query_as::<_, MonthlySummary>(
        "SELECT s.id AS staff_id, (s.first_name || ' ' || s.last_name) AS staff_name, \
         COUNT(a.id) FILTER (WHERE a.status = 'present')::BIGINT AS present, \
         COUNT(a.id) FILTER (WHERE a.status = 'absent')::BIGINT AS absent, \
         COUNT(a.id) FILTER (WHERE a.status = 'late')::BIGINT AS late, \
         COUNT(a.id) FILTER (WHERE a.status = 'leave')::BIGINT AS leave, \
         COUNT(a.id)::BIGINT AS total \
         FROM staff s LEFT JOIN staff_attendance a \
           ON a.staff_id = s.id AND a.date >= $1 AND a.date <= $2 \
         GROUP BY s.id, s.first_name, s.last_name ORDER BY s.last_name, s.first_name",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn monthly_summary_for_staff<M>(
    pool: &PgPool,
    staff_id: i64,
    month: M,
) -> Result<MonthlySummary, AppError>
where
    M: MonthInput,
{
    let (start, end) = month.month_range().map_err(AppError::BadRequest)?;
    let row = sqlx::query_as::<_, MonthlySummary>(
        "SELECT s.id AS staff_id, (s.first_name || ' ' || s.last_name) AS staff_name, \
         COUNT(a.id) FILTER (WHERE a.status = 'present')::BIGINT AS present, \
         COUNT(a.id) FILTER (WHERE a.status = 'absent')::BIGINT AS absent, \
         COUNT(a.id) FILTER (WHERE a.status = 'late')::BIGINT AS late, \
         COUNT(a.id) FILTER (WHERE a.status = 'leave')::BIGINT AS leave, \
         COUNT(a.id)::BIGINT AS total \
         FROM staff s LEFT JOIN staff_attendance a \
           ON a.staff_id = s.id AND a.date >= $1 AND a.date <= $2 \
         WHERE s.id = $3 GROUP BY s.id, s.first_name, s.last_name",
    )
    .bind(start)
    .bind(end)
    .bind(staff_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn staff_monthly_summary<M>(
    pool: &PgPool,
    staff_id: i64,
    month: M,
) -> Result<MonthlySummary, AppError>
where
    M: MonthInput,
{
    monthly_summary_for_staff(pool, staff_id, month).await
}

// ---------------------------------------------------------------------------
// Staff lookup helpers
// ---------------------------------------------------------------------------

pub async fn staff_id_for_user(pool: &PgPool, email: &str) -> Result<Option<i64>, AppError> {
    let id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM staff WHERE lower(email) = lower($1) LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

pub async fn find_staff_row_for_user(pool: &PgPool, email: &str) -> Result<Option<StaffRow>, AppError> {
    let row = sqlx::query_as::<_, StaffRow>(
        "SELECT id, employee_no, first_name, last_name, email, phone, job_title, department, hired_on, status \
         FROM staff WHERE lower(email) = lower($1) LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Tests (pure)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attendance_status_valid() {
        for s in ATTENDANCE_STATUSES {
            assert!(validate_attendance_status(s).is_ok());
        }
    }

    #[test]
    fn attendance_status_invalid() {
        assert!(validate_attendance_status("foo").is_err());
        assert!(validate_attendance_status("").is_err());
    }

    #[test]
    fn leave_type_valid() {
        for t in LEAVE_TYPES {
            assert!(validate_leave(t, "2026-01-01", "2026-01-02").is_ok());
        }
    }

    #[test]
    fn leave_rejects_bad_type() {
        assert!(validate_leave("holiday", "2026-01-01", "2026-01-02").is_err());
    }

    #[test]
    fn leave_end_before_start_rejected() {
        assert!(validate_leave("annual", "2026-01-05", "2026-01-01").is_err());
    }

    #[test]
    fn leave_equal_dates_ok() {
        assert!(validate_leave("annual", "2026-01-01", "2026-01-01").is_ok());
    }

    #[test]
    fn leave_accepts_naive_dates() {
        let s = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let e = NaiveDate::from_ymd_opt(2026, 1, 3).unwrap();
        assert!(validate_leave("sick", s, e).is_ok());
    }

    #[test]
    fn calculate_net_basic() {
        assert_eq!(calculate_net(100, 10, 5), 105);
        assert_eq!(calculate_net(10, 0, 20), 0);
        assert_eq!(calculate_net(0, 0, 0), 0);
    }

    #[test]
    fn calculate_net_min_zero() {
        assert_eq!(calculate_net(5, 0, 100), 0);
    }

    #[test]
    fn validate_payroll_ok() {
        assert!(validate_payroll("Jan 2026", 100, 10, 5).is_ok());
    }

    #[test]
    fn validate_payroll_empty_period() {
        assert!(validate_payroll("  ", 100, 10, 5).is_err());
        assert!(validate_payroll("", 100, 10, 5).is_err());
    }

    #[test]
    fn validate_payroll_negative_amounts() {
        assert!(validate_payroll("Jan", -1, 0, 0).is_err());
        assert!(validate_payroll("Jan", 0, -1, 0).is_err());
        assert!(validate_payroll("Jan", 0, 0, -1).is_err());
    }

    #[test]
    fn validate_payroll_rejects_negative_net() {
        assert!(validate_payroll("Jan", 10, 0, 20).is_err());
    }

    #[test]
    fn month_range_works() {
        let (s, e) = parse_month_str("2026-02").unwrap();
        assert_eq!(s, NaiveDate::from_ymd_opt(2026, 2, 1).unwrap());
        assert_eq!(e, NaiveDate::from_ymd_opt(2026, 2, 28).unwrap());
    }

    #[test]
    fn month_range_leap() {
        let (_, e) = parse_month_str("2024-02").unwrap();
        assert_eq!(e, NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
    }
}
