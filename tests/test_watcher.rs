use async_log_watch::{LogError, LogWatcher};

#[cfg(feature = "tokio-runtime")]
use tokio::{
    fs::{remove_file, File},
    io::AsyncWriteExt,
    sync::mpsc::channel,
    task,
    time::sleep,
};

#[cfg(feature = "async-std-runtime")]
use async_std::{
    channel::bounded as channel,
    fs::{remove_file, File},
    io::WriteExt,
    task::{self, sleep},
};

#[cfg(feature = "tokio-runtime")]
#[tokio::test(flavor = "multi_thread")]
async fn test_log_watcher() {
    test_log_watcher_impl().await
}

#[cfg(feature = "async-std-runtime")]
#[async_std::test]
async fn test_log_watcher() {
    test_log_watcher_impl().await
}

async fn test_log_watcher_impl() {
    let mut log_watcher = LogWatcher::new();

    let (tx, mut rx) = channel(1);
    let filepath = "test-log.txt";

    let _ = remove_file(filepath).await; // Remove the file if it exists

    let mut file = File::create(filepath).await.unwrap();

    log_watcher
        .register(filepath, move |line: String, err: Option<LogError>| {
            let tx = tx.clone();
            println!("{}", line);
            async move {
                println!("{}", line);
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
    sleep(std::time::Duration::from_secs(1)).await;

    file.write_all(b"New log line\n").await.unwrap();
    file.flush().await.unwrap();

    // Wait for the received line
    let received_line = rx.recv().await.unwrap();
    assert_eq!(received_line, "New log line");

    // Clean up the test log file
    remove_file(filepath).await.unwrap();
}
