use eframe::egui;

use crate::app::JumpyApp;
use crate::network::MouseControlMsg;
use crate::platform::Edge;

impl eframe::App for JumpyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Force the UI to refresh constantly so we get 60+ FPS for smooth mouse tracking
        ctx.request_repaint();

        // =========================================================
        // 1. Edge Detection (Transitioning to Remote Mode)
        // =========================================================
        // Check if the hardware mouse has hit the user-configured edge of the physical screen.
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
                // We hit the edge! Transition into Remote Control Mode.
                // We hit the edge! Transition into Remote Control Mode.
                // To successfully lock the cursor, it must be physically inside the Jumpy window.
                let mut warp_x = w / 2;
                let mut warp_y = h / 2;
                
                let ppp = ctx.pixels_per_point();
                ctx.input(|i| {
                    if let Some(outer_rect) = i.viewport().outer_rect {
                        // outer_rect is in logical points! We MUST multiply by DPI scaling
                        // so SetCursorPos (which takes physical pixels) hits the window exactly!
                        warp_x = (outer_rect.center().x * ppp) as i32;
                        warp_y = (outer_rect.center().y * ppp) as i32;
                    }
                });
                
                {
                    let mut s = self.state.lock().unwrap();
                    s.is_controlling_remote = true;
                    // Calculate the starting position of the virtual cursor on the remote machine
                    match target_edge {
                        Edge::Left => { s.virtual_x = 1920.0; s.virtual_y = y as f32; }
                        Edge::Right => { s.virtual_x = 0.0; s.virtual_y = y as f32; }
                        Edge::Top => { s.virtual_x = x as f32; s.virtual_y = 1080.0; }
                        Edge::Bottom => { s.virtual_x = x as f32; s.virtual_y = 0.0; }
                        _ => {}
                    }
                }
                println!("Action: Transitioned to Remote Mode at edge {:?}", target_edge);
                
                // Grab UI focus and lock the cursor so it vanishes
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::Locked));
                
                // Warp the physical mouse directly into the Jumpy window so Windows allows the grab
                self.platform.set_mouse_pos(warp_x, warp_y);
                self.last_x = warp_x;
                self.last_y = warp_y;
            }
        }

        // =========================================================
        // 2. Remote Mode Handling (Tracking & Sending Mouse Events)
        // =========================================================
        // If we are actively controlling another computer, calculate mouse deltas and send them.
        if self.state.lock().unwrap().is_controlling_remote {
            let (x, y) = self.platform.get_mouse_pos();
            let (w, h) = self.platform.get_screen_size();
            
            let mut dx = x - self.last_x;
            let mut dy = y - self.last_y;
            
            // If the cursor is locked, the hardware position might not change. Fallback to egui's raw pointer delta.
            ctx.input(|i| {
                if dx == 0 && dy == 0 {
                    dx = i.pointer.delta().x as i32;
                    dy = i.pointer.delta().y as i32;
                }
            });
            
            // Ignore massive jumps, they are artifacts of set_mouse_pos warping or fullscreen transitions
            if dx.abs() > 500 || dy.abs() > 500 {
                println!("Action: Ignored massive jump (dx: {}, dy: {}) - likely warp artifact", dx, dy);
                self.last_x = x;
                self.last_y = y;
            } else if dx != 0 || dy != 0 {
                let scaled_dx = (dx as f32) * self.sensitivity;
                let scaled_dy = (dy as f32) * self.sensitivity;
                self.accum_x += scaled_dx;
                self.accum_y += scaled_dy;
                let send_dx = self.accum_x.trunc();
                let send_dy = self.accum_y.trunc();
                self.accum_x -= send_dx;
                self.accum_y -= send_dy;

                if send_dx != 0.0 || send_dy != 0.0 {
                    println!("Action: Sending Mouse Move (dx: {:.2}, dy: {:.2})", send_dx, send_dy);
                    self.send_mouse_msg(MouseControlMsg::Move { 
                        dx: send_dx, 
                        dy: send_dy 
                    });
                }
                
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
            // =========================================================
            // 3. Returning Control to Host
            // =========================================================
            // The virtual cursor hit the remote edge, so we return to the host computer.
                    println!("Action: Returning control to host!");
                    let mut s = self.state.lock().unwrap();
                    s.is_controlling_remote = false;
                    
                    // Release the OS mouse lock
                    ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
                    
                    // Pop the mouse cursor out just inside the edge of the physical screen
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

            // Emergency Return (ESC Key)
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                let mut s = self.state.lock().unwrap();
                s.is_controlling_remote = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
            }
        }

        let primary = self.primary_accent();

        if !self.state.lock().unwrap().is_controlling_remote {
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
                    
                    if let Some(pairing_peer_id) = self.pairing_with_id.clone() {
                        // =========================================================
                        // PIN ENTRY SCREEN (Host Side)
                        // =========================================================
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label(egui::RichText::new("Pairing Required").strong().size(24.0).color(egui::Color32::WHITE));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Look at the screen of the computer you are trying to control and enter the PIN displayed.").color(egui::Color32::GRAY));
                            ui.add_space(20.0);
                            
                            ui.add(egui::TextEdit::singleline(&mut self.entered_pin)
                                .font(egui::FontId::proportional(32.0))
                                .horizontal_align(egui::Align::Center)
                                .desired_width(200.0)
                            );
                            
                            ui.add_space(20.0);
                            ui.horizontal_centered(|ui| {
                                if ui.button(egui::RichText::new("Cancel").size(18.0)).clicked() {
                                    self.pairing_with_id = None;
                                    self.entered_pin.clear();
                                }
                                ui.add_space(20.0);
                                if ui.button(egui::RichText::new("Submit").size(18.0).color(primary)).clicked() {
                                    // Send PairSubmit
                                    let local_id = { self.state.lock().unwrap().local_id.clone() };
                                    if let Some(peer) = { self.state.lock().unwrap().peers.get(&pairing_peer_id).cloned() } {
                                        if let Ok(serialized) = serde_json::to_string(&MouseControlMsg::PairSubmit { 
                                            host_id: local_id.clone(), 
                                            pin: self.entered_pin.clone() 
                                        }) {
                                            let target = format!("{}:{}", peer.ip, peer.mouse_port);
                                            let _ = self.client_socket.send_to(serialized.as_bytes(), target);
                                        }
                                    }
                                    
                                    // Assume success and connect (if it failed, the client will just ignore us)
                                    // We also add them to our own trusted_hosts so we don't ask for a PIN again.
                                    {
                                        let mut s = self.state.lock().unwrap();
                                        s.trusted_hosts.insert(pairing_peer_id.clone());
                                        crate::network::save_trusted_hosts(&s.trusted_hosts);
                                    }
                                    
                                    self.selected_peer_id = Some(pairing_peer_id);
                                    self.pairing_with_id = None;
                                    self.entered_pin.clear();
                                    
                                    // Send connect notification
                                    let host_name = local_name.clone();
                                    self.send_mouse_msg(MouseControlMsg::ConnectNotification { host_name });
                                }
                            });
                        });
                    } else {
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
                                                    let is_trusted = {
                                                        let s = self.state.lock().unwrap();
                                                        s.trusted_hosts.contains(&peer.id)
                                                    };
                                                    
                                                    if is_trusted {
                                                        // Instantly connect
                                                        self.selected_peer_id = Some(peer.id.clone());
                                                        let host_name = local_name.clone();
                                                        self.send_mouse_msg(MouseControlMsg::ConnectNotification { host_name });
                                                    } else {
                                                        // Require pairing
                                                        self.pairing_with_id = Some(peer.id.clone());
                                                        let local_id = { self.state.lock().unwrap().local_id.clone() };
                                                        let host_name = local_name.clone();
                                                        if let Ok(serialized) = serde_json::to_string(&MouseControlMsg::PairRequest { host_id: local_id, host_name }) {
                                                            let target = format!("{}:{}", peer.ip, peer.mouse_port);
                                                            let _ = self.client_socket.send_to(serialized.as_bytes(), target);
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    });
                                });
                                ui.add_space(4.0);
                            }
                        }
                    }
                });
        } else {
            // =========================================================
            // 5. Remote Mode UI (Capture Panel)
            // =========================================================
            // When actively controlling the remote machine, we replace the entire UI
            // with a blank capture panel that hides the mouse and consumes clicks.
            ctx.set_cursor_icon(egui::CursorIcon::None);
            
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(egui::Color32::from_rgb(10, 11, 16)))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new("Remote Control Active\n\nPress ESC or move mouse to edge to return")
                            .color(primary)
                            .size(24.0)
                            .strong());
                    });
                    
                    // Allocate an invisible response area covering the entire window to catch clicks
                    let response = ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());
                    
                    if response.clicked() {
                        self.send_mouse_msg(MouseControlMsg::Click { button: "Left".to_string(), pressed: true });
                        self.send_mouse_msg(MouseControlMsg::Click { button: "Left".to_string(), pressed: false });
                    }
                    if response.secondary_clicked() {
                        self.send_mouse_msg(MouseControlMsg::Click { button: "Right".to_string(), pressed: true });
                        self.send_mouse_msg(MouseControlMsg::Click { button: "Right".to_string(), pressed: false });
                    }
                    if response.middle_clicked() {
                        self.send_mouse_msg(MouseControlMsg::Click { button: "Middle".to_string(), pressed: true });
                        self.send_mouse_msg(MouseControlMsg::Click { button: "Middle".to_string(), pressed: false });
                    }
                    
                    // Handle scroll
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        self.send_mouse_msg(MouseControlMsg::Scroll { dy: scroll });
                    }
                });
        }
        
        // =========================================================
        // 6. Pairing Request Overlay (Client Side)
        // =========================================================
        // If a host wants to connect to us, we show the PIN prominently.
        let (pending_pin, pending_host) = {
            let s = self.state.lock().unwrap();
            (s.pending_pair_pin.clone(), s.pending_pair_host_name.clone())
        };
        
        if let (Some(pin), Some(host_name)) = (pending_pin, pending_host) {
            egui::Window::new("Pairing Request")
                .fixed_pos(egui::pos2(0.0, 0.0))
                .fixed_size(ctx.screen_rect().size())
                .title_bar(false)
                .frame(egui::Frame::none().fill(egui::Color32::from_black_alpha(240)))
                .show(ctx, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(format!("{} wants to connect", host_name)).size(32.0).color(egui::Color32::WHITE));
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new("Enter this PIN on the host device:").size(24.0).color(egui::Color32::GRAY));
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(&pin).size(80.0).strong().color(primary));
                            ui.add_space(40.0);
                            if ui.button(egui::RichText::new("Reject").size(24.0)).clicked() {
                                let mut s = self.state.lock().unwrap();
                                s.pending_pair_pin = None;
                                s.pending_pair_host = None;
                                s.pending_pair_host_name = None;
                            }
                        });
                    });
                });
        }
    }
}
