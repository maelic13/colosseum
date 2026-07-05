//! Native file dialogs parented to the main window.
//!
//! rfd's file pickers are real OS windows. Without an owner window Windows
//! positions them on the primary monitor (or wherever it likes), so on a
//! multi-monitor setup they can open on a different screen from Colosseum.
//! Parenting them to the main window makes the OS place them over it (same
//! monitor) and modal to the app.
//!
//! The window/display handles are captured once at startup (they live for the
//! whole program) and stashed in a main-thread `thread_local`, so every call
//! site — including free functions with no access to app state — can build a
//! parented dialog via [`file_dialog`].

use std::cell::RefCell;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};

/// The main window's raw handles, used to parent native dialogs.
#[derive(Clone, Copy)]
pub struct DialogParent {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

impl DialogParent {
    #[must_use]
    pub fn new(window: RawWindowHandle, display: RawDisplayHandle) -> Self {
        Self { window, display }
    }
}

impl HasWindowHandle for DialogParent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: these are the main window's handles; the window lives for the
        // entire program and outlives any dialog created from them.
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}

impl HasDisplayHandle for DialogParent {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: as above — the display connection outlives the dialog.
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

thread_local! {
    static DIALOG_PARENT: RefCell<Option<DialogParent>> = const { RefCell::new(None) };
}

/// Record the main window's handles (call once, at startup).
pub fn set_parent(parent: DialogParent) {
    DIALOG_PARENT.with(|p| *p.borrow_mut() = Some(parent));
}

/// A native file dialog parented to the main window when the handles are
/// known, so it opens on the same monitor as Colosseum. Use this in place of
/// `rfd::FileDialog::new()`.
#[must_use]
pub fn file_dialog() -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new();
    DIALOG_PARENT.with(|p| match &*p.borrow() {
        // `set_parent` copies the raw handles, so nothing is borrowed past this.
        Some(parent) => dialog.set_parent(parent),
        None => dialog,
    })
}
