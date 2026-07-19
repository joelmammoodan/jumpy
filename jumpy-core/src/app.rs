use std::sync::{Arc, Mutex};
use std::net::UdpSocket;
use rand::Rng;

use eframe::egui;

use crate::network::{AppState, get_local_ip, spawn_network_threads, MouseControlMsg};
use crate::platform::{PlatformHandler, Edge};

pub struct JumpyApp {
    pub state: Arc<Mutex<AppState>>,
    pub selected_peer_id: Option<String>,
    pub sensitivity: f32,
    pub client_socket: UdpSocket,
    
    pub target_dx: f32,
    pub target_dy: f32,
    pub current_dx: f32,
    pub current_dy: f32,
    pub accum_x: f32,
    pub accum_y: f32,
    pub accum_scroll: f32,
    pub accent_hue: f32,
    pub last_x: i32,
    pub last_y: i32,
    
    pub platform: Box<dyn PlatformHandler>,
    
    // Logo image handles
    pub logo: Option<egui::TextureHandle>,
}

impl JumpyApp {
    pub fn new(cc: &eframe::CreationContext<'_>, platform: Box<dyn PlatformHandler>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(10, 11, 16);
        visuals.window_fill = egui::Color32::from_rgb(18, 20, 27);
        cc.egui_ctx.set_visuals(visuals);

        let local_id = rand::thread_rng().gen_range(100000..999999).to_string();
        let local_name = "Jumpy Host".to_string();
        let local_ip = get_local_ip();

        let state = Arc::new(Mutex::new(AppState {
            local_id,
            local_name,
            local_ip,
            mouse_port: 0,
            discovery_enabled: true,
            is_receiver_active: true,
            peers: std::collections::HashMap::new(),
            remote_edge: Edge::None,
            is_controlling_remote: false,
            virtual_x: 0.0,
            virtual_y: 0.0,
        }));

        spawn_network_threads(Arc::clone(&state));

        // Mouse Receiver Server
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

        // We can't share platform directly to a thread if it's not clonable in this simple design. 
        // We'll let the main app update loop handle network receiving if possible, or we need to pass a clone of platform.
        // Actually, OS APIs for mouse moving can just be called directly from the thread if they are stateless functions.
        // Since `PlatformHandler` is `Send + Sync`, we could wrap it in an `Arc`.
        // Let's modify the app structure to make platform an Arc<Box<dyn PlatformHandler>>
        
        let client_socket = UdpSocket::bind("0.0.0.0:0").expect("Failed to bind client sending socket");

        // Load Logo Texture at runtime gracefully
        let logo_texture = if let Ok(logo_bytes) = std::fs::read("assets/logo.png") {
            if let Ok(image) = image::load_from_memory(&logo_bytes) {
                let size = [image.width() as _, image.height() as _];
                let image_buffer = image.to_rgba8();
                let pixels = image_buffer.as_flat_samples();
                let logo_color_image = egui::ColorImage::from_rgba_unmultiplied(
                    size,
                    pixels.as_slice(),
                );
                Some(cc.egui_ctx.load_texture(
                    "logo",
                    logo_color_image,
                    Default::default()
                ))
            } else {
                None
            }
        } else {
            None
        };

        Self {
            state,
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
            accent_hue: 265.0,
            last_x: 0,
            last_y: 0,
            platform,
            logo: logo_texture,
        }
    }

    pub fn start_mouse_receiver(state: Arc<Mutex<AppState>>, platform: Arc<dyn PlatformHandler + Send + Sync>) {
        let (mouse_socket, port) = {
            let s = state.lock().unwrap();
            let port = s.mouse_port;
            (UdpSocket::bind(format!("0.0.0.0:{}", port)).unwrap(), port)
        };
        println!("Action: Mouse receiver listening on 0.0.0.0:{}", port);

        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                if let Ok((amt, src)) = mouse_socket.recv_from(&mut buf) {
                    let is_active = {
                        let s = state.lock().unwrap();
                        s.is_receiver_active
                    };
                    if !is_active {
                        continue;
                    }
                    match serde_json::from_slice::<MouseControlMsg>(&buf[..amt]) {
                        Ok(msg) => match msg {
                            MouseControlMsg::Move { dx, dy } => {
                                println!("Action: Received Mouse Move (dx: {:.2}, dy: {:.2})", dx, dy);
                                platform.send_mouse_move(dx as i32, dy as i32);
                            }
                            MouseControlMsg::Click { button, pressed } => {
                                println!("Action: Received Mouse Click (button: {}, pressed: {})", button, pressed);
                                platform.send_mouse_click(&button, pressed);
                            }
                            MouseControlMsg::Scroll { dy } => {
                                println!("Action: Received Mouse Scroll (dy: {:.2})", dy);
                                platform.send_mouse_scroll(dy as i32);
                            }
                            MouseControlMsg::ReturnControl => {
                                println!("Action: Received Return Control");
                                let mut s = state.lock().unwrap();
                                s.is_controlling_remote = false;
                            }
                            MouseControlMsg::ConnectNotification { host_name } => {
                                println!("Action: Received Connect Notification from {}", host_name);
                                let _ = notify_rust::Notification::new()
                                    .summary("Jumpy Connected")
                                    .body(&format!("{} is now controlling this machine.", host_name))
                                    .show();
                            }
                        },
                        Err(e) => {
                            println!("Action: Failed to deserialize msg from {}: {:?}", src, e);
                        }
                    }
                }
            }
        });
    }

    pub fn primary_accent(&self) -> egui::Color32 {
        egui::Color32::from_rgb(120, 140, 255)
    }

    pub fn send_mouse_msg(&self, msg: MouseControlMsg) {
        if let Some(peer_id) = &self.selected_peer_id {
            let peer_opt = {
                let s = self.state.lock().unwrap();
                s.peers.get(peer_id).cloned()
            };
            if let Some(peer) = peer_opt {
                if let Ok(serialized) = serde_json::to_string(&msg) {
                    let target = format!("{}:{}", peer.ip, peer.mouse_port);
                    println!("Action: Sending to {} -> {}", target, serialized);
                    let res = self.client_socket.send_to(serialized.as_bytes(), target);
                    if let Err(e) = res {
                        println!("Action: Failed to send UDP packet: {:?}", e);
                    }
                } else {
                    println!("Action: Failed to serialize msg");
                }
            } else {
                println!("Action: Peer {} not found in state", peer_id);
            }
        }
    }
}
