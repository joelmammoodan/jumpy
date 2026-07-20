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
            let edge_threshold = 5;
            match target_edge {
                Edge::Left => if x <= edge_threshold { hit = true; },
                Edge::Right => if x >= w - 1 - edge_threshold { hit = true; },
                Edge::Top => if y <= edge_threshold { hit = true; },
                Edge::Bottom => if y >= h - 1 - edge_threshold { hit = true; },
                _ => {}
            }

            if hit {
                // We hit the edge! Transition into Remote Control Mode.
                
                // We lock the cursor exactly where it hit the edge of the screen!
                // We shift the anchor slightly inward to prevent Windows from violently
                // clamping the cursor against the physical monitor bounds, which causes infinite delta spam.
                let mut warp_x = x;
                let mut warp_y = y;
                
                // When we transition, we want the remote cursor to start at the opposite edge
                // of the screen, preserving the perpendicular coordinate.
                match target_edge {
                    Edge::Left => {
                        warp_x += 20;
                        self.send_mouse_msg(MouseControlMsg::Move { dx: 30000.0, dy: -30000.0 });
                        self.send_mouse_msg(MouseControlMsg::Move { dx: 0.0, dy: y as f32 });
                    }
                    Edge::Right => {
                        warp_x -= 20;
                        self.send_mouse_msg(MouseControlMsg::Move { dx: -30000.0, dy: -30000.0 });
                        self.send_mouse_msg(MouseControlMsg::Move { dx: 0.0, dy: y as f32 });
                    }
                    Edge::Top => {
                        warp_y += 20;
                        self.send_mouse_msg(MouseControlMsg::Move { dx: -30000.0, dy: 30000.0 });
                        self.send_mouse_msg(MouseControlMsg::Move { dx: x as f32, dy: 0.0 });
                    }
                    Edge::Bottom => {
                        warp_y -= 20;
                        self.send_mouse_msg(MouseControlMsg::Move { dx: -30000.0, dy: -30000.0 });
                        self.send_mouse_msg(MouseControlMsg::Move { dx: x as f32, dy: 0.0 });
                    }
                    _ => {}
                }
                
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
                
                // Hide the egui cursor
                ctx.set_cursor_icon(egui::CursorIcon::None);
                
                // Enable OS-level capture (ClipCursor) at the exact window location
                self.platform.set_capture_mode(true, warp_x, warp_y);
                
                // Set last_x and last_y to the anchor point
                self.last_x = warp_x;
                self.last_y = warp_y;
            }
        }

        // =========================================================
        // 2. Remote Mode Handling (Tracking & Sending Mouse Events)
        // =========================================================
        // If we are actively controlling another computer, calculate mouse deltas and send them.
        if self.state.lock().unwrap().is_controlling_remote {
            let (w, h) = self.platform.get_screen_size();
            
            // Calculate delta from the center of the screen if polling is required
            let mut dx = 0;
            let mut dy = 0;
            
            if self.platform.uses_polling_capture() {
                let (x, y) = self.platform.get_mouse_pos();
                let cx = self.last_x;
                let cy = self.last_y;
                
                dx = x - cx;
                dy = y - cy;
                
                // Instantly snap the cursor back to the center of the screen so it has room to move next frame
                if dx != 0 || dy != 0 {
                    self.platform.set_mouse_pos(cx, cy);
                }
            }
            
            // Ignore massive jumps (e.g. from the initial warp to the center)
            if dx.abs() > 500 || dy.abs() > 500 {
                dx = 0;
                dy = 0;
            }
            
            let mut should_return = false;

            if dx != 0 || dy != 0 {
                let scaled_dx = (dx as f32) * self.sensitivity;
                let scaled_dy = (dy as f32) * self.sensitivity;
                self.accum_x += scaled_dx;
                self.accum_y += scaled_dy;
                let send_dx = self.accum_x.trunc();
                let send_dy = self.accum_y.trunc();
                self.accum_x -= send_dx;
                self.accum_y -= send_dy;

                if send_dx != 0.0 || send_dy != 0.0 {
                    // println!("Action: Sending Mouse Move (dx: {:.2}, dy: {:.2})", send_dx, send_dy);
                    self.send_mouse_msg(MouseControlMsg::Move { 
                        dx: send_dx, 
                        dy: send_dy 
                    });
                }
                
                let mut s = self.state.lock().unwrap();
                s.virtual_x = (s.virtual_x + scaled_dx).clamp(-100.0, 3840.0 + 100.0);
                s.virtual_y = (s.virtual_y + scaled_dy).clamp(-100.0, 2160.0 + 100.0);
                
                let target = s.remote_edge;
                match target {
                    Edge::Left => if s.virtual_x > 1920.0 + 50.0 { should_return = true; }
                    Edge::Right => if s.virtual_x < -50.0 { should_return = true; }
                    Edge::Top => if s.virtual_y > 1080.0 + 50.0 { should_return = true; }
                    Edge::Bottom => if s.virtual_y < -50.0 { should_return = true; }
                    _ => {}
                }
            }

                // Forward any swallowed global events from the OS hook to the remote machine
                for ev in self.platform.get_grabbed_events() {
                    match &ev {
                        MouseControlMsg::ReturnControl => {
                            should_return = true;
                        },
                        MouseControlMsg::Move { dx, dy } => {
                            let mut s = self.state.lock().unwrap();
                            let scaled_dx = *dx * self.sensitivity;
                            let scaled_dy = *dy * self.sensitivity;
                            s.virtual_x = (s.virtual_x + scaled_dx).clamp(-100.0, 3840.0 + 100.0);
                            s.virtual_y = (s.virtual_y + scaled_dy).clamp(-100.0, 2160.0 + 100.0);
                            
                            // CRITICAL: We must drop the state lock before calling send_mouse_msg,
                            // because send_mouse_msg will try to acquire it again, causing a deadlock!
                            drop(s);
                            
                            self.send_mouse_msg(ev);
                        },
                        _ => {
                            self.send_mouse_msg(ev);
                        }
                    }
                }
                
                // Also check if the new virtual_x/virtual_y triggered a return
                {
                    let s = self.state.lock().unwrap();
                    let target = s.remote_edge;
                    match target {
                        Edge::Left => if s.virtual_x > 1920.0 + 50.0 { should_return = true; }
                        Edge::Right => if s.virtual_x < -50.0 { should_return = true; }
                        Edge::Top => if s.virtual_y > 1080.0 + 50.0 { should_return = true; }
                        Edge::Bottom => if s.virtual_y < -50.0 { should_return = true; }
                        _ => {}
                    }
                }
                
                // CRITICAL: Because our low-level OS hook swallows keyboard keys and mouse clicks,
                // the egui/winit event loop will not receive them and will go to sleep to save battery.
                // This causes a massive lag because the events sit in the channel until the user moves the mouse.
                // We MUST request continuous repaints while capturing to poll the channel instantly.
                ctx.request_repaint();

                if should_return {
            // =========================================================
            // 3. Returning Control to Host
            // =========================================================
            // The virtual cursor hit the remote edge, so we return to the host computer.
                    println!("Action: Returning control to host!");
                    let mut s = self.state.lock().unwrap();
                    s.is_controlling_remote = false;
                    
                    // Release the OS mouse lock
                    self.platform.set_capture_mode(false, 0, 0);
                    ctx.set_cursor_icon(egui::CursorIcon::Default);
                    
                    // Pop the mouse cursor out safely away from the edge threshold
                    let return_x = match s.remote_edge {
                        Edge::Left => 30,
                        Edge::Right => w - 30,
                        _ => w / 2,
                    };
                    let return_y = match s.remote_edge {
                        Edge::Top => 30,
                        Edge::Bottom => h - 30,
                        _ => h / 2,
                    };
                    self.platform.set_mouse_pos(return_x, return_y);
                    self.last_x = return_x;
                    self.last_y = return_y;
                }

            // Emergency Return (ESC Key)
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                let mut s = self.state.lock().unwrap();
                s.is_controlling_remote = false;
                self.platform.set_capture_mode(false, 0, 0);
                ctx.set_cursor_icon(egui::CursorIcon::Default);
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
                    let (local_ip, local_name) = {
                        let s = self.state.lock().unwrap();
                        (s.local_ip.clone(), s.local_name.clone())
                    };

                    // Local Info Section (Centered)
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(&local_name).strong().size(40.0).color(primary));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(format!("IP Address: {}", local_ip)).size(16.0).color(egui::Color32::GRAY));
                    });
                    ui.add_space(24.0);
                    
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
                    
                    if let Some(pairing_peer_id) = self.pairing_with_id.clone() {
                        // =========================================================
                        // PIN ENTRY SCREEN (Host Side)
                        // =========================================================
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);
                            ui.label(egui::RichText::new("Pairing Required").strong().size(28.0).color(egui::Color32::WHITE));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("Look at the screen of the computer you are trying to control and enter the PIN displayed.").color(egui::Color32::GRAY));
                            ui.add_space(30.0);
                            
                            ui.add(egui::TextEdit::singleline(&mut self.entered_pin)
                                .font(egui::FontId::proportional(36.0))
                                .horizontal_align(egui::Align::Center)
                                .desired_width(220.0)
                            );
                            
                            ui.add_space(30.0);
                            ui.horizontal_centered(|ui| {
                                if ui.button(egui::RichText::new("Cancel").size(20.0)).clicked() {
                                    self.pairing_with_id = None;
                                    self.entered_pin.clear();
                                }
                                ui.add_space(30.0);
                                if ui.button(egui::RichText::new("Submit").size(20.0).color(primary)).clicked() {
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
                        let peers = {
                            let s = self.state.lock().unwrap();
                            s.peers.values().cloned().collect::<Vec<_>>()
                        };
                        let is_scanning = peers.is_empty();

                        // Network Devices Section
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Discovered Devices").strong().size(22.0).color(egui::Color32::WHITE));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::click());
                                
                                if response.hovered() {
                                    ui.painter().rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                                }
                                
                                if is_scanning {
                                    ui.allocate_ui_at_rect(rect, |ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.add(egui::Spinner::new().size(18.0).color(primary));
                                        });
                                    });
                                } else {
                                    ui.painter().text(
                                        rect.center(), 
                                        egui::Align2::CENTER_CENTER, 
                                        "🔄", 
                                        egui::FontId::proportional(18.0), 
                                        ui.visuals().text_color()
                                    );
                                }
                                
                                if response.clicked() {
                                    let mut s = self.state.lock().unwrap();
                                    s.peers.clear();
                                    self.selected_peer_id = None;
                                }
                            });
                        });
                        
                        ui.add_space(12.0);
                        
                        if peers.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(60.0);
                                
                                let time = ui.input(|i| i.time);
                                let dots = (time * 3.0) as usize % 4;
                                let dots_str = ".".repeat(dots);
                                let text = format!("Scanning network{}", dots_str);
                                
                                let alpha = (time.sin() * 0.5 + 0.5) as f32 * 0.5 + 0.5;
                                let color = primary.linear_multiply(alpha);
                                
                                ui.label(egui::RichText::new(text).italics().size(18.0).color(color));
                                ui.add_space(30.0);
                                
                                // Custom orbiting circles animation
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 60.0), egui::Sense::hover());
                                let center = rect.center();
                                let radius = 25.0;
                                let painter = ui.painter();
                                
                                for i in 0..3 {
                                    let offset = (i as f64) * std::f64::consts::PI * 2.0 / 3.0;
                                    let current = time * 3.0 + offset;
                                    let pos = center + egui::vec2(current.cos() as f32, current.sin() as f32) * radius;
                                    painter.circle_filled(pos, 7.0, primary.linear_multiply(0.8));
                                }
                            });
                        } else {
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                for peer in peers {
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new(&peer.name).strong().size(18.0).color(egui::Color32::WHITE));
                                                ui.add_space(4.0);
                                                ui.label(egui::RichText::new(format!("IP: {}", peer.ip)).size(14.0).color(egui::Color32::GRAY));
                                            });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let is_selected = self.selected_peer_id.as_ref() == Some(&peer.id);
                                                if is_selected {
                                                    if ui.button(egui::RichText::new("Disconnect").size(16.0)).clicked() {
                                                        self.selected_peer_id = None;
                                                    }
                                                } else {
                                                    if ui.button(egui::RichText::new("Connect").size(16.0).color(primary)).clicked() {
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
                                    ui.add_space(8.0);
                                }
                            });
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
                    
                    // Clicks and scrolls are now handled entirely by the global OS hook
                    // so we do not need to capture them via egui response.
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
