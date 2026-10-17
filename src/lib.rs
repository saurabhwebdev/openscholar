#![allow(dead_code)] // Askama templates read struct fields via reflection
#![allow(clippy::too_many_arguments)] // Some handlers have legitimate parameter counts
#![allow(clippy::collapsible_if)] // Guard clauses are intentional
#![allow(clippy::needless_borrows_for_generic_args)] // Borrows sometimes needed for trait bounds
#![allow(clippy::redundant_field_names)] // Style preference in some contexts
#![allow(clippy::unnecessary_cast)] // Some casts are explicit for clarity

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
