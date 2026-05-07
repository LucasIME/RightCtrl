use std::time::{Duration, Instant};

use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::apps::App;

struct Last {
    letter: char,
    when: Instant,
    index: usize,
}

pub struct Cycler {
    last: Option<Last>,
    debounce: Duration,
}

impl Cycler {
    pub fn new(debounce: Duration) -> Self {
        Self { last: None, debounce }
    }

    pub fn set_debounce(&mut self, debounce: Duration) {
        self.debounce = debounce;
    }

    pub fn pick(&mut self, letter: char, mut candidates: Vec<App>) -> Option<App> {
        candidates.sort_by(|a, b| a.exe_path.cmp(&b.exe_path).then(a.hwnd.cmp(&b.hwnd)));
        if candidates.is_empty() {
            return None;
        }

        let now = Instant::now();
        let fg = unsafe { GetForegroundWindow() }.0 as isize;

        let idx = match &self.last {
            Some(l) if l.letter == letter && now.duration_since(l.when) <= self.debounce => {
                (l.index + 1) % candidates.len()
            }
            _ => candidates
                .iter()
                .position(|a| a.hwnd == fg)
                .map(|p| (p + 1) % candidates.len())
                .unwrap_or(0),
        };

        let chosen = candidates[idx].clone();
        self.last = Some(Last { letter, when: now, index: idx });
        Some(chosen)
    }
}
