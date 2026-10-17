//! Hostel module: rooms, allocations, validation, runtime SQL.

use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RoomRow {
    pub id: i64,
    pub name: String,
    pub block: Option<String>,
    pub capacity: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[sqlx(default)]
    pub open_count: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AllocationRow {
    pub id: i64,
    pub student_id: i64,
    pub room_id: i64,
    pub bed_label: Option<String>,
    pub allocated_on: chrono::NaiveDate,
    pub released_on: Option<chrono::NaiveDate>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[sqlx(default)]
    pub room_name: Option<String>,
    #[sqlx(default)]
    pub block: Option<String>,
    #[sqlx(default)]
    pub student_name: Option<String>,
    #[sqlx(default)]
    pub capacity: Option<i32>,
}

pub fn validate_room(name: &str, capacity: i32) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Room name is required".to_string());
    }
    if capacity <= 0 {
        return Err("Capacity must be greater than 0".to_string());
    }
    Ok(())
}

pub fn validate_allocation(
    student_id: i64,
    room_id: i64,
    bed_label: Option<&str>,
) -> Result<(), String> {
    if student_id <= 0 {
        return Err("Student is required".to_string());
    }
    if room_id <= 0 {
        return Err("Room is required".to_string());
    }
    if let Some(b) = bed_label {
        if b.trim().chars().count() > 32 {
            return Err("Bed label must be at most 32 characters".to_string());
        }
    }
    Ok(())
}

pub fn can_allocate(has_open_allocation: bool, open_count: i64, capacity: i64) -> bool {
    !has_open_allocation && open_count < capacity
}

pub async fn create_room(
    pool: &PgPool,
    name: &str,
    block: Option<String>,
    capacity: i32,
) -> Result<RoomRow, AppError> {
    if let Err(msg) = validate_room(name, capacity) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, RoomRow>(
        "INSERT INTO hostel_rooms (name, block, capacity)
         VALUES ($1, $2, $3)
         RETURNING id, name, block, capacity, created_at",
    )
    .bind(name.trim())
    .bind(block)
    .bind(capacity)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn list_rooms(pool: &PgPool) -> Result<Vec<RoomRow>, AppError> {
    let rows = sqlx::query_as::<_, RoomRow>(
        "SELECT id, name, block, capacity, created_at
         FROM hostel_rooms ORDER BY name ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find_room(pool: &PgPool, id: i64) -> Result<Option<RoomRow>, AppError> {
    let row = sqlx::query_as::<_, RoomRow>(
        "SELECT id, name, block, capacity, created_at
         FROM hostel_rooms WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn delete_room(pool: &PgPool, id: i64) -> Result<(), AppError> {
    // Only block on *open* allocations; a room whose occupants have all been
    // released should still be decommissionable.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM hostel_allocations WHERE room_id = $1 AND released_on IS NULL",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    if count > 0 {
        return Err(AppError::BadRequest(
            "Room has current occupants and cannot be deleted".to_string(),
        ));
    }
    let done = sqlx::query("DELETE FROM hostel_rooms WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn room_occupancy(pool: &PgPool) -> Result<Vec<RoomRow>, AppError> {
    let rows = sqlx::query_as::<_, RoomRow>(
        "SELECT r.id AS id, r.name AS name, r.block AS block, r.capacity AS capacity,
                r.created_at AS created_at,
                COALESCE((SELECT COUNT(*)::BIGINT FROM hostel_allocations a
                          WHERE a.room_id = r.id AND a.released_on IS NULL), 0) AS open_count
         FROM hostel_rooms r
         ORDER BY r.name ASC, r.id ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add_allocation(
    pool: &PgPool,
    student_id: i64,
    room_id: i64,
    bed_label: Option<String>,
) -> Result<AllocationRow, AppError> {
    if let Err(msg) = validate_allocation(student_id, room_id, bed_label.as_deref()) {
        return Err(AppError::BadRequest(msg));
    }
    let mut tx = pool.begin().await?;
    let capacity: Option<i32> =
        sqlx::query_scalar("SELECT capacity FROM hostel_rooms WHERE id = $1 FOR UPDATE")
            .bind(room_id)
            .fetch_optional(&mut *tx)
            .await?;
    let capacity = capacity.ok_or(AppError::NotFound)?;
    let has_open: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM hostel_allocations WHERE student_id = $1 AND released_on IS NULL)",
    )
    .bind(student_id)
    .fetch_one(&mut *tx)
    .await?;
    let open_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM hostel_allocations WHERE room_id = $1 AND released_on IS NULL",
    )
    .bind(room_id)
    .fetch_one(&mut *tx)
    .await?;

    if !can_allocate(has_open, open_count, capacity as i64) {
        tx.rollback().await.ok();
        if has_open {
            return Err(AppError::BadRequest(
                "Student already has an open allocation".to_string(),
            ));
        }
        return Err(AppError::BadRequest("Room is full".to_string()));
    }

    let bed = bed_label
        .map(|b| b.trim().to_string())
        .filter(|s| !s.is_empty());

    let row = sqlx::query_as::<_, AllocationRow>(
        "INSERT INTO hostel_allocations (student_id, room_id, bed_label)
         VALUES ($1, $2, $3)
         RETURNING id, student_id, room_id, bed_label, allocated_on, released_on, created_at",
    )
    .bind(student_id)
    .bind(room_id)
    .bind(bed)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn release_allocation(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let done = sqlx::query(
        "UPDATE hostel_allocations SET released_on = CURRENT_DATE WHERE id = $1 AND released_on IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn list_for_student(
    pool: &PgPool,
    student_id: i64,
) -> Result<Vec<AllocationRow>, AppError> {
    let rows = sqlx::query_as::<_, AllocationRow>(
        "SELECT a.id AS id, a.student_id AS student_id, a.room_id AS room_id,
                a.bed_label AS bed_label, a.allocated_on AS allocated_on,
                a.released_on AS released_on, a.created_at AS created_at,
                r.name AS room_name, r.block AS block
         FROM hostel_allocations a
         JOIN hostel_rooms r ON r.id = a.room_id
         WHERE a.student_id = $1
         ORDER BY a.allocated_on DESC, a.id DESC",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn open_allocation_for_student(
    pool: &PgPool,
    student_id: i64,
) -> Result<Option<AllocationRow>, AppError> {
    let rows = sqlx::query_as::<_, AllocationRow>(
        "SELECT a.id AS id, a.student_id AS student_id, a.room_id AS room_id,
                a.bed_label AS bed_label, a.allocated_on AS allocated_on,
                a.released_on AS released_on, a.created_at AS created_at,
                r.name AS room_name, r.block AS block
         FROM hostel_allocations a
         JOIN hostel_rooms r ON r.id = a.room_id
         WHERE a.student_id = $1 AND a.released_on IS NULL
         ORDER BY a.allocated_on DESC, a.id DESC
         LIMIT 1",
    )
    .bind(student_id)
    .fetch_optional(pool)
    .await?;
    Ok(rows)
}

pub async fn allocations_list(pool: &PgPool) -> Result<Vec<AllocationRow>, AppError> {
    let rows = sqlx::query_as::<_, AllocationRow>(
        "SELECT a.id AS id, a.student_id AS student_id, a.room_id AS room_id,
                a.bed_label AS bed_label, a.allocated_on AS allocated_on,
                a.released_on AS released_on, a.created_at AS created_at,
                r.name AS room_name, r.block AS block,
                (s.first_name || ' ' || s.last_name) AS student_name
         FROM hostel_allocations a
         JOIN hostel_rooms r ON r.id = a.room_id
         JOIN students s ON s.id = a.student_id
         ORDER BY (a.released_on IS NULL) DESC, a.allocated_on DESC, a.id DESC LIMIT 1000",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_room_requires_name() {
        assert!(validate_room("", 2).is_err());
        assert!(validate_room("   ", 2).is_err());
    }

    #[test]
    fn validate_room_requires_positive_capacity() {
        assert!(validate_room("A1", 0).is_err());
        assert!(validate_room("A1", -1).is_err());
    }

    #[test]
    fn validate_room_accepts_valid() {
        assert!(validate_room("A1", 1).is_ok());
    }

    #[test]
    fn validate_allocation_requires_student_and_room() {
        assert!(validate_allocation(0, 1, None).is_err());
        assert!(validate_allocation(1, 0, None).is_err());
        assert!(validate_allocation(1, 1, None).is_ok());
    }

    #[test]
    fn validate_allocation_bed_label_length() {
        assert!(validate_allocation(1, 1, Some("bed-1")).is_ok());
        assert!(validate_allocation(1, 1, Some(&"x".repeat(33))).is_err());
        assert!(validate_allocation(1, 1, Some(&"x".repeat(32))).is_ok());
    }

    #[test]
    fn can_allocate_logic() {
        assert!(can_allocate(false, 0, 2));
        assert!(!can_allocate(true, 0, 2));
        assert!(!can_allocate(false, 2, 2));
        assert!(can_allocate(false, 1, 2));
    }
}
