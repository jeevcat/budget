use apalis::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct NoOpJob;

/// Handler for the no-op test job.
///
/// # Errors
///
/// Currently infallible, but returns `Result` to match the apalis handler contract.
#[allow(clippy::unused_async)] // apalis requires async handlers
pub async fn handle_noop_job(_job: NoOpJob) -> Result<(), BoxDynError> {
    tracing::info!("no-op job executed");
    Ok(())
}
