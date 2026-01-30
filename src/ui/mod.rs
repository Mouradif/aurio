mod piano_roll;

use crate::timing::{Sequence, StaticPattern};
use crate::{EngineCommand, EngineHandle, EngineUpdate, Project, TrackData};
use eframe::egui;
use piano_roll::{PianoRoll, PianoRollState};
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
    piano_roll_states: HashMap<(usize, String), PianoRollState>,
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
            piano_roll_states: HashMap::new(),
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
            let state_key = (track_id, node_id.clone());
            let state = self
                .piano_roll_states
                .entry(state_key)
                .or_insert_with(PianoRollState::default);
            if state.vertical_zoom == 20.0 && state.horizontal_zoom == 50.0 {
                state.fit_to_pattern(&pattern, egui::Vec2::new(800.0, 300.0));
            }
            egui::TopBottomPanel::bottom("piano_roll")
                .min_height(350.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(format!("Piano Roll: {}", node_name));
                        if ui.button("✕ Close").clicked() {
                            close_piano_roll = true;
                        }
                    });

                    let response = PianoRoll::new(&mut pattern, state).show(ui);

                    if response.modified {
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
