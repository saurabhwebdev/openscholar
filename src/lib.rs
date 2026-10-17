pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod auth;
pub mod comms;
pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod notifications;
pub mod pagination;
pub mod routes;
pub mod seed;
pub mod seed_demo;
pub mod state;
pub mod storage;
pub mod util;
