use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HWND, LPARAM, TRUE};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    WS_EX_TOOLWINDOW,
};
use windows::core::{PCWSTR, PWSTR};

#[derive(Clone, Debug)]
pub struct App {
    pub hwnd: isize,
    pub pid: u32,
    pub exe_path: PathBuf,
    pub display_name: String,
    #[allow(dead_code)]
    pub window_title: String,
}

impl App {
    pub fn default_letter(&self) -> Option<char> {
        self.display_name
            .chars()
            .find(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
    }
}

pub struct AppCache {
    last: Option<(Instant, Vec<App>)>,
    ttl: Duration,
}

impl AppCache {
    pub fn new(ttl: Duration) -> Self {
        Self { last: None, ttl }
    }

    pub fn get(&mut self) -> &Vec<App> {
        let fresh = self
            .last
            .as_ref()
            .map(|(t, _)| t.elapsed() < self.ttl)
            .unwrap_or(false);
        if !fresh {
            self.last = Some((Instant::now(), enumerate()));
        }
        &self.last.as_ref().unwrap().1
    }

    pub fn invalidate(&mut self) {
        self.last = None;
    }
}

pub fn enumerate() -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    let ptr = &mut out as *mut Vec<App> as isize;
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(ptr));
    }
    out
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<App>) };
        if let Some(app) = unsafe { build_app(hwnd) } {
            out.push(app);
        }
    }));
    TRUE
}

unsafe fn build_app(hwnd: HWND) -> Option<App> {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return None;
    }
    // GetWindow(_, GW_OWNER) returns Err (null) when there's no owner → top-level.
    if unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok() {
        return None;
    }
    let title_len = unsafe { GetWindowTextLengthW(hwnd) };
    if title_len <= 0 {
        return None;
    }
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 {
        return None;
    }
    // DWM cloaked filter — excludes ghost UWP windows on other desktops.
    let mut cloaked: u32 = 0;
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if hr.is_ok() && cloaked != 0 {
        return None;
    }

    let title = unsafe { read_window_text(hwnd, title_len as usize) };

    let (pid, exe) = unsafe { resolve_process(hwnd) }?;
    let display_name = friendly_name(&exe, &title);

    Some(App {
        hwnd: hwnd.0 as isize,
        pid,
        exe_path: exe,
        display_name,
        window_title: title,
    })
}

unsafe fn read_window_text(hwnd: HWND, hinted_len: usize) -> String {
    let cap = hinted_len.saturating_add(1).max(2);
    let mut buf = vec![0u16; cap];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

unsafe fn resolve_process(hwnd: HWND) -> Option<(u32, PathBuf)> {
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let exe = unsafe { exe_for_pid(pid) }?;

    // UWP: if the host is ApplicationFrameHost.exe, find a child with a different PID.
    if exe
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("ApplicationFrameHost.exe"))
        .unwrap_or(false)
    {
        if let Some(child_pid) = unsafe { find_uwp_child_pid(hwnd, pid) } {
            if let Some(child_exe) = unsafe { exe_for_pid(child_pid) } {
                return Some((child_pid, child_exe));
            }
        }
    }
    Some((pid, exe))
}

unsafe fn exe_for_pid(pid: u32) -> Option<PathBuf> {
    let h: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 32768];
    let mut sz = buf.len() as u32;
    let res = unsafe {
        QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut sz)
    };
    let _ = unsafe { CloseHandle(h) };
    res.ok()?;
    if sz == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..sz as usize]);
    Some(PathBuf::from(s))
}

struct ChildCtx {
    host_pid: u32,
    found: u32,
}

unsafe fn find_uwp_child_pid(parent: HWND, host_pid: u32) -> Option<u32> {
    let mut ctx = ChildCtx { host_pid, found: 0 };
    let ptr = &mut ctx as *mut ChildCtx as isize;
    unsafe {
        let _ = EnumChildWindows(parent, Some(child_proc), LPARAM(ptr));
    }
    if ctx.found != 0 { Some(ctx.found) } else { None }
}

unsafe extern "system" fn child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut keep_going = TRUE;
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let ctx = unsafe { &mut *(lparam.0 as *mut ChildCtx) };
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != 0 && pid != ctx.host_pid {
            ctx.found = pid;
            keep_going = BOOL(0);
        }
    }));
    keep_going
}

fn friendly_name(exe: &Path, window_title: &str) -> String {
    if let Some(d) = file_description(exe) {
        if !d.eq_ignore_ascii_case("Application Frame Host") {
            return d;
        }
    }
    // Fallbacks: window title first (useful for UWP), then exe stem.
    let trimmed = window_title.trim();
    if !trimmed.is_empty() {
        // UWP titles are often "Document - AppName"; prefer the tail.
        if let Some((_, tail)) = trimmed.rsplit_once(" - ") {
            if !tail.is_empty() {
                return tail.to_string();
            }
        }
        return trimmed.to_string();
    }
    exe.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn file_description(exe: &Path) -> Option<String> {
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let pcw = PCWSTR(wide.as_ptr());

    let size = unsafe { GetFileVersionInfoSizeW(pcw, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    unsafe { GetFileVersionInfoW(pcw, 0, size, buf.as_mut_ptr() as *mut c_void) }.ok()?;

    // Read translation table to learn lang + codepage.
    let trans_key: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
    let mut tptr: *mut c_void = std::ptr::null_mut();
    let mut tlen: u32 = 0;
    let ok = unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const c_void,
            PCWSTR(trans_key.as_ptr()),
            &mut tptr,
            &mut tlen,
        )
    };
    if !ok.as_bool() || tptr.is_null() || tlen < 4 {
        return None;
    }
    let (lang, cp) = unsafe { (*(tptr as *const u16), *((tptr as *const u16).add(1))) };

    let sub_str = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription\0", lang, cp);
    let sub: Vec<u16> = sub_str.encode_utf16().collect();
    let mut vptr: *mut c_void = std::ptr::null_mut();
    let mut vlen: u32 = 0;
    let ok = unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const c_void,
            PCWSTR(sub.as_ptr()),
            &mut vptr,
            &mut vlen,
        )
    };
    if !ok.as_bool() || vptr.is_null() || vlen == 0 {
        return None;
    }
    let wide = unsafe { std::slice::from_raw_parts(vptr as *const u16, vlen as usize) };
    let s = String::from_utf16_lossy(wide);
    let s = s.trim_end_matches('\0').trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
