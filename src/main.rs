#[macro_use]
extern crate log;

// use notify::{Watcher,

use notify::{raw_watcher, RawEvent, RecursiveMode, Watcher};
use std::sync::mpsc::channel;

fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    let (tx, rx) = channel();
    let mut watcher = raw_watcher(tx).unwrap();

    watcher
        .watch("/home/gemin/notify", RecursiveMode::Recursive)
        .unwrap();

    loop {
        match rx.recv() {
            Ok(RawEvent {
                path: Some(path),
                op: Ok(op),
                cookie,
            }) => {
                println!("{:?} {:?} ({:?})", op, path, cookie);
                //
            }
            Ok(event) => {
                println!("broken event: {:?}", event);
                //
            }
            Err(e) => {
                println!("watch error: {:?}", e);
            }
        }
    }
}
