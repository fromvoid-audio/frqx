//! Capture the live hosted GPUI surface for release review.

#![allow(unexpected_cfgs)]

#[cfg(target_os = "macos")]
fn main() {
    use cocoa::appkit::{NSApp, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
    use cocoa::base::{id, nil};
    use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize};
    use frqx::gui_gpui::{
        new_dense_screenshot_gui, new_playing_note_screenshot_gui, new_screenshot_gui,
        new_scrolled_screenshot_gui, HEIGHT, WIDTH,
    };
    use image::{ImageFormat, RgbaImage};
    use objc::{sel, sel_impl};
    use raw_window_handle::{AppKitWindowHandle, RawWindowHandle};
    use std::ptr::NonNull;
    use std::thread;
    use std::time::Duration;

    unsafe {
        let pool = NSAutoreleasePool::new(nil);
        let _ = NSApp();
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(WIDTH), f64::from(HEIGHT)),
        );
        let window: id = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
            frame,
            NSWindowStyleMask::NSBorderlessWindowMask,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        );
        let view: id = NSView::alloc(nil).initWithFrame_(frame);
        window.setContentView_(view);
        window.makeKeyAndOrderFront_(nil);
        let mut parent = AppKitWindowHandle::empty();
        parent.ns_view = NonNull::new(view as *mut _).expect("fixture view").as_ptr();
        let output = std::env::var_os("TOYBOX_UI_SCREENSHOT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("target/ui-screenshots"))
            .join("frqx");
        std::fs::create_dir_all(&output).expect("screenshot directory");
        {
            let mut gui = new_screenshot_gui();
            gui.set_parent_raw(RawWindowHandle::AppKit(parent));
            assert!(gui.open(), "GPUI editor failed to open");
            for _ in 0..12 {
                gui.pump();
                thread::sleep(Duration::from_millis(5));
            }
            let (width, height, pixels) = gui.capture_rgba().expect("GPUI capture");
            let image = RgbaImage::from_raw(width, height, pixels).expect("valid RGBA capture");
            image
                .save_with_format(output.join("all-views-1600x1000.png"), ImageFormat::Png)
                .expect("write screenshot");
            gui.close();
        }
        {
            let mut gui = new_playing_note_screenshot_gui();
            gui.set_parent_raw(RawWindowHandle::AppKit(parent));
            assert!(gui.open(), "GPUI MIDI screenshot editor failed to open");
            for _ in 0..12 {
                gui.pump();
                thread::sleep(Duration::from_millis(5));
            }
            let (width, height, pixels) = gui.capture_rgba().expect("GPUI MIDI capture");
            let image = RgbaImage::from_raw(width, height, pixels).expect("valid RGBA capture");
            image
                .save_with_format(output.join("playing-note-1600x1000.png"), ImageFormat::Png)
                .expect("write MIDI screenshot");
            gui.close();
        }
        {
            let mut gui = new_dense_screenshot_gui();
            gui.set_parent_raw(RawWindowHandle::AppKit(parent));
            assert!(gui.open(), "GPUI 48-note editor failed to open");
            for _ in 0..12 {
                gui.pump();
                thread::sleep(Duration::from_millis(5));
            }
            let (width, height, pixels) = gui.capture_rgba().expect("GPUI 48-note capture");
            let image = RgbaImage::from_raw(width, height, pixels).expect("valid RGBA capture");
            image
                .save_with_format(output.join("dense-scale-1600x1000.png"), ImageFormat::Png)
                .expect("write 48-note screenshot");
            gui.close();
        }
        {
            let mut gui = new_scrolled_screenshot_gui();
            gui.set_parent_raw(RawWindowHandle::AppKit(parent));
            assert!(gui.open(), "GPUI scrolled editor failed to open");
            for _ in 0..12 {
                gui.pump();
                thread::sleep(Duration::from_millis(5));
            }
            let (width, height, pixels) = gui.capture_rgba().expect("GPUI scrolled capture");
            RgbaImage::from_raw(width, height, pixels)
                .expect("valid RGBA capture")
                .save_with_format(
                    output.join("scrolled-scale-1600x1000.png"),
                    ImageFormat::Png,
                )
                .expect("write scrolled screenshot");
            gui.close();
        }
        window.setContentView_(nil);
        let _: () = objc::msg_send![view, release];
        let _: () = objc::msg_send![window, release];
        pool.drain();
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The GPUI screenshot runner requires macOS");
}
