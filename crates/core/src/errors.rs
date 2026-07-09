/// Errors returned by the [`crate::LocalFlow`] service.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Not found")]
    NotFound,

    /// User input was rejected; each entry is one readable problem.
    #[error("{}", .0.join(" "))]
    Validation(Vec<String>),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Scheduler error: {0}")]
    Scheduler(String),
}

pub type CoreResult<T> = Result<T, CoreError>;
