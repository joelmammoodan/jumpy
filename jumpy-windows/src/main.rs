use eframe::egui;
use jumpy_core::platform::PlatformHandler;
use jumpy_core::JumpyApp;
use std::sync::Arc;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_WHEEL,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, ClipCursor};
use windows_sys::Win32::Foundation::{POINT, RECT};
use std::mem::{size_of, zeroed};

struct WindowsPlatform;

impl PlatformHandler for WindowsPlatform {
    fn get_mouse_pos(&self) -> (i32, i32) {
        unsafe {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            (pt.x, pt.y)
        }
    }

    fn set_mouse_pos(&self, x: i32, y: i32) {
        unsafe {
            SetCursorPos(x, y);
        }
    }

    fn get_screen_size(&self) -> (i32, i32) {
        unsafe {
            let w = GetSystemMetrics(SM_CXSCREEN);
            let h = GetSystemMetrics(SM_CYSCREEN);
            (w, h)
        }
    }

    fn send_mouse_move(&self, dx: i32, dy: i32) {
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_MOUSE;
            let mut mi: MOUSEINPUT = zeroed();
            mi.dx = dx;
            mi.dy = dy;
            mi.dwFlags = MOUSEEVENTF_MOVE;
            input.Anonymous.mi = mi;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }

    fn send_mouse_click(&self, button: &str, pressed: bool) {
        let flags = match (button, pressed) {
            ("Left", true) => MOUSEEVENTF_LEFTDOWN,
            ("Left", false) => MOUSEEVENTF_LEFTUP,
            ("Right", true) => MOUSEEVENTF_RIGHTDOWN,
            ("Right", false) => MOUSEEVENTF_RIGHTUP,
            ("Middle", true) => MOUSEEVENTF_MIDDLEDOWN,
            ("Middle", false) => MOUSEEVENTF_MIDDLEUP,
            _ => return,
        };
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_MOUSE;
            let mut mi: MOUSEINPUT = zeroed();
            mi.dwFlags = flags;
            input.Anonymous.mi = mi;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }

    fn send_mouse_scroll(&self, dy: i32) {
        unsafe {
            let mut input: INPUT = zeroed();
            input.r#type = INPUT_MOUSE;
            let mut mi: MOUSEINPUT = zeroed();
            mi.mouseData = (dy * 120) as u32;
            mi.dwFlags = MOUSEEVENTF_WHEEL;
            input.Anonymous.mi = mi;
            SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
        }
    }
    
    fn set_capture_mode(&self, active: bool, cx: i32, cy: i32) {
        unsafe {
            if active {
                SetCursorPos(cx, cy);
                
                let rect = RECT {
                    left: cx - 10,
                    top: cy - 10,
                    right: cx + 10,
                    bottom: cy + 10,
                };
                ClipCursor(&rect);
            } else {
                ClipCursor(std::ptr::null());
            }
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 520.0])
            .with_min_inner_size([680.0, 480.0])
            .with_resizable(true),
        ..Default::default()
    };

    eframe::run_native(
        "JUMPY - Windows",
        options,
        Box::new(|cc| {
            let platform = Box::new(WindowsPlatform);
            let app = JumpyApp::new(cc, platform);
            
            // Start the receiver in background
            let platform_arc = Arc::new(WindowsPlatform) as Arc<dyn PlatformHandler + Send + Sync>;
            JumpyApp::start_mouse_receiver(Arc::clone(&app.state), platform_arc);
            
            Ok(Box::new(app))
        }),
    )
}
