use async_log_watch::{LogEvent, LogWatcher};

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut log_watcher = LogWatcher::new();

    let filepath = "~/.pm2/logs/r1-out.log";
    log_watcher
        .register(
            filepath,
            |log_event: LogEvent| async move {
                if let Some(err) = log_event.get_log_error() {
                    eprintln!("{}", err);
                } else {
                    println!("New log line: {}", log_event.get_line().unwrap());
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
