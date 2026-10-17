//! Local-first object storage with an S3-shaped configuration/key scheme.
//!
//! NOTE (limitation): this build stores bytes on the local filesystem under
//! `./uploads` for BOTH the `Local` and `S3` backends. The S3 configuration is
//! parsed and carried so a real signed-PUT backend can drop in later, but no
//! network calls are made. See README.md.

use std::collections::HashMap;
use std::path::PathBuf;

use axum::Router;
use thiserror::Error;
use tokio::fs;
use tower_http::services::ServeDir;

#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
    pub public_base_url: Option<String>,
}

impl StorageConfig {
    pub fn from_env() -> Self {
        Self::from_getter(&|key| std::env::var(key).ok())
    }

    fn from_getter(get: &dyn Fn(&str) -> Option<String>) -> Self {
        storage_config_from(getter_map(get))
    }

    pub fn backend(&self) -> StorageBackend {
        StorageBackend::from_config(self)
    }
}

fn getter_map(get: &dyn Fn(&str) -> Option<String>) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for k in [
        "S3_ENDPOINT",
        "S3_BUCKET",
        "S3_REGION",
        "S3_ACCESS_KEY",
        "S3_SECRET_KEY",
        "S3_PUBLIC_BASE_URL",
    ] {
        if let Some(v) = get(k) {
            m.insert(k.to_string(), v);
        }
    }
    m
}

/// Pure parsing from an env-like map. Empty/missing keys => "local" mode.
pub fn storage_config_from<I, K, V>(vars: I) -> StorageConfig
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let mut map: HashMap<String, String> = HashMap::new();
    for (k, v) in vars {
        map.insert(k.as_ref().to_string(), v.as_ref().to_string());
    }
    let access_key = map.get("S3_ACCESS_KEY").cloned().unwrap_or_default();
    let secret_key = map.get("S3_SECRET_KEY").cloned().unwrap_or_default();
    let region = map
        .get("S3_REGION")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("us-east-1")
        .to_string();
    StorageConfig {
        endpoint: opt_nonempty(map.get("S3_ENDPOINT")),
        bucket: opt_nonempty(map.get("S3_BUCKET")),
        region,
        access_key,
        secret_key,
        public_base_url: opt_nonempty(map.get("S3_PUBLIC_BASE_URL")),
    }
}

fn opt_nonempty(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Clone, Debug)]
pub enum StorageBackend {
    Local { root: PathBuf },
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        public_base_url: Option<String>,
    },
}

impl StorageBackend {
    pub fn from_config(cfg: &StorageConfig) -> Self {
        if cfg.access_key.trim().is_empty() || cfg.secret_key.trim().is_empty() {
            StorageBackend::Local {
                root: PathBuf::from("uploads"),
            }
        } else {
            StorageBackend::S3 {
                endpoint: cfg.endpoint.clone().unwrap_or_default(),
                bucket: cfg.bucket.clone().unwrap_or_default(),
                region: cfg.region.clone(),
                access_key: cfg.access_key.clone(),
                secret_key: cfg.secret_key.clone(),
                public_base_url: cfg.public_base_url.clone(),
            }
        }
    }

    pub async fn store_bytes(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: Option<&str>,
    ) -> Result<StoredObject, StorageError> {
        store_bytes(self, key, bytes, content_type).await
    }
}

#[derive(Clone, Debug)]
pub struct StoredObject {
    pub key: String,
    pub url: String,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(String),
    #[error("invalid key: {0}")]
    InvalidKey(String),
}

pub async fn store_bytes(
    backend: &StorageBackend,
    key: &str,
    bytes: &[u8],
    _content_type: Option<&str>,
) -> Result<StoredObject, StorageError> {
    if key.trim().is_empty() || key.contains("..") {
        return Err(StorageError::InvalidKey(key.to_string()));
    }
    // Always persist locally for now (see module note).
    let root = PathBuf::from("uploads");
    let path = root.join(key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| StorageError::Io(e.to_string()))?;
    }
    fs::write(&path, bytes)
        .await
        .map_err(|e| StorageError::Io(e.to_string()))?;

    let url = match backend {
        StorageBackend::Local { .. } => format!("/uploads/{}", key),
        StorageBackend::S3 { public_base_url, .. } => match public_base_url {
            Some(base) => format!("{}/{}", base.trim_end_matches('/'), key),
            None => format!("/uploads/{}", key),
        },
    };
    Ok(StoredObject {
        key: key.to_string(),
        url,
    })
}

/// Router exposing the uploads directory at /uploads.
pub fn serve_files() -> Router {
    Router::new().nest_service("/uploads", ServeDir::new("uploads"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn config_defaults_region_and_local_mode() {
        let vars: HashMap<String, String> = HashMap::new();
        let cfg = storage_config_from(&vars);
        assert_eq!(cfg.region, "us-east-1");
        assert!(cfg.access_key.is_empty());
        assert!(cfg.secret_key.is_empty());
        assert!(cfg.endpoint.is_none());
        assert!(cfg.bucket.is_none());
        assert!(cfg.public_base_url.is_none());
    }

    #[test]
    fn config_honors_values() {
        let vars = HashMap::from([
            ("S3_ENDPOINT".to_string(), "https://s3.example.com".to_string()),
            ("S3_BUCKET".to_string(), "bucket".to_string()),
            ("S3_REGION".to_string(), "eu-west-1".to_string()),
            ("S3_ACCESS_KEY".to_string(), "AK".to_string()),
            ("S3_SECRET_KEY".to_string(), "SK".to_string()),
            ("S3_PUBLIC_BASE_URL".to_string(), "https://cdn.example.com".to_string()),
        ]);
        let cfg = storage_config_from(&vars);
        assert_eq!(cfg.region, "eu-west-1");
        assert_eq!(cfg.access_key, "AK");
        assert_eq!(cfg.secret_key, "SK");
        assert_eq!(cfg.endpoint.as_deref(), Some("https://s3.example.com"));
        assert_eq!(cfg.bucket.as_deref(), Some("bucket"));
        assert_eq!(cfg.public_base_url.as_deref(), Some("https://cdn.example.com"));
    }

    #[test]
    fn empty_keys_select_local_backend() {
        let cfg = storage_config_from(HashMap::<String, String>::new());
        assert!(matches!(cfg.backend(), StorageBackend::Local { .. }));
    }

    #[test]
    fn nonempty_keys_select_s3_backend() {
        let vars = HashMap::from([
            ("S3_ACCESS_KEY".to_string(), "AK".to_string()),
            ("S3_SECRET_KEY".to_string(), "SK".to_string()),
        ]);
        let cfg = storage_config_from(&vars);
        assert!(matches!(cfg.backend(), StorageBackend::S3 { .. }));
    }

    #[tokio::test]
    async fn store_bytes_writes_local_and_returns_url() {
        let backend = StorageBackend::Local {
            root: PathBuf::from("uploads"),
        };
        let key = "test/unit-storage.txt";
        let obj = store_bytes(&backend, key, b"hello", Some("text/plain"))
            .await
            .unwrap();
        assert_eq!(obj.key, key);
        assert_eq!(obj.url, "/uploads/test/unit-storage.txt");
        let data = tokio::fs::read("uploads/test/unit-storage.txt").await.unwrap();
        assert_eq!(data, b"hello");
        let _ = tokio::fs::remove_file("uploads/test/unit-storage.txt").await;
        let _ = tokio::fs::remove_dir("uploads/test").await;
    }

    #[test]
    fn region_defaults_when_blank() {
        let vars = HashMap::from([("S3_REGION".to_string(), "   ".to_string())]);
        let cfg = storage_config_from(&vars);
        assert_eq!(cfg.region, "us-east-1");
    }
}
