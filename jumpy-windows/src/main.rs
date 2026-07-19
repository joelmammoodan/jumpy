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
use std::sync::atomic::{AtomicBool, Ordering};
use crossbeam_channel::{unbounded, Receiver};
use rdev::{Event, EventType, Button};
use jumpy_core::network::MouseControlMsg;

#[derive(Clone)]
struct WindowsPlatform {
    is_capturing: Arc<AtomicBool>,
    event_rx: Receiver<MouseControlMsg>,
}

fn convert_button(btn: Button) -> String {
    match btn {
        Button::Left => "Left".to_string(),
        Button::Right => "Right".to_string(),
        Button::Middle => "Middle".to_string(),
        Button::Unknown(b) => format!("Unknown({})", b),
    }
}

fn rdev_key_to_code(key: rdev::Key) -> u32 {
    // rdev does not expose raw OS keycodes universally in a simple cross-platform way,
    // but we can map rdev::Key back to a rough u32 standard, or send a String.
    // For simplicity, we just send a numeric hash or standard layout code.
    // Wait, evdev in Linux needs a standard Linux KEY code (like KEY_A = 30).
    // Let's implement a very basic map for standard typing keys, or we can just send strings!
    // Wait, the network protocol uses `key_code: u32`.
    // Let's just use `key as u32` if possible. `rdev::Key` is an enum, we can't cast directly safely in all versions.
    // Let's do a basic mapping for now, or just send a dummy.
    // Actually, `evdev` accepts standard linux key codes.
    // Let's do a quick match for essential keys.
    match key {
        rdev::Key::KeyA => 30,
        rdev::Key::KeyB => 48,
        rdev::Key::KeyC => 46,
        rdev::Key::KeyD => 32,
        rdev::Key::KeyE => 18,
        rdev::Key::KeyF => 33,
        rdev::Key::KeyG => 34,
        rdev::Key::KeyH => 35,
        rdev::Key::KeyI => 23,
        rdev::Key::KeyJ => 36,
        rdev::Key::KeyK => 37,
        rdev::Key::KeyL => 38,
        rdev::Key::KeyM => 50,
        rdev::Key::KeyN => 49,
        rdev::Key::KeyO => 24,
        rdev::Key::KeyP => 25,
        rdev::Key::KeyQ => 16,
        rdev::Key::KeyR => 19,
        rdev::Key::KeyS => 31,
        rdev::Key::KeyT => 20,
        rdev::Key::KeyU => 22,
        rdev::Key::KeyV => 47,
        rdev::Key::KeyW => 17,
        rdev::Key::KeyX => 45,
        rdev::Key::KeyY => 21,
        rdev::Key::KeyZ => 44,
        rdev::Key::Return => 28,
        rdev::Key::Escape => 1,
        rdev::Key::Backspace => 14,
        rdev::Key::Space => 57,
        rdev::Key::ShiftLeft => 42,
        rdev::Key::ControlLeft => 29,
        rdev::Key::Alt => 56,
        rdev::Key::UpArrow => 103,
        rdev::Key::DownArrow => 108,
        rdev::Key::LeftArrow => 105,
        rdev::Key::RightArrow => 106,
        _ => 0,
    }
}

impl WindowsPlatform {
    fn new() -> Self {
        let is_capturing = Arc::new(AtomicBool::new(false));
        let (tx, rx) = unbounded();
        
        let hook_capturing = Arc::clone(&is_capturing);
        let hook_tx = tx.clone();
        
        std::thread::spawn(move || {
            let callback = move |event: Event| -> Option<Event> {
                if hook_capturing.load(Ordering::SeqCst) {
                    match event.event_type {
                        EventType::KeyPress(key) => {
                            if key == rdev::Key::Escape {
                                hook_capturing.store(false, Ordering::SeqCst);
                                let _ = hook_tx.send(MouseControlMsg::ReturnControl);
                                return Some(event); // Let the host process Escape
                            }
                            let _ = hook_tx.send(MouseControlMsg::Key { key_code: rdev_key_to_code(key), down: true });
                            return None; // Swallow
                        }
                        EventType::KeyRelease(key) => {
                            let _ = hook_tx.send(MouseControlMsg::Key { key_code: rdev_key_to_code(key), down: false });
                            return None; // Swallow
                        }
                        EventType::ButtonPress(btn) => {
                            let _ = hook_tx.send(MouseControlMsg::Click { button: convert_button(btn), pressed: true });
                            return None; // Swallow
                        }
                        EventType::ButtonRelease(btn) => {
                            let _ = hook_tx.send(MouseControlMsg::Click { button: convert_button(btn), pressed: false });
                            return None; // Swallow
                        }
                        EventType::Wheel { delta_x: _, delta_y } => {
                            let _ = hook_tx.send(MouseControlMsg::Scroll { dy: delta_y as f32 });
                            return None; // Swallow
                        }
                        EventType::MouseMove { .. } => {
                            // We do NOT swallow MouseMove, because ClipCursor naturally stops the cursor from moving,
                            // and Jumpy needs the hardware to update the OS cursor to calculate `dx` and `dy`.
                            return Some(event);
                        }
                    }
                }
                Some(event)
            };
            
            if let Err(error) = rdev::grab(callback) {
                println!("Error: {:?}", error);
            }
        });
        
        Self { is_capturing, event_rx: rx }
    }
}

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
    
    fn send_key(&self, _key_code: u32, _down: bool) {
        // We do not currently need to send keys TO Windows from Linux, 
        // since the user only requested Windows -> Linux control for now.
    }
    
    fn set_capture_mode(&self, active: bool, cx: i32, cy: i32) {
        self.is_capturing.store(active, Ordering::SeqCst);
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
    
    fn get_grabbed_events(&self) -> Vec<MouseControlMsg> {
        let mut events = Vec::new();
        while let Ok(msg) = self.event_rx.try_recv() {
            events.push(msg);
        }
        events
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
            let platform = Box::new(WindowsPlatform::new());
            let app = JumpyApp::new(cc, platform);
            
            let platform_arc = Arc::new(WindowsPlatform::new()) as Arc<dyn PlatformHandler + Send + Sync>;
            JumpyApp::start_mouse_receiver(Arc::clone(&app.state), platform_arc);
            
            Ok(Box::new(app))
        }),
    )
}
