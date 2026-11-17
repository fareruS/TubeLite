//! Native video surface (Windows): a plain Win32 child window that
//! mpv renders into via its `wid` option, so video plays inside the
//! app window instead of mpv opening its own top-level window.
//!
//! The surface is created on the first `report_video_rect` and then
//! moved/resized with every new rect. Z-order: the child is placed
//! above the webview (`HWND_TOP` among siblings) — the only
//! arrangement in which the picture is visible, because the webview
//! paints an opaque black `#video-hole` over anything beneath it. The
//! hole area carries no web controls (play/pause/seek live below
//! it), so nothing interactive is covered; clicks on the video
//! itself land on the child window and do nothing.
//!
//! Everything is best-effort: any failure is logged and the caller
//! keeps the old behaviour (standalone mpv window). Nothing here can
//! fail startup or playback.
//!
//! Non-Windows: the same API exists but is a no-op, so the app crate
//! needs no `#[cfg]` at the call sites beyond the parent-HWND lookup.

#[cfg(windows)]
use std::sync::Mutex;

/// The native video surface. On Windows it owns one `WS_CHILD`
/// window parented to the Tauri main window; everywhere else it is a
/// no-op with the same API.
pub struct VideoWindow {
    #[cfg(windows)]
    inner: Mutex<Option<VideoWindowInner>>,
    #[cfg(not(windows))]
    _private: (),
}

impl Default for VideoWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
struct VideoWindowInner {
    /// Raw Win32 handles as `i64` (not `HWND`): the `windows`
    /// handle types wrap raw pointers and are not `Send`, but
    /// `AppState` must be `Send` for `app.manage`.
    hwnd: i64,
    parent: i64,
}

#[cfg(windows)]
impl VideoWindow {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Create the child window under `parent_hwnd` (a raw Win32 HWND
    /// as `i64`) at the given rect, or move/resize it when it
    /// already exists. Physical pixels, relative to the parent's
    /// client area. Non-positive sizes are ignored (nothing to show
    /// yet). Failures are logged; the caller falls back to mpv's own
    /// window.
    pub fn place(&self, parent_hwnd: i64, x: i32, y: i32, width: i32, height: i32) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            DestroyWindow, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        };

        if width <= 0 || height <= 0 {
            return;
        }
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let recreate = match guard.as_ref() {
            Some(inner) => inner.parent != parent_hwnd,
            None => true,
        };
        if recreate {
            if let Some(old) = guard.take() {
                unsafe {
                    let _ = DestroyWindow(HWND(old.hwnd as *mut core::ffi::c_void));
                }
            }
            match create_child(parent_hwnd, x, y, width, height) {
                Ok(hwnd) => {
                    *guard = Some(VideoWindowInner {
                        hwnd,
                        parent: parent_hwnd,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        "cannot create video surface ({e}); mpv will use its own window"
                    );
                }
            }
            return;
        }
        if let Some(inner) = guard.as_ref() {
            let hwnd = HWND(inner.hwnd as *mut core::ffi::c_void);
            if let Err(e) = unsafe {
                SetWindowPos(
                    hwnd,
                    Some(HWND_TOP),
                    x,
                    y,
                    width,
                    height,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE,
                )
            } {
                tracing::warn!("cannot move video surface ({e})");
            }
        }
    }

    /// The child window's handle for mpv's `wid`, or `None` when no
    /// surface exists yet (no rect reported, or creation failed).
    pub fn hwnd(&self) -> Option<i64> {
        self.hwnd_raw()
    }

    /// Show the surface (e.g. after an audio-only stretch hid it).
    /// Also re-asserts it above the webview.
    pub fn show(&self) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
        };

        if let Some(hwnd) = self.hwnd_raw() {
            if let Err(e) = unsafe {
                SetWindowPos(
                    HWND(hwnd as *mut core::ffi::c_void),
                    Some(HWND_TOP),
                    0,
                    0,
                    0,
                    0,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                )
            } {
                tracing::warn!("cannot show video surface ({e})");
            }
        }
    }

    /// Hide the surface (audio-only playback, or leaving the Watch
    /// page). The window is kept, so showing it again is cheap.
    pub fn hide(&self) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

        if let Some(hwnd) = self.hwnd_raw() {
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut core::ffi::c_void), SW_HIDE);
            }
        }
    }

    fn hwnd_raw(&self) -> Option<i64> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|inner| inner.hwnd)
    }
}

#[cfg(windows)]
mod win32_impl {
    use std::sync::OnceLock;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassExW, CS_HREDRAW, CS_VREDRAW, WINDOW_EX_STYLE,
        WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
    };

    const CLASS_NAME: PCWSTR = w!("TubeLiteVideoSurface");
    /// `ERROR_CLASS_ALREADY_EXISTS` — a previous registration (or a
    /// second app instance in the same session) is fine.
    const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

    static CLASS: OnceLock<Result<(), String>> = OnceLock::new();

    /// The child window needs no custom handling — default processing
    /// is enough (mpv subclasses it for its own sizing once `wid`
    /// is set). `DefWindowProcW` itself is a plain Rust-ABI wrapper
    /// in windows 0.62, not an `extern "system"` fn, so it cannot be
    /// the class procedure directly; this thin shim can.
    unsafe extern "system" fn video_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn register_class() -> Result<(), String> {
        CLASS
            .get_or_init(|| {
                let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }
                    .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(video_wnd_proc),
                    hInstance: HINSTANCE(instance.0),
                    lpszClassName: CLASS_NAME,
                    ..Default::default()
                };
                let atom = unsafe { RegisterClassExW(&wc) };
                if atom == 0 {
                    let code = unsafe { GetLastError() }.0;
                    if code == ERROR_CLASS_ALREADY_EXISTS {
                        return Ok(());
                    }
                    return Err(format!("RegisterClassExW failed (win32 error {code})"));
                }
                Ok(())
            })
            .clone()
    }

    pub(super) fn create_child(
        parent_hwnd: i64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> Result<i64, String> {
        register_class()?;
        let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                x,
                y,
                width,
                height,
                Some(HWND(parent_hwnd as *mut core::ffi::c_void)),
                None,
                Some(HINSTANCE(instance.0)),
                None,
            )
        }
        .map_err(|e| format!("CreateWindowExW failed: {e}"))?;
        Ok(hwnd.0 as i64)
    }
}

#[cfg(windows)]
use win32_impl::create_child;

#[cfg(not(windows))]
impl VideoWindow {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn place(&self, _parent_hwnd: i64, _x: i32, _y: i32, _width: i32, _height: i32) {}

    pub fn hwnd(&self) -> Option<i64> {
        None
    }

    pub fn show(&self) {}

    pub fn hide(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_has_no_handle() {
        assert_eq!(VideoWindow::new().hwnd(), None);
    }

    #[test]
    fn place_hide_show_are_noops_without_a_window() {
        // No parent exists in a test: on Windows this logs a warning
        // and stays handle-less; elsewhere it is a plain no-op.
        let w = VideoWindow::new();
        w.place(0, 0, 0, 100, 100);
        w.hide();
        w.show();
        assert_eq!(w.hwnd(), None);
    }
}
