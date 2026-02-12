use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Mutex, Once, OnceLock, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

pub const LOG_EVENT: &str = "backend-log";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
    pub level: String,
    pub target: String,
    pub message: String,
    pub timestamp_ms: u128,
}

impl LogEntry {
    fn new(level: log::Level, target: &str, message: String) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        Self {
            level: level.to_string(),
            target: target.to_string(),
            message,
            timestamp_ms,
        }
    }
}

struct LogStore {
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl LogStore {
    fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    fn entries(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }
}

pub struct BridgeLogger {
    store: Mutex<LogStore>,
    handle: RwLock<Option<AppHandle>>,
}

impl BridgeLogger {
    fn new(capacity: usize) -> Self {
        Self {
            store: Mutex::new(LogStore::new(capacity)),
            handle: RwLock::new(None),
        }
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        let mut guard = self.handle.write().expect("log handle lock poisoned");
        *guard = Some(handle);
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        let guard = self.store.lock().expect("log store lock poisoned");
        guard.entries()
    }

    pub fn emit_event<T: Serialize>(&self, event: &str, payload: &T) {
        if let Some(handle) = self
            .handle
            .read()
            .expect("log handle lock poisoned")
            .as_ref()
        {
            let _ = handle.emit_all(event, payload);
        }
    }

    fn push_entry(&self, entry: LogEntry) {
        {
            let mut guard = self.store.lock().expect("log store lock poisoned");
            guard.push(entry.clone());
        }

        self.emit_event(LOG_EVENT, &entry);
    }
}

impl log::Log for BridgeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let entry = LogEntry::new(
            record.level(),
            record.target(),
            format!("{}", record.args()),
        );
        self.push_entry(entry);
    }

    fn flush(&self) {}
}

static LOGGER: OnceLock<&'static BridgeLogger> = OnceLock::new();

pub fn init_logging() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let logger = Box::new(BridgeLogger::new(500));
        let logger_ref: &'static BridgeLogger = Box::leak(logger);
        let _ = LOGGER.set(logger_ref);
        let _ = log::set_logger(logger_ref);
        log::set_max_level(log::LevelFilter::Info);
    });
}

pub fn logger() -> &'static BridgeLogger {
    LOGGER.get().expect("logger not initialized")
}

pub fn emit_app_event<T: Serialize>(event: &str, payload: &T) {
    if let Some(logger) = LOGGER.get() {
        logger.emit_event(event, payload);
    }
}

pub fn attach_app_handle(handle: AppHandle) {
    logger().set_app_handle(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(message: &str) -> LogEntry {
        LogEntry {
            level: "info".to_string(),
            target: "test".to_string(),
            message: message.to_string(),
            timestamp_ms: 0,
        }
    }

    #[test]
    fn log_store_respects_capacity() {
        let mut store = LogStore::new(2);

        store.push(entry("first"));
        store.push(entry("second"));
        store.push(entry("third"));

        let entries = store.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "second");
        assert_eq!(entries[1].message, "third");
    }

    #[test]
    fn bridge_logger_collects_entries() {
        let logger = BridgeLogger::new(10);

        logger.push_entry(entry("hello"));
        let entries = logger.entries();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "hello");
    }
}
