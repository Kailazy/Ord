//! The main window's AppKit chrome. The native title bar is folded into the
//! content (`main.rs`): the toolbar row is the title bar, and the traffic
//! lights float over its left end, centred on it by [`set_titlebar_height`].

#[cfg(target_os = "macos")]
pub(crate) use imp::{main_window, set_titlebar_height, title_bar_double_click};

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_titlebar_height(_frame: &eframe::Frame, _height: f32) {}

#[cfg(not(target_os = "macos"))]
pub(crate) fn title_bar_double_click(zoomed: bool) -> Option<egui::ViewportCommand> {
    Some(egui::ViewportCommand::Maximized(!zoomed))
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSUserDefaults};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// The app's main `NSWindow`, via eframe's AppKit handle. Borrowed for the
    /// duration of the call only — never stashed, since eframe may recreate the
    /// view (same rule as `macos_drag`).
    pub(crate) fn main_window(frame: &eframe::Frame) -> Option<Retained<NSWindow>> {
        let handle = frame.window_handle().ok()?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else {
            return None;
        };
        let view: Retained<NSView> =
            unsafe { Retained::retain(h.ns_view.as_ptr() as *mut NSView)? };
        view.window()
    }

    /// What a double-click on the title bar does: the user's choice in System
    /// Settings › Desktop & Dock ("Double-click a window's title bar to"),
    /// minimize or nothing, else the zoom toggle (Maximize, Fill, unset).
    pub(crate) fn title_bar_double_click(zoomed: bool) -> Option<egui::ViewportCommand> {
        let action = unsafe {
            NSUserDefaults::standardUserDefaults()
                .stringForKey(&NSString::from_str("AppleActionOnDoubleClick"))
        };
        match action.map(|s| s.to_string()).as_deref() {
            Some("Minimize") => Some(egui::ViewportCommand::Minimized(true)),
            Some("None") => None,
            _ => Some(egui::ViewportCommand::Maximized(!zoomed)),
        }
    }

    /// Make the title bar `height` points tall with the traffic lights centred
    /// in it. AppKit lays the bar out for its own 32 pt strip, and again on
    /// every resize and fullscreen transition, so this runs each frame and
    /// moves a view only when it has drifted. The bar's views grow with the
    /// buttons: a button outside its parent's bounds takes no clicks. The
    /// content view underneath keeps its clicks wherever a button isn't.
    pub(crate) fn set_titlebar_height(frame: &eframe::Frame, height: f32) {
        let Some(window) = main_window(frame) else { return };
        let buttons = [
            NSWindowButton::NSWindowCloseButton,
            NSWindowButton::NSWindowMiniaturizeButton,
            NSWindowButton::NSWindowZoomButton,
        ]
        .map(|b| window.standardWindowButton(b));
        // The buttons' parent is the title bar view, inside its container at
        // the top of the window; both are laid out in points, y up.
        let Some(bar) = buttons[0].as_ref().and_then(|b| unsafe { b.superview() }) else {
            return;
        };
        let Some(container) = (unsafe { bar.superview() }) else { return };
        let height = f64::from(height);
        let window_h = window.frame().size.height;
        let want = NSRect::new(
            NSPoint::new(0.0, window_h - height),
            NSSize::new(container.frame().size.width, height),
        );
        if container.frame() != want {
            unsafe { container.setFrame(want) };
        }
        let bar_want = NSRect::new(NSPoint::new(0.0, 0.0), want.size);
        if bar.frame() != bar_want {
            unsafe { bar.setFrame(bar_want) };
        }
        for button in buttons.iter().flatten() {
            let f = button.frame();
            let y = (height - f.size.height) / 2.0;
            if (f.origin.y - y).abs() > 0.5 {
                unsafe { button.setFrameOrigin(NSPoint::new(f.origin.x, y)) };
            }
        }
    }
}
