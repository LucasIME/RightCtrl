use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::apps::{App, AppCache};
use crate::config::{Config, effective_letter};
use crate::cycle::Cycler;
use crate::ipc::Queue;

pub struct AppState {
    pub config: Config,
    pub cache: AppCache,
    pub cycler: Cycler,
    pub queue: Queue,
}

pub type SharedState = Rc<RefCell<AppState>>;

impl AppState {
    pub fn new(config: Config, queue: Queue) -> Self {
        let debounce = Duration::from_millis(config.settings.cycle_debounce_ms);
        Self {
            config,
            cache: AppCache::new(Duration::from_secs(2)),
            cycler: Cycler::new(debounce),
            queue,
        }
    }

    pub fn rescan(&mut self) {
        self.cache.invalidate();
        let _ = self.cache.get();
    }

    /// Return a cloned snapshot of the current app list (triggering an
    /// enumeration if the cache is stale). Used by the preferences UI.
    pub fn cache_snapshot(&mut self) -> Vec<App> {
        self.cache.get().clone()
    }

    pub fn trigger_letter(&mut self, letter: char) {
        let letter = letter.to_ascii_uppercase();
        let apps = self.cache.get();
        let cfg = &self.config;
        let candidates: Vec<App> = apps
            .iter()
            .filter(|a| effective_letter(a, cfg) == Some(letter))
            .cloned()
            .collect();

        match self.cycler.pick(letter, candidates) {
            Some(app) => {
                tracing::info!(
                    "activating '{}' hwnd={:#x} pid={}",
                    app.display_name,
                    app.hwnd,
                    app.pid
                );
                let _ = crate::focus::activate(app.hwnd);
            }
            None => tracing::debug!("no match for letter '{letter}'"),
        }
    }

    pub fn apply_settings(&mut self) {
        self.cycler
            .set_debounce(Duration::from_millis(self.config.settings.cycle_debounce_ms));
    }
}

pub fn shared(state: AppState) -> SharedState {
    Rc::new(RefCell::new(state))
}
