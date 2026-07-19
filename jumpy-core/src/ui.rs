use eframe::egui;

use crate::app::JumpyApp;
use crate::network::MouseControlMsg;
use crate::platform::Edge;

impl eframe::App for JumpyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        // 1. Check Edge Detection for Seamless Mode
        let is_controlling = {
            let s = self.state.lock().unwrap();
            s.is_controlling_remote
        };
        let target_edge = {
            let s = self.state.lock().unwrap();
            s.remote_edge
        };

        if !is_controlling && target_edge != Edge::None && self.selected_peer_id.is_some() {
            let (x, y) = self.platform.get_mouse_pos();
            let (w, h) = self.platform.get_screen_size();
            
            let mut hit = false;
            match target_edge {
                Edge::Left => if x <= 0 { hit = true; },
                Edge::Right => if x >= w - 1 { hit = true; },
                Edge::Top => if y <= 0 { hit = true; },
                Edge::Bottom => if y >= h - 1 { hit = true; },
                _ => {}
            }

            if hit {
                let center_x = w / 2;
                let center_y = h / 2;
                {
                    let mut s = self.state.lock().unwrap();
                    s.is_controlling_remote = true;
                    match target_edge {
                        Edge::Left => { s.virtual_x = 1920.0; s.virtual_y = y as f32; }
                        Edge::Right => { s.virtual_x = 0.0; s.virtual_y = y as f32; }
                        Edge::Top => { s.virtual_x = x as f32; s.virtual_y = 1080.0; }
                        Edge::Bottom => { s.virtual_x = x as f32; s.virtual_y = 0.0; }
                        _ => {}
                    }
                }
                println!("Action: Transitioned to Remote Mode at edge {:?}", target_edge);
                self.platform.set_mouse_pos(center_x, center_y);
                self.last_x = center_x;
                self.last_y = center_y;
            }
        }

        // 2. If in Remote Mode, handle trapping and sending deltas
        if { self.state.lock().unwrap().is_controlling_remote } {
            let (x, y) = self.platform.get_mouse_pos();
            let (w, h) = self.platform.get_screen_size();
            
            let dx = x - self.last_x;
            let dy = y - self.last_y;
            
            // Ignore massive jumps, they are artifacts of set_mouse_pos warping
            if dx.abs() > 250 || dy.abs() > 250 {
                println!("Action: Ignored massive jump (dx: {}, dy: {}) - likely warp artifact", dx, dy);
                self.last_x = x;
                self.last_y = y;
            } else if dx != 0 || dy != 0 {
                let scaled_dx = (dx as f32) * self.sensitivity;
                let scaled_dy = (dy as f32) * self.sensitivity;

                println!("Action: Sending Mouse Move (dx: {:.2}, dy: {:.2})", scaled_dx, scaled_dy);
                self.send_mouse_msg(MouseControlMsg::Move { 
                    dx: scaled_dx, 
                    dy: scaled_dy 
                });
                self.last_x = x;
                self.last_y = y;

                let mut should_return = false;
                {
                    let mut s = self.state.lock().unwrap();
                    s.virtual_x += scaled_dx;
                    s.virtual_y += scaled_dy;
                    
                    let target = s.remote_edge;
                    match target {
                        Edge::Left => if s.virtual_x > 1920.0 + 50.0 { should_return = true; }
                        Edge::Right => if s.virtual_x < -50.0 { should_return = true; }
                        Edge::Top => if s.virtual_y > 1080.0 + 50.0 { should_return = true; }
                        Edge::Bottom => if s.virtual_y < -50.0 { should_return = true; }
                        _ => {}
                    }
                }

                if should_return {
                    println!("Action: Returning control to host!");
                    let mut s = self.state.lock().unwrap();
                    s.is_controlling_remote = false;
                    let return_x = match s.remote_edge {
                        Edge::Left => 10,
                        Edge::Right => w - 10,
                        _ => w / 2,
                    };
                    let return_y = match s.remote_edge {
                        Edge::Top => 10,
                        Edge::Bottom => h - 10,
                        _ => h / 2,
                    };
                    self.platform.set_mouse_pos(return_x, return_y);
                    self.last_x = return_x;
                    self.last_y = return_y;
                }
            }

            // Only trap if we haven't exited, and we are near the edge
            if { self.state.lock().unwrap().is_controlling_remote } {
                if x < 200 || x > w - 200 || y < 200 || y > h - 200 {
                    let center_x = w / 2;
                    let center_y = h / 2;
                    self.platform.set_mouse_pos(center_x, center_y);
                    self.last_x = center_x;
                    self.last_y = center_y;
                }
            }

            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                let mut s = self.state.lock().unwrap();
                s.is_controlling_remote = false;
            }
        }

        let primary = self.primary_accent();

        // UI Top Bar
        egui::TopBottomPanel::top("top_navigation_bar")
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(18, 20, 27)).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::new(logo).fit_to_exact_size(egui::vec2(32.0, 32.0)));
                    }
                    ui.label(egui::RichText::new("JUMPY").font(egui::FontId::proportional(18.0)).color(primary).strong());
                });
            });

        // UI Main Panel
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(10, 11, 16)).inner_margin(24.0))
            .show(ctx, |ui| {
                // Local Info Section
                ui.label(egui::RichText::new("Local Machine").strong().size(18.0).color(egui::Color32::WHITE));
                ui.add_space(8.0);
                
                let (local_ip, local_name) = {
                    let s = self.state.lock().unwrap();
                    (s.local_ip.clone(), s.local_name.clone())
                };
                
                ui.label(format!("Name: {}", local_name));
                ui.label(format!("IP Address: {}", local_ip));
                ui.add_space(16.0);
                
                // Seamless Setup Section
                ui.label(egui::RichText::new("Seamless Edge Configuration").strong().size(16.0).color(egui::Color32::WHITE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Move your mouse to this edge to control the connected remote.").color(egui::Color32::GRAY));
                ui.add_space(4.0);
                
                let mut current_edge = { self.state.lock().unwrap().remote_edge };
                egui::ComboBox::from_label("Target Edge")
                    .selected_text(format!("{:?}", current_edge))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut current_edge, Edge::None, "None (Disabled)");
                        ui.selectable_value(&mut current_edge, Edge::Left, "Left");
                        ui.selectable_value(&mut current_edge, Edge::Right, "Right");
                        ui.selectable_value(&mut current_edge, Edge::Top, "Top");
                        ui.selectable_value(&mut current_edge, Edge::Bottom, "Bottom");
                    });
                
                if { self.state.lock().unwrap().remote_edge } != current_edge {
                    self.state.lock().unwrap().remote_edge = current_edge;
                }
                
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(16.0);
                
                // Network Devices Section
                ui.label(egui::RichText::new("Discovered Clients").strong().size(18.0).color(egui::Color32::WHITE));
                ui.add_space(8.0);
                
                let peers = {
                    let s = self.state.lock().unwrap();
                    s.peers.values().cloned().collect::<Vec<_>>()
                };
                
                if peers.is_empty() {
                    ui.label(egui::RichText::new("Scanning network...").italics().color(egui::Color32::GRAY));
                } else {
                    for peer in peers {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(&peer.name).strong().color(egui::Color32::WHITE));
                                    ui.label(egui::RichText::new(format!("IP: {}", peer.ip)).size(12.0).color(egui::Color32::GRAY));
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let is_selected = self.selected_peer_id.as_ref() == Some(&peer.id);
                                    if is_selected {
                                        if ui.button("Disconnect").clicked() {
                                            self.selected_peer_id = None;
                                        }
                                    } else {
                                        if ui.button("Connect").clicked() {
                                            self.selected_peer_id = Some(peer.id.clone());
                                            
                                            // Send Notification
                                            let host_name = local_name.clone();
                                            if let Ok(serialized) = serde_json::to_string(&MouseControlMsg::ConnectNotification { host_name }) {
                                                let target = format!("{}:{}", peer.ip, peer.mouse_port);
                                                let _ = self.client_socket.send_to(serialized.as_bytes(), target);
                                            }
                                        }
                                    }
                                });
                            });
                        });
                        ui.add_space(4.0);
                    }
                }
            });
    }
}
