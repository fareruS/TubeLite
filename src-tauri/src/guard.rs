//! IPC guard: reject calls from any webview whose label is not an
//! approved UI window. Scope capabilities per window; each window gets
//! the minimum permissions.
//!
//! The official IFrame mode (Phase 6, OfficialSource) runs in a dedicated,
//! lazily created webview with NO IPC access at all — it is not in this list.

/// Labels allowed to invoke Tauri commands.
pub const APPROVED_UI_WINDOWS: &[&str] = &["main", "mini"];

pub fn is_approved_window(label: &str) -> bool {
    APPROVED_UI_WINDOWS.contains(&label)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guard_allows_main_and_mini_only() {
        assert!(is_approved_window("main"));
        assert!(is_approved_window("mini"));
        assert!(!is_approved_window("iframe-player"));
        assert!(!is_approved_window(""));
        assert!(!is_approved_window("main-evil"));
    }
}
