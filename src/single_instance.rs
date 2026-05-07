use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::PCWSTR;

const MUTEX_NAME: &str = "Local\\rightctrl.singleton";

pub struct InstanceGuard {
    _handle: HANDLE,
}

pub enum AcquireResult {
    Acquired(InstanceGuard),
    AlreadyRunning,
    Error(windows::core::Error),
}

pub fn acquire() -> AcquireResult {
    let mut wide: Vec<u16> = MUTEX_NAME.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(wide.as_mut_ptr())) };
    match handle {
        Ok(h) => {
            let err = unsafe { GetLastError() };
            if err == ERROR_ALREADY_EXISTS {
                unsafe {
                    let _ = CloseHandle(h);
                }
                AcquireResult::AlreadyRunning
            } else {
                AcquireResult::Acquired(InstanceGuard { _handle: h })
            }
        }
        Err(e) => AcquireResult::Error(e),
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self._handle);
        }
    }
}
