//! Clipboard access only on explicit copy, cut or paste input.
#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, ptr};
    type Handle = *mut c_void;
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(owner: Handle) -> i32;
        fn CloseClipboard() -> i32;
        fn GetActiveWindow() -> Handle;
        fn GetClipboardData(format: u32) -> Handle;
        fn EmptyClipboard() -> i32;
        fn SetClipboardData(format: u32, data: Handle) -> Handle;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
        fn GlobalLock(block: Handle) -> Handle;
        fn GlobalUnlock(block: Handle) -> i32;
        fn GlobalFree(block: Handle) -> Handle;
        fn GlobalSize(block: Handle) -> usize;
    }
    const UNICODE_TEXT: u32 = 13;
    struct Clipboard;
    impl Clipboard {
        fn open() -> Option<Self> {
            (unsafe { OpenClipboard(GetActiveWindow()) } != 0).then(|| Self)
        }
    }
    impl Drop for Clipboard {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    pub fn get() -> String {
        let Some(_open) = Clipboard::open() else {
            return String::new();
        };
        unsafe {
            let block = GetClipboardData(UNICODE_TEXT);
            if block.is_null() {
                return String::new();
            }
            let size = GlobalSize(block) / 2;
            let data = GlobalLock(block) as *const u16;
            if data.is_null() {
                return String::new();
            }
            let contents = std::slice::from_raw_parts(data, size.min(1 << 20));
            let end = contents
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(contents.len());
            let result = String::from_utf16_lossy(&contents[..end]);
            GlobalUnlock(block);
            result
        }
    }
    pub fn set(text: &str) {
        let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let Some(_open) = Clipboard::open() else {
            return;
        };
        unsafe {
            let block = GlobalAlloc(2, text.len() * 2);
            if block.is_null() {
                return;
            }
            let dest = GlobalLock(block) as *mut u16;
            if !dest.is_null() {
                ptr::copy_nonoverlapping(text.as_ptr(), dest, text.len());
                GlobalUnlock(block);
                if EmptyClipboard() != 0 && !SetClipboardData(UNICODE_TEXT, block).is_null() {
                    return;
                }
            }
            GlobalFree(block);
        }
    }
}
#[cfg(not(windows))]
mod platform {
    pub fn get() -> String {
        String::new()
    }
    pub fn set(_: &str) {}
}
pub use platform::{get, set};
