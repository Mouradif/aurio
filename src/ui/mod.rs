use crate::timing::{Sequence, StaticPattern};
use crate::{EngineCommand, EngineHandle, EngineUpdate, Project, TrackData};
use eframe::egui;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct AurioApp {
    engine: EngineHandle,
    current_project: Option<Project>,
    project_path: Option<PathBuf>,
    error_message: Option<String>,
    selected_track: Option<usize>,
    selected_node: Option<(usize, String)>,
    playing: bool,
    current_nodes: HashMap<usize, String>,
    project_modified: bool,
}

impl AurioApp {
    pub fn new(engine: EngineHandle) -> Self {
        Self {
            engine,
            current_project: None,
            project_path: None,
            error_message: None,
            selected_track: None,
            selected_node: None,
            playing: false,
            current_nodes: HashMap::new(),
            project_modified: false,
        }
    }

    fn process_engine_updates(&mut self) {
        while let Ok(update) = self.engine.update_rx.try_recv() {
            match update {
                EngineUpdate::ProjectLoaded { project } => {
                    self.current_project = Some(project);
                    self.error_message = None;
                    self.selected_track = Some(0);
                }
                EngineUpdate::CurrentNodes { track_nodes } => {
                    self.current_nodes = track_nodes.into_iter().collect();
                }
                EngineUpdate::PlaybackState { playing } => {
                    self.playing = playing;
                }
                EngineUpdate::Error { message } => {
                    self.error_message = Some(message);
                }
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Project...").clicked() {
                    // TODO
                    ui.close();
                }

                if ui.button("Open Project...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Open Aurio Project")
                        .pick_folder()
                    {
                        self.project_path = Some(path.clone());
                        let _ = self
                            .engine
                            .command_tx
                            .send(EngineCommand::LoadProject(path));
                        ui.close();
                    }
                }

                let save_button = if self.project_modified {
                    ui.button("💾 Save Project *")
                } else {
                    ui.button("💾 Save Project")
                };

                if save_button.clicked() {
                    if let (Some(project), Some(path)) = (&self.current_project, &self.project_path)
                    {
                        match project.save(path) {
                            Ok(_) => {
                                self.project_modified = false;
                                println!("Project saved successfully");
                            }
                            Err(e) => {
                                self.error_message = Some(format!("Failed to save project: {}", e));
                            }
                        }
                    }
                    ui.close();
                }

                ui.separator();

                if ui.button("Quit").clicked() {
                    std::process::exit(0);
                }
            });
            if let Some(project) = &self.current_project {
                ui.menu_button("Project", |ui| {
                    let title = if self.project_modified {
                        format!("{} *", project.name)
                    } else {
                        project.name.clone()
                    };
                    let _ = ui.button(title);
                });
            }
        });
    }

    fn transport_controls(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.playing {
                if ui.button("⏸ Pause").clicked() {
                    let _ = self.engine.command_tx.send(EngineCommand::Pause);
                }
            } else {
                if ui.button("▶ Play").clicked() {
                    let _ = self.engine.command_tx.send(EngineCommand::Play);
                }
            }

            if ui.button("⏹ Stop").clicked() {
                let _ = self.engine.command_tx.send(EngineCommand::Stop);
            }
        });
    }

    fn draw_graph(&mut self, ui: &mut egui::Ui, track: &TrackData) {
        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(ui.available_width(), ui.available_height()),
            egui::Sense::click_and_drag(),
        );

        let to_screen = egui::emath::RectTransform::from_to(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(800.0, 600.0)),
            response.rect,
        );

        let current_node = self.current_nodes.get(&track.id);

        for edge in &track.graph.edges {
            if let (Some(from_idx), Some(to_idx)) = (
                track.graph.nodes.iter().position(|n| n.id == edge.from),
                track.graph.nodes.iter().position(|n| n.id == edge.to),
            ) {
                let from_pos = node_position(from_idx);
                let to_pos = node_position(to_idx);

                let from_screen = to_screen.transform_pos(from_pos);
                let to_screen_pos = to_screen.transform_pos(to_pos);

                painter.arrow(
                    from_screen,
                    to_screen_pos - from_screen,
                    egui::Stroke::new(2.0, egui::Color32::GRAY),
                );

                let mid = (from_screen + to_screen_pos.to_vec2()) / 2.0;
                painter.text(
                    mid,
                    egui::Align2::CENTER_CENTER,
                    &edge.condition,
                    egui::FontId::proportional(10.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        }

        for (i, node) in track.graph.nodes.iter().enumerate() {
            let pos = node_position(i);
            let screen_pos = to_screen.transform_pos(pos);

            let node_size = egui::Vec2::new(100.0, 60.0);
            let rect = egui::Rect::from_center_size(screen_pos, node_size);

            let fill_color = if current_node == Some(&node.id) {
                egui::Color32::from_rgb(60, 180, 100)
            } else if node.id == track.initial_node {
                egui::Color32::from_rgb(60, 80, 100)
            } else {
                egui::Color32::from_rgb(40, 40, 40)
            };

            painter.rect_filled(rect, 5.0, fill_color);
            painter.rect_stroke(
                rect,
                5.0,
                egui::Stroke::new(2.0, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );

            painter.text(
                screen_pos,
                egui::Align2::CENTER_CENTER,
                &node.id,
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );

            let seq_type = match &node.sequence {
                crate::timing::Sequence::Static(_) => "Static",
                crate::timing::Sequence::Generated(_) => "Lua",
            };
            painter.text(
                screen_pos + egui::Vec2::new(0.0, 15.0),
                egui::Align2::CENTER_CENTER,
                seq_type,
                egui::FontId::proportional(10.0),
                egui::Color32::LIGHT_GRAY,
            );

            if response.clicked() {
                if let Some(click_pos) = response.interact_pointer_pos() {
                    if rect.contains(click_pos) {
                        self.selected_node = Some((track.id, node.id.clone()));
                    }
                }
            }
        }
    }

    fn draw_piano_roll(
        &mut self,
        ui: &mut egui::Ui,
        pattern: &mut StaticPattern,
        node_name: &str,
    ) -> bool {
        ui.heading(format!("Piano Roll: {}", node_name));

        if ui.button("Close Piano Roll").clicked() {}

        ui.separator();

        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(ui.available_width(), 300.0),
            egui::Sense::click(),
        );

        let rect = response.rect;
        let mut modified = false;

        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(30, 30, 30));

        let piano_key_width = 60.0;
        let min_pitch = 48; // C3
        let max_pitch = 84; // C6
        let pitch_range = max_pitch - min_pitch;
        let row_height = rect.height() / pitch_range as f32;

        let time_signature = pattern.time_signature;
        let beats_per_bar = time_signature.0 as f32;
        let total_beats = beats_per_bar * pattern.duration_bars as f32;

        let grid_area_width = rect.width() - piano_key_width;
        let pixels_per_beat = grid_area_width / total_beats;

        for pitch in min_pitch..max_pitch {
            let y = rect.top() + (max_pitch - pitch - 1) as f32 * row_height;
            let key_rect = egui::Rect::from_min_size(
                egui::Pos2::new(rect.left(), y),
                egui::Vec2::new(piano_key_width, row_height),
            );

            let is_black_key = matches!(pitch % 12, 1 | 3 | 6 | 8 | 10);
            let key_color = if is_black_key {
                egui::Color32::from_rgb(20, 20, 20)
            } else {
                egui::Color32::from_rgb(200, 200, 200)
            };

            painter.rect_filled(key_rect, 0.0, key_color);
            painter.rect_stroke(
                key_rect,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 100, 100)),
                egui::StrokeKind::Inside,
            );

            if pitch % 12 == 0 {
                let octave = (pitch / 12) - 1;
                let text_color = if is_black_key {
                    egui::Color32::WHITE
                } else {
                    egui::Color32::BLACK
                };
                painter.text(
                    key_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("C{}", octave),
                    egui::FontId::proportional(10.0),
                    text_color,
                );
            }
        }

        let grid_rect = egui::Rect::from_min_size(
            egui::Pos2::new(rect.left() + piano_key_width, rect.top()),
            egui::Vec2::new(grid_area_width, rect.height()),
        );
        painter.rect_filled(grid_rect, 0.0, egui::Color32::from_rgb(40, 40, 40));

        for beat in 0..=total_beats as i32 {
            let x = rect.left() + piano_key_width + (beat as f32 * pixels_per_beat);

            let is_bar_line = beat % beats_per_bar as i32 == 0;
            let color = if is_bar_line {
                egui::Color32::from_rgb(100, 100, 100)
            } else {
                egui::Color32::from_rgb(60, 60, 60)
            };
            let width = if is_bar_line { 2.0 } else { 1.0 };

            painter.line_segment(
                [
                    egui::Pos2::new(x, rect.top()),
                    egui::Pos2::new(x, rect.bottom()),
                ],
                egui::Stroke::new(width, color),
            );
        }

        for pitch in min_pitch..=max_pitch {
            let y = rect.top() + (max_pitch - pitch) as f32 * row_height;
            painter.line_segment(
                [
                    egui::Pos2::new(rect.left() + piano_key_width, y),
                    egui::Pos2::new(rect.right(), y),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 60)),
            );
        }

        let mut note_to_delete: Option<usize> = None;
        for (idx, note) in pattern.notes.iter().enumerate() {
            if note.pitch < min_pitch || note.pitch >= max_pitch {
                continue;
            }

            let note_x = rect.left() + piano_key_width + (note.start_beat * pixels_per_beat);
            let note_width = note.duration_beats * pixels_per_beat;
            let note_y = rect.top() + (max_pitch - note.pitch - 1) as f32 * row_height;

            let note_rect = egui::Rect::from_min_size(
                egui::Pos2::new(note_x, note_y),
                egui::Vec2::new(note_width, row_height),
            );

            let velocity_factor = note.velocity as f32 / 127.0;
            let note_color = egui::Color32::from_rgb(
                (100.0 + 155.0 * velocity_factor) as u8,
                (150.0 + 105.0 * velocity_factor) as u8,
                (200.0 + 55.0 * velocity_factor) as u8,
            );

            painter.rect_filled(note_rect, 2.0, note_color);
            painter.rect_stroke(
                note_rect,
                2.0,
                egui::Stroke::new(1.0, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );

            if response.secondary_clicked() {
                if let Some(click_pos) = response.interact_pointer_pos() {
                    if note_rect.contains(click_pos) {
                        note_to_delete = Some(idx);
                    }
                }
            }
        }

        if let Some(idx) = note_to_delete {
            pattern.notes.remove(idx);
            modified = true;
        }

        if response.clicked() {
            if let Some(click_pos) = response.interact_pointer_pos() {
                if click_pos.x > rect.left() + piano_key_width {
                    let relative_y = click_pos.y - rect.top();
                    let pitch = max_pitch - (relative_y / row_height) as u8;

                    if pitch >= min_pitch && pitch < max_pitch {
                        let relative_x = click_pos.x - rect.left() - piano_key_width;
                        let beat = (relative_x / pixels_per_beat).round();

                        if beat < total_beats {
                            let note_exists = pattern
                                .notes
                                .iter()
                                .any(|n| n.pitch == pitch && (n.start_beat - beat).abs() < 0.1);

                            if !note_exists {
                                pattern.notes.push(crate::timing::Note {
                                    pitch,
                                    velocity: 100,
                                    start_beat: beat,
                                    duration_beats: 1.0,
                                });
                                modified = true;
                            }
                        }
                    }
                }
            }
        }

        for bar in 0..pattern.duration_bars {
            let x = rect.left() + piano_key_width + (bar as f32 * beats_per_bar * pixels_per_beat);
            painter.text(
                egui::Pos2::new(x + 5.0, rect.top() + 10.0),
                egui::Align2::LEFT_TOP,
                format!("Bar {}", bar + 1),
                egui::FontId::proportional(10.0),
                egui::Color32::LIGHT_GRAY,
            );
        }

        modified
    }
}

fn node_position(index: usize) -> egui::Pos2 {
    let spacing = 200.0;
    let x = 100.0 + (index as f32 * spacing);
    let y = 300.0;
    egui::Pos2::new(x, y)
}

impl eframe::App for AurioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_engine_updates();

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            self.menu_bar(ui);
        });

        if let Some(ref error) = self.error_message {
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.colored_label(egui::Color32::RED, error);
            });
        }

        let mut close_piano_roll = false;
        let mut modified_pattern: Option<(usize, String, StaticPattern)> = None;

        let piano_roll_data: Option<(usize, String, StaticPattern, String)> =
            if let Some((track_id, node_id)) = &self.selected_node {
                if let Some(ref project) = self.current_project {
                    if let Some(track) = project.tracks.iter().find(|t| t.id == *track_id) {
                        if let Some(node) = track.graph.get_node(node_id) {
                            if let Sequence::Static(pattern) = &node.sequence {
                                Some((*track_id, node_id.clone(), pattern.clone(), node.id.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

        if let Some((track_id, node_id, mut pattern, node_name)) = piano_roll_data {
            egui::TopBottomPanel::bottom("piano_roll")
                .min_height(350.0)
                .show(ctx, |ui| {
                    if ui.button("✕ Close Piano Roll").clicked() {
                        close_piano_roll = true;
                    }

                    if self.draw_piano_roll(ui, &mut pattern, &node_name) {
                        self.project_modified = true;
                        modified_pattern = Some((track_id, node_id, pattern));
                    }
                });
        }

        if let Some((track_id, node_id, new_pattern)) = modified_pattern {
            if let Some(ref mut project) = self.current_project {
                if let Some(track) = project.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(node) = track.graph.nodes.iter_mut().find(|n| n.id == node_id) {
                        node.sequence = Sequence::Static(new_pattern);
                        let _ = self
                            .engine
                            .command_tx
                            .send(EngineCommand::ReloadProject(project.clone()));
                    }
                }
            }
        }

        if close_piano_roll {
            self.selected_node = None;
        }

        if self.current_project.is_some() {
            egui::SidePanel::left("tracks")
                .min_width(200.0)
                .show(ctx, |ui| {
                    ui.heading("Tracks");

                    self.transport_controls(ui);

                    ui.separator();

                    if let Some(ref project) = self.current_project {
                        for (i, track) in project.tracks.iter().enumerate() {
                            let is_selected = self.selected_track == Some(i);
                            if ui.selectable_label(is_selected, &track.name).clicked() {
                                self.selected_track = Some(i);
                            }
                        }
                    }
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(track_idx) = self.selected_track {
                    if let Some(ref project) = self.current_project {
                        if let Some(track) = project.tracks.get(track_idx) {
                            ui.heading(format!("Graph: {}", track.name));
                            ui.label(format!("BPM: {}", project.bpm));
                            if let Some(current) = self.current_nodes.get(&track.id) {
                                ui.label(format!("▶ Currently playing: {}", current));
                            }
                            ui.separator();

                            let track_clone = track.clone();
                            self.draw_graph(ui, &track_clone);
                        }
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.heading("Select a track to view its graph");
                    });
                }
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("No project loaded");
                    ui.label("File → Open Project to get started");
                });
            });
        }

        ctx.request_repaint();
    }
}
