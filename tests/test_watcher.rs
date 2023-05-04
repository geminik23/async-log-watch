use async_log_watch::{LogError, LogWatcher};
use async_std::channel::bounded;
use async_std::fs::File;
use async_std::io::WriteExt;
use async_std::task;

#[async_std::test]
async fn test_log_watcher() {
    let mut log_watcher = LogWatcher::new();

    let (tx, rx) = bounded(1);
    let filepath = "test-log.txt";

    let _ = async_std::fs::remove_file(filepath).await; // Remove the file if it exists

    let mut file = File::create(filepath).await.unwrap();

    log_watcher
        .register(filepath, move |line: String, err: Option<LogError>| {
            let tx = tx.clone();
            async move {
                if err.is_none() {
                    tx.try_send(line).unwrap();
                }
            }
        })
        .await;

    task::spawn(async move {
        log_watcher
            .monitoring(std::time::Duration::from_secs(1))
            .await
            .unwrap();
    });

    // Give some time for the log watcher to start monitoring
    task::sleep(std::time::Duration::from_secs(1)).await;

    file.write_all(b"New log line\n").await.unwrap();
    file.flush().await.unwrap();

    // Wait for the received line
    let received_line = rx.recv().await.unwrap();
    assert_eq!(received_line, "New log line");

    // Clean up the test log file
    async_std::fs::remove_file(filepath).await.unwrap();
}
