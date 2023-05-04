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
                                    // let path_str = path.to_string_lossy().to_string();
                                    let callback = Arc::clone(callback);
                                    let file_positions_clone = Arc::clone(&file_positions);

                                    task::spawn(async move {
                                        let mut file_positions = file_positions_clone.lock().await;
                                        let position =
                                            file_positions.entry(path_str.clone()).or_insert(0);

                                        let file = File::open(&path_str).await.unwrap();
                                        let mut reader = BufReader::new(file);
                                        let mut line = String::new();

                                        reader
                                            .seek(std::io::SeekFrom::Start(*position))
                                            .await
                                            .unwrap();
                                        if reader.read_line(&mut line).await.unwrap() > 0 {
                                            *position += line.len() as u64;
                                            callback(line).await;
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
