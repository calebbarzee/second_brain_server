use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

/// A file change event after debouncing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileChange {
    /// File was created or modified.
    Modified(PathBuf),
    /// File was deleted.
    Deleted(PathBuf),
}

/// Configuration for the file watcher.
pub struct WatcherConfig {
    /// Debounce window — events within this duration are coalesced.
    pub debounce_ms: u64,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self { debounce_ms: 300 }
    }
}

/// A file watcher that monitors directories for markdown file changes.
/// Debounces rapid changes and sends coalesced events through a channel.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    watch_paths: Vec<PathBuf>,
}

impl FileWatcher {
    /// Start watching the given directories. Returns a receiver for debounced file changes.
    pub fn start(
        paths: Vec<PathBuf>,
        config: WatcherConfig,
    ) -> anyhow::Result<(Self, mpsc::Receiver<FileChange>)> {
        let (raw_tx, mut raw_rx) = mpsc::channel::<Event>(256);
        let (debounced_tx, debounced_rx) = mpsc::channel::<FileChange>(64);

        // Create the filesystem watcher
        let tx_clone = raw_tx.clone();
        let mut watcher = RecommendedWatcher::new(
            move |result: Result<Event, notify::Error>| {
                if let Ok(event) = result {
                    let _ = tx_clone.blocking_send(event);
                }
            },
            Config::default(),
        )?;

        // Watch all configured paths
        for path in &paths {
            if path.exists() {
                watcher.watch(path, RecursiveMode::Recursive)?;
                tracing::info!("watching directory: {}", path.display());
            } else {
                tracing::warn!("watch path does not exist: {}", path.display());
            }
        }

        // Spawn debounce task
        let debounce_duration = Duration::from_millis(config.debounce_ms);
        tokio::spawn(async move {
            debounce_loop(&mut raw_rx, &debounced_tx, debounce_duration).await;
        });

        Ok((
            Self {
                _watcher: watcher,
                watch_paths: paths,
            },
            debounced_rx,
        ))
    }

    pub fn watch_paths(&self) -> &[PathBuf] {
        &self.watch_paths
    }
}

/// Debounce loop: collects raw filesystem events and coalesces them by path.
/// After the debounce window, sends the final state of each path.
async fn debounce_loop(
    raw_rx: &mut mpsc::Receiver<Event>,
    debounced_tx: &mpsc::Sender<FileChange>,
    debounce_duration: Duration,
) {
    let mut pending: HashMap<PathBuf, FileChange> = HashMap::new();

    loop {
        // Wait for an event or timeout to flush pending events
        let event = if pending.is_empty() {
            // No pending events — block until we get one
            match raw_rx.recv().await {
                Some(e) => Some(e),
                None => break, // Channel closed
            }
        } else {
            // We have pending events — wait up to debounce_duration for more
            match tokio::time::timeout(debounce_duration, raw_rx.recv()).await {
                Ok(Some(e)) => Some(e),
                Ok(None) => break,     // Channel closed
                Err(_timeout) => None, // Debounce window expired — flush
            }
        };

        if let Some(event) = event {
            // Process the raw event — only care about markdown files
            for path in &event.paths {
                if !is_markdown(path) {
                    continue;
                }

                let change = match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        FileChange::Modified(path.clone())
                    }
                    EventKind::Remove(_) => FileChange::Deleted(path.clone()),
                    _ => continue,
                };

                pending.insert(path.clone(), change);
            }
        } else {
            // Debounce window expired — flush all pending events
            for (_, change) in pending.drain() {
                if debounced_tx.send(change).await.is_err() {
                    return; // Receiver dropped
                }
            }
        }
    }

    // Flush remaining on shutdown
    for (_, change) in pending.drain() {
        let _ = debounced_tx.send(change).await;
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_markdown() {
        assert!(is_markdown(Path::new("/notes/foo.md")));
        assert!(is_markdown(Path::new("bar.md")));
        assert!(!is_markdown(Path::new("foo.txt")));
        assert!(!is_markdown(Path::new("foo.rs")));
        assert!(!is_markdown(Path::new("foo")));
    }

    #[test]
    fn test_file_change_equality() {
        let a = FileChange::Modified(PathBuf::from("/a.md"));
        let b = FileChange::Modified(PathBuf::from("/a.md"));
        assert_eq!(a, b);

        let c = FileChange::Deleted(PathBuf::from("/a.md"));
        assert_ne!(a, c);
    }
}
