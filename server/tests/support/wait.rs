use std::{error::Error, time::Duration};

/// How long a reaction to a recorded event (an Accept reaching a remote
/// inbox, its outcome being recorded) may take. Longer than the remote
/// client's 10-second timeout, so one unresponsive remote ahead in the queue
/// still leaves time for the event behind it.
pub const REACTION_DEADLINE: Duration = Duration::from_secs(30);

const POLL: Duration = Duration::from_millis(20);

/// Re-evaluates `check` until it holds, failing once `within` has passed.
/// The reaction runs in the background, so its effects are awaited as a
/// state rather than after a fixed delay.
pub async fn eventually(
    what: &str,
    within: Duration,
    mut check: impl AsyncFnMut() -> Result<bool, Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if check().await? {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(format!("{what}: did not happen within {within:?}").into());
        }
        tokio::time::sleep(POLL).await;
    }
}
