/*
===============================================================================
                JUMPY - LAN MOUSE & DISCOVERY (MATERIAL 3 EDITION)
===============================================================================
*/

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use eframe::egui;

// --- Windows-Specific Input Simulation ---
#[cfg(windows)]
fn send_mouse_move(dx: i32, dy: i32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    use std::mem::{size_of, zeroed};

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

#[cfg(windows)]
fn send_mouse_click(button: &str, pressed: bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT,
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    };
    use std::mem::{size_of, zeroed};

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

#[cfg(windows)]
fn send_mouse_scroll(dy: i32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_WHEEL,
    };
    use std::mem::{size_of, zeroed};

    let wheel_delta = dy * 120;

    unsafe {
        let mut input: INPUT = zeroed();
        input.r#type = INPUT_MOUSE;

        let mut mi: MOUSEINPUT = zeroed();
        mi.mouseData = wheel_delta as u32;
        mi.dwFlags = MOUSEEVENTF_WHEEL;

        input.Anonymous.mi = mi;
        SendInput(1, &input as *const INPUT, size_of::<INPUT>() as i32);
    }
}

// --- Linux-Specific Input Simulation (via xdotool) ---
#[cfg(target_os = "linux")]
fn send_mouse_move(dx: i32, dy: i32) {
    let _ = std::process::Command::new("xdotool")
        .args(&["mousemove_relative", "--", &dx.to_string(), &dy.to_string()])
        .spawn();
}

#[cfg(target_os = "linux")]
fn send_mouse_click(button: &str, pressed: bool) {
    let btn_num = match button {
        "Left" => "1",
        "Middle" => "2",
        "Right" => "3",
        _ => return,
    };
    let action = if pressed { "mousedown" } else { "mouseup" };
    let _ = std::process::Command::new("xdotool")
        .args(&[action, btn_num])
        .spawn();
}

#[cfg(target_os = "linux")]
fn send_mouse_scroll(dy: i32) {
    let btn_num = if dy > 0 { "4" } else { "5" };
    let count = dy.abs();
    for _ in 0..count {
        let _ = std::process::Command::new("xdotool")
            .args(&["click", btn_num])
            .spawn();
    }
}

// Fallback stubs for other OS platforms
#[cfg(not(any(windows, target_os = "linux")))]
fn send_mouse_move(_dx: i32, _dy: i32) {}
#[cfg(not(any(windows, target_os = "linux")))]
fn send_mouse_click(_button: &str, _pressed: bool) {}
#[cfg(not(any(windows, target_os = "linux")))]
fn send_mouse_scroll(_dy: i32) {}

// --- Color Conversion Helper (HSL to RGB) ---
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> egui::Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    egui::Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

// --- Peer Discovery & Communication Structures ---

#[derive(Serialize, Deserialize, Clone, Debug)]
struct DiscoveryMessage {
    id: String,
    name: String,
    ip: String,
    mouse_port: u16,
    device_type: String,
}

#[derive(Clone, Debug)]
struct PeerDevice {
    id: String,
    name: String,
    ip: String,
    mouse_port: u16,
    device_type: String,
    last_seen: Instant,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
enum MouseControlMsg {
    Move { dx: f32, dy: f32 },
    Click { button: String, pressed: bool },
    Scroll { dy: f32 },
}

struct AppState {
    local_id: String,
    local_name: String,
    local_ip: String,
    mouse_port: u16,
    discovery_enabled: bool,
    is_receiver_active: bool,
    peers: HashMap<String, PeerDevice>,
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Tab {
    Receive,
    Send,
    LanMouse,
    Settings,
}

struct JumpyApp {
    state: Arc<Mutex<AppState>>,
    current_tab: Tab,
    selected_peer_id: Option<String>,
    sensitivity: f32,
    client_socket: UdpSocket,
    
    // Smooth Input Interpolation
    target_dx: f32,
    target_dy: f32,
    current_dx: f32,
    current_dy: f32,
    
    accum_x: f32,
    accum_y: f32,
    accum_scroll: f32,
    accent_hue: f32, 
}

fn generate_friendly_name() -> String {
    let adjectives = vec![
        "Swift", "Sleek", "Warm", "Cool", "Quiet", "Bright", "Quick", "Calm", "Gentle", "Jolly",
        "Happy", "Clever", "Kind", "Brave", "Witty", "Fluffy", "Nimble", "Eager", "Polite", "Peppy"
    ];
    let animals = vec![
        "Fox", "Owl", "Koala", "Otter", "Panda", "Rabbit", "Deer", "Squirrel", "Falcon", "Eagle",
        "Dolphin", "Turtle", "Beaver", "Hedgehog", "Badger", "Puffin", "Gecko", "Cheetah", "Tiger", "Bear"
    ];
    let mut rng = rand::thread_rng();
    let adj = adjectives.choose(&mut rng).copied().unwrap_or("Swift");
    let anim = animals.choose(&mut rng).copied().unwrap_or("Mouse");
    format!("{} {}", adj, anim)
}

fn get_local_ip() -> String {
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(local_addr) = socket.local_addr() {
                return local_addr.ip().to_string();
            }
        }
    }
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

impl JumpyApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Material 3 Dark Palette Base Configuration
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(10, 11, 16); // Deep Obsidian background
        visuals.window_fill = egui::Color32::from_rgb(18, 20, 27); // Surface container
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(120, 140, 255);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(100, 120, 240);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(26, 29, 39);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(18, 20, 27);
        cc.egui_ctx.set_visuals(visuals);

        let local_id = rand::thread_rng().gen_range(100000..999999).to_string();
        let local_name = generate_friendly_name();
        let local_ip = get_local_ip();

        let state = Arc::new(Mutex::new(AppState {
            local_id,
            local_name,
            local_ip,
            mouse_port: 0,
            discovery_enabled: true,
            is_receiver_active: true,
            peers: HashMap::new(),
        }));

        // --- Spawn Discovery Broadcaster ---
        let b_state = Arc::clone(&state);
        std::thread::spawn(move || {
            let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind broadcast socket");
            socket.set_broadcast(true).expect("Failed to enable broadcasting");
            loop {
                std::thread::sleep(Duration::from_millis(1500));
                let msg = {
                    let s = b_state.lock().unwrap();
                    if !s.discovery_enabled {
                        continue;
                    }
                    DiscoveryMessage {
                        id: s.local_id.clone(),
                        name: s.local_name.clone(),
                        ip: s.local_ip.clone(),
                        mouse_port: s.mouse_port,
                        device_type: "Desktop".to_string(),
                    }
                };

                if let Ok(serialized) = serde_json::to_string(&msg) {
                    let _ = socket.send_to(serialized.as_bytes(), "255.255.255.255:52637");
                }
            }
        });

        // --- Spawn Discovery Listener ---
        let l_state = Arc::clone(&state);
        std::thread::spawn(move || {
            let socket_addr: std::net::SocketAddr = "0.0.0.0:52637".parse().unwrap();
            let socket_res = (|| -> Result<UdpSocket, Box<dyn std::error::Error>> {
                use socket2::{Socket, Domain, Type, Protocol, SockAddr};
                let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
                socket.set_reuse_address(true)?;

                #[cfg(not(windows))]
                socket.set_reuse_port(true)?;
                
                socket.bind(&SockAddr::from(socket_addr))?;
                Ok(socket.into())
            })();

            let socket = match socket_res {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to bind discovery socket: {:?}", e);
                    return;
                }
            };

            let mut buf = [0u8; 1024];
            loop {
                if let Ok((amt, _src)) = socket.recv_from(&mut buf) {
                    if let Ok(msg) = serde_json::from_slice::<DiscoveryMessage>(&buf[..amt]) {
                        let mut s = l_state.lock().unwrap();
                        if msg.id != s.local_id {
                            let peer = PeerDevice {
                                id: msg.id.clone(),
                                name: msg.name,
                                ip: msg.ip,
                                mouse_port: msg.mouse_port,
                                device_type: msg.device_type,
                                last_seen: Instant::now(),
                            };
                            s.peers.insert(peer.id.clone(), peer);
                        }
                    }
                }
            }
        });

        // --- Spawn Peer Pruning Thread ---
        let p_state = Arc::clone(&state);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let mut s = p_state.lock().unwrap();
                let now = Instant::now();
                s.peers.retain(|_, peer| {
                    now.duration_since(peer.last_seen) < Duration::from_secs(5)
                });
            }
        });

        // --- Bind Mouse Receiver (Server) ---
        let (mouse_socket, bound_port) = {
            if let Ok(socket) = UdpSocket::bind("0.0.0.0:52638") {
                (socket, 52638)
            } else {
                let socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind random mouse port");
                let port = socket.local_addr().unwrap().port();
                (socket, port)
            }
        };

        {
            let mut s = state.lock().unwrap();
            s.mouse_port = bound_port;
        }

        // --- Spawn Mouse Event Processor ---
        let m_state = Arc::clone(&state);
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                if let Ok((amt, _src)) = mouse_socket.recv_from(&mut buf) {
                    let is_active = {
                        let s = m_state.lock().unwrap();
                        s.is_receiver_active
                    };
                    if !is_active {
                        continue;
                    }
                    if let Ok(msg) = serde_json::from_slice::<MouseControlMsg>(&buf[..amt]) {
                        match msg {
                            MouseControlMsg::Move { dx, dy } => {
                                send_mouse_move(dx as i32, dy as i32);
                            }
                            MouseControlMsg::Click { button, pressed } => {
                                send_mouse_click(&button, pressed);
                            }
                            MouseControlMsg::Scroll { dy } => {
                                send_mouse_scroll(dy as i32);
                            }
                        }
                    }
                }
            }
        });

        let client_socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind client sending socket");

        Self {
            state,
            current_tab: Tab::Receive,
            selected_peer_id: None,
            sensitivity: 1.2,
            client_socket,
            target_dx: 0.0,
            target_dy: 0.0,
            current_dx: 0.0,
            current_dy: 0.0,
            accum_x: 0.0,
            accum_y: 0.0,
            accum_scroll: 0.0,
            accent_hue: 265.0, // Material 3 Muted Royal Purple default
        }
    }

    fn primary_accent(&self) -> egui::Color32 {
        hsl_to_rgb(self.accent_hue, 0.75, 0.62)
    }

    fn hover_accent(&self) -> egui::Color32 {
        hsl_to_rgb(self.accent_hue, 0.80, 0.72)
    }

    fn selection_tint(&self) -> egui::Color32 {
        hsl_to_rgb(self.accent_hue, 0.45, 0.16)
    }

    fn send_mouse_msg(&self, msg: MouseControlMsg) {
        if let Some(peer_id) = &self.selected_peer_id {
            let peer_opt = {
                let s = self.state.lock().unwrap();
                s.peers.get(peer_id).cloned()
            };
            if let Some(peer) = peer_opt {
                if let Ok(serialized) = serde_json::to_string(&msg) {
                    let target = format!("{}:{}", peer.ip, peer.mouse_port);
                    let _ = self.client_socket.send_to(serialized.as_bytes(), target);
                }
            }
        }
    }

    fn show_receive_tab(&mut self, ui: &mut egui::Ui) {
        let (local_name, local_ip, is_active, discovery_enabled, mouse_port) = {
            let s = self.state.lock().unwrap();
            (
                s.local_name.clone(),
                s.local_ip.clone(),
                s.is_receiver_active,
                s.discovery_enabled,
                s.mouse_port,
            )
        };

        let primary = self.primary_accent();

        ui.add_space(10.0);
        
        // Large Material 3 Rounded Hero Card
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(18, 20, 27))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 31, 41)))
            .rounding(24.0)
            .inner_margin(32.0)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    let size = egui::vec2(140.0, 140.0);
                    let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let center = rect.center();

                    // Modern pulsing halo animation
                    if discovery_enabled {
                        let time = ui.input(|i| i.time);
                        let phase = (time) % 2.0;
                        let progress = phase / 2.0;
                        let radius = 30.0 + progress as f32 * 45.0;
                        let alpha = 0.25 * (1.0 - progress as f32);
                        let color = egui::Color32::from_rgba_unmultiplied(
                            primary.r(),
                            primary.g(),
                            primary.b(),
                            (alpha * 255.0) as u8,
                        );
                        ui.painter().circle_filled(center, radius, color);
                    }

                    let main_color = if discovery_enabled {
                        primary
                    } else {
                        egui::Color32::from_rgb(60, 63, 75)
                    };

                    ui.painter().circle_filled(center, 32.0, main_color);
                    ui.painter().text(
                        center,
                        egui::Align2::CENTER_CENTER,
                        "🖥",
                        egui::FontId::proportional(26.0),
                        egui::Color32::WHITE,
                    );

                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(local_name)
                            .font(egui::FontId::proportional(24.0))
                            .color(egui::Color32::WHITE)
                            .strong(),
                    );
                    
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("IP: {}", local_ip))
                            .font(egui::FontId::proportional(14.0))
                            .color(egui::Color32::from_rgb(140, 145, 160)),
                    );
                    ui.label(
                        egui::RichText::new(format!("Inbound Port: {}", mouse_port))
                            .font(egui::FontId::proportional(12.0))
                            .color(egui::Color32::from_rgb(95, 100, 115)),
                    );
                });
            });

        ui.add_space(20.0);

        // Control Sliders / Checkboxes
        ui.columns(2, |cols| {
            cols[0].vertical(|ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 20, 27))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 31, 41)))
                    .rounding(16.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("LAN Discovery").color(egui::Color32::WHITE).strong());
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Show device to others on local interfaces.").color(egui::Color32::from_rgb(120, 120, 130)).font(egui::FontId::proportional(11.0)));
                        ui.add_space(12.0);
                        let mut disc = discovery_enabled;
                        if ui.add(egui::Checkbox::new(&mut disc, "Broadcast Presence")).changed() {
                            let mut s = self.state.lock().unwrap();
                            s.discovery_enabled = disc;
                        }
                    });
            });

            cols[1].vertical(|ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 20, 27))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 31, 41)))
                    .rounding(16.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Input Receiver").color(egui::Color32::WHITE).strong());
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Permit external remote nodes to control pointer.").color(egui::Color32::from_rgb(120, 120, 130)).font(egui::FontId::proportional(11.0)));
                        ui.add_space(12.0);
                        let mut recv = is_active;
                        if ui.add(egui::Checkbox::new(&mut recv, "Receive Events")).changed() {
                            let mut s = self.state.lock().unwrap();
                            s.is_receiver_active = recv;
                        }
                    });
            });
        });
    }

    fn show_send_tab(&mut self, ui: &mut egui::Ui) {
        let peers = {
            let s = self.state.lock().unwrap();
            s.peers.values().cloned().collect::<Vec<PeerDevice>>()
        };

        let primary = self.primary_accent();
        let selection_bg = self.selection_tint();

        ui.add_space(10.0);

        if peers.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.spinner();
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new("Scanning network for active nodes...")
                        .color(egui::Color32::from_rgb(140, 145, 160))
                        .font(egui::FontId::proportional(15.0)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Ensure sibling instances have 'LAN Discovery' enabled.")
                        .color(egui::Color32::from_rgb(90, 95, 110))
                        .font(egui::FontId::proportional(12.0)),
                );
            });
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for peer in peers {
                    let is_selected = self.selected_peer_id.as_ref() == Some(&peer.id);
                    let bg_fill = if is_selected { selection_bg } else { egui::Color32::from_rgb(18, 20, 27) };
                    let border_color = if is_selected { primary } else { egui::Color32::from_rgb(28, 31, 41) };

                    let response = egui::Frame::none()
                        .fill(bg_fill)
                        .stroke(egui::Stroke::new(1.0, border_color))
                        .inner_margin(16.0)
                        .rounding(16.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (avatar_rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                                let avatar_bg = if is_selected { primary } else { egui::Color32::from_rgb(32, 35, 47) };
                                ui.painter().circle_filled(avatar_rect.center(), 20.0, avatar_bg);
                                ui.painter().text(
                                    avatar_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "🖥",
                                    egui::FontId::proportional(18.0),
                                    egui::Color32::WHITE,
                                );

                                ui.add_space(12.0);

                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&peer.name)
                                            .color(egui::Color32::WHITE)
                                            .font(egui::FontId::proportional(16.0))
                                            .strong(),
                                    );
                                    ui.label(
                                        egui::RichText::new(format!("Address: {}", peer.ip))
                                            .color(egui::Color32::from_rgb(130, 135, 150))
                                            .font(egui::FontId::proportional(12.0)),
                                    );
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if is_selected {
                                        let btn = ui.add(egui::Button::new(
                                            egui::RichText::new("Disconnect")
                                                .color(egui::Color32::from_rgb(255, 100, 100))
                                        ).fill(egui::Color32::TRANSPARENT));
                                        if btn.clicked() {
                                            self.selected_peer_id = None;
                                        }
                                    } else {
                                        let btn = ui.add(egui::Button::new("Connect").fill(egui::Color32::from_rgb(32, 35, 47)));
                                        if btn.clicked() {
                                            self.selected_peer_id = Some(peer.id.clone());
                                            self.current_tab = Tab::LanMouse;
                                        }
                                    }
                                });
                            });
                        }).response;

                    if response.interact(egui::Sense::click()).clicked() {
                        if !is_selected {
                            self.selected_peer_id = Some(peer.id.clone());
                            self.current_tab = Tab::LanMouse;
                        }
                    }

                    ui.add_space(10.0);
                }
            });
        }
    }

    fn show_lan_mouse_tab(&mut self, ui: &mut egui::Ui) {
        let primary = self.primary_accent();
        let selection_bg = self.selection_tint();

        ui.add_space(10.0);

        let selected_peer = if let Some(peer_id) = &self.selected_peer_id {
            let s = self.state.lock().unwrap();
            s.peers.get(peer_id).cloned()
        } else {
            None
        };

        match selected_peer {
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(60.0);
                    ui.label(
                        egui::RichText::new("No Session Active")
                            .color(egui::Color32::from_rgb(255, 110, 110))
                            .font(egui::FontId::proportional(18.0))
                            .strong(),
                    );
                    ui.add_space(8.0);
                    ui.label("Select an discovered node from the connections deck first.");
                    ui.add_space(20.0);
                    if ui.button("Search for Devices").clicked() {
                        self.current_tab = Tab::Send;
                    }
                });
            }
            Some(peer) => {
                // Connected Info Topdeck Card
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 20, 27))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 31, 41)))
                    .rounding(16.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Streaming Control To:");
                            ui.label(
                                egui::RichText::new(&peer.name)
                                    .color(primary)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("({} : {})", peer.ip, peer.mouse_port))
                                    .color(egui::Color32::from_rgb(100, 105, 120))
                                    .font(egui::FontId::proportional(11.0)),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Disconnect").clicked() {
                                    self.selected_peer_id = None;
                                }
                            });
                        });
                    });

                ui.add_space(16.0);

                // Control Area Card Frame (Trackpad + Scroll)
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(12, 13, 19))
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 31, 41)))
                    .rounding(24.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let total_width = ui.available_width();
                            let scroll_width = 50.0;
                            let trackpad_width = total_width - scroll_width - 12.0;
                            let height = 240.0;

                            // 1. Touchpad
                            let (pad_rect, pad_resp) = ui.allocate_exact_size(
                                egui::vec2(trackpad_width, height),
                                egui::Sense::drag().union(egui::Sense::click()),
                            );

                            let pad_bg = if pad_resp.dragged() {
                                selection_bg
                            } else if pad_resp.hovered() {
                                egui::Color32::from_rgb(18, 20, 27)
                            } else {
                                egui::Color32::from_rgb(14, 15, 21)
                            };

                            let pad_border = if pad_resp.dragged() {
                                primary
                            } else {
                                egui::Color32::from_rgb(34, 37, 49)
                            };

                            ui.painter().rect(
                                pad_rect,
                                16.0,
                                pad_bg,
                                egui::Stroke::new(1.5, pad_border),
                            );

                            ui.painter().text(
                                pad_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "TOUCHPAD\n\nSlide to glide • Tap to click",
                                egui::FontId::proportional(13.0),
                                egui::Color32::from_rgb(100, 105, 120),
                            );

                            // Smooth interpolation calculations (Lerp filter)
                            if pad_resp.dragged() {
                                let delta = pad_resp.drag_delta();
                                self.target_dx += delta.x * self.sensitivity;
                                self.target_dy += delta.y * self.sensitivity;
                            }

                            // Lerp formula: current = current + (target - current) * factor
                            // Factor 0.35 yields rapid yet extremely organic deceleration curve
                            self.current_dx += (self.target_dx - self.current_dx) * 0.35;
                            self.current_dy += (self.target_dy - self.current_dy) * 0.35;

                            self.accum_x += self.current_dx;
                            self.accum_y += self.current_dy;

                            // Decay target values back to 0
                            self.target_dx *= 0.65;
                            self.target_dy *= 0.65;

                            let send_x = self.accum_x.trunc();
                            let send_y = self.accum_y.trunc();

                            self.accum_x -= send_x;
                            self.accum_y -= send_y;

                            if send_x.abs() >= 1.0 || send_y.abs() >= 1.0 {
                                self.send_mouse_msg(MouseControlMsg::Move { dx: send_x, dy: send_y });
                            }

                            if pad_resp.clicked() {
                                self.send_mouse_msg(MouseControlMsg::Click {
                                    button: "Left".to_string(),
                                    pressed: true,
                                });
                                std::thread::sleep(Duration::from_millis(15));
                                self.send_mouse_msg(MouseControlMsg::Click {
                                    button: "Left".to_string(),
                                    pressed: false,
                                });
                            }

                            // 2. Scroll Strip
                            let (scroll_rect, scroll_resp) = ui.allocate_exact_size(
                                egui::vec2(scroll_width, height),
                                egui::Sense::drag(),
                            );

                            let scroll_bg = if scroll_resp.dragged() {
                                selection_bg
                            } else {
                                egui::Color32::from_rgb(14, 15, 21)
                            };

                            let scroll_border = if scroll_resp.dragged() {
                                primary
                            } else {
                                egui::Color32::from_rgb(34, 37, 49)
                            };

                            ui.painter().rect(
                                scroll_rect,
                                16.0,
                                scroll_bg,
                                egui::Stroke::new(1.5, scroll_border),
                            );

                            ui.painter().text(
                                scroll_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "⇅\n\nS\nC\nR\nO\nL\nL",
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_rgb(100, 105, 120),
                            );

                            if scroll_resp.dragged() {
                                let delta = scroll_resp.drag_delta();
                                self.accum_scroll += -delta.y * 0.12;
                                
                                let scroll_dy = self.accum_scroll.trunc();
                                self.accum_scroll -= scroll_dy;

                                if scroll_dy.abs() >= 1.0 {
                                    self.send_mouse_msg(MouseControlMsg::Scroll { dy: scroll_dy });
                                }
                            }
                        });
                    });

                ui.add_space(16.0);

                // Tactile Rounded Action Buttons
                ui.horizontal(|ui| {
                    let total_width = ui.available_width();
                    let btn_height = 46.0;
                    let spacing = 12.0;
                    let outer_w = (total_width - spacing * 2.0) * 0.44;
                    let mid_w = (total_width - spacing * 2.0) * 0.12;

                    // Left Click
                    let left_btn = ui.add_sized(
                        [outer_w, btn_height],
                        egui::Button::new("Left Click")
                            .fill(egui::Color32::from_rgb(18, 20, 27))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(34, 37, 49))),
                    );
                    if left_btn.clicked() {
                        self.send_mouse_msg(MouseControlMsg::Click {
                            button: "Left".to_string(),
                            pressed: true,
                        });
                        std::thread::sleep(Duration::from_millis(15));
                        self.send_mouse_msg(MouseControlMsg::Click {
                            button: "Left".to_string(),
                            pressed: false,
                        });
                    }

                    // Middle
                    let mid_btn = ui.add_sized(
                        [mid_w, btn_height],
                        egui::Button::new("⚙")
                            .fill(egui::Color32::from_rgb(18, 20, 27))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(34, 37, 49))),
                    );
                    if mid_btn.clicked() {
                        self.send_mouse_msg(MouseControlMsg::Click {
                            button: "Middle".to_string(),
                            pressed: true,
                        });
                        std::thread::sleep(Duration::from_millis(15));
                        self.send_mouse_msg(MouseControlMsg::Click {
                            button: "Middle".to_string(),
                            pressed: false,
                        });
                    }

                    // Right Click
                    let right_btn = ui.add_sized(
                        [outer_w, btn_height],
                        egui::Button::new("Right Click")
                            .fill(egui::Color32::from_rgb(18, 20, 27))
                            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(34, 37, 49))),
                    );
                    if right_btn.clicked() {
                        self.send_mouse_msg(MouseControlMsg::Click {
                            button: "Right".to_string(),
                            pressed: true,
                        });
                        std::thread::sleep(Duration::from_millis(15));
                        self.send_mouse_msg(MouseControlMsg::Click {
                            button: "Right".to_string(),
                            pressed: false,
                        });
                    }
                });

                ui.add_space(14.0);
                
                // Sensitivity slider directly integrated below
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Glide Sensitivity:").color(egui::Color32::from_rgb(130, 135, 150)));
                    ui.add(egui::Slider::new(&mut self.sensitivity, 0.2..=3.0).text(""));
                });
            }
        }
    }

    fn show_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);

        egui::Frame::none()
            .fill(egui::Color32::from_rgb(18, 20, 27))
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(28, 31, 41)))
            .rounding(20.0)
            .inner_margin(24.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Configure System").font(egui::FontId::proportional(18.0)).color(egui::Color32::WHITE).strong());
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    ui.label("Device Alias Name:");
                    let mut name = {
                        let s = self.state.lock().unwrap();
                        s.local_name.clone()
                    };
                    if ui.text_edit_singleline(&mut name).changed() {
                        let mut s = self.state.lock().unwrap();
                        s.local_name = name;
                    }
                });

                ui.add_space(15.0);

                ui.label(egui::RichText::new("Theme Color Hue:").strong().color(egui::Color32::WHITE));
                ui.horizontal(|ui| {
                    let (color_preview_rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    ui.painter().rect_filled(color_preview_rect, 6.0, self.primary_accent());
                    ui.add(egui::Slider::new(&mut self.accent_hue, 0.0..=360.0).text(""));
                });

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(15.0);

                let local_ip = {
                    let s = self.state.lock().unwrap();
                    s.local_ip.clone()
                };

                ui.label(format!("Interface Address: {}", local_ip));
                ui.label("Network Mode: UDP Peer Subnet Broadcast");
                
                #[cfg(windows)]
                ui.label("Active Driver Core: Win32 User32 (Highly-optimized)");
                #[cfg(target_os = "linux")]
                ui.label("Active Driver Core: Linux xdotool CLI Pipeline");
            });
    }
}

impl eframe::App for JumpyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // High frequency UI execution frames keep animations and smoothing vectors moving perfectly
        ctx.request_repaint();

        let primary = self.primary_accent();
        let hover = self.hover_accent();
        let select_bg = self.selection_tint();

        let mut visuals = ctx.style().visuals.clone();
        visuals.widgets.active.bg_fill = primary;
        visuals.widgets.hovered.bg_fill = hover;
        visuals.widgets.inactive.bg_fill = select_bg;
        ctx.set_visuals(visuals);

        // --- Top Material 3 Navigation Bar (Ditched the sidebar) ---
        egui::TopBottomPanel::top("top_navigation_bar")
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 20, 27))
                    .inner_margin(egui::Margin { left: 24.0, right: 24.0, top: 12.0, bottom: 12.0 }))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    // Logo Accent
                    ui.label(
                        egui::RichText::new("⚡ JUMPY")
                            .font(egui::FontId::proportional(18.0))
                            .color(primary)
                            .strong(),
                    );
                    
                    ui.add_space(32.0);

                    // Material 3 Styled Pill Selection Group
                    let tabs = [
                        (Tab::Receive, "📥 Receive"),
                        (Tab::Send, "📤 Send & Connect"),
                        (Tab::LanMouse, "🖱 Trackpad"),
                        (Tab::Settings, "⚙ Settings"),
                    ];

                    for (tab, label) in tabs {
                        let is_active = self.current_tab == tab;
                        let text_color = if is_active {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_rgb(150, 155, 170)
                        };

                        let bg_color = if is_active {
                            primary
                        } else {
                            egui::Color32::TRANSPARENT
                        };

                        let button_style = egui::Button::new(
                            egui::RichText::new(label)
                                .color(text_color)
                                .font(egui::FontId::proportional(13.0))
                        )
                        .fill(bg_color)
                        .rounding(16.0); // Pill rounded shapes

                        if ui.add_sized([115.0, 32.0], button_style).clicked() {
                            self.current_tab = tab;
                        }
                        
                        ui.add_space(8.0);
                    }
                });
            });

        // --- Main Central Container ---
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(10, 11, 16))
                    .inner_margin(24.0),
            )
            .show(ctx, |ui| match self.current_tab {
                Tab::Receive => self.show_receive_tab(ui),
                Tab::Send => self.show_send_tab(ui),
                Tab::LanMouse => self.show_lan_mouse_tab(ui),
                Tab::Settings => self.show_settings_tab(ui),
            });
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
        "JUMPY - LAN Mouse & Discovery",
        options,
        Box::new(|cc| Ok(Box::new(JumpyApp::new(cc)))),
    )
}