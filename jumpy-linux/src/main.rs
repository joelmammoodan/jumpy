use eframe::egui;
use jumpy_core::platform::PlatformHandler;
use jumpy_core::JumpyApp;
use std::sync::Arc;
use std::sync::Mutex;
use evdev::uinput::{VirtualDeviceBuilder, VirtualDevice};
use evdev::{AttributeSet, EventType, InputEvent, RelativeAxisType, Key};

struct LinuxPlatform {
    device: Option<Mutex<VirtualDevice>>,
}

impl LinuxPlatform {
    fn new() -> Self {
        let mut keys = AttributeSet::new();
        keys.insert(Key::BTN_LEFT);
        keys.insert(Key::BTN_RIGHT);
        keys.insert(Key::BTN_MIDDLE);
        
        // Register standard keyboard keys (1 to 255)
        for i in 1..255 {
            keys.insert(Key::new(i));
        }

        let mut rel_axes = AttributeSet::new();
        rel_axes.insert(RelativeAxisType::REL_X);
        rel_axes.insert(RelativeAxisType::REL_Y);
        rel_axes.insert(RelativeAxisType::REL_WHEEL);

        let device = match VirtualDeviceBuilder::new() {
            Ok(builder) => builder
                .name("Jumpy Virtual Mouse")
                .with_keys(&keys).unwrap()
                .with_relative_axes(&rel_axes).unwrap()
                .build(),
            Err(e) => {
                println!("Error: Failed to create VirtualDeviceBuilder: {:?}", e);
                Err(e)
            }
        };
        
        match device {
            Ok(dev) => Self { device: Some(Mutex::new(dev)) },
            Err(e) => {
                println!("Error: Failed to create uinput device. You need permission to write to /dev/uinput. Error: {:?}", e);
                Self { device: None }
            }
        }
    }
}

impl PlatformHandler for LinuxPlatform {
    fn get_mouse_pos(&self) -> (i32, i32) {
        // Wayland blocks getting global coords anyway. Returning (0,0) is fine for the client receiver.
        (0, 0)
    }

    fn set_mouse_pos(&self, _x: i32, _y: i32) {
        // Not used by the client receiver
    }

    fn get_screen_size(&self) -> (i32, i32) {
        (1920, 1080)
    }

    fn send_mouse_move(&self, dx: i32, dy: i32) {
        if let Some(dev_mutex) = &self.device {
            if let Ok(mut dev) = dev_mutex.lock() {
                let _ = dev.emit(&[
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_X.0, dx),
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_Y.0, dy),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }
        }
    }

    fn send_mouse_click(&self, button: &str, pressed: bool) {
        let key = match button {
            "Left" => Key::BTN_LEFT,
            "Middle" => Key::BTN_MIDDLE,
            "Right" => Key::BTN_RIGHT,
            _ => return,
        };
        let value = if pressed { 1 } else { 0 };
        if let Some(dev_mutex) = &self.device {
            if let Ok(mut dev) = dev_mutex.lock() {
                let _ = dev.emit(&[
                    InputEvent::new(EventType::KEY, key.code(), value),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }
        }
    }

    fn send_mouse_scroll(&self, dy: i32) {
        // Jumpy sends a positive dy for scrolling up, negative for down.
        // uinput REL_WHEEL expects positive for scrolling up.
        if let Some(dev_mutex) = &self.device {
            if let Ok(mut dev) = dev_mutex.lock() {
                let _ = dev.emit(&[
                    InputEvent::new(EventType::RELATIVE, RelativeAxisType::REL_WHEEL.0, dy),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
            }
        }
    }

    fn send_key(&self, key_code: u32, down: bool) {
        let value = if down { 1 } else { 0 };
        if let Some(dev_mutex) = &self.device {
            if let Ok(mut dev) = dev_mutex.lock() {
                let _ = dev.emit(&[
                    InputEvent::new(EventType::KEY, key_code as u16, value),
                    InputEvent::new(EventType::SYNCHRONIZATION, 0, 0),
                ]);
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
        "JUMPY - Linux",
        options,
        Box::new(|cc| {
            let platform = Box::new(LinuxPlatform::new());
            let app = JumpyApp::new(cc, platform);
            
            let platform_arc = Arc::new(LinuxPlatform::new()) as Arc<dyn PlatformHandler + Send + Sync>;
            JumpyApp::start_mouse_receiver(Arc::clone(&app.state), platform_arc);
            
            Ok(Box::new(app))
        }),
    )
}
