/// Database error type that hides the sqlx implementation detail.
///
/// Downstream crates match on the semantic variants (e.g. `UniqueViolation`)
/// instead of depending on sqlx directly.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("already exists")]
    UniqueViolation,
    #[error("{0}")]
    Query(sqlx::Error),
    #[error("{0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e
            && db_err.is_unique_violation()
        {
            return Self::UniqueViolation;
        }
        Self::Query(e)
    }
}
