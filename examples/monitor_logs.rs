use async_log_watcher::LogWatcher;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut log_watcher = LogWatcher::new();

    let filepath = "~/.pm2/logs/r1-out.log";
    log_watcher
        .register(filepath, |line: String| async move {
            println!("New log line: {}", line);
        })
        .await;

    log_watcher
        .monitoring(std::time::Duration::from_secs(1))
        .await?;
    Ok(())
}
