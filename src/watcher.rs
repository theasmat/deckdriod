use notify_debouncer_mini::{new_debouncer, notify::*, Debouncer, DebounceEventResult};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use anyhow::Result;

pub fn start_watcher(latency: f64, tx: Sender<()>) -> Result<Debouncer<RecommendedWatcher>> {
    let mut debouncer = new_debouncer(Duration::from_secs_f64(latency), move |res: DebounceEventResult| {
        match res {
            Ok(events) => {
                if !events.is_empty() {
                    let _ = tx.blocking_send(());
                }
            }
            Err(e) => eprintln!("watch error: {:?}", e),
        }
    })?;

    if Path::new("androidApp/src").exists() {
        debouncer.watcher().watch(Path::new("androidApp/src"), RecursiveMode::Recursive)?;
    }
    if Path::new("shared/src").exists() {
        debouncer.watcher().watch(Path::new("shared/src"), RecursiveMode::Recursive)?;
    }

    Ok(debouncer)
}
