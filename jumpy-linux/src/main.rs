use eframe::egui;
use jumpy_core::platform::PlatformHandler;
use jumpy_core::JumpyApp;
use std::sync::Arc;
use std::process::Command;

use std::io::Write;
use std::sync::Mutex;

struct LinuxPlatform {
    xdotool_stdin: Option<Mutex<std::process::ChildStdin>>,
}

impl LinuxPlatform {
    fn new() -> Self {
        let child = Command::new("xdotool")
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .spawn();
            
        let stdin = child.ok().and_then(|mut c| c.stdin.take()).map(Mutex::new);
        
        Self { xdotool_stdin: stdin }
    }
    
    fn send_cmd(&self, cmd: &str) {
        if let Some(stdin_mutex) = &self.xdotool_stdin {
            if let Ok(mut stdin) = stdin_mutex.lock() {
                let _ = writeln!(stdin, "{}", cmd);
                let _ = stdin.flush();
            }
        }
    }
}

impl PlatformHandler for LinuxPlatform {
    fn get_mouse_pos(&self) -> (i32, i32) {
        let output = Command::new("xdotool")
            .arg("getmouselocation")
            .arg("--shell")
            .output();
            
        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut x = 0;
            let mut y = 0;
            for line in stdout.lines() {
                if line.starts_with("X=") {
                    x = line[2..].parse().unwrap_or(0);
                } else if line.starts_with("Y=") {
                    y = line[2..].parse().unwrap_or(0);
                }
            }
            (x, y)
        } else {
            (0, 0)
        }
    }

    fn set_mouse_pos(&self, x: i32, y: i32) {
        self.send_cmd(&format!("mousemove {} {}", x, y));
    }

    fn get_screen_size(&self) -> (i32, i32) {
        let output = Command::new("xdpyinfo").output();
        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("dimensions:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let dims: Vec<&str> = parts[1].split('x').collect();
                        if dims.len() == 2 {
                            return (
                                dims[0].parse().unwrap_or(1920),
                                dims[1].parse().unwrap_or(1080)
                            );
                        }
                    }
                }
            }
        }
        (1920, 1080) // fallback
    }

    fn send_mouse_move(&self, dx: i32, dy: i32) {
        self.send_cmd(&format!("mousemove_relative -- {} {}", dx, dy));
    }

    fn send_mouse_click(&self, button: &str, pressed: bool) {
        let btn_num = match button {
            "Left" => "1",
            "Middle" => "2",
            "Right" => "3",
            _ => return,
        };
        let action = if pressed { "mousedown" } else { "mouseup" };
        self.send_cmd(&format!("{} {}", action, btn_num));
    }

    fn send_mouse_scroll(&self, dy: i32) {
        let btn_num = if dy > 0 { "4" } else { "5" };
        let count = dy.abs();
        for _ in 0..count {
            self.send_cmd(&format!("click {}", btn_num));
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
