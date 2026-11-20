//! The `shuttlepub-server` binary as a child process.

use std::{
    error::Error,
    io::BufRead,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

pub const BIND: &str = "SHUTTLEPUB_BIND";
pub const ACTOR: &str = "SHUTTLEPUB_ACTOR";
pub const REMOTE_ORIGINS: &str = "SHUTTLEPUB_REMOTE_ORIGINS";
pub const DATABASE_URL: &str = "SHUTTLEPUB_DATABASE_URL";

/// Every setting the binary reads. Each is removed from the inherited
/// environment, so a developer's own settings never reach a test.
const SETTINGS: [&str; 4] = [BIND, ACTOR, REMOTE_ORIGINS, DATABASE_URL];

/// How long the binary may take to start listening, or to give up starting.
/// Above sqlx's default 30-second acquire timeout, the longest a refused
/// database connection can keep the binary waiting.
pub const STARTUP_DEADLINE: Duration = Duration::from_secs(90);

/// The running binary; killed when dropped.
pub struct ServerProcess {
    child: Child,
    address: String,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ServerProcess {
    /// Starts the binary with exactly `vars` among its settings and waits
    /// until it reports the address it listens on.
    pub async fn start(vars: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let mut child = command(vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child.stdout.take().ok_or("missing stdout")?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let line = line?;
                if let Some(address) = listening_address(&line) {
                    let _ = tx.send(address.to_owned());
                    break;
                }
            }
            Ok::<_, std::io::Error>(())
        });
        let mut process = ServerProcess {
            child,
            address: String::new(),
        };
        process.address = tokio::time::timeout(STARTUP_DEADLINE, rx)
            .await
            .map_err(|_| "the server did not report a listening address in time")?
            .map_err(|_| "the server exited without reporting a listening address")?;
        Ok(process)
    }

    /// `host:port` the binary listens on.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Stops the binary the way an operator's Ctrl-C does.
    pub async fn stop(mut self) -> Result<(), Box<dyn Error>> {
        let status = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status()?;
        if !status.success() {
            return Err(format!("kill -INT failed: {status}").into());
        }
        wait_for_exit(&mut self.child).await?;
        Ok(())
    }
}

/// Runs the binary with exactly `vars` among its settings until it exits on
/// its own, and returns its exit status and every line it logged.
pub async fn run_until_exit(
    vars: &[(&str, &str)],
) -> Result<(ExitStatus, Vec<String>), Box<dyn Error>> {
    let mut child = command(vars)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let lines = tokio::task::spawn_blocking(move || {
        std::io::BufReader::new(stdout)
            .lines()
            .collect::<Result<Vec<_>, _>>()
    });
    let mut process = ServerProcess {
        child,
        address: String::new(),
    };
    let status = wait_for_exit(&mut process.child).await?;
    Ok((status, lines.await??))
}

/// Whether a logged line announces that the binary accepts HTTP connections.
pub fn listening_address(line: &str) -> Option<&str> {
    line.split("address=")
        .nth(1)
        .and_then(|value| value.split_whitespace().next())
}

fn command(vars: &[(&str, &str)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shuttlepub-server"));
    for name in SETTINGS {
        command.env_remove(name);
    }
    command.envs(vars.iter().copied()).env("RUST_LOG", "info");
    command
}

async fn wait_for_exit(child: &mut Child) -> Result<ExitStatus, Box<dyn Error>> {
    let deadline = tokio::time::Instant::now() + STARTUP_DEADLINE;
    let mut poll = tokio::time::interval(Duration::from_millis(50));
    loop {
        poll.tick().await;
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if tokio::time::Instant::now() > deadline {
            return Err(format!("the server kept running for {STARTUP_DEADLINE:?}").into());
        }
    }
}
