use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};

struct ServerProcess(Child);

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn binary_serves_timeline_and_rejects_unsigned_inbox()
-> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shuttlepub-server"))
        .env("SHUTTLEPUB_BIND", "127.0.0.1:0")
        .env("RUST_LOG", "info")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let _process = ServerProcess(child);
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line?;
            if let Some(address) = line
                .split("address=")
                .nth(1)
                .and_then(|value| value.split_whitespace().next())
            {
                let _ = tx.send(address.to_owned());
                break;
            }
        }
        Ok::<_, std::io::Error>(())
    });
    let address = tokio::time::timeout(Duration::from_secs(30), rx).await??;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .get(format!("http://{address}/api/timeline"))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.json::<serde_json::Value>().await?,
        serde_json::json!([])
    );
    let response = client
        .post(format!("http://{address}/inbox"))
        .body("{}")
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    Ok(())
}
