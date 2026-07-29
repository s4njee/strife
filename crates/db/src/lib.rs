//! `PostgreSQL` access and migration support for Strife.

use sqlx::{PgPool, migrate::Migrator};

/// Embedded, versioned database migrations.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Confirms that a pool can execute a minimal query.
///
/// # Errors
///
/// Returns the database error when the query cannot be completed.
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    let _: i32 = sqlx::query_scalar!(r#"SELECT 1 AS "value!""#)
        .fetch_one(pool)
        .await?;

    Ok(())
}
