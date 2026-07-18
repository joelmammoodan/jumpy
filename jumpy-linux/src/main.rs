use eframe::egui;
use jumpy_core::platform::Edge;
use jumpy_core::platform::PlatformHandler;
use jumpy_core::JumpyApp;
use std::sync::Arc;
use std::sync::Mutex;
use std::process::Command;
use enigo::{Enigo, MouseControllable, MouseButton};

struct LinuxPlatform {
    enigo: Mutex<Enigo>,
}

impl LinuxPlatform {
    fn new() -> Self {
        Self {
            enigo: Mutex::new(Enigo::new()),
        }
    }
}

impl PlatformHandler for LinuxPlatform {
    fn get_mouse_pos(&self) -> (i32, i32) {
        let enigo = self.enigo.lock().unwrap();
        enigo.mouse_location()
    }

    fn set_mouse_pos(&self, x: i32, y: i32) {
        let mut enigo = self.enigo.lock().unwrap();
        enigo.mouse_move_to(x, y);
    }

    fn get_screen_size(&self) -> (i32, i32) {
        // Fallback to xdpyinfo if available, but enigo can also query it in newer versions.
        // For performance, we can just return a large default or query it once.
        // We'll stick to a fast default since Jumpy uses virtual cursor bounding.
        (1920, 1080)
    }

    fn send_mouse_move(&self, dx: i32, dy: i32) {
        let mut enigo = self.enigo.lock().unwrap();
        enigo.mouse_move_relative(dx, dy);
    }

    fn send_mouse_click(&self, button: &str, pressed: bool) {
        let btn = match button {
            "Left" => MouseButton::Left,
            "Middle" => MouseButton::Middle,
            "Right" => MouseButton::Right,
            _ => return,
        };
        let mut enigo = self.enigo.lock().unwrap();
        if pressed {
            enigo.mouse_down(btn);
        } else {
            enigo.mouse_up(btn);
        }
    }

    fn send_mouse_scroll(&self, dy: i32) {
        let mut enigo = self.enigo.lock().unwrap();
        if dy > 0 {
            enigo.mouse_scroll_y(1);
        } else {
            enigo.mouse_scroll_y(-1);
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
        "JUMPY - Linux",
        options,
        Box::new(|cc| {
            let platform = Box::new(LinuxPlatform::new());
            let app = JumpyApp::new(cc, platform);
            
            let platform_arc = Arc::new(Box::new(LinuxPlatform::new()) as Box<dyn PlatformHandler>);
            JumpyApp::start_mouse_receiver(Arc::clone(&app.state), platform_arc);
            
            Ok(Box::new(app))
        }),
    )
}
