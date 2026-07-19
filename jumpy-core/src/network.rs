use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::platform::Edge;

/// The payload broadcasted over UDP to discover other Jumpy instances on the local network.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DiscoveryMessage {
    pub id: String,         // Unique random ID of the instance
    pub name: String,       // Human readable name (e.g., "Jumpy Host")
    pub ip: String,         // The local IP address of the sender
    pub mouse_port: u16,    // The specific port listening for mouse control packets
    pub device_type: String,
}

/// Represents another Jumpy computer discovered on the local network.
#[derive(Clone, Debug)]
pub struct PeerDevice {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub mouse_port: u16,
    pub device_type: String,
    /// Used by the pruner thread to drop peers if we haven't heard from them in a while.
    pub last_seen: Instant,
}

/// The commands sent from the host computer to the client computer to control the mouse.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MouseControlMsg {
    Move { dx: f32, dy: f32 },
    Click { button: String, pressed: bool },
    Scroll { dy: f32 },
    Key { key_code: u32, down: bool },
    /// Tells the client to drop control back to the host.
    ReturnControl,
    
    // --- Pairing Protocol ---
    /// Sent by host to request pairing. Client will generate a PIN.
    PairRequest { host_id: String, host_name: String },
    /// Sent by host with the PIN they entered.
    PairSubmit { host_id: String, pin: String },
    /// Sent by client to confirm or reject the PIN.
    PairResponse { success: bool },
    
    /// Sent when a paired host first connects to display a desktop notification.
    ConnectNotification { host_name: String },
}

/// The core state of the application, shared across multiple threads via Arc<Mutex>.
pub struct AppState {
    pub local_id: String,
    pub local_name: String,
    pub local_ip: String,
    pub mouse_port: u16,
    pub discovery_enabled: bool,
    pub is_receiver_active: bool,
    /// Map of all discovered computers on the network.
    pub peers: HashMap<String, PeerDevice>,
    
    // Seamless mode settings
    /// The physical screen edge configured to trigger control transfer.
    pub remote_edge: Edge,
    /// Flag indicating whether this machine is actively controlling another machine.
    pub is_controlling_remote: bool,
    pub virtual_x: f32,
    pub virtual_y: f32,
    
    // Security & Pairing
    /// IDs of machines we trust to control us.
    pub trusted_hosts: HashSet<String>,
    /// If we are currently being paired, the PIN we generated.
    pub pending_pair_pin: Option<String>,
    /// The ID of the host currently trying to pair with us.
    pub pending_pair_host: Option<String>,
    /// The Name of the host currently trying to pair with us.
    pub pending_pair_host_name: Option<String>,
}

pub fn load_trusted_hosts() -> HashSet<String> {
    if let Ok(data) = std::fs::read_to_string("trusted_hosts.json") {
        if let Ok(hosts) = serde_json::from_str(&data) {
            return hosts;
        }
    }
    HashSet::new()
}

pub fn save_trusted_hosts(hosts: &HashSet<String>) {
    if let Ok(data) = serde_json::to_string(hosts) {
        let _ = std::fs::write("trusted_hosts.json", data);
    }
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

/// Spawns three background threads to handle network discovery:
/// 1. **Broadcaster**: Continuously shouts our presence to the local network.
/// 2. **Listener**: Listens for shouts from other Jumpy instances and adds them to our peers map.
/// 3. **Pruner**: Removes peers that haven't shouted in a few seconds (e.g., they closed the app).
pub fn spawn_network_threads(state: Arc<Mutex<AppState>>) {
    // Broadcaster Thread
    let b_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let local_ip = { b_state.lock().unwrap().local_ip.clone() };
        let socket = std::net::UdpSocket::bind(format!("{}:0", local_ip))
            .unwrap_or_else(|_| std::net::UdpSocket::bind("0.0.0.0:0").expect("Failed to bind"));
        socket.set_broadcast(true).expect("Failed to enable broadcasting");
        
        loop {
            std::thread::sleep(Duration::from_millis(1000));
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
                // Broadcast to the entire local subnet on port 52637
                let _ = socket.send_to(serialized.as_bytes(), "255.255.255.255:52637");
            }
        }
    });

    // Listener Thread
    let l_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let socket_addr: std::net::SocketAddr = "0.0.0.0:52637".parse().unwrap();
        // Use socket2 to allow multiple Jumpy instances on the same machine to bind to the same port
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
                if let Ok((amt, src)) = socket.recv_from(&mut buf) {
                    if let Ok(msg) = serde_json::from_slice::<DiscoveryMessage>(&buf[..amt]) {
                        let mut s = l_state.lock().unwrap();
                        // Make sure we don't discover ourselves
                        if msg.id != s.local_id {
                            let peer = PeerDevice {
                                id: msg.id.clone(),
                                name: msg.name,
                                ip: src.ip().to_string(), // Trust the physical packet origin IP over the reported one
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

    // Pruner Thread
    let p_state = Arc::clone(&state);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let mut s = p_state.lock().unwrap();
            let now = Instant::now();
            // Drop any peers that haven't broadcasted in the last 15 seconds
            s.peers.retain(|_, peer| {
                now.duration_since(peer.last_seen) < Duration::from_secs(15)
            });
        }
    });
}
