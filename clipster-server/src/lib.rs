pub mod retention;
pub mod routes;
pub mod setup;
pub mod state;

// Re-export shared types so callers (tests, agent, etc.) can keep using
// `clipster_server::db::Database` etc. as before.
pub use clipster_core as core;
pub use clipster_core::db;
pub use clipster_core::Identity;
