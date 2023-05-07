use async_log_watch::{LogEvent, LogWatcher};

use async_std::{
    channel::bounded as channel,
    fs::{remove_file, File},
    io::WriteExt,
    task::{self, sleep},
};

async fn test_log_watcher() {
    let mut log_watcher = LogWatcher::new();

    let (tx, rx) = channel(1);
    let filepath = "test-log.txt";

    let _ = remove_file(filepath).await; // Remove the file if it exists

    let mut file = File::create(filepath).await.unwrap();

    log_watcher
        .register(
            filepath,
            move |log_event: LogEvent| {
                let tx = tx.clone();
                async move {
                    if let Some(line) = log_event.get_line() {
                        tx.try_send(line.clone()).unwrap();
                    }
                }
            },
            None,
        )
        .await;

    task::spawn(async move {
        log_watcher
            .monitoring(std::time::Duration::from_secs(1))
            .await
            .unwrap();
    });

    // Give some time for the log watcher to start monitoring
    sleep(std::time::Duration::from_secs(1)).await;

    file.write_all(b"New log line\n").await.unwrap();
    file.flush().await.unwrap();

    // Wait for the received line
    let received_line = rx.recv().await.unwrap();
    assert_eq!(received_line, "New log line");

    // Clean up the test log file
    remove_file(filepath).await.unwrap();
}
