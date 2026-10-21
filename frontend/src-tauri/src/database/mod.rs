pub mod commands;
pub mod manager;
pub mod models;
pub mod repositories;
pub(crate) mod startup;

/// Begin a transaction that writes to the library.
///
/// `BEGIN IMMEDIATE` takes SQLite's write lock first, where the busy timeout
/// applies. A deferred `BEGIN` takes a read snapshot at its first read and must
/// upgrade it to write; SQLite refuses that upgrade at once (`SQLITE_BUSY` or
/// `SQLITE_BUSY_SNAPSHOT`) whenever another connection is writing or has
/// committed since, because waiting could deadlock. The read can be implicit:
/// preparing a statement whose triggers reach the FTS5 index makes FTS5 read
/// its configuration on a connection's first use. Read-only transactions keep
/// the pool's deferred `begin()`.
pub(crate) async fn begin_write(
    pool: &sqlx::SqlitePool,
) -> sqlx::Result<sqlx::Transaction<'static, sqlx::Sqlite>> {
    pool.begin_with("BEGIN IMMEDIATE").await
}
