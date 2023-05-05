use async_std::fs::File;
use async_std::io::BufReader;
use async_std::prelude::*;
use async_std::sync::{Arc, Mutex};
use async_std::task;
use notify::event::{DataChange, ModifyKind};
use notify::{event::EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::mpsc::channel;

use shellexpand::tilde;

#[derive(Debug, thiserror::Error)]
pub enum ErrorKind {
    #[error("failed to open file - {0}")]
    FileOpenError(std::io::Error),
    #[error("failed to seek file - {0}")]
    FileSeekError(std::io::Error),
}

#[derive(Debug)]
pub struct LogError {
    pub kind: ErrorKind,
    pub path: String,
}

impl LogError {
    // Display the error message
    pub fn display_error(&self) -> String {
        match &self.kind {
            ErrorKind::FileOpenError(err) => {
                format!("{:?} - {}", err, self.path)
            }
            ErrorKind::FileSeekError(err) => {
                format!("{:?} - {}", err, self.path)
            }
        }
    }
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_error())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("event error - {0}")]
    EventError(notify::Error),
    #[error("failed to receive data - {0}")]
    RecvError(std::sync::mpsc::RecvError),
}

pub type LogCallback = Arc<
    dyn Fn(String, Option<LogError>) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>>
        + Send
        + Sync,
>;

pub struct LogWatcher {
    log_callbacks: Arc<Mutex<HashMap<String, LogCallback>>>,
    watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
}

impl LogWatcher {
    pub fn new() -> Self {
        Self {
            log_callbacks: Arc::new(Mutex::new(HashMap::new())),
            watcher: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn change_file_path(&mut self, old_path: &str, new_path: &str) -> Result<(), Error> {
        // change into absolute path
        let old_path = self.make_absolute_path(&Path::new(old_path));
        let old_path = old_path.into_os_string().into_string().unwrap();

        let callback = self.log_callbacks.lock().await.remove(&old_path);
        if let Some(callback) = callback {
            self.log_callbacks
                .lock()
                .await
                .insert(new_path.to_owned(), callback);
            let mut watcher = self.watcher.lock().await;
            if let Some(watcher) = &mut *watcher {
                watcher
                    .unwatch(Path::new(&old_path))
                    .map_err(|e| Error::EventError(e))?;
                watcher
                    .watch(Path::new(new_path), RecursiveMode::NonRecursive)
                    .map_err(|e| Error::EventError(e))?;
            }
        }
        Ok(())
    }

    pub async fn stop_monitoring_file(&mut self, path: &str) -> Result<(), Error> {
        // change into absolute path
        let path = self.make_absolute_path(&Path::new(path));
        let path = path.into_os_string().into_string().unwrap();

        self.log_callbacks.lock().await.remove(&path);
        let mut watcher = self.watcher.lock().await;
        if let Some(watcher) = &mut *watcher {
            watcher
                .unwatch(Path::new(&path))
                .map_err(|e| Error::EventError(e))?;
        }
        Ok(())
    }

    // helper function to convert a relative path into an absolute path
    fn make_absolute_path(&self, path: &Path) -> PathBuf {
        let expanded_path = tilde(&path.to_string_lossy()).into_owned();
        let expanded_path = Path::new(&expanded_path);

        if expanded_path.is_absolute() {
            expanded_path.to_path_buf()
        } else {
            std::env::current_dir().unwrap().join(expanded_path)
        }
    }

    // register a file path and its associated callback function.
    pub async fn register<P: AsRef<Path>, F, Fut>(&mut self, path: P, callback: F)
    where
        F: Fn(String, Option<LogError>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + Sync + 'static,
    {
        let path = self.make_absolute_path(path.as_ref());
        let path = path.into_os_string().into_string().unwrap();

        let callback = Arc::new(
            move |line: String,
                  error: Option<LogError>|
                  -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
                Box::pin(callback(line, error))
            },
        );
        self.log_callbacks.lock().await.insert(path, callback);
    }

    // Start monitoring
    pub async fn monitoring(&self, poll_interval: std::time::Duration) -> Result<(), Error> {
        let (tx, rx) = channel();

        let config = notify::Config::default().with_poll_interval(poll_interval);

        let watcher: RecommendedWatcher = Watcher::new(tx, config).unwrap();
        *self.watcher.lock().await = Some(watcher);

        for path in self.log_callbacks.lock().await.keys() {
            self.watcher
                .lock()
                .await
                .as_mut()
                .unwrap()
                .watch(Path::new(&path), RecursiveMode::NonRecursive)
                .map_err(|e| Error::EventError(e))?;
        }

        let file_positions = Arc::new(Mutex::new(HashMap::<String, u64>::new()));
        loop {
            match rx.recv() {
                Ok(event) => match event {
                    Ok(event) => match event.kind {
                        EventKind::Modify(ModifyKind::Data(DataChange::Any)) => {
                            let paths = &event.paths;
                            for path in paths {
                                let path_str = path.clone().into_os_string().into_string().unwrap();

                                // clone the contianers
                                let log_callbacks = Arc::clone(&self.log_callbacks);
                                let file_positions_clone = Arc::clone(&file_positions);

                                task::spawn(async move {
                                    let log_callbacks = log_callbacks.lock().await;

                                    // TODO deadlock if I modify the log_callbacks.
                                    if let Some(callback) = log_callbacks.get(&path_str) {
                                        let callback = Arc::clone(callback);

                                        let mut file_positions = file_positions_clone.lock().await;
                                        let position = file_positions
                                            .entry(path_str.clone())
                                            .or_insert(std::u64::MAX);

                                        // file open
                                        match File::open(&path_str).await {
                                            Ok(file) => {
                                                let mut reader = BufReader::new(file);
                                                let mut line = String::new();

                                                // need to set initial position
                                                if *position == std::u64::MAX {
                                                    *position = find_last_line(&mut reader).await;
                                                }

                                                // seek from *position
                                                match reader
                                                    .seek(std::io::SeekFrom::Start(*position))
                                                    .await
                                                {
                                                    Ok(_) => {
                                                        // check if a full line has been read
                                                        if reader
                                                            .read_line(&mut line)
                                                            .await
                                                            .unwrap()
                                                            > 0
                                                            && line.ends_with('\n')
                                                        {
                                                            *position += line.len() as u64;

                                                            // remove trailing newline character, if present
                                                            if line.ends_with('\n') {
                                                                line.pop();
                                                                if line.ends_with('\r') {
                                                                    line.pop();
                                                                }
                                                            }
                                                            callback(line, None).await;
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let log_error = LogError {
                                                            kind: ErrorKind::FileSeekError(e),
                                                            path: path_str.clone(),
                                                        };
                                                        callback("".into(), Some(log_error)).await;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let log_error = LogError {
                                                    kind: ErrorKind::FileOpenError(e),
                                                    path: path_str.clone(),
                                                };
                                                callback("".into(), Some(log_error)).await;
                                            }
                                        }
                                    }
                                });
                                // }
                                //
                                // if let Some(callback) = self.log_callbacks.get(&path_str) {
                                //     let callback = Arc::clone(callback);
                                //     let file_positions_clone = Arc::clone(&file_positions);
                                //
                                //     task::spawn(async move {
                                //         let mut file_positions = file_positions_clone.lock().await;
                                //         let position = file_positions
                                //             .entry(path_str.clone())
                                //             .or_insert(std::u64::MAX);
                                //
                                //         // file open
                                //         match File::open(&path_str).await {
                                //             Ok(file) => {
                                //                 let mut reader = BufReader::new(file);
                                //                 let mut line = String::new();
                                //
                                //                 // need to set initial position
                                //                 if *position == std::u64::MAX {
                                //                     *position = find_last_line(&mut reader).await;
                                //                 }
                                //
                                //                 // seek from *position
                                //                 match reader
                                //                     .seek(std::io::SeekFrom::Start(*position))
                                //                     .await
                                //                 {
                                //                     Ok(_) => {
                                //                         // check if a full line has been read
                                //                         if reader
                                //                             .read_line(&mut line)
                                //                             .await
                                //                             .unwrap()
                                //                             > 0
                                //                             && line.ends_with('\n')
                                //                         {
                                //                             *position += line.len() as u64;
                                //
                                //                             // remove trailing newline character, if present
                                //                             if line.ends_with('\n') {
                                //                                 line.pop();
                                //                                 if line.ends_with('\r') {
                                //                                     line.pop();
                                //                                 }
                                //                             }
                                //                             callback(line, None).await;
                                //                         }
                                //                     }
                                //                     Err(e) => {
                                //                         let log_error = LogError {
                                //                             kind: ErrorKind::FileSeekError(e),
                                //                             path: path_str.clone(),
                                //                         };
                                //                         callback("".into(), Some(log_error)).await;
                                //                     }
                                //                 }
                                //             }
                                //             Err(e) => {
                                //                 let log_error = LogError {
                                //                     kind: ErrorKind::FileOpenError(e),
                                //                     path: path_str.clone(),
                                //                 };
                                //                 callback("".into(), Some(log_error)).await;
                                //             }
                                //         }
                                //     });
                                // }
                            }
                        }
                        _ => {}
                    },
                    Err(e) => return Err(Error::EventError(e)),
                },
                Err(e) => return Err(Error::RecvError(e)),
            }
        }
    }
}

// find the position of last line.
async fn find_last_line(reader: &mut BufReader<File>) -> u64 {
    let mut last_line_start = 0;
    let mut last_line = String::new();
    let mut current_position = 0;

    while let Ok(len) = reader.read_line(&mut last_line).await {
        if len == 0 || !last_line.ends_with('\n') {
            break;
        }
        last_line_start = current_position;
        current_position += len as u64;
        last_line.clear();
    }

    last_line_start
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_std::{
        fs::File,
        io::{BufReader, WriteExt},
    };

    use super::find_last_line;
    #[async_std::test]
    async fn test_find_last_line() {
        //
        let filepath = "test-log.txt";

        let _ = async_std::fs::remove_file(filepath).await; // Remove the file if it exists

        let mut file = File::create(filepath).await.unwrap();

        file.write_all(b"0\n").await.unwrap();
        file.write_all(b"1\n").await.unwrap();
        file.write_all(b"2\n").await.unwrap();
        file.write_all(b"3\n").await.unwrap();
        file.flush().await.unwrap();

        let ofile = File::open(&filepath).await.unwrap();
        let mut reader = BufReader::new(ofile);
        let position = find_last_line(&mut reader).await;

        // assert last line position
        assert_eq!(position, 6);

        let mut line = String::new();
        reader
            .seek(std::io::SeekFrom::Start(position))
            .await
            .unwrap();
        reader.read_line(&mut line).await.unwrap();
        // assert last line
        assert_eq!(line, "3\n");

        let _ = async_std::fs::remove_file(filepath).await; // Remove the file if it exists
    }

    #[async_std::test]
    async fn test_log_watcher() {
        let mut log_watcher = LogWatcher::new();

        let log_file_1 = "test-log1.txt";
        let log_file_2 = "test-log2.txt";
        let log_file_3 = "test-log3.txt";

        // create log files
        let mut file_1 = File::create(log_file_1).await.unwrap();
        let mut file_2 = File::create(log_file_2).await.unwrap();
        let mut file_3 = File::create(log_file_3).await.unwrap();

        log_watcher.register(log_file_1, |_, _| async {}).await;
        log_watcher.register(log_file_2, |_, _| async {}).await;

        // write data to log files
        file_1.write_all(b"line 1\n").await.unwrap();
        file_1.sync_all().await.unwrap();
        file_2.write_all(b"line 2\n").await.unwrap();
        file_2.sync_all().await.unwrap();

        // stop monitoring log_file_1
        log_watcher.stop_monitoring_file(log_file_1).await.unwrap();
        // change the path of log_file_2 to log_file_3
        log_watcher
            .change_file_path(log_file_2, log_file_3)
            .await
            .unwrap();

        // write data to log files
        file_1.write_all(b"line 3\n").await.unwrap();
        file_1.sync_all().await.unwrap();
        file_3.write_all(b"line 4\n").await.unwrap();
        file_3.sync_all().await.unwrap();

        assert!(!log_watcher
            .log_callbacks
            .lock()
            .await
            .contains_key(log_file_1));
        assert!(!log_watcher
            .log_callbacks
            .lock()
            .await
            .contains_key(log_file_2));
        assert!(log_watcher
            .log_callbacks
            .lock()
            .await
            .contains_key(log_file_3));

        // remove the test log files
        async_std::fs::remove_file(log_file_1).await.unwrap();
        async_std::fs::remove_file(log_file_2).await.unwrap();
        async_std::fs::remove_file(log_file_3).await.unwrap();
    }
}
