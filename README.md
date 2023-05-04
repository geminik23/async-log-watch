# Async log watch

`async_log_watch` is a simple Rust library developed as a part of a personal project. It is designed to monitor log files and trigger an async callback whenever a new line is added to the file. The library allows users to easily integrate log file monitoring into their projects, with support for monitoring multiple log files simultaneously.

The primary motivation behind creating this library was to efficiently detect new log lines generated by tools like `pm2`. The library is built using the `async-std` runtime and the `notify` crate for file system event monitoring.

## Usage

Add `async-log-watch` to your `Cargo.toml` dependencies:

```toml
[dependencies]
async-log-watch= "0.1"
```

### Example

```rust
use async_log_watch::{LogWatcher, LogError};

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut log_watcher = LogWatcher::new();

    let filepath = "~/.pm2/logs/r1-out.log";
    log_watcher
        .register(filepath, |line: String, err: Option<LogError>| async move {
            if err.is_none() {
                println!("New log line: {}", line);
            } else {
                eprintln!("{}", err.unwrap());
            }
        })
        .await;

    log_watcher
        .monitoring(std::time::Duration::from_secs(1))
        .await?;
    Ok(())
}

```

## TODO
- [x] Implement basic log monitoring.
- [x] Support async callbacks
- [x] Allow monitoring multiple log files simultaneously
- [X] Update with new version of dependencies.
- [x] FIXED: When convert into absolute filepath, tilde('~') is used as folder name.
- [x] FIXED: At the first time, watcher read the first line.
- [x] Error handling for file read errors
- [x] Improve error handling with the `thiserror` library. - file errors occurs in spawn. 
- [x] Notify error through callback
- [x] Added methods : stop_monitoring_file and change_file_path
- [x] FIXED: absolute path in added methods | test code 
- [ ] FIX : poll_interval issue.
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

## Future Works

- Add support for log file rotation 
- Add filtering options to process specific log lines based on patterns

## License

This project is licensed under the MIT License - see the [LICENSE](./LICENSE) file for details.
