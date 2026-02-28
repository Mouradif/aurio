mod piano_roll;

use crate::audio::{ADSRConfig, Instrument, OscConfig, Wave};
use crate::graph::{
    AudioGraph, GraphEdge, GraphNode, MixerChannel, NodeId, NodeKind, PortKind, StateMachineEdge,
};
use crate::timing::StaticPattern;
use crate::{EngineCommand, EngineHandle, EngineUpdate, Project};
use eframe::egui;
use piano_roll::{PianoRoll, PianoRollState};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone)]
struct GraphViewState {
    pan_x: f32,
    pan_y: f32,
    zoom: f32,
}

impl Default for GraphViewState {
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

impl GraphViewState {
    fn world_to_screen(&self, world: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let center = rect.center();
        egui::Pos2::new(
            center.x + (world.x - self.pan_x) * self.zoom,
            center.y + (world.y - self.pan_y) * self.zoom,
        )
    }

    fn screen_to_world(&self, screen: egui::Pos2, rect: egui::Rect) -> egui::Pos2 {
        let center = rect.center();
        egui::Pos2::new(
            (screen.x - center.x) / self.zoom + self.pan_x,
            (screen.y - center.y) / self.zoom + self.pan_y,
        )
    }
}

struct NewProjectDialog {
    name: String,
    path: Option<PathBuf>,
    open: bool,
}

impl Default for NewProjectDialog {
    fn default() -> Self {
        Self {
            name: "New Project".to_string(),
            path: None,
            open: false,
        }
    }
}

/// Tracks an in-progress edge-drawing operation (Alt+drag from a node).
struct EdgeDragState {
    from_node: NodeId,
    current_screen_pos: egui::Pos2,
}

/// Dialog for entering a state machine edge condition.
struct SmEdgeDialog {
    from_node: NodeId,
    to_node: NodeId,
    condition: String,
    open: bool,
}

pub struct AurioApp {
    engine: EngineHandle,
    current_project: Option<Project>,
    project_path: Option<PathBuf>,
    error_message: Option<String>,
    selected_node: Option<NodeId>,
    playing: bool,
    active_patterns: HashMap<NodeId, NodeId>,
    project_modified: bool,
    piano_roll_states: HashMap<NodeId, PianoRollState>,
    graph_view: GraphViewState,
    new_project_dialog: NewProjectDialog,
    context_menu_world_pos: Option<egui::Pos2>,
    context_menu_screen_pos: Option<egui::Pos2>,
    /// Which node is currently being dragged to reposition (plain drag).
    dragging_node: Option<NodeId>,
    /// In-progress edge drawing (Alt+drag).
    edge_drag: Option<EdgeDragState>,
    /// State machine edge condition dialog.
    sm_edge_dialog: Option<SmEdgeDialog>,
    /// Rect of the graph panel (set each frame so we can check hover).
    graph_rect: Option<egui::Rect>,
    /// Rect of the piano roll panel.
    piano_roll_rect: Option<egui::Rect>,
}

impl AurioApp {
    pub fn new(engine: EngineHandle) -> Self {
        Self {
            engine,
            current_project: None,
            project_path: None,
            error_message: None,
            selected_node: None,
            playing: false,
            active_patterns: HashMap::new(),
            project_modified: false,
            piano_roll_states: HashMap::new(),
            graph_view: GraphViewState::default(),
            new_project_dialog: NewProjectDialog::default(),
            context_menu_world_pos: None,
            context_menu_screen_pos: None,
            dragging_node: None,
            edge_drag: None,
            sm_edge_dialog: None,
            graph_rect: None,
            piano_roll_rect: None,
        }
    }

    fn process_engine_updates(&mut self) {
        while let Ok(update) = self.engine.update_rx.try_recv() {
            match update {
                EngineUpdate::ProjectLoaded { project } => {
                    self.current_project = Some(project);
                    self.error_message = None;
                    self.graph_view = GraphViewState::default();
                }
                EngineUpdate::CurrentNodes { active_patterns } => {
                    self.active_patterns = active_patterns;
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
                    self.new_project_dialog.open = true;
                    self.new_project_dialog.name = "New Project".to_string();
                    self.new_project_dialog.path = None;
                    ui.close();
                }

                if ui.button("Open Project...").clicked()
                    && let Some(path) = rfd::FileDialog::new()
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

                let save_button = if self.project_modified {
                    ui.button("Save Project *")
                } else {
                    ui.button("Save Project")
                };

                if save_button.clicked() {
                    if let (Some(project), Some(path)) =
                        (&self.current_project, &self.project_path)
                    {
                        match project.save(path) {
                            Ok(_) => {
                                self.project_modified = false;
                                tracing::info!(path = %path.display(), "Project saved successfully");
                            }
                            Err(e) => {
                                self.error_message =
                                    Some(format!("Failed to save project: {}", e));
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
                if ui.button("Pause").clicked() {
                    let _ = self.engine.command_tx.send(EngineCommand::Pause);
                }
            } else if ui.button("Play").clicked() {
                let _ = self.engine.command_tx.send(EngineCommand::Play);
            }

            if ui.button("Stop").clicked() {
                let _ = self.engine.command_tx.send(EngineCommand::Stop);
            }
        });
    }

    /// Returns true if the mouse pointer is inside `rect`.
    fn pointer_in_rect(ctx: &egui::Context, rect: egui::Rect) -> bool {
        ctx.input(|i| {
            i.pointer
                .latest_pos()
                .is_some_and(|pos| rect.contains(pos))
        })
    }

    fn handle_graph_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        rect: egui::Rect,
        graph: &AudioGraph,
    ) {
        let modifiers = ctx.input(|i| i.modifiers);

        // Only process scroll if pointer is over the graph
        if Self::pointer_in_rect(ctx, rect) {
            ctx.input(|i| {
                let scroll_delta = i.smooth_scroll_delta;

                if modifiers.alt {
                    // Alt + scroll = zoom
                    let zoom_factor = 1.0 + scroll_delta.y * 0.01;
                    self.graph_view.zoom =
                        (self.graph_view.zoom * zoom_factor).clamp(0.1, 5.0);
                } else {
                    // 2-finger trackpad scroll = pan
                    self.graph_view.pan_x -= scroll_delta.x / self.graph_view.zoom;
                    self.graph_view.pan_y -= scroll_delta.y / self.graph_view.zoom;
                }
            });
        }

        // Shift + drag = pan
        if response.dragged() && modifiers.shift && self.edge_drag.is_none() {
            let delta = response.drag_delta();
            self.graph_view.pan_x -= delta.x / self.graph_view.zoom;
            self.graph_view.pan_y -= delta.y / self.graph_view.zoom;
        }

        // --- Alt+drag from node = start edge drawing ---
        if response.drag_started() && modifiers.alt
            && let Some(pos) = response.interact_pointer_pos()
                && let Some(node_id) = self.node_at_screen_pos(pos, rect, graph) {
                    self.edge_drag = Some(EdgeDragState {
                        from_node: node_id,
                        current_screen_pos: pos,
                    });
                }

        // Update edge drag position
        if response.dragged() && self.edge_drag.is_some()
            && let Some(pos) = response.interact_pointer_pos()
                && let Some(ref mut drag) = self.edge_drag {
                    drag.current_screen_pos = pos;
                }

        // Release edge drag → try to connect
        if response.drag_stopped() && self.edge_drag.is_some()
            && let Some(drag) = self.edge_drag.take()
                && let Some(pos) = ctx.input(|i| i.pointer.latest_pos())
                    && let Some(to_node_id) = self.node_at_screen_pos(pos, rect, graph)
                        && to_node_id != drag.from_node {
                            self.try_create_edge(&drag.from_node, &to_node_id);
                        }

        // --- Node dragging (plain drag, no modifier, on a node) ---
        if response.drag_started() && !modifiers.shift && !modifiers.alt
            && let Some(pos) = response.interact_pointer_pos()
                && let Some(node_id) = self.node_at_screen_pos(pos, rect, graph) {
                    self.dragging_node = Some(node_id);
                }

        if response.dragged() && self.dragging_node.is_some() && !modifiers.shift {
            let delta = response.drag_delta();
            let world_delta = egui::Vec2::new(
                delta.x / self.graph_view.zoom,
                delta.y / self.graph_view.zoom,
            );
            if let Some(ref node_id) = self.dragging_node
                && let Some(ref mut project) = self.current_project
                    && let Some(node) = project.graph.get_node_mut(node_id) {
                        node.position.0 += world_delta.x;
                        node.position.1 += world_delta.y;
                        self.project_modified = true;
                    }
        }

        if response.drag_stopped() {
            self.dragging_node = None;
        }

        // Secondary click (right-click) on empty area → context menu
        if response.secondary_clicked()
            && let Some(click_pos) = response.interact_pointer_pos()
        {
            let world_pos = self.graph_view.screen_to_world(click_pos, rect);
            self.context_menu_world_pos = Some(world_pos);
            self.context_menu_screen_pos = Some(click_pos);
        }
    }

    /// Find which node (if any) is at the given screen position.
    fn node_at_screen_pos(
        &self,
        screen_pos: egui::Pos2,
        rect: egui::Rect,
        graph: &AudioGraph,
    ) -> Option<NodeId> {
        let node_size = egui::Vec2::new(120.0, 60.0) * self.graph_view.zoom;
        for node in &graph.nodes {
            let node_screen = self.graph_view.world_to_screen(
                egui::Pos2::new(node.position.0, node.position.1),
                rect,
            );
            let node_rect = egui::Rect::from_center_size(node_screen, node_size);
            if node_rect.contains(screen_pos) {
                return Some(node.id.clone());
            }
        }
        None
    }

    /// Try to create an edge between two nodes based on their types.
    fn try_create_edge(&mut self, from_id: &NodeId, to_id: &NodeId) {
        let Some(ref project) = self.current_project else {
            return;
        };
        let Some(from_node) = project.graph.get_node(from_id) else {
            return;
        };
        let Some(to_node) = project.graph.get_node(to_id) else {
            return;
        };

        let from_is_pattern = from_node.kind.is_pattern();
        let to_is_pattern = to_node.kind.is_pattern();

        // Both patterns → state machine edge (open condition dialog)
        if from_is_pattern && to_is_pattern {
            self.sm_edge_dialog = Some(SmEdgeDialog {
                from_node: from_id.clone(),
                to_node: to_id.clone(),
                condition: "true".to_string(),
                open: true,
            });
            return;
        }

        // Event edge: pattern → instrument (synth)
        if from_node.kind.event_outputs() > 0 && to_node.kind.event_inputs() > 0 {
            if let Some(ref mut project) = self.current_project {
                project.graph.edges.push(GraphEdge {
                    from_node: from_id.clone(),
                    from_port: 0,
                    to_node: to_id.clone(),
                    to_port: 0,
                    kind: PortKind::Event,
                });
                self.project_modified = true;
                let _ = self
                    .engine
                    .command_tx
                    .send(EngineCommand::ReloadProject(project.clone()));
            }
            return;
        }

        // Audio edge: synth → mixer, mixer → output, synth → output
        if from_node.kind.audio_outputs() > 0 && to_node.kind.audio_inputs().is_some()
            && let Some(ref mut project) = self.current_project {
                // Figure out the next available input port on the target
                let next_port = project
                    .graph
                    .edges
                    .iter()
                    .filter(|e| e.to_node == *to_id && e.kind == PortKind::Audio)
                    .count();

                // If target is a mixer, grow its channels if needed
                if let Some(to_mut) = project.graph.get_node_mut(to_id)
                    && let NodeKind::Mixer { ref mut channels } = to_mut.kind {
                        while channels.len() <= next_port {
                            channels.push(MixerChannel::default());
                        }
                    }

                project.graph.edges.push(GraphEdge {
                    from_node: from_id.clone(),
                    from_port: 0,
                    to_node: to_id.clone(),
                    to_port: next_port,
                    kind: PortKind::Audio,
                });
                self.project_modified = true;
                let _ = self
                    .engine
                    .command_tx
                    .send(EngineCommand::ReloadProject(project.clone()));
            }
    }

    fn draw_graph(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, graph: &AudioGraph) {
        let (response, painter) = ui.allocate_painter(
            egui::Vec2::new(ui.available_width(), ui.available_height()),
            egui::Sense::click_and_drag(),
        );

        let rect = response.rect;
        self.graph_rect = Some(rect);

        self.handle_graph_input(ctx, &response, rect, graph);

        // Draw background
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(25, 25, 30));

        // Draw grid
        self.draw_graph_grid(&painter, rect);

        // Draw edges
        for edge in &graph.edges {
            if let (Some(from_node), Some(to_node)) = (
                graph.get_node(&edge.from_node),
                graph.get_node(&edge.to_node),
            ) {
                let from_screen = self.graph_view.world_to_screen(
                    egui::Pos2::new(from_node.position.0, from_node.position.1),
                    rect,
                );
                let to_screen = self.graph_view.world_to_screen(
                    egui::Pos2::new(to_node.position.0, to_node.position.1),
                    rect,
                );

                let (color, width) = match edge.kind {
                    PortKind::Event => (egui::Color32::from_rgb(100, 180, 255), 1.5),
                    PortKind::Audio => (egui::Color32::from_rgb(100, 255, 100), 2.0),
                };

                painter.arrow(
                    from_screen,
                    to_screen - from_screen,
                    egui::Stroke::new(width * self.graph_view.zoom.sqrt(), color),
                );
            }
        }

        // Draw state machine edges
        for sm_edge in &graph.state_machine_edges {
            if let (Some(from_node), Some(to_node)) = (
                graph.get_node(&sm_edge.from),
                graph.get_node(&sm_edge.to),
            ) {
                let from_screen = self.graph_view.world_to_screen(
                    egui::Pos2::new(from_node.position.0, from_node.position.1),
                    rect,
                );
                let to_screen = self.graph_view.world_to_screen(
                    egui::Pos2::new(to_node.position.0, to_node.position.1),
                    rect,
                );

                let offset = egui::Vec2::new(0.0, -15.0 * self.graph_view.zoom);

                painter.arrow(
                    from_screen + offset,
                    (to_screen + offset) - (from_screen + offset),
                    egui::Stroke::new(
                        1.0 * self.graph_view.zoom.sqrt(),
                        egui::Color32::from_rgb(255, 200, 100),
                    ),
                );

                let mid = ((from_screen + offset) + (to_screen + offset).to_vec2()) / 2.0;
                painter.text(
                    mid,
                    egui::Align2::CENTER_CENTER,
                    &sm_edge.condition,
                    egui::FontId::proportional(10.0 * self.graph_view.zoom.sqrt()),
                    egui::Color32::from_rgb(255, 200, 100),
                );
            }
        }

        // Draw in-progress edge drag line
        if let Some(ref drag) = self.edge_drag
            && let Some(from_node) = graph.get_node(&drag.from_node) {
                let from_screen = self.graph_view.world_to_screen(
                    egui::Pos2::new(from_node.position.0, from_node.position.1),
                    rect,
                );
                painter.arrow(
                    from_screen,
                    drag.current_screen_pos - from_screen,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 255, 100)),
                );
            }

        // Draw nodes
        let mut clicked_node: Option<NodeId> = None;
        for node in &graph.nodes {
            let screen_pos = self.graph_view.world_to_screen(
                egui::Pos2::new(node.position.0, node.position.1),
                rect,
            );

            let node_size = egui::Vec2::new(120.0, 60.0) * self.graph_view.zoom;
            let node_rect = egui::Rect::from_center_size(screen_pos, node_size);

            if !rect.intersects(node_rect) {
                continue;
            }

            let is_active = graph.initial_pattern_nodes.iter().any(|init| {
                self.active_patterns.values().any(|v| v == &node.id)
                    || (self.active_patterns.is_empty() && init == &node.id)
            });

            let fill_color = match &node.kind {
                NodeKind::PianoRoll { .. } | NodeKind::PatternScript { .. } => {
                    if is_active {
                        egui::Color32::from_rgb(60, 180, 100)
                    } else {
                        egui::Color32::from_rgb(60, 80, 100)
                    }
                }
                NodeKind::Synth { .. } => egui::Color32::from_rgb(100, 60, 100),
                NodeKind::Mixer { .. } => egui::Color32::from_rgb(80, 80, 60),
                NodeKind::Output { .. } => egui::Color32::from_rgb(60, 60, 80),
            };

            // Highlight if this is the edge drag target
            let is_edge_drag_target = self.edge_drag.is_some()
                && ctx
                    .input(|i| i.pointer.latest_pos())
                    .is_some_and(|p| node_rect.contains(p));

            let stroke_width = if is_edge_drag_target {
                3.0
            } else if self.selected_node.as_ref() == Some(&node.id) {
                2.0
            } else {
                1.0
            };
            let stroke_color = if is_edge_drag_target {
                egui::Color32::YELLOW
            } else {
                egui::Color32::WHITE
            };

            painter.rect_filled(node_rect, 5.0 * self.graph_view.zoom, fill_color);
            painter.rect_stroke(
                node_rect,
                5.0 * self.graph_view.zoom,
                egui::Stroke::new(stroke_width, stroke_color),
                egui::StrokeKind::Inside,
            );

            painter.text(
                screen_pos,
                egui::Align2::CENTER_CENTER,
                &node.name,
                egui::FontId::proportional(12.0 * self.graph_view.zoom.sqrt()),
                egui::Color32::WHITE,
            );

            let kind_label = match &node.kind {
                NodeKind::PianoRoll { .. } => "Piano Roll",
                NodeKind::PatternScript { .. } => "Lua Script",
                NodeKind::Synth { .. } => "Synth",
                NodeKind::Mixer { .. } => "Mixer",
                NodeKind::Output { .. } => "Output",
            };
            painter.text(
                screen_pos + egui::Vec2::new(0.0, 15.0 * self.graph_view.zoom),
                egui::Align2::CENTER_CENTER,
                kind_label,
                egui::FontId::proportional(10.0 * self.graph_view.zoom.sqrt()),
                egui::Color32::LIGHT_GRAY,
            );

            if response.clicked()
                && let Some(click_pos) = response.interact_pointer_pos()
                && node_rect.contains(click_pos)
            {
                clicked_node = Some(node.id.clone());
            }
        }

        if let Some(node_id) = clicked_node {
            self.selected_node = Some(node_id);
        }
    }

    fn draw_graph_grid(&self, painter: &egui::Painter, rect: egui::Rect) {
        let grid_spacing = 100.0;

        let top_left = self.graph_view.screen_to_world(rect.min, rect);
        let bottom_right = self.graph_view.screen_to_world(rect.max, rect);

        let start_x = (top_left.x / grid_spacing).floor() as i32;
        let end_x = (bottom_right.x / grid_spacing).ceil() as i32;
        let start_y = (top_left.y / grid_spacing).floor() as i32;
        let end_y = (bottom_right.y / grid_spacing).ceil() as i32;

        let grid_color = egui::Color32::from_rgb(35, 35, 40);
        let grid_stroke = egui::Stroke::new(1.0, grid_color);

        for gx in start_x..=end_x {
            let world_x = gx as f32 * grid_spacing;
            let screen =
                self.graph_view
                    .world_to_screen(egui::Pos2::new(world_x, 0.0), rect);
            if screen.x >= rect.left() && screen.x <= rect.right() {
                painter.line_segment(
                    [
                        egui::Pos2::new(screen.x, rect.top()),
                        egui::Pos2::new(screen.x, rect.bottom()),
                    ],
                    grid_stroke,
                );
            }
        }

        for gy in start_y..=end_y {
            let world_y = gy as f32 * grid_spacing;
            let screen =
                self.graph_view
                    .world_to_screen(egui::Pos2::new(0.0, world_y), rect);
            if screen.y >= rect.top() && screen.y <= rect.bottom() {
                painter.line_segment(
                    [
                        egui::Pos2::new(rect.left(), screen.y),
                        egui::Pos2::new(rect.right(), screen.y),
                    ],
                    grid_stroke,
                );
            }
        }
    }

    fn show_context_menu(&mut self, ctx: &egui::Context) {
        if self.context_menu_world_pos.is_none() {
            return;
        }

        let mut add_node: Option<(&str, NodeKind)> = None;
        let mut close = false;

        let screen_pos = self.context_menu_screen_pos.unwrap_or_default();

        egui::Area::new(egui::Id::new("graph_context_menu"))
            .fixed_pos(screen_pos)
            .pivot(egui::Align2::LEFT_TOP)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ui.label("Add Node");
                    ui.separator();

                    if ui.button("Piano Roll").clicked() {
                        add_node = Some((
                            "piano-roll",
                            NodeKind::PianoRoll {
                                pattern: StaticPattern {
                                    duration_bars: 2,
                                    time_signature: (4, 4),
                                    notes: Vec::new(),
                                },
                            },
                        ));
                        close = true;
                    }

                    if ui.button("Synth").clicked() {
                        add_node = Some((
                            "synth",
                            NodeKind::Synth {
                                instrument: Instrument::MultiOsc {
                                    oscillators: vec![OscConfig {
                                        wave: Wave::Sine,
                                        gain: 0.3,
                                        semitone: 0,
                                    }],
                                },
                                adsr: ADSRConfig {
                                    attack: 0.01,
                                    decay: 0.1,
                                    sustain: 0.7,
                                    release: 0.2,
                                },
                            },
                        ));
                        close = true;
                    }

                    if ui.button("Mixer").clicked() {
                        add_node = Some((
                            "mixer",
                            NodeKind::Mixer {
                                channels: vec![MixerChannel::default()],
                            },
                        ));
                        close = true;
                    }

                    if ui.button("Output").clicked() {
                        add_node = Some(("output", NodeKind::Output { device: None }));
                        close = true;
                    }

                    if ui.input(|i| i.pointer.any_click()) && !ui.ui_contains_pointer() {
                        close = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        close = true;
                    }
                });
            });

        if let Some((prefix, kind)) = add_node {
            if let Some(ref mut project) = self.current_project {
                let world_pos = self.context_menu_world_pos.unwrap();
                let node_id = project.graph.generate_node_id(prefix);
                let name = node_id.0.clone();

                project.graph.nodes.push(GraphNode {
                    id: node_id.clone(),
                    name,
                    kind,
                    position: (world_pos.x, world_pos.y),
                });

                self.project_modified = true;
                self.selected_node = Some(node_id);
                let _ = self
                    .engine
                    .command_tx
                    .send(EngineCommand::ReloadProject(project.clone()));
            }
            self.context_menu_world_pos = None;
            self.context_menu_screen_pos = None;
        }

        if close {
            self.context_menu_world_pos = None;
            self.context_menu_screen_pos = None;
        }
    }

    fn show_sm_edge_dialog(&mut self, ctx: &egui::Context) {
        let Some(ref mut dialog) = self.sm_edge_dialog else {
            return;
        };
        if !dialog.open {
            self.sm_edge_dialog = None;
            return;
        }

        let mut should_create = false;
        let mut should_close = false;

        egui::Window::new("State Machine Edge")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{} → {}", dialog.from_node, dialog.to_node));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Condition:");
                    let te = ui.text_edit_singleline(&mut dialog.condition);
                    // Auto-focus the text field
                    te.request_focus();
                    if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        should_create = true;
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        should_create = true;
                    }
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });

        if should_create {
            let from = dialog.from_node.clone();
            let to = dialog.to_node.clone();
            let condition = dialog.condition.clone();
            self.sm_edge_dialog = None;

            if let Some(ref mut project) = self.current_project {
                let priority = project
                    .graph
                    .state_machine_edges
                    .iter()
                    .filter(|e| e.from == from)
                    .count() as u32;

                project.graph.state_machine_edges.push(StateMachineEdge {
                    from,
                    to,
                    condition,
                    priority,
                });
                self.project_modified = true;
                let _ = self
                    .engine
                    .command_tx
                    .send(EngineCommand::ReloadProject(project.clone()));
            }
        } else if should_close {
            self.sm_edge_dialog = None;
        }
    }

    fn show_new_project_dialog(&mut self, ctx: &egui::Context) {
        if !self.new_project_dialog.open {
            return;
        }

        let mut should_create = false;
        let mut should_close = false;

        egui::Window::new("New Project")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Project Name:");
                    ui.text_edit_singleline(&mut self.new_project_dialog.name);
                });

                ui.horizontal(|ui| {
                    ui.label("Location:");
                    if let Some(ref path) = self.new_project_dialog.path {
                        ui.label(path.display().to_string());
                    } else {
                        ui.label("(not selected)");
                    }
                    if ui.button("Browse...").clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title("Choose project location")
                            .pick_folder()
                    {
                        self.new_project_dialog.path = Some(dir);
                    }
                });

                ui.separator();

                ui.horizontal(|ui| {
                    let can_create = !self.new_project_dialog.name.is_empty()
                        && self.new_project_dialog.path.is_some();

                    if ui
                        .add_enabled(can_create, egui::Button::new("Create"))
                        .clicked()
                    {
                        should_create = true;
                    }

                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                });
            });

        if should_create {
            let name = self.new_project_dialog.name.clone();
            if let Some(ref base_path) = self.new_project_dialog.path {
                let project_dir = base_path.join(format!("{}.aurio", name));
                let project = Project::new_empty(name);

                match project.save(&project_dir) {
                    Ok(_) => {
                        tracing::info!(path = %project_dir.display(), "Created new project");
                        self.current_project = Some(project);
                        self.project_path = Some(project_dir);
                        self.project_modified = false;
                        self.selected_node = None;
                        self.graph_view = GraphViewState::default();
                    }
                    Err(e) => {
                        self.error_message =
                            Some(format!("Failed to create project: {}", e));
                    }
                }
            }
            self.new_project_dialog.open = false;
        }

        if should_close {
            self.new_project_dialog.open = false;
        }
    }
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

        // Dialogs
        self.show_new_project_dialog(ctx);
        self.show_context_menu(ctx);
        self.show_sm_edge_dialog(ctx);

        let mut close_piano_roll = false;
        let mut modified_pattern: Option<(NodeId, StaticPattern)> = None;

        // Check if selected node is a PianoRoll pattern
        let piano_roll_data: Option<(NodeId, StaticPattern)> =
            if let Some(ref node_id) = self.selected_node
                && let Some(ref project) = self.current_project
                && let Some(node) = project.graph.get_node(node_id)
                && let NodeKind::PianoRoll { pattern } = &node.kind
            {
                Some((node_id.clone(), pattern.clone()))
            } else {
                None
            };

        if let Some((node_id, mut pattern)) = piano_roll_data {
            let state = self
                .piano_roll_states
                .entry(node_id.clone())
                .or_default();
            if state.vertical_zoom == 20.0 && state.horizontal_zoom == 50.0 {
                state.fit_to_pattern(&pattern, egui::Vec2::new(800.0, 300.0));
            }
            egui::TopBottomPanel::bottom("piano_roll")
                .min_height(350.0)
                .show(ctx, |ui| {
                    // Store the piano roll rect for hover checking
                    self.piano_roll_rect = Some(ui.max_rect());

                    ui.horizontal(|ui| {
                        ui.heading(format!("Piano Roll: {}", node_id));
                        if ui.button("Close").clicked() {
                            close_piano_roll = true;
                        }
                    });

                    let pr_rect = self.piano_roll_rect;
                    let response =
                        PianoRoll::new(&mut pattern, state).show(ui, ctx, pr_rect);

                    if response.modified {
                        self.project_modified = true;
                        modified_pattern = Some((node_id, pattern));
                    }
                });
        } else {
            self.piano_roll_rect = None;
        }

        if let Some((node_id, new_pattern)) = modified_pattern
            && let Some(ref mut project) = self.current_project
            && let Some(node) = project.graph.get_node_mut(&node_id)
        {
            node.kind = NodeKind::PianoRoll {
                pattern: new_pattern,
            };
            let _ = self
                .engine
                .command_tx
                .send(EngineCommand::ReloadProject(project.clone()));
        }

        if close_piano_roll {
            self.selected_node = None;
        }

        if self.current_project.is_some() {
            egui::SidePanel::left("nodes")
                .min_width(200.0)
                .show(ctx, |ui| {
                    ui.heading("Nodes");

                    self.transport_controls(ui);

                    ui.separator();

                    if let Some(ref project) = self.current_project {
                        ui.label("Patterns");
                        for node in &project.graph.nodes {
                            if node.kind.is_pattern() {
                                let is_selected =
                                    self.selected_node.as_ref() == Some(&node.id);
                                if ui.selectable_label(is_selected, &node.name).clicked() {
                                    self.selected_node = Some(node.id.clone());
                                }
                            }
                        }

                        ui.separator();
                        ui.label("Instruments");
                        for node in &project.graph.nodes {
                            if matches!(node.kind, NodeKind::Synth { .. }) {
                                let is_selected =
                                    self.selected_node.as_ref() == Some(&node.id);
                                if ui.selectable_label(is_selected, &node.name).clicked() {
                                    self.selected_node = Some(node.id.clone());
                                }
                            }
                        }

                        ui.separator();
                        ui.label("Mixer / Output");
                        for node in &project.graph.nodes {
                            if matches!(
                                node.kind,
                                NodeKind::Mixer { .. } | NodeKind::Output { .. }
                            ) {
                                let is_selected =
                                    self.selected_node.as_ref() == Some(&node.id);
                                if ui.selectable_label(is_selected, &node.name).clicked() {
                                    self.selected_node = Some(node.id.clone());
                                }
                            }
                        }
                    }
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(ref project) = self.current_project {
                    ui.horizontal(|ui| {
                        ui.heading("Audio Graph");
                        ui.label(format!("BPM: {}", project.bpm));
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(format!(
                                    "Zoom: {:.0}%",
                                    self.graph_view.zoom * 100.0
                                ));
                            },
                        );
                    });
                    ui.separator();

                    let graph = project.graph.clone();
                    self.draw_graph(ctx, ui, &graph);
                }
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("No project loaded");
                    ui.label("File > Open Project to get started");
                    ui.label("File > New Project to create one");
                });
            });
        }

        ctx.request_repaint();
    }
}
