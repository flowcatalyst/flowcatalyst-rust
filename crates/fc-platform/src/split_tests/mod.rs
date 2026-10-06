//! Unit tests of fc-platform-core code that need the platform's own
//! aggregates (every domain enum, a representative event from every
//! domain). They can't live in core, which depends on none of them.

mod enum_str;
mod event_persistence_snapshot;
mod function_documents;
mod sql_literals;
