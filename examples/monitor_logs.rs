use async_log_watch::{LogError, LogWatcher};

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut log_watcher = LogWatcher::new();

    let filepath = "~/.pm2/logs/r1-out.log";
    log_watcher
        .register(
            filepath,
            |line: String, err: Option<LogError>| async move {
                if err.is_none() {
                    println!("New log line: {}", line);
                } else {
                    eprintln!("{}", err.unwrap());
                }
            },
            None,
        )
        .await;

    log_watcher
        .monitoring(std::time::Duration::from_secs(1))
        .await?;
    Ok(())
}
