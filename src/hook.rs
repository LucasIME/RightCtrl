use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use once_cell::sync::OnceCell;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, VIRTUAL_KEY, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::PCWSTR;

use crate::ipc::AppEvent;

pub struct HookHandle {
    hook: AtomicIsize,
}

impl HookHandle {
    /// Release the hook and force-release RWin if it's being held (best effort).
    pub fn shutdown(&self) {
        let h = self.hook.swap(0, Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = UnhookWindowsHookEx(HHOOK(h as _));
                synth_key(VK_RWIN, true); // release RWin
            }
        }
    }
}

static GLOBAL: OnceCell<Arc<HookHandle>> = OnceCell::new();

/// Install the low-level keyboard hook on a dedicated thread.
///
/// `queue` is the shared inbox drained by the main thread.
/// `wake` is the `nwg::Notice` sender that wakes the main thread after a push.
pub fn spawn(
    queue: Arc<Mutex<std::collections::VecDeque<AppEvent>>>,
    wake: nwg::NoticeSender,
) -> (Arc<HookHandle>, JoinHandle<()>) {
    let handle = Arc::new(HookHandle { hook: AtomicIsize::new(0) });
    let _ = GLOBAL.set(handle.clone());

    let handle_for_thread = handle.clone();
    let join = thread::Builder::new()
        .name("rightctrl-hook".into())
        .spawn(move || hook_thread(handle_for_thread, queue, wake))
        .expect("spawn hook thread");

    (handle, join)
}

thread_local! {
    static CTX: RefCell<Option<HookCtx>> = const { RefCell::new(None) };
}

struct HookCtx {
    queue: Arc<Mutex<std::collections::VecDeque<AppEvent>>>,
    wake: nwg::NoticeSender,
    rwin_down: bool,
    rwin_consumed: bool,
}

fn hook_thread(
    handle: Arc<HookHandle>,
    queue: Arc<Mutex<std::collections::VecDeque<AppEvent>>>,
    wake: nwg::NoticeSender,
) {
    CTX.with(|c| {
        *c.borrow_mut() = Some(HookCtx {
            queue,
            wake,
            rwin_down: false,
            rwin_consumed: false,
        });
    });

    let hmod = unsafe { GetModuleHandleW(PCWSTR::null()) }
        .map(|m| HINSTANCE(m.0))
        .unwrap_or(HINSTANCE(std::ptr::null_mut()));

    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), hmod, 0) };
    let hook = match hook {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("SetWindowsHookExW failed: {e:?}");
            return;
        }
    };
    handle.hook.store(hook.0 as isize, Ordering::SeqCst);

    // Standard message pump; the hook fires on this thread.
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    handle.shutdown();
}

unsafe extern "system" fn ll_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let mut swallow = false;
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        swallow = unsafe { handle_event(wparam, lparam) };
    }));

    if swallow {
        LRESULT(1)
    } else {
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

unsafe fn handle_event(wparam: WPARAM, lparam: LPARAM) -> bool {
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };

    // Ignore our own synthesized keystrokes.
    if kb.flags.0 & LLKHF_INJECTED.0 != 0 {
        return false;
    }

    let vk = kb.vkCode;
    let msg = wparam.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    // Right Windows key
    if vk == VK_RWIN.0 as u32 {
        if is_down {
            CTX.with(|c| {
                if let Some(ctx) = c.borrow_mut().as_mut() {
                    ctx.rwin_down = true;
                }
            });
            return true;
        }
        if is_up {
            let was_consumed = CTX.with(|c| {
                let mut b = c.borrow_mut();
                let ctx = match b.as_mut() {
                    Some(x) => x,
                    None => return false,
                };
                let was = ctx.rwin_consumed;
                ctx.rwin_down = false;
                ctx.rwin_consumed = false;
                was
            });
            if !was_consumed {
                // Plain RWin tap: synthesize a LWin tap so Start menu still opens.
                unsafe {
                    synth_key(VK_LWIN, false);
                    synth_key(VK_LWIN, true);
                }
            }
            return true;
        }
        return false;
    }

    // RWin + letter combo
    if is_down {
        let rwin_down = CTX.with(|c| c.borrow().as_ref().map(|x| x.rwin_down).unwrap_or(false));
        if rwin_down {
            if let Some(letter) = vk_to_letter(vk) {
                CTX.with(|c| {
                    if let Some(ctx) = c.borrow_mut().as_mut() {
                        ctx.rwin_consumed = true;
                        if let Ok(mut q) = ctx.queue.lock() {
                            q.push_back(AppEvent::HotkeyLetter(letter));
                        }
                        ctx.wake.notice();
                    }
                });
                return true;
            }
            // Any other key used while RWin is held still counts as "consumed"
            // so we don't accidentally open Start.
            CTX.with(|c| {
                if let Some(ctx) = c.borrow_mut().as_mut() {
                    ctx.rwin_consumed = true;
                }
            });
        }
    }

    false
}

fn vk_to_letter(vk: u32) -> Option<char> {
    if (0x41..=0x5A).contains(&vk) {
        // 'A'..='Z' — ASCII letter VKs match their uppercase chars.
        Some(vk as u8 as char)
    } else {
        None
    }
}

unsafe fn synth_key(vk: VIRTUAL_KEY, key_up: bool) {
    let mut flags = KEYEVENTF_EXTENDEDKEY;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(flags.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Install a panic hook that releases the keyboard hook and RWin on panic,
/// to avoid leaving a stuck modifier.
pub fn install_panic_guard() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(h) = GLOBAL.get() {
            h.shutdown();
        }
        prev(info);
    }));
}
