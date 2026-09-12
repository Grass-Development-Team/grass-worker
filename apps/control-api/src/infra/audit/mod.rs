//! Structured audit logs and durable request/business history.
mod context;
mod event;
pub mod http;
mod logging;
mod query;
mod redaction;
mod retention;
mod transaction;
mod writer;

pub use context::AuditErrorContext;
pub use event::*;
pub use query::*;
pub use redaction::redact_json;
pub use retention::prune_events_before;
pub use transaction::{AuditConnection, AuditTransaction};
pub use writer::*;

#[cfg(test)]
use event::domain_actor_type;
#[cfg(test)]
mod tests;
