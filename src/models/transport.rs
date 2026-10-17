//! Transport module: vehicles, routes, stops, riders, validation, runtime SQL.

use sqlx::PgPool;

use crate::error::AppError;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct VehicleRow {
    pub id: i64,
    pub reg_no: String,
    pub kind: String,
    pub capacity: i32,
    pub driver_name: Option<String>,
    pub driver_phone: Option<String>,
    pub active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RouteRow {
    pub id: i64,
    pub name: String,
    pub vehicle_id: Option<i64>,
    pub fee_cents: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[sqlx(default)]
    pub vehicle_reg: Option<String>,
    #[sqlx(default)]
    pub vehicle_reg_no: Option<String>,
    #[sqlx(default)]
    pub reg_no: Option<String>,
    #[sqlx(default)]
    pub stop_name: Option<String>,
    #[sqlx(default)]
    pub stop: Option<String>,
    #[sqlx(default)]
    pub riders_count: i64,
    #[sqlx(default)]
    pub vehicle_capacity: Option<i32>,
}

impl RouteRow {
    /// Route fee formatted as currency (e.g. `5.00`), for templates. The raw
    /// `fee_cents` must never be rendered directly — it would show `500`.
    pub fn fee(&self) -> String {
        crate::util::format_cents(self.fee_cents)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StopRow {
    pub id: i64,
    pub route_id: i64,
    pub name: String,
    pub sequence: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RiderRow {
    pub id: i64,
    pub student_id: i64,
    pub route_id: i64,
    pub stop_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    #[sqlx(default)]
    pub student_name: String,
    #[sqlx(default)]
    pub stop_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VehicleInput {
    pub reg_no: String,
    pub kind: String,
    pub capacity: i32,
    pub driver_name: Option<String>,
    pub driver_phone: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct RouteInput {
    pub name: String,
    pub vehicle_id: Option<i64>,
    pub fee_cents: i64,
}

#[derive(Debug, Clone)]
pub struct StopInput {
    pub route_id: i64,
    pub name: String,
    pub sequence: i32,
}

#[derive(Debug, Clone)]
pub struct RiderInput {
    pub student_id: i64,
    pub route_id: i64,
    pub stop_id: Option<i64>,
}

// ---------------------------------------------------------------------------
// Validation (pure)
// ---------------------------------------------------------------------------

pub fn validate_vehicle(reg_no: &str, capacity: i32) -> Result<(), String> {
    if reg_no.trim().is_empty() {
        return Err("Registration number is required".to_string());
    }
    if capacity <= 0 {
        return Err("Capacity must be greater than 0".to_string());
    }
    Ok(())
}

pub fn validate_route(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Route name is required".to_string());
    }
    Ok(())
}

pub fn validate_stop(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Stop name is required".to_string());
    }
    Ok(())
}

pub fn validate_rider(student_id: i64, route_id: i64) -> Result<(), String> {
    if student_id <= 0 {
        return Err("Student is required".to_string());
    }
    if route_id <= 0 {
        return Err("Route is required".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Vehicles CRUD
// ---------------------------------------------------------------------------

pub async fn list_vehicles(pool: &PgPool) -> Result<Vec<VehicleRow>, AppError> {
    let rows = sqlx::query_as::<_, VehicleRow>(
        "SELECT id, reg_no, kind, capacity, driver_name, driver_phone, active, created_at
         FROM vehicles ORDER BY reg_no ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_vehicles(pool: &PgPool) -> Result<Vec<VehicleRow>, AppError> {
    list_vehicles(pool).await
}

pub async fn find_vehicle(pool: &PgPool, id: i64) -> Result<Option<VehicleRow>, AppError> {
    let row = sqlx::query_as::<_, VehicleRow>(
        "SELECT id, reg_no, kind, capacity, driver_name, driver_phone, active, created_at
         FROM vehicles WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_vehicle(pool: &PgPool, id: i64) -> Result<Option<VehicleRow>, AppError> {
    find_vehicle(pool, id).await
}

pub async fn create_vehicle(pool: &PgPool, input: &VehicleInput) -> Result<VehicleRow, AppError> {
    if let Err(msg) = validate_vehicle(&input.reg_no, input.capacity) {
        return Err(AppError::BadRequest(msg));
    }
    let kind = if input.kind.trim().is_empty() {
        "bus".to_string()
    } else {
        input.kind.trim().to_string()
    };
    let row = sqlx::query_as::<_, VehicleRow>(
        "INSERT INTO vehicles (reg_no, kind, capacity, driver_name, driver_phone, active)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, reg_no, kind, capacity, driver_name, driver_phone, active, created_at",
    )
    .bind(input.reg_no.trim())
    .bind(kind)
    .bind(input.capacity)
    .bind(input.driver_name.clone())
    .bind(input.driver_phone.clone())
    .bind(input.active)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_vehicle_fields(
    pool: &PgPool,
    reg_no: &str,
    kind: &str,
    capacity: i32,
    driver_name: Option<String>,
    driver_phone: Option<String>,
    active: bool,
) -> Result<VehicleRow, AppError> {
    let input = VehicleInput {
        reg_no: reg_no.to_string(),
        kind: kind.to_string(),
        capacity,
        driver_name,
        driver_phone,
        active,
    };
    create_vehicle(pool, &input).await
}

pub async fn update_vehicle(
    pool: &PgPool,
    id: i64,
    input: &VehicleInput,
) -> Result<VehicleRow, AppError> {
    if let Err(msg) = validate_vehicle(&input.reg_no, input.capacity) {
        return Err(AppError::BadRequest(msg));
    }
    let kind = if input.kind.trim().is_empty() {
        "bus".to_string()
    } else {
        input.kind.trim().to_string()
    };
    let row = sqlx::query_as::<_, VehicleRow>(
        "UPDATE vehicles SET reg_no = $2, kind = $3, capacity = $4, driver_name = $5, driver_phone = $6, active = $7
         WHERE id = $1
         RETURNING id, reg_no, kind, capacity, driver_name, driver_phone, active, created_at",
    )
    .bind(id)
    .bind(input.reg_no.trim())
    .bind(kind)
    .bind(input.capacity)
    .bind(input.driver_name.clone())
    .bind(input.driver_phone.clone())
    .bind(input.active)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_vehicle(pool: &PgPool, id: i64) -> Result<(), AppError> {
    // blocked if referenced by routes
    let refs: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM transport_routes WHERE vehicle_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?;
    if refs > 0 {
        return Err(AppError::BadRequest(
            "Cannot delete vehicle referenced by routes".to_string(),
        ));
    }
    let res = sqlx::query("DELETE FROM vehicles WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_vehicle(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_vehicle(pool, id).await
}

// ---------------------------------------------------------------------------
// Routes CRUD
// ---------------------------------------------------------------------------

pub async fn list_routes(pool: &PgPool) -> Result<Vec<RouteRow>, AppError> {
    let rows = sqlx::query_as::<_, RouteRow>(
        "SELECT tr.id, tr.name, tr.vehicle_id, tr.fee_cents, tr.created_at,
                v.reg_no AS vehicle_reg, v.reg_no AS vehicle_reg_no, v.reg_no AS reg_no,
                v.capacity AS vehicle_capacity,
                (SELECT COUNT(*)::BIGINT FROM transport_riders r WHERE r.route_id = tr.id) AS riders_count
         FROM transport_routes tr LEFT JOIN vehicles v ON v.id = tr.vehicle_id
         ORDER BY tr.name ASC, tr.id ASC LIMIT 500",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_routes(pool: &PgPool) -> Result<Vec<RouteRow>, AppError> {
    list_routes(pool).await
}

pub async fn find_route(pool: &PgPool, id: i64) -> Result<Option<RouteRow>, AppError> {
    let row = sqlx::query_as::<_, RouteRow>(
        "SELECT tr.id, tr.name, tr.vehicle_id, tr.fee_cents, tr.created_at,
                v.reg_no AS vehicle_reg, v.reg_no AS vehicle_reg_no, v.reg_no AS reg_no,
                v.capacity AS vehicle_capacity,
                (SELECT COUNT(*)::BIGINT FROM transport_riders r WHERE r.route_id = tr.id) AS riders_count
         FROM transport_routes tr LEFT JOIN vehicles v ON v.id = tr.vehicle_id
         WHERE tr.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_route(pool: &PgPool, id: i64) -> Result<Option<RouteRow>, AppError> {
    find_route(pool, id).await
}

pub async fn create_route(pool: &PgPool, input: &RouteInput) -> Result<RouteRow, AppError> {
    if let Err(msg) = validate_route(&input.name) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, RouteRow>(
        "INSERT INTO transport_routes (name, vehicle_id, fee_cents)
         VALUES ($1, $2, $3)
         RETURNING id, name, vehicle_id, fee_cents, created_at",
    )
    .bind(input.name.trim())
    .bind(input.vehicle_id)
    .bind(input.fee_cents)
    .fetch_one(pool)
    .await?;
    // re-fetch to populate vehicle reg
    if let Some(r) = find_route(pool, row.id).await? {
        return Ok(r);
    }
    Ok(row)
}

pub async fn create_route_fields(
    pool: &PgPool,
    name: &str,
    vehicle_id: Option<i64>,
    fee_cents: i64,
) -> Result<RouteRow, AppError> {
    let input = RouteInput {
        name: name.to_string(),
        vehicle_id,
        fee_cents,
    };
    create_route(pool, &input).await
}

pub async fn update_route(
    pool: &PgPool,
    id: i64,
    input: &RouteInput,
) -> Result<RouteRow, AppError> {
    if let Err(msg) = validate_route(&input.name) {
        return Err(AppError::BadRequest(msg));
    }
    sqlx::query(
        "UPDATE transport_routes SET name = $2, vehicle_id = $3, fee_cents = $4 WHERE id = $1",
    )
    .bind(id)
    .bind(input.name.trim())
    .bind(input.vehicle_id)
    .bind(input.fee_cents)
    .execute(pool)
    .await?;
    find_route(pool, id).await?.ok_or(AppError::NotFound)
}

pub async fn delete_route(pool: &PgPool, id: i64) -> Result<(), AppError> {
    // Don't cascade away the record of who rode this route.
    let riders: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM transport_riders WHERE route_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?;
    if riders > 0 {
        return Err(AppError::BadRequest(
            "This route has riders assigned; remove them before deleting it.".to_string(),
        ));
    }
    let res = sqlx::query("DELETE FROM transport_routes WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_route(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_route(pool, id).await
}

// ---------------------------------------------------------------------------
// Stops CRUD
// ---------------------------------------------------------------------------

pub async fn list_stops(pool: &PgPool, route_id: i64) -> Result<Vec<StopRow>, AppError> {
    let rows = sqlx::query_as::<_, StopRow>(
        "SELECT id, route_id, name, sequence, created_at
         FROM transport_stops WHERE route_id = $1 ORDER BY sequence ASC, id ASC",
    )
    .bind(route_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn all_stops(pool: &PgPool) -> Result<Vec<StopRow>, AppError> {
    let rows = sqlx::query_as::<_, StopRow>(
        "SELECT id, route_id, name, sequence, created_at
         FROM transport_stops ORDER BY route_id, sequence, id LIMIT 1000",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find_stop(pool: &PgPool, id: i64) -> Result<Option<StopRow>, AppError> {
    let row = sqlx::query_as::<_, StopRow>(
        "SELECT id, route_id, name, sequence, created_at
         FROM transport_stops WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_stop(pool: &PgPool, id: i64) -> Result<Option<StopRow>, AppError> {
    find_stop(pool, id).await
}

pub async fn create_stop(pool: &PgPool, input: &StopInput) -> Result<StopRow, AppError> {
    if let Err(msg) = validate_stop(&input.name) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, StopRow>(
        "INSERT INTO transport_stops (route_id, name, sequence)
         VALUES ($1, $2, $3)
         RETURNING id, route_id, name, sequence, created_at",
    )
    .bind(input.route_id)
    .bind(input.name.trim())
    .bind(input.sequence)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_stop_fields(
    pool: &PgPool,
    route_id: i64,
    name: &str,
    sequence: i32,
) -> Result<StopRow, AppError> {
    let input = StopInput {
        route_id,
        name: name.to_string(),
        sequence,
    };
    create_stop(pool, &input).await
}

pub async fn update_stop(pool: &PgPool, id: i64, input: &StopInput) -> Result<StopRow, AppError> {
    if let Err(msg) = validate_stop(&input.name) {
        return Err(AppError::BadRequest(msg));
    }
    let row = sqlx::query_as::<_, StopRow>(
        "UPDATE transport_stops SET route_id = $2, name = $3, sequence = $4
         WHERE id = $1
         RETURNING id, route_id, name, sequence, created_at",
    )
    .bind(id)
    .bind(input.route_id)
    .bind(input.name.trim())
    .bind(input.sequence)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_stop(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM transport_stops WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_stop(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_stop(pool, id).await
}

pub async fn stops_for_route(pool: &PgPool, route_id: i64) -> Result<Vec<StopRow>, AppError> {
    list_stops(pool, route_id).await
}

// ---------------------------------------------------------------------------
// Riders
// ---------------------------------------------------------------------------

pub async fn list_riders(pool: &PgPool) -> Result<Vec<RiderRow>, AppError> {
    let rows = sqlx::query_as::<_, RiderRow>(
        "SELECT tr.id, tr.student_id, tr.route_id, tr.stop_id, tr.created_at,
                (s.first_name || ' ' || s.last_name) AS student_name,
                st.name AS stop_name
         FROM transport_riders tr
         JOIN students s ON s.id = tr.student_id
         LEFT JOIN transport_stops st ON st.id = tr.stop_id
         ORDER BY s.last_name, s.first_name LIMIT 1000",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find_rider(pool: &PgPool, id: i64) -> Result<Option<RiderRow>, AppError> {
    let row = sqlx::query_as::<_, RiderRow>(
        "SELECT id, student_id, route_id, stop_id, created_at FROM transport_riders WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn add_rider(pool: &PgPool, input: &RiderInput) -> Result<RiderRow, AppError> {
    if let Err(msg) = validate_rider(input.student_id, input.route_id) {
        return Err(AppError::BadRequest(msg));
    }
    // Lock the route row so the capacity check and the insert are serialized —
    // two concurrent assignments can't both slip past a full bus.
    let mut tx = pool.begin().await?;
    let vehicle_cap: Option<Option<i32>> =
        sqlx::query_scalar("SELECT v.capacity FROM transport_routes r LEFT JOIN vehicles v ON v.id = r.vehicle_id WHERE r.id = $1 FOR UPDATE OF r")
            .bind(input.route_id)
            .fetch_optional(&mut *tx)
            .await?;
    // Route must exist; `None` capacity = no vehicle assigned = uncapped.
    let cap = vehicle_cap.ok_or_else(|| AppError::BadRequest("Route not found".to_string()))?;

    // Duplicate (same student already on this route).
    let dup: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM transport_riders WHERE student_id = $1 AND route_id = $2)",
    )
    .bind(input.student_id)
    .bind(input.route_id)
    .fetch_one(&mut *tx)
    .await?;
    if dup {
        return Err(AppError::BadRequest(
            "Student already on this route".to_string(),
        ));
    }

    // Enforce vehicle capacity when a vehicle (with a capacity) is assigned.
    if let Some(cap) = cap {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM transport_riders WHERE route_id = $1")
                .bind(input.route_id)
                .fetch_one(&mut *tx)
                .await?;
        if count >= cap as i64 {
            return Err(AppError::BadRequest(
                "Route is at vehicle capacity".to_string(),
            ));
        }
    }

    // A chosen stop must belong to this route.
    if let Some(stop_id) = input.stop_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM transport_stops WHERE id = $1 AND route_id = $2)",
        )
        .bind(stop_id)
        .bind(input.route_id)
        .fetch_one(&mut *tx)
        .await?;
        if !ok {
            return Err(AppError::BadRequest(
                "Stop does not belong to this route".to_string(),
            ));
        }
    }

    let row = sqlx::query_as::<_, RiderRow>(
        "INSERT INTO transport_riders (student_id, route_id, stop_id)
         VALUES ($1, $2, $3)
         RETURNING id, student_id, route_id, stop_id, created_at",
    )
    .bind(input.student_id)
    .bind(input.route_id)
    .bind(input.stop_id)
    .fetch_one(&mut *tx)
    .await;
    match row {
        Ok(r) => {
            tx.commit().await?;
            Ok(r)
        }
        Err(e) => {
            let app = AppError::Db(e);
            if is_duplicate_rider(&app) {
                Err(AppError::BadRequest(
                    "Student already on this route".to_string(),
                ))
            } else if matches!(&app, AppError::Db(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23503"))
            {
                Err(AppError::BadRequest(
                    "Student or route not found".to_string(),
                ))
            } else {
                Err(app)
            }
        }
    }
}

pub async fn create_rider(pool: &PgPool, input: &RiderInput) -> Result<RiderRow, AppError> {
    add_rider(pool, input).await
}

pub async fn add_rider_fields(
    pool: &PgPool,
    student_id: i64,
    route_id: i64,
    stop_id: Option<i64>,
) -> Result<RiderRow, AppError> {
    let input = RiderInput {
        student_id,
        route_id,
        stop_id,
    };
    add_rider(pool, &input).await
}

pub async fn create_rider_fields(
    pool: &PgPool,
    student_id: i64,
    route_id: i64,
    stop_id: Option<i64>,
) -> Result<RiderRow, AppError> {
    add_rider_fields(pool, student_id, route_id, stop_id).await
}

pub async fn remove_rider(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM transport_riders WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_rider(pool: &PgPool, id: i64) -> Result<(), AppError> {
    remove_rider(pool, id).await
}

pub fn is_duplicate_rider(err: &AppError) -> bool {
    match err {
        AppError::Db(sqlx::Error::Database(db)) => db.code().as_deref() == Some("23505"),
        _ => false,
    }
}

pub fn is_duplicate(err: &AppError) -> bool {
    is_duplicate_rider(err)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

pub async fn riders_for_route(pool: &PgPool, route_id: i64) -> Result<Vec<RiderRow>, AppError> {
    let rows = sqlx::query_as::<_, RiderRow>(
        "SELECT tr.id, tr.student_id, tr.route_id, tr.stop_id, tr.created_at,
                (s.first_name || ' ' || s.last_name) AS student_name,
                st.name AS stop_name
         FROM transport_riders tr
         JOIN students s ON s.id = tr.student_id
         LEFT JOIN transport_stops st ON st.id = tr.stop_id
         WHERE tr.route_id = $1
         ORDER BY s.last_name, s.first_name",
    )
    .bind(route_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn riders_for_student(pool: &PgPool, student_id: i64) -> Result<Vec<RiderRow>, AppError> {
    let rows = sqlx::query_as::<_, RiderRow>(
        "SELECT tr.id, tr.student_id, tr.route_id, tr.stop_id, tr.created_at,
                (s.first_name || ' ' || s.last_name) AS student_name,
                st.name AS stop_name
         FROM transport_riders tr
         JOIN students s ON s.id = tr.student_id
         LEFT JOIN transport_stops st ON st.id = tr.stop_id
         WHERE tr.student_id = $1
         ORDER BY tr.id",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn routes_for_student(pool: &PgPool, student_id: i64) -> Result<Vec<RouteRow>, AppError> {
    let rows = sqlx::query_as::<_, RouteRow>(
        "SELECT tr.id, tr.name, tr.vehicle_id, tr.fee_cents, tr.created_at,
                v.reg_no AS vehicle_reg, v.reg_no AS vehicle_reg_no, v.reg_no AS reg_no,
                v.capacity AS vehicle_capacity,
                st.name AS stop_name, st.name AS stop
         FROM transport_riders r
         JOIN transport_routes tr ON tr.id = r.route_id
         LEFT JOIN vehicles v ON v.id = tr.vehicle_id
         LEFT JOIN transport_stops st ON st.id = r.stop_id
         WHERE r.student_id = $1
         ORDER BY tr.name",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn routes_for_student_with_stop(
    pool: &PgPool,
    student_id: i64,
) -> Result<Vec<RouteRow>, AppError> {
    routes_for_student(pool, student_id).await
}

pub async fn rider_count_for_route(pool: &PgPool, route_id: i64) -> Result<i64, AppError> {
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM transport_riders WHERE route_id = $1")
            .bind(route_id)
            .fetch_one(pool)
            .await?;
    Ok(n)
}

pub async fn count_riders(pool: &PgPool, route_id: i64) -> Result<i64, AppError> {
    rider_count_for_route(pool, route_id).await
}

pub async fn vehicle_capacity_for_route(
    pool: &PgPool,
    route_id: i64,
) -> Result<Option<i32>, AppError> {
    let cap: Option<i32> = sqlx::query_scalar(
        "SELECT v.capacity FROM transport_routes tr LEFT JOIN vehicles v ON v.id = tr.vehicle_id WHERE tr.id = $1",
    )
    .bind(route_id)
    .fetch_optional(pool)
    .await?
    .flatten();
    Ok(cap)
}

pub async fn capacity_check(pool: &PgPool, route_id: i64) -> Result<bool, AppError> {
    let riders = rider_count_for_route(pool, route_id).await?;
    let cap = vehicle_capacity_for_route(pool, route_id)
        .await?
        .unwrap_or(i32::MAX);
    Ok(riders > cap as i64)
}

pub async fn is_over_capacity(pool: &PgPool, route_id: i64) -> Result<bool, AppError> {
    capacity_check(pool, route_id).await
}

pub async fn capacity_counts(pool: &PgPool, route_id: i64) -> Result<(i64, i32), AppError> {
    let riders = rider_count_for_route(pool, route_id).await?;
    let cap = vehicle_capacity_for_route(pool, route_id)
        .await?
        .unwrap_or(0);
    Ok((riders, cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_vehicle_requires_reg_no() {
        assert!(validate_vehicle("", 4).is_err());
        assert!(validate_vehicle("   ", 4).is_err());
        assert!(validate_vehicle("BUS-1", 4).is_ok());
    }

    #[test]
    fn validate_vehicle_capacity_positive() {
        assert!(validate_vehicle("BUS-1", 0).is_err());
        assert!(validate_vehicle("BUS-1", -1).is_err());
        assert!(validate_vehicle("BUS-1", 1).is_ok());
    }

    #[test]
    fn validate_route_requires_name() {
        assert!(validate_route("").is_err());
        assert!(validate_route("   ").is_err());
        assert!(validate_route("Route A").is_ok());
    }

    #[test]
    fn validate_stop_requires_name() {
        assert!(validate_stop("").is_err());
        assert!(validate_stop("Stop").is_ok());
    }

    #[test]
    fn validate_rider_requires_student_and_route() {
        assert!(validate_rider(0, 1).is_err());
        assert!(validate_rider(1, 0).is_err());
        assert!(validate_rider(1, 1).is_ok());
    }
}
