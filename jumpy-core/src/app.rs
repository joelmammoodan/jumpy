use std::sync::{Arc, Mutex};
use std::net::UdpSocket;
use rand::Rng;

use eframe::egui;

use crate::network::{AppState, get_local_ip, spawn_network_threads, MouseControlMsg};
use crate::platform::{PlatformHandler, Edge};

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Tab {
    Receive,
    Send,
    LanMouse,
    Settings,
}

pub struct JumpyApp {
    pub state: Arc<Mutex<AppState>>,
    pub current_tab: Tab,
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

        // Load Logo Texture
        let logo_bytes = include_bytes!(r"C:\Users\LEGION\.gemini\antigravity-ide\brain\02be4a7d-0897-455d-8e8a-6c2e809714b0\jumpy_logo_1784392645422.png");
        let image = image::load_from_memory(logo_bytes).unwrap();
        let size = [image.width() as _, image.height() as _];
        let image_buffer = image.to_rgba8();
        let pixels = image_buffer.as_flat_samples();
        let logo_color_image = egui::ColorImage::from_rgba_unmultiplied(
            size,
            pixels.as_slice(),
        );
        let logo_texture = cc.egui_ctx.load_texture(
            "logo",
            logo_color_image,
            Default::default()
        );

        Self {
            state,
            current_tab: Tab::Settings,
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
            platform,
            logo: Some(logo_texture),
        }
    }

    pub fn start_mouse_receiver(state: Arc<Mutex<AppState>>, platform: Arc<Box<dyn PlatformHandler>>) {
        let (mouse_socket, _) = {
            let s = state.lock().unwrap();
            let port = s.mouse_port;
            (UdpSocket::bind(format!("0.0.0.0:{}", port)).unwrap(), port)
        };

        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            loop {
                if let Ok((amt, _src)) = mouse_socket.recv_from(&mut buf) {
                    let is_active = {
                        let s = state.lock().unwrap();
                        s.is_receiver_active
                    };
                    if !is_active {
                        continue;
                    }
                    if let Ok(msg) = serde_json::from_slice::<MouseControlMsg>(&buf[..amt]) {
                        match msg {
                            MouseControlMsg::Move { dx, dy } => {
                                platform.send_mouse_move(dx as i32, dy as i32);
                            }
                            MouseControlMsg::Click { button, pressed } => {
                                platform.send_mouse_click(&button, pressed);
                            }
                            MouseControlMsg::Scroll { dy } => {
                                platform.send_mouse_scroll(dy as i32);
                            }
                            MouseControlMsg::ReturnControl => {
                                let mut s = state.lock().unwrap();
                                s.is_controlling_remote = false;
                            }
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
                    let _ = self.client_socket.send_to(serialized.as_bytes(), target);
                }
            }
        }
    }
}
