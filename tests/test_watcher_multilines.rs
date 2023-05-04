use async_log_watch::{LogError, LogWatcher};
use async_std::{fs::File, io::prelude::*, task};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

async fn write_lines(file_path: &str, lines: Vec<&str>, delay: Duration) {
    let mut file = File::create(file_path).await.unwrap();

    for line in lines {
        file.write_all(line.as_bytes()).await.unwrap();
        task::sleep(delay).await;
    }
}

#[async_std::test]
async fn log_watcher_test() {
    // ready for log file
    let log_path = "test_log.txt";
    let _ = async_std::fs::remove_file(log_path).await; // remove the file if it exists

    // initialize the watcher
    let log_watcher = LogWatcher::new();
    let mut log_watcher = log_watcher;

    let detected_line_count = Arc::new(AtomicUsize::new(0));

    let detected_line_count_clone = detected_line_count.clone();
    log_watcher
        .register(log_path, move |line: String, err: Option<LogError>| {
            let detected_line_count = detected_line_count_clone.clone();
            async move {
                if err.is_none() {
                    println!("New line detected: {}", line);
                    detected_line_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        })
        .await;

    // start monitoring
    let _monitoring_handle = task::spawn(async move {
        log_watcher
            .monitoring(Duration::from_millis(100))
            .await
            .unwrap();
    });

    let test_lines = vec!["test 1\n", "test 2\n", "test 3\n", "test 4\n"];

    let write_handle = task::spawn(write_lines(
        log_path,
        test_lines.clone(),
        Duration::from_millis(500), // write one line every 1 sec.
    ));

    write_handle.await;

    task::sleep(Duration::from_millis(500)).await;

    // remove test log file
    async_std::fs::remove_file(log_path).await.unwrap();

    // assert line counts.
    assert_eq!(
        detected_line_count.load(Ordering::Relaxed),
        test_lines.len()
    );
}
