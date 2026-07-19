use eframe::egui;
use jumpy_core::platform::PlatformHandler;
use jumpy_core::JumpyApp;
use std::sync::Arc;
use std::sync::Mutex;
use std::os::unix::io::AsRawFd;
use evdev::uinput::{VirtualDeviceBuilder, VirtualDevice};
use evdev::{AttributeSet, EventType, InputEvent, RelativeAxisType, Key};
use crossbeam_channel::{unbounded, Receiver, Sender};
use jumpy_core::network::MouseControlMsg;
use std::sync::atomic::{AtomicBool, Ordering};

struct LinuxPlatform {
    device: Option<Mutex<VirtualDevice>>,
    capturing: Mutex<Option<(Arc<AtomicBool>, Receiver<MouseControlMsg>)>>,
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
            Ok(dev) => Self { device: Some(Mutex::new(dev)), capturing: Mutex::new(None) },
            Err(e) => {
                println!("Error: Failed to create uinput device. You need permission to write to /dev/uinput. Error: {:?}", e);
                Self { device: None, capturing: Mutex::new(None) }
            }
        }
    }
}

impl PlatformHandler for LinuxPlatform {
    fn get_mouse_pos(&self) -> (i32, i32) {
        // Retrieve the current user's UID to find the Hyprland instance signature if running under sudo
        let uid = std::process::Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|_| "1000".to_string());
        
        // Pass the environment variables explicitly in case we are running under sudo
        let mut cmd = std::process::Command::new("hyprctl");
        cmd.arg("cursorpos");
        
        // Inherit or discover the hyprland instance if possible
        if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            cmd.env("HYPRLAND_INSTANCE_SIGNATURE", sig);
        }

        if let Ok(output) = cmd.output() {
            let s = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = s.trim().split(',').collect();
            if parts.len() == 2 {
                // Hyprland sometimes outputs floats like 123.45
                let px = parts[0].trim().parse::<f32>().map(|v| v as i32).or_else(|_| parts[0].trim().parse::<i32>());
                let py = parts[1].trim().parse::<f32>().map(|v| v as i32).or_else(|_| parts[1].trim().parse::<i32>());
                if let (Ok(x), Ok(y)) = (px, py) {
                    return (x, y);
                }
            }
        } else {
            // Print error periodically?
            // println!("Failed to execute hyprctl cursorpos");
        }
        (0, 0)
    }

    fn set_mouse_pos(&self, x: i32, y: i32) {
        let _ = std::process::Command::new("hyprctl")
            .arg("dispatch")
            .arg("movecursor")
            .arg(format!("{},{}", x, y))
            .output();
    }

    fn get_screen_size(&self) -> (i32, i32) {
        let mut cmd = std::process::Command::new("hyprctl");
        cmd.arg("monitors");
        if let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
            cmd.env("HYPRLAND_INSTANCE_SIGNATURE", sig);
        }
        
        if let Ok(output) = cmd.output() {
            let s = String::from_utf8_lossy(&output.stdout);
            // Example:
            // Monitor DP-1 (ID 0):
            // 2560x1440@144.00101 at 0x0
            for line in s.lines() {
                if let Some(pos) = line.find("x") {
                    if line.contains("@") && line.contains(" at ") {
                        let parts: Vec<&str> = line.trim().split_whitespace().collect();
                        if let Some(res_str) = parts.first() {
                            let res_parts: Vec<&str> = res_str.split('@').next().unwrap_or("").split('x').collect();
                            if res_parts.len() == 2 {
                                if let (Ok(w), Ok(h)) = (res_parts[0].parse::<i32>(), res_parts[1].parse::<i32>()) {
                                    return (w, h);
                                }
                            }
                        }
                    }
                }
            }
        }
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
    
    fn set_capture_mode(&self, active: bool, cx: i32, cy: i32) {
        let mut capt = self.capturing.lock().unwrap();
        if active {
            if capt.is_some() { return; }
            
            self.set_mouse_pos(cx, cy);
                
            let (tx, rx) = unbounded();
            let keep_running = Arc::new(AtomicBool::new(true));
            let kr = Arc::clone(&keep_running);
            
            *capt = Some((keep_running, rx));
            
            std::thread::spawn(move || {
                let mut grabbed_devices = Vec::new();
                if let Ok(entries) = std::fs::read_dir("/dev/input") {
                    for entry in entries.flatten() {
                        if let Ok(mut dev) = evdev::Device::open(entry.path()) {
                            if dev.supported_keys().map_or(false, |k| k.contains(evdev::Key::BTN_LEFT) || k.contains(evdev::Key::KEY_A)) {
                                if dev.grab().is_ok() {
                                    let fd = dev.as_raw_fd();
                                    unsafe {
                                        let flags = libc::fcntl(fd, libc::F_GETFL);
                                        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                                    }
                                    grabbed_devices.push(dev);
                                }
                            }
                        }
                    }
                }
                
                while kr.load(Ordering::SeqCst) {
                    for dev in grabbed_devices.iter_mut() {
                        if let Ok(events) = dev.fetch_events() {
                            for ev in events {
                                match ev.event_type() {
                                    EventType::RELATIVE => {
                                        if ev.code() == RelativeAxisType::REL_X.0 {
                                            tx.send(MouseControlMsg::Move { dx: ev.value() as f32, dy: 0.0 }).unwrap();
                                        } else if ev.code() == RelativeAxisType::REL_Y.0 {
                                            tx.send(MouseControlMsg::Move { dx: 0.0, dy: ev.value() as f32 }).unwrap();
                                        } else if ev.code() == RelativeAxisType::REL_WHEEL.0 {
                                            tx.send(MouseControlMsg::Scroll { dy: ev.value() as f32 }).unwrap();
                                        }
                                    },
                                    EventType::KEY => {
                                        let key_code = ev.code();
                                        let down = ev.value() != 0;
                                        if key_code == Key::BTN_LEFT.code() {
                                            tx.send(MouseControlMsg::Click { button: "Left".to_string(), pressed: down }).unwrap();
                                        } else if key_code == Key::BTN_RIGHT.code() {
                                            tx.send(MouseControlMsg::Click { button: "Right".to_string(), pressed: down }).unwrap();
                                        } else if key_code == Key::BTN_MIDDLE.code() {
                                            tx.send(MouseControlMsg::Click { button: "Middle".to_string(), pressed: down }).unwrap();
                                        } else {
                                            if key_code == Key::KEY_ESC.code() && down {
                                                kr.store(false, Ordering::SeqCst);
                                                tx.send(MouseControlMsg::ReturnControl).unwrap();
                                            } else {
                                                tx.send(MouseControlMsg::Key { key_code: key_code as u32, down }).unwrap();
                                            }
                                        }
                                    },
                                    _ => {}
                                }
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            });
        } else {
            if let Some((kr, _)) = capt.take() {
                kr.store(false, Ordering::SeqCst);
            }
        }
    }
    
    fn get_grabbed_events(&self) -> Vec<MouseControlMsg> {
        let mut events = Vec::new();
        if let Some((_, rx)) = &*self.capturing.lock().unwrap() {
            while let Ok(msg) = rx.try_recv() {
                events.push(msg);
            }
        }
        events
    }
    
    fn uses_polling_capture(&self) -> bool { false }
}

fn test_hyprctl_startup() {
    println!("--- DIAGNOSTIC: Testing hyprctl ---");
    let mut cmd1 = std::process::Command::new("hyprctl");
    cmd1.arg("cursorpos");
    match cmd1.output() {
        Ok(out) => println!("cursorpos output: {:?}", String::from_utf8_lossy(&out.stdout).trim()),
        Err(e) => println!("cursorpos error: {:?}", e),
    }
    
    let mut cmd2 = std::process::Command::new("hyprctl");
    cmd2.arg("monitors");
    match cmd2.output() {
        Ok(out) => {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines().take(3) {
                println!("monitors (first lines): {}", line);
            }
        },
        Err(e) => println!("monitors error: {:?}", e),
    }
    println!("-----------------------------------");
}

fn main() -> Result<(), eframe::Error> {
    test_hyprctl_startup();
    
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
