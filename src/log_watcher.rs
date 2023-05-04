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

pub type LogCallback =
    Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> + Send + Sync>;

pub struct LogWatcher {
    log_callbacks: HashMap<String, LogCallback>,
}

impl LogWatcher {
    pub fn new() -> Self {
        Self {
            log_callbacks: HashMap::new(),
        }
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
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + Sync + 'static,
    {
        let path = self.make_absolute_path(path.as_ref());
        let path = path.into_os_string().into_string().unwrap();

        let callback = Arc::new(
            move |line: String| -> Pin<Box<dyn Future<Output = ()> + Send + Sync>> {
                Box::pin(callback(line))
            },
        );
        self.log_callbacks.insert(path, callback);
    }

    // Start monitoring
    pub async fn monitoring(&self, poll_interval: std::time::Duration) -> notify::Result<()> {
        let (tx, rx) = channel();

        let config = notify::Config::default().with_poll_interval(poll_interval);

        let mut watcher: RecommendedWatcher = Watcher::new(tx, config).unwrap();

        for path in self.log_callbacks.keys() {
            watcher.watch(Path::new(&path), RecursiveMode::NonRecursive)?;
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
                                if let Some(callback) = self.log_callbacks.get(&path_str) {
                                    let callback = Arc::clone(callback);
                                    let file_positions_clone = Arc::clone(&file_positions);

                                    task::spawn(async move {
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
                                                            callback(line).await;
                                                        }
                                                    }
                                                    Err(e) => {
                                                        println!(
                                                            "Failed to seek file '{}': {:?}",
                                                            path_str, e
                                                        );
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                println!(
                                                    "Failed to open file '{}': {:?}",
                                                    path_str, e
                                                );
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        _ => {}
                    },
                    Err(e) => {
                        println!("Event error: {:?}", e);
                    }
                },
                Err(e) => println!("Watch error: {:?}", e),
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
    use async_std::prelude::*;
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
}
