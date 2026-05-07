use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_CREATE_KEY_DISPOSITION, REG_SAM_FLAGS,
    REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW,
};
use windows::core::PCWSTR;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "rightctrl";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn wide_os(p: &std::path::Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("current_exe")
}

fn open_run_key(sam: REG_SAM_FLAGS) -> Result<HKEY> {
    let sub = wide(RUN_KEY);
    let mut hkey = HKEY::default();
    unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), 0, sam, &mut hkey).ok()?;
    }
    Ok(hkey)
}

fn create_run_key() -> Result<HKEY> {
    let sub = wide(RUN_KEY);
    let mut hkey = HKEY::default();
    let mut disp = REG_CREATE_KEY_DISPOSITION::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            0,
            PCWSTR::null(),
            windows::Win32::System::Registry::REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_READ,
            None,
            &mut hkey,
            Some(&mut disp as *mut _),
        )
        .ok()?;
    }
    Ok(hkey)
}

pub fn set(enable: bool) -> Result<()> {
    if enable {
        let exe = current_exe()?;
        // Wrap in quotes for robustness against paths with spaces.
        let command = format!("\"{}\"", exe.display());
        let value_w: Vec<u16> = wide(&command);
        let value_name_w = wide(VALUE_NAME);
        let hkey = create_run_key()?;
        let bytes = unsafe {
            std::slice::from_raw_parts(
                value_w.as_ptr() as *const u8,
                value_w.len() * std::mem::size_of::<u16>(),
            )
        };
        let res = unsafe {
            RegSetValueExW(hkey, PCWSTR(value_name_w.as_ptr()), 0, REG_SZ, Some(bytes)).ok()
        };
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        res?;
        Ok(())
    } else {
        let hkey = match open_run_key(KEY_SET_VALUE) {
            Ok(h) => h,
            Err(e) => {
                tracing::debug!("autostart: Run key not open ({e:?}); treating as already removed");
                return Ok(());
            }
        };
        let name = wide(VALUE_NAME);
        let res = unsafe { RegDeleteValueW(hkey, PCWSTR(name.as_ptr())) };
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        if res.is_err() && res.0 != ERROR_FILE_NOT_FOUND.0 {
            anyhow::bail!("RegDeleteValueW failed: {res:?}");
        }
        Ok(())
    }
}

pub fn is_enabled() -> bool {
    let hkey = match open_run_key(KEY_READ) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let name = wide(VALUE_NAME);
    let mut sz: u32 = 0;
    let mut ty: windows::Win32::System::Registry::REG_VALUE_TYPE = Default::default();
    let probe = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut sz),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    probe.is_ok()
}

/// Make sure what's on disk matches the desired state.
/// Called on every save so the user's "Launch at login" toggle is durable.
pub fn sync(desired: bool) -> Result<()> {
    if desired == is_enabled() {
        return Ok(());
    }
    set(desired)
}

// Silence unused-import warnings on non-windows (we gate everything via cfg(windows) elsewhere).
#[allow(dead_code)]
fn _touch() {
    let _ = wide_os;
}
