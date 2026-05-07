use std::cell::RefCell;
use std::rc::Rc;

use nwg::NativeUi;

use crate::app::SharedState;
use crate::ipc::AppEvent;

pub struct Tray {
    ui: Rc<TrayUi>,
}

#[derive(Default)]
pub struct TrayUi {
    pub window: nwg::MessageWindow,
    pub icon: nwg::Icon,
    pub tray: nwg::TrayNotification,
    pub menu: nwg::Menu,
    pub mi_prefs: nwg::MenuItem,
    pub mi_rescan: nwg::MenuItem,
    pub mi_sep: nwg::MenuSeparator,
    pub mi_quit: nwg::MenuItem,
    pub notice: nwg::Notice,
    pub state: RefCell<Option<SharedState>>,
}

impl TrayUi {
    fn state(&self) -> Option<SharedState> {
        self.state.borrow().clone()
    }

    fn open_preferences(&self) {
        if let Some(state) = self.state() {
            crate::ui::show(state);
        }
    }

    fn quit(&self) {
        nwg::stop_thread_dispatch();
    }

    fn rescan(&self) {
        if let Some(state) = self.state() {
            state.borrow_mut().rescan();
        }
    }

    fn drain_queue(&self) {
        let state = match self.state() {
            Some(s) => s,
            None => return,
        };
        let queue = state.borrow().queue.clone();
        loop {
            let ev = {
                let mut q = match queue.lock() {
                    Ok(q) => q,
                    Err(_) => return,
                };
                q.pop_front()
            };
            let Some(ev) = ev else { return };
            self.handle_event(ev, &state);
        }
    }

    fn handle_event(&self, ev: AppEvent, state: &SharedState) {
        match ev {
            AppEvent::HotkeyLetter(c) => {
                state.borrow_mut().trigger_letter(c);
            }
        }
    }
}

impl NativeUi<Rc<TrayUi>> for TrayUi {
    fn build_ui(mut data: TrayUi) -> Result<Rc<TrayUi>, nwg::NwgError> {
        use nwg::Event as E;

        nwg::MessageWindow::builder().build(&mut data.window)?;

        // Try to load IDI_APP from the embedded resource; fall back to the
        // system application icon if the resource isn't present.
        let embed = nwg::EmbedResource::load(None).ok();
        if let Some(embed) = embed.as_ref() {
            let _ = nwg::Icon::builder()
                .source_embed(Some(embed))
                .source_embed_str(Some("IDI_APP"))
                .strict(false)
                .build(&mut data.icon);
        }
        if data.icon.handle.is_null() {
            let _ = nwg::Icon::builder()
                .source_system(Some(nwg::OemIcon::Sample))
                .build(&mut data.icon);
        }

        nwg::TrayNotification::builder()
            .parent(&data.window)
            .icon(Some(&data.icon))
            .tip(Some("rightctrl"))
            .build(&mut data.tray)?;

        nwg::Menu::builder()
            .popup(true)
            .parent(&data.window)
            .build(&mut data.menu)?;

        nwg::MenuItem::builder()
            .text("Preferences…")
            .parent(&data.menu)
            .build(&mut data.mi_prefs)?;
        nwg::MenuItem::builder()
            .text("Rescan apps")
            .parent(&data.menu)
            .build(&mut data.mi_rescan)?;
        nwg::MenuSeparator::builder()
            .parent(&data.menu)
            .build(&mut data.mi_sep)?;
        nwg::MenuItem::builder()
            .text("Quit")
            .parent(&data.menu)
            .build(&mut data.mi_quit)?;

        nwg::Notice::builder()
            .parent(&data.window)
            .build(&mut data.notice)?;

        let ui = Rc::new(data);

        let weak = Rc::downgrade(&ui);
        let handler = move |evt, _evt_data, handle| {
            let Some(ui) = weak.upgrade() else { return };
            match evt {
                E::OnNotice if handle == ui.notice.handle => ui.drain_queue(),
                E::OnContextMenu if handle == ui.tray.handle => {
                    let (x, y) = nwg::GlobalCursor::position();
                    ui.menu.popup(x, y);
                }
                E::OnMousePress(nwg::MousePressEvent::MousePressLeftUp)
                    if handle == ui.tray.handle =>
                {
                    ui.open_preferences();
                }
                E::OnMenuItemSelected => {
                    if handle == ui.mi_prefs.handle {
                        ui.open_preferences();
                    } else if handle == ui.mi_rescan.handle {
                        ui.rescan();
                    } else if handle == ui.mi_quit.handle {
                        ui.quit();
                    }
                }
                _ => {}
            }
        };

        nwg::full_bind_event_handler(&ui.window.handle, handler);

        Ok(ui)
    }
}

impl Tray {
    pub fn build(state: SharedState) -> anyhow::Result<Tray> {
        let mut ui = TrayUi::default();
        ui.state = RefCell::new(Some(state));
        let root = TrayUi::build_ui(ui).map_err(|e| anyhow::anyhow!("tray ui: {e:?}"))?;
        Ok(Tray { ui: root })
    }

    pub fn notice_sender(&self) -> nwg::NoticeSender {
        self.ui.notice.sender()
    }
}
