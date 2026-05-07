use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use nwg::NativeUi;

use crate::app::SharedState;
use crate::apps::App;
use crate::config::{AppConfig, effective_letter};

#[derive(Default)]
pub struct PrefsUi {
    pub window: nwg::Window,
    pub layout: nwg::GridLayout,

    pub list: nwg::ListView,

    pub lbl_selected: nwg::Label,
    pub lbl_override: nwg::Label,
    pub txt_override: nwg::TextInput,

    pub btn_toggle: nwg::Button,
    pub btn_apply_override: nwg::Button,
    pub btn_clear_override: nwg::Button,
    pub btn_rescan: nwg::Button,
    pub btn_save: nwg::Button,
    pub btn_close: nwg::Button,

    pub autostart_check: nwg::CheckBox,

    pub state: RefCell<Option<SharedState>>,
    /// Snapshot of apps, in the same order as rows in the ListView.
    pub row_apps: RefCell<Vec<App>>,
}

impl PrefsUi {
    fn state(&self) -> Option<SharedState> {
        self.state.borrow().clone()
    }

    fn refresh(&self) {
        let Some(state) = self.state() else { return };
        let (apps, config) = {
            let mut s = state.borrow_mut();
            s.rescan();
            (s.cache_snapshot(), s.config.clone())
        };

        self.list.clear();
        for (row, app) in apps.iter().enumerate() {
            let per = config.apps.get(&app.exe_path).cloned().unwrap_or_default();
            let eff = effective_letter(app, &config)
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".to_string());
            let default = app
                .default_letter()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".to_string());
            let ovr = per
                .letter_override
                .map(|c| c.to_string())
                .unwrap_or_default();
            let exe = app.exe_path.display().to_string();
            let enabled = if per.enabled { "on" } else { "off" };

            self.list.insert_item(nwg::InsertListViewItem {
                index: Some(row as i32),
                column_index: 0,
                text: Some(enabled.to_string()),
                image: None,
            });
            for (col, text) in [
                (1, app.display_name.clone()),
                (2, default),
                (3, ovr),
                (4, eff),
                (5, exe),
            ] {
                self.list.insert_item(nwg::InsertListViewItem {
                    index: Some(row as i32),
                    column_index: col,
                    text: Some(text),
                    image: None,
                });
            }
        }
        *self.row_apps.borrow_mut() = apps;
        self.update_selection_labels();
    }

    fn selected_row(&self) -> Option<usize> {
        // nwg 1.0 exposes `selected_items` returning selected row indices.
        let items = self.list.selected_items();
        items.first().copied()
    }

    fn selected_exe(&self) -> Option<PathBuf> {
        let row = self.selected_row()?;
        self.row_apps.borrow().get(row).map(|a| a.exe_path.clone())
    }

    fn update_selection_labels(&self) {
        let Some(state) = self.state() else { return };
        match self.selected_row() {
            None => {
                self.lbl_selected.set_text("Selected: (none)");
                self.txt_override.set_text("");
            }
            Some(row) => {
                let Some(app) = self.row_apps.borrow().get(row).cloned() else { return };
                let s = state.borrow();
                let entry = s.config.apps.get(&app.exe_path).cloned().unwrap_or_default();
                self.lbl_selected
                    .set_text(&format!("Selected: {}", app.display_name));
                self.txt_override
                    .set_text(&entry.letter_override.map(|c| c.to_string()).unwrap_or_default());
            }
        }
    }

    fn toggle_selected(&self) {
        let Some(state) = self.state() else { return };
        let Some(exe) = self.selected_exe() else { return };
        {
            let mut s = state.borrow_mut();
            let entry = s.config.apps.entry(exe).or_insert_with(AppConfig::default);
            entry.enabled = !entry.enabled;
        }
        self.refresh();
    }

    fn apply_override(&self) {
        let Some(state) = self.state() else { return };
        let Some(exe) = self.selected_exe() else { return };
        let input = self.txt_override.text();
        let new = input
            .chars()
            .find(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase());
        {
            let mut s = state.borrow_mut();
            let entry = s.config.apps.entry(exe).or_insert_with(AppConfig::default);
            entry.letter_override = new;
        }
        self.refresh();
    }

    fn clear_override(&self) {
        let Some(state) = self.state() else { return };
        let Some(exe) = self.selected_exe() else { return };
        {
            let mut s = state.borrow_mut();
            if let Some(entry) = s.config.apps.get_mut(&exe) {
                entry.letter_override = None;
            }
        }
        self.refresh();
    }

    fn rescan(&self) {
        if let Some(state) = self.state() {
            state.borrow_mut().rescan();
        }
        self.refresh();
    }

    fn save(&self) {
        let Some(state) = self.state() else { return };
        let desired_autostart = self.autostart_check.check_state() == nwg::CheckBoxState::Checked;
        let cfg = {
            let mut s = state.borrow_mut();
            s.config.settings.launch_at_login = desired_autostart;
            s.apply_settings();
            s.config.clone()
        };
        if let Err(e) = cfg.save() {
            tracing::error!("save config: {e:?}");
            nwg::modal_error_message(&self.window.handle, "rightctrl", &format!("Save failed:\n{e}"));
            return;
        }
        if let Err(e) = crate::autostart::sync(desired_autostart) {
            tracing::error!("autostart sync: {e:?}");
            nwg::modal_error_message(
                &self.window.handle,
                "rightctrl",
                &format!("Autostart update failed:\n{e}"),
            );
        }
    }

    fn hide(&self) {
        self.window.set_visible(false);
    }
}

impl NativeUi<Rc<PrefsUi>> for PrefsUi {
    fn build_ui(mut data: PrefsUi) -> Result<Rc<PrefsUi>, nwg::NwgError> {
        use nwg::Event as E;

        nwg::Window::builder()
            .flags(nwg::WindowFlags::MAIN_WINDOW | nwg::WindowFlags::VISIBLE)
            .size((960, 560))
            .position((300, 300))
            .title("rightctrl — Preferences")
            .build(&mut data.window)?;

        nwg::ListView::builder()
            .parent(&data.window)
            .list_style(nwg::ListViewStyle::Detailed)
            .ex_flags(nwg::ListViewExFlags::GRID | nwg::ListViewExFlags::FULL_ROW_SELECT)
            .build(&mut data.list)?;
        let cols = [
            (0, 70, "Enabled"),
            (1, 270, "Display name"),
            (2, 70, "Default"),
            (3, 80, "Override"),
            (4, 80, "Effective"),
            (5, 340, "Exe"),
        ];
        for (index, width, text) in cols {
            data.list.insert_column(nwg::InsertListViewColumn {
                index: Some(index),
                width: Some(width),
                fmt: None,
                text: Some(text.into()),
            });
        }

        nwg::Label::builder()
            .parent(&data.window)
            .text("Selected: (none)")
            .build(&mut data.lbl_selected)?;
        nwg::Label::builder()
            .parent(&data.window)
            .text("Override letter:")
            .build(&mut data.lbl_override)?;
        nwg::TextInput::builder()
            .parent(&data.window)
            .build(&mut data.txt_override)?;

        nwg::Button::builder().parent(&data.window).text("Toggle enabled").build(&mut data.btn_toggle)?;
        nwg::Button::builder().parent(&data.window).text("Apply override").build(&mut data.btn_apply_override)?;
        nwg::Button::builder().parent(&data.window).text("Clear override").build(&mut data.btn_clear_override)?;
        nwg::Button::builder().parent(&data.window).text("Rescan").build(&mut data.btn_rescan)?;
        nwg::Button::builder().parent(&data.window).text("Save").build(&mut data.btn_save)?;
        nwg::Button::builder().parent(&data.window).text("Close").build(&mut data.btn_close)?;

        nwg::CheckBox::builder()
            .parent(&data.window)
            .text("Launch at login")
            .build(&mut data.autostart_check)?;

        nwg::GridLayout::builder()
            .parent(&data.window)
            .spacing(2)
            .max_row(Some(12))
            .max_column(Some(6))
            .child_item(nwg::GridLayoutItem::new(&data.list, 0, 0, 6, 8))
            .child_item(nwg::GridLayoutItem::new(&data.lbl_selected, 0, 8, 3, 1))
            .child_item(nwg::GridLayoutItem::new(&data.lbl_override, 3, 8, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.txt_override, 4, 8, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.btn_apply_override, 5, 8, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.btn_toggle, 0, 9, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.btn_clear_override, 1, 9, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.btn_rescan, 2, 9, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.btn_save, 3, 9, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.btn_close, 4, 9, 1, 1))
            .child_item(nwg::GridLayoutItem::new(&data.autostart_check, 5, 9, 1, 1))
            .build(&mut data.layout)?;

        let ui = Rc::new(data);

        let weak = Rc::downgrade(&ui);
        let handler = move |evt, _evt_data, handle| {
            let Some(ui) = weak.upgrade() else { return };
            match evt {
                E::OnButtonClick => {
                    if handle == ui.btn_toggle.handle {
                        ui.toggle_selected();
                    } else if handle == ui.btn_apply_override.handle {
                        ui.apply_override();
                    } else if handle == ui.btn_clear_override.handle {
                        ui.clear_override();
                    } else if handle == ui.btn_rescan.handle {
                        ui.rescan();
                    } else if handle == ui.btn_save.handle {
                        ui.save();
                    } else if handle == ui.btn_close.handle {
                        ui.hide();
                    }
                }
                E::OnListViewItemChanged if handle == ui.list.handle => {
                    ui.update_selection_labels();
                }
                E::OnWindowClose if handle == ui.window.handle => {
                    ui.hide();
                }
                _ => {}
            }
        };

        nwg::full_bind_event_handler(&ui.window.handle, handler);

        Ok(ui)
    }
}

thread_local! {
    static PREFS: RefCell<Option<Rc<PrefsUi>>> = const { RefCell::new(None) };
}

pub fn show(state: SharedState) {
    PREFS.with(|cell| {
        let mut b = cell.borrow_mut();
        if let Some(existing) = b.as_ref() {
            existing.refresh();
            existing.window.set_visible(true);
            existing.window.set_focus();
            return;
        }

        let mut data = PrefsUi::default();
        data.state = RefCell::new(Some(state.clone()));
        match PrefsUi::build_ui(data) {
            Ok(ui) => {
                let launch = state.borrow().config.settings.launch_at_login;
                ui.autostart_check.set_check_state(if launch {
                    nwg::CheckBoxState::Checked
                } else {
                    nwg::CheckBoxState::Unchecked
                });
                ui.refresh();
                ui.window.set_visible(true);
                ui.window.set_focus();
                *b = Some(ui);
            }
            Err(e) => tracing::error!("open preferences failed: {e:?}"),
        }
    });
}
