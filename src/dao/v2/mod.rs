pub mod models;
pub mod reference;
pub mod rows;
pub mod schema;
pub mod upsert;

/// DB tests change shared constraints/rows, so they run one at a time
#[cfg(test)]
pub(crate) static DB_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
