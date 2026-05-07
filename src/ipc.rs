use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Cross-thread events from the keyboard hook to the main (UI) thread.
///
/// Only hotkey letters travel this channel; UI actions (tray / menu clicks)
/// are handled synchronously on the main thread and don't need to go through
/// the queue.
#[derive(Debug, Clone, Copy)]
pub enum AppEvent {
    HotkeyLetter(char),
}

pub type Queue = Arc<Mutex<VecDeque<AppEvent>>>;

pub fn new_queue() -> Queue {
    Arc::new(Mutex::new(VecDeque::new()))
}
