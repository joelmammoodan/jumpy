use eframe::egui;

use crate::app::{JumpyApp, Tab};
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
                {
                    let mut s = self.state.lock().unwrap();
                    s.is_controlling_remote = true;
                }
                println!("Transitioned to Remote Mode at edge {:?}", target_edge);
                // Move cursor to center so it doesn't immediately exit or click local things
                self.platform.set_mouse_pos(w / 2, h / 2);
            }
        }

        // 2. If in Remote Mode, handle trapping and sending deltas
        if is_controlling {
            let (x, y) = self.platform.get_mouse_pos();
            let (w, h) = self.platform.get_screen_size();
            let center_x = w / 2;
            let center_y = h / 2;
            
            let dx = x - center_x;
            let dy = y - center_y;
            
            if dx != 0 || dy != 0 {
                self.send_mouse_msg(MouseControlMsg::Move { 
                    dx: (dx as f32) * self.sensitivity, 
                    dy: (dy as f32) * self.sensitivity 
                });
                self.platform.set_mouse_pos(center_x, center_y);
            }

            // Check if user wants to return locally (hit escape or opposite edge on remote machine - handled by msg)
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
                    
                    ui.add_space(32.0);
                    
                    let tabs = [
                        (Tab::Settings, "⚙ Settings"),
                        (Tab::Send, "📤 Connect"),
                    ];
                    for (tab, label) in tabs {
                        let is_active = self.current_tab == tab;
                        let text_color = if is_active { egui::Color32::WHITE } else { egui::Color32::from_rgb(150, 155, 170) };
                        let bg_color = if is_active { primary } else { egui::Color32::TRANSPARENT };
                        let button_style = egui::Button::new(egui::RichText::new(label).color(text_color)).fill(bg_color).rounding(16.0);
                        if ui.add_sized([115.0, 32.0], button_style).clicked() {
                            self.current_tab = tab;
                        }
                    }
                });
            });

        // UI Main Panel
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(10, 11, 16)).inner_margin(24.0))
            .show(ctx, |ui| match self.current_tab {
                Tab::Settings => {
                    ui.label(egui::RichText::new("Seamless Setup").strong().color(egui::Color32::WHITE));
                    ui.add_space(10.0);
                    ui.label("When active, moving your mouse to the chosen edge of your screen will transfer control to the selected remote computer.");
                    ui.add_space(10.0);
                    
                    let mut current_edge = { self.state.lock().unwrap().remote_edge };
                    egui::ComboBox::from_label("Remote Screen Edge")
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
                    
                    ui.add_space(20.0);
                    ui.label(format!("Local IP: {}", { self.state.lock().unwrap().local_ip.clone() }));
                },
                Tab::Send => {
                    let peers = {
                        let s = self.state.lock().unwrap();
                        s.peers.values().cloned().collect::<Vec<_>>()
                    };
                    if peers.is_empty() {
                        ui.label("Scanning network...");
                    } else {
                        for peer in peers {
                            ui.horizontal(|ui| {
                                ui.label(&peer.name);
                                ui.label(&peer.ip);
                                let is_selected = self.selected_peer_id.as_ref() == Some(&peer.id);
                                if is_selected {
                                    if ui.button("Disconnect").clicked() {
                                        self.selected_peer_id = None;
                                    }
                                } else {
                                    if ui.button("Connect").clicked() {
                                        self.selected_peer_id = Some(peer.id.clone());
                                    }
                                }
                            });
                        }
                    }
                },
                _ => {}
            });
    }
}
