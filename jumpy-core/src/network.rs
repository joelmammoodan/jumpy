use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::platform::Edge;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiscoveryMessage {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub mouse_port: u16,
    pub device_type: String,
}

#[derive(Clone, Debug)]
pub struct PeerDevice {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub mouse_port: u16,
    pub device_type: String,
    pub last_seen: Instant,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MouseControlMsg {
    Move { dx: f32, dy: f32 },
    Click { button: String, pressed: bool },
    Scroll { dy: f32 },
    ReturnControl,
    ConnectNotification { host_name: String },
}

pub struct AppState {
    pub local_id: String,
    pub local_name: String,
    pub local_ip: String,
    pub mouse_port: u16,
    pub discovery_enabled: bool,
    pub is_receiver_active: bool,
    pub peers: HashMap<String, PeerDevice>,
    
    // Seamless mode settings
    pub remote_edge: Edge,
    pub is_controlling_remote: bool,
    pub virtual_x: f32,
    pub virtual_y: f32,
}

pub fn get_local_ip() -> String {
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

pub fn spawn_network_threads(state: Arc<Mutex<AppState>>) {
    // Broadcaster
    let b_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let local_ip = { b_state.lock().unwrap().local_ip.clone() };
        let socket = std::net::UdpSocket::bind(format!("{}:0", local_ip))
            .unwrap_or_else(|_| std::net::UdpSocket::bind("0.0.0.0:0").expect("Failed to bind"));
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

    // Listener
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

        if let Ok(socket) = socket_res {
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
        }
    });

    // Pruner
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
}
