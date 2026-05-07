#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod apps;
mod autostart;
mod config;
mod cycle;
mod focus;
mod hook;
mod ipc;
mod single_instance;
mod tray;
mod ui;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

use crate::app::{AppState, shared};
use crate::config::Config;
use crate::single_instance::{AcquireResult, acquire};

fn main() -> Result<()> {
    init_logging();
    hook::install_panic_guard();

    match acquire() {
        AcquireResult::Acquired(_guard) => run()?,
        AcquireResult::AlreadyRunning => {
            tracing::info!("another instance is already running; exiting");
            return Ok(());
        }
        AcquireResult::Error(e) => {
            tracing::warn!("single-instance check failed: {e:?}; running anyway");
            run()?;
        }
    }

    Ok(())
}

fn init_logging() {
    let target = config::log_dir().ok();
    let env_filter = EnvFilter::try_from_env("RIGHTCTRL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    if let Some(dir) = target.as_deref() {
        let _ = std::fs::create_dir_all(dir);
        let appender = tracing_appender::rolling::daily(dir, "rightctrl.log");
        let (nb, guard) = tracing_appender::non_blocking(appender);
        // Leak the guard so logs keep flushing for the life of the process.
        Box::leak(Box::new(guard));
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(nb)
            .with_ansi(false)
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init();
    }
}

fn run() -> Result<()> {
    nwg::init().map_err(|e| anyhow::anyhow!("nwg init: {e:?}"))?;
    let _ = nwg::Font::set_global_family("Segoe UI");

    let cfg = Config::load().unwrap_or_else(|e| {
        tracing::warn!("config load failed: {e:?}; starting with defaults");
        Config::default()
    });

    // Keep autostart state in sync with the persisted setting.
    if let Err(e) = autostart::sync(cfg.settings.launch_at_login) {
        tracing::warn!("autostart sync at startup: {e:?}");
    }

    let queue = ipc::new_queue();
    let mut state = AppState::new(cfg, queue.clone());
    // Prime the cache once so the first hotkey feels instant.
    state.rescan();
    let shared_state = shared(state);

    let tray = tray::Tray::build(shared_state.clone()).context("build tray")?;
    let wake = tray.notice_sender();
    let (_hook_handle, _hook_join) = hook::spawn(queue, wake);

    nwg::dispatch_thread_events();
    Ok(())
}
