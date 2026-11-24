use crate::messages::{to_wide_null, WM_APP_FS_CHANGED};
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_ACTION, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// Safe handle wrapper to send across thread boundary for shutdown signaling.
struct SendHandle(isize);
unsafe impl Send for SendHandle {}

pub struct FolderWatcher {
    stop_event: SendHandle,
    join: Option<JoinHandle<()>>,
}

impl FolderWatcher {
    pub fn start(path: PathBuf, target_hwnd: HWND) -> Option<Self> {
        unsafe {
            let event = CreateEventW(None, true, false, None).ok()?;
            let stop_raw = event.0 as isize;
            let hwnd_raw = target_hwnd.0 as isize;

            let join = thread::spawn(move || {
                watch_loop(path, HWND(hwnd_raw as *mut _), HANDLE(stop_raw as *mut _));
            });

            Some(Self {
                stop_event: SendHandle(stop_raw),
                join: Some(join),
            })
        }
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        unsafe {
            let ev = HANDLE(self.stop_event.0 as *mut _);
            let _ = SetEvent(ev);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
            let _ = CloseHandle(ev);
        }
    }
}

unsafe fn watch_loop(path: PathBuf, target_hwnd: HWND, stop_event: HANDLE) {
    let wide = to_wide_null(&path.to_string_lossy());
    let Ok(dir_handle) = CreateFileW(
        PCWSTR(wide.as_ptr()),
        FILE_LIST_DIRECTORY.0,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        None,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
        None,
    ) else {
        return;
    };

    let Ok(io_event) = CreateEventW(None, true, false, None) else {
        let _ = CloseHandle(dir_handle);
        return;
    };

    let mut buffer = vec![0u8; 8192];
    let filter =
        FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;

    loop {
        let mut overlapped = OVERLAPPED {
            hEvent: io_event,
            ..Default::default()
        };

        let ok = ReadDirectoryChangesW(
            dir_handle,
            buffer.as_mut_ptr() as *mut _,
            buffer.len() as u32,
            false,
            filter,
            None,
            Some(&mut overlapped),
            None,
        );
        if ok.is_err() {
            break;
        }

        let handles = [stop_event, io_event];
        let wait = WaitForMultipleObjects(&handles, false, INFINITE);
        if wait == WAIT_OBJECT_0 {
            break;
        }

        let mut bytes_transferred = 0u32;
        if GetOverlappedResult(dir_handle, &overlapped, &mut bytes_transferred, false).is_ok()
            && bytes_transferred > 0
        {
            let _action: FILE_ACTION = FILE_ACTION(0);
            let _ = PostMessageW(
                Some(target_hwnd),
                WM_APP_FS_CHANGED,
                Default::default(),
                Default::default(),
            );
        }
    }

    let _ = CloseHandle(io_event);
    let _ = CloseHandle(dir_handle);
}
