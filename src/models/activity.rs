use sqlx::PgPool;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActivityRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub supervisor_id: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MemberRow {
    pub id: i64,
    pub activity_id: i64,
    pub student_id: i64,
    pub joined_on: chrono::NaiveDate,
    /// Joined-in student name (empty on INSERT RETURNING rows).
    #[sqlx(default)]
    pub student_name: String,
}

#[derive(Debug, Clone)]
pub struct ActivityInput {
    pub name: String,
    pub kind: String,
    pub description: Option<String>,
    pub supervisor_id: Option<i64>,
}

pub fn validate_activity(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Activity name is required".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Activities CRUD
// ---------------------------------------------------------------------------

pub async fn list_activities(pool: &PgPool) -> Result<Vec<ActivityRow>, AppError> {
    let rows = sqlx::query_as::<_, ActivityRow>(
        "SELECT id, name, kind, description, supervisor_id, created_at
         FROM activities ORDER BY name, id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn find_activity(pool: &PgPool, id: i64) -> Result<Option<ActivityRow>, AppError> {
    let row = sqlx::query_as::<_, ActivityRow>(
        "SELECT id, name, kind, description, supervisor_id, created_at
         FROM activities WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_activity(pool: &PgPool, id: i64) -> Result<Option<ActivityRow>, AppError> {
    find_activity(pool, id).await
}

pub async fn create_activity(
    pool: &PgPool,
    input: &ActivityInput,
) -> Result<ActivityRow, AppError> {
    if let Err(msg) = validate_activity(&input.name) {
        return Err(AppError::BadRequest(msg));
    }
    let kind = if input.kind.trim().is_empty() {
        "club".to_string()
    } else {
        input.kind.trim().to_string()
    };
    let row = sqlx::query_as::<_, ActivityRow>(
        "INSERT INTO activities (name, kind, description, supervisor_id)
         VALUES ($1, $2, $3, $4)
         RETURNING id, name, kind, description, supervisor_id, created_at",
    )
    .bind(input.name.trim())
    .bind(kind)
    .bind(input.description.clone())
    .bind(input.supervisor_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn create_activity_fields(
    pool: &PgPool,
    name: &str,
    kind: &str,
    description: Option<String>,
    supervisor_id: Option<i64>,
) -> Result<ActivityRow, AppError> {
    let input = ActivityInput {
        name: name.to_string(),
        kind: kind.to_string(),
        description,
        supervisor_id,
    };
    create_activity(pool, &input).await
}

// aliases
pub async fn add_activity(pool: &PgPool, input: &ActivityInput) -> Result<ActivityRow, AppError> {
    create_activity(pool, input).await
}
pub async fn insert_activity(
    pool: &PgPool,
    input: &ActivityInput,
) -> Result<ActivityRow, AppError> {
    create_activity(pool, input).await
}

pub async fn update_activity(
    pool: &PgPool,
    id: i64,
    input: &ActivityInput,
) -> Result<ActivityRow, AppError> {
    if let Err(msg) = validate_activity(&input.name) {
        return Err(AppError::BadRequest(msg));
    }
    let kind = if input.kind.trim().is_empty() {
        "club".to_string()
    } else {
        input.kind.trim().to_string()
    };
    let row = sqlx::query_as::<_, ActivityRow>(
        "UPDATE activities SET name=$2, kind=$3, description=$4, supervisor_id=$5
         WHERE id=$1
         RETURNING id, name, kind, description, supervisor_id, created_at",
    )
    .bind(id)
    .bind(input.name.trim())
    .bind(kind)
    .bind(input.description.clone())
    .bind(input.supervisor_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn delete_activity(pool: &PgPool, id: i64) -> Result<(), AppError> {
    // Don't cascade away the membership roster.
    let members: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM activity_members WHERE activity_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await?;
    if members > 0 {
        return Err(AppError::BadRequest(
            "This activity has members; remove them before deleting it.".to_string(),
        ));
    }
    let res = sqlx::query("DELETE FROM activities WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn remove_activity(pool: &PgPool, id: i64) -> Result<(), AppError> {
    delete_activity(pool, id).await
}

// ---------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------

pub async fn members_for_activity(
    pool: &PgPool,
    activity_id: i64,
) -> Result<Vec<MemberRow>, AppError> {
    let rows = sqlx::query_as::<_, MemberRow>(
        "SELECT m.id, m.activity_id, m.student_id, m.joined_on,
                (s.first_name || ' ' || s.last_name) AS student_name
         FROM activity_members m JOIN students s ON s.id = m.student_id
         WHERE m.activity_id = $1 ORDER BY m.joined_on, m.id",
    )
    .bind(activity_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn members_for(pool: &PgPool, activity_id: i64) -> Result<Vec<MemberRow>, AppError> {
    members_for_activity(pool, activity_id).await
}

pub async fn list_members(pool: &PgPool, activity_id: i64) -> Result<Vec<MemberRow>, AppError> {
    members_for_activity(pool, activity_id).await
}

pub async fn activities_for_student(
    pool: &PgPool,
    student_id: i64,
) -> Result<Vec<ActivityRow>, AppError> {
    let rows = sqlx::query_as::<_, ActivityRow>(
        "SELECT a.id, a.name, a.kind, a.description, a.supervisor_id, a.created_at
         FROM activities a
         JOIN activity_members m ON m.activity_id = a.id
         WHERE m.student_id = $1
         ORDER BY a.name",
    )
    .bind(student_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn activities_for(pool: &PgPool, student_id: i64) -> Result<Vec<ActivityRow>, AppError> {
    activities_for_student(pool, student_id).await
}

pub async fn add_member(
    pool: &PgPool,
    activity_id: i64,
    student_id: i64,
    joined_on: Option<chrono::NaiveDate>,
) -> Result<MemberRow, AppError> {
    // duplicate check
    let dup: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM activity_members WHERE activity_id = $1 AND student_id = $2)",
    )
    .bind(activity_id)
    .bind(student_id)
    .fetch_one(pool)
    .await?;
    if dup {
        return Err(AppError::BadRequest("Student already a member".to_string()));
    }
    let joined = joined_on.unwrap_or_else(|| chrono::Utc::now().date_naive());
    let row = sqlx::query_as::<_, MemberRow>(
        "INSERT INTO activity_members (activity_id, student_id, joined_on)
         VALUES ($1, $2, $3)
         RETURNING id, activity_id, student_id, joined_on",
    )
    .bind(activity_id)
    .bind(student_id)
    .bind(joined)
    .fetch_one(pool)
    .await;
    match row {
        Ok(r) => Ok(r),
        Err(e) => {
            let app = AppError::Db(e);
            if is_duplicate_member(&app) {
                Err(AppError::BadRequest("Student already a member".to_string()))
            } else {
                Err(app)
            }
        }
    }
}

pub async fn add_member_fields(
    pool: &PgPool,
    activity_id: i64,
    student_id: i64,
) -> Result<MemberRow, AppError> {
    add_member(pool, activity_id, student_id, None).await
}

pub async fn create_member(
    pool: &PgPool,
    activity_id: i64,
    student_id: i64,
) -> Result<MemberRow, AppError> {
    add_member_fields(pool, activity_id, student_id).await
}

pub async fn remove_member(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let res = sqlx::query("DELETE FROM activity_members WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_member(pool: &PgPool, id: i64) -> Result<(), AppError> {
    remove_member(pool, id).await
}

pub async fn remove_member_by_activity_student(
    pool: &PgPool,
    activity_id: i64,
    student_id: i64,
) -> Result<(), AppError> {
    let res =
        sqlx::query("DELETE FROM activity_members WHERE activity_id = $1 AND student_id = $2")
            .bind(activity_id)
            .bind(student_id)
            .execute(pool)
            .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub fn is_duplicate_member(err: &AppError) -> bool {
    match err {
        AppError::Db(sqlx::Error::Database(db)) => db.code().as_deref() == Some("23505"),
        _ => false,
    }
}
