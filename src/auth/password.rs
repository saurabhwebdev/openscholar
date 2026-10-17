use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};

use crate::error::AppError;

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err("password must be at least 8 characters");
    }
    Ok(())
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    validate_password(password).map_err(|msg| AppError::BadRequest(msg.to_string()))?;
    let hash = Argon2::default().hash_password(password.as_bytes())?;
    Ok(hash.to_string())
}

/// Returns `Ok(())` when the password matches, `Err(Unauthenticated)` otherwise
/// (same response for unknown hash so logins don't leak which half failed).
pub fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let parsed = PasswordHash::new(hash)
        .map_err(|_| AppError::BadRequest("invalid password hash".into()))?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthenticated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_passwords() {
        assert!(validate_password("short1").is_err());
        assert!(validate_password("").is_err());
        assert!(validate_password("exactly8").is_ok());
        assert!(hash_password("short1").is_err());
    }

    #[test]
    fn hash_then_verify_round_trip() {
        let hash = hash_password("correct-horse-123").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct-horse-123", &hash).is_ok());
    }

    #[test]
    fn wrong_password_is_unauthenticated() {
        let hash = hash_password("correct-horse-123").unwrap();
        assert!(matches!(
            verify_password("wrong-password-1", &hash),
            Err(AppError::Unauthenticated)
        ));
    }

    #[test]
    fn salts_are_random_per_hash() {
        let a = hash_password("same-password-1").unwrap();
        let b = hash_password("same-password-1").unwrap();
        assert_ne!(a, b);
        assert!(verify_password("same-password-1", &a).is_ok());
        assert!(verify_password("same-password-1", &b).is_ok());
    }

    #[test]
    fn malformed_hash_is_bad_request() {
        assert!(matches!(
            verify_password("anything12", "not-a-hash"),
            Err(AppError::BadRequest(_))
        ));
    }
}
