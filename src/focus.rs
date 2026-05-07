use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SW_RESTORE,
    SetForegroundWindow, ShowWindow,
};

pub fn activate(hwnd_raw: isize) -> bool {
    let hwnd = HWND(hwnd_raw as _);
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return false;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let our_tid = GetCurrentThreadId();
        let fg = GetForegroundWindow();
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let tgt_tid = GetWindowThreadProcessId(hwnd, None);

        let a1 = if fg_tid != 0 && fg_tid != our_tid {
            AttachThreadInput(our_tid, fg_tid, true).as_bool()
        } else {
            false
        };
        let a2 = if tgt_tid != 0 && tgt_tid != our_tid && tgt_tid != fg_tid {
            AttachThreadInput(our_tid, tgt_tid, true).as_bool()
        } else {
            false
        };

        let _ = BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd).as_bool();
        let _ = SetFocus(hwnd);

        if a2 {
            let _ = AttachThreadInput(our_tid, tgt_tid, false);
        }
        if a1 {
            let _ = AttachThreadInput(our_tid, fg_tid, false);
        }

        ok
    }
}
