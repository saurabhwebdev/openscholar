use std::fmt;
use std::str::FromStr;

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Teacher,
    Staff,
    Parent,
    Student,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Teacher => "teacher",
            Role::Staff => "staff",
            Role::Parent => "parent",
            Role::Student => "student",
        }
    }

    /// Roles allowed to perform staff-side mutations.
    pub fn is_staff_side(&self) -> bool {
        matches!(self, Role::Admin | Role::Teacher | Role::Staff)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "teacher" => Ok(Role::Teacher),
            "staff" => Ok(Role::Staff),
            "parent" => Ok(Role::Parent),
            "student" => Ok(Role::Student),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

/// Row for login lookups.
#[derive(Debug, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub full_name: String,
    pub active: bool,
}

/// The authenticated identity carried in request extensions by [`crate::auth::middleware`].
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CurrentUser {
    pub id: i64,
    pub email: String,
    pub full_name: String,
    pub role: String,
}

impl CurrentUser {
    pub fn role_enum(&self) -> Result<Role, AppError> {
        self.role.parse().map_err(AppError::Internal)
    }

    pub fn has_role(&self, allowed: &[Role]) -> bool {
        match self.role_enum() {
            Ok(role) => allowed.contains(&role),
            Err(_) => false,
        }
    }
}

/// Current password hash for a user (for the self-change "old password" check).
pub async fn password_hash_for(pool: &PgPool, user_id: i64) -> Result<Option<String>, AppError> {
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(hash)
}

/// Set a user's password hash. Returns false if the user id is unknown.
pub async fn update_password(pool: &PgPool, user_id: i64, hash: &str) -> Result<bool, AppError> {
    let r = sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(user_id)
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn find_user_by_email(pool: &PgPool, email: &str) -> Result<Option<UserRow>, AppError> {
    let row = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, password_hash, role, full_name, active
         FROM users WHERE lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn find_session_user(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<CurrentUser>, AppError> {
    let row = sqlx::query_as::<_, CurrentUser>(
        "SELECT u.id, u.email, u.full_name, u.role
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.id = $1 AND s.expires_at > now() AND u.active = TRUE",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_roles() {
        assert_eq!("admin".parse::<Role>().unwrap(), Role::Admin);
        assert_eq!("teacher".parse::<Role>().unwrap(), Role::Teacher);
        assert_eq!("staff".parse::<Role>().unwrap(), Role::Staff);
        assert_eq!("parent".parse::<Role>().unwrap(), Role::Parent);
        assert_eq!("student".parse::<Role>().unwrap(), Role::Student);
        assert!("superadmin".parse::<Role>().is_err());
        assert!("".parse::<Role>().is_err());
    }

    #[test]
    fn role_round_trips_through_str() {
        for role in [
            Role::Admin,
            Role::Teacher,
            Role::Staff,
            Role::Parent,
            Role::Student,
        ] {
            assert_eq!(role.as_str().parse::<Role>().unwrap(), role);
            assert_eq!(role.to_string(), role.as_str());
        }
    }

    #[test]
    fn staff_side_covers_staff_roles_only() {
        assert!(Role::Admin.is_staff_side());
        assert!(Role::Teacher.is_staff_side());
        assert!(Role::Staff.is_staff_side());
        assert!(!Role::Parent.is_staff_side());
        assert!(!Role::Student.is_staff_side());
    }

    fn user_with_role(role: &str) -> CurrentUser {
        CurrentUser {
            id: 1,
            email: "a@example.com".to_string(),
            full_name: "A".to_string(),
            role: role.to_string(),
        }
    }

    #[test]
    fn has_role_allows_listed_roles_only() {
        let admin = user_with_role("admin");
        assert!(admin.has_role(&[Role::Admin]));
        assert!(admin.has_role(&[Role::Admin, Role::Staff]));
        assert!(!admin.has_role(&[Role::Teacher, Role::Staff]));
    }

    #[test]
    fn unknown_role_never_authorizes() {
        let weird = user_with_role("superadmin");
        assert!(!weird.has_role(&[Role::Admin, Role::Teacher, Role::Staff]));
    }
}
