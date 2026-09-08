//! Outward Kismet adapters. Foreign records never become domain objects by alias.
//! The database reader and normalizer expose packet evidence, never lifetime
//! device aggregates.
pub mod database;
pub mod live;
