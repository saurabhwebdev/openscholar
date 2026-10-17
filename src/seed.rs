use sqlx::PgPool;

use crate::auth::password;
use crate::config::Config;
use crate::error::AppError;

/// Creates the initial admin from env config when no such user exists.
/// Returns `true` when a user was created. Safe to run on every boot.
pub async fn ensure_admin(pool: &PgPool, config: &Config) -> Result<bool, AppError> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE lower(email) = lower($1))")
            .bind(&config.admin_email)
            .fetch_one(pool)
            .await?;
    if exists {
        return Ok(false);
    }
    let hash = password::hash_password(&config.admin_password)?;
    sqlx::query(
        "INSERT INTO users (email, password_hash, role, full_name)
         VALUES ($1, $2, 'admin', $3)",
    )
    .bind(&config.admin_email)
    .bind(&hash)
    .bind(&config.admin_name)
    .execute(pool)
    .await?;
    Ok(true)
}
