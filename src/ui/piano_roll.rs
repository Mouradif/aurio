use crate::timing::{Note, StaticPattern};
use eframe::egui;

#[derive(Clone)]
pub struct PianoRollState {
    pub vertical_zoom: f32,
    pub horizontal_zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
}

impl Default for PianoRollState {
    fn default() -> Self {
        Self {
            vertical_zoom: 20.0,
            horizontal_zoom: 50.0,
            pan_x: 0.0,
            pan_y: 0.0,
        }
    }
}

impl PianoRollState {
    pub fn fit_to_pattern(&mut self, pattern: &StaticPattern, available_size: egui::Vec2) {
        if pattern.notes.is_empty() {
            return;
        }

        let min_pitch = pattern.notes.iter().map(|n| n.pitch).min().unwrap_or(48);
        let max_pitch = pattern.notes.iter().map(|n| n.pitch).max().unwrap_or(84);
        let pitch_range = (max_pitch - min_pitch + 1) as f32;

        let time_signature = pattern.time_signature;
        let beats_per_bar = time_signature.0 as f32;
        let total_beats = beats_per_bar * pattern.duration_bars as f32;

        let piano_key_width = 60.0;
        let available_width = available_size.x - piano_key_width;

        self.horizontal_zoom = (available_width * 0.9) / total_beats;
        self.vertical_zoom = (available_size.y * 0.9) / pitch_range;

        let center_pitch = (min_pitch + max_pitch) as f32 / 2.0;
        self.pan_y = center_pitch - (available_size.y / self.vertical_zoom / 2.0);
        self.pan_x = 0.0;
    }
}

pub struct PianoRoll<'a> {
    pattern: &'a mut StaticPattern,
    state: &'a mut PianoRollState,
}

impl<'a> PianoRoll<'a> {
    pub fn new(pattern: &'a mut StaticPattern, state: &'a mut PianoRollState) -> Self {
        Self { pattern, state }
    }

    pub fn show(mut self, ui: &mut egui::Ui) -> PianoRollResponse {
        let mut response = PianoRollResponse { modified: false };

        let available_size = ui.available_size();
        let (rect_response, painter) =
            ui.allocate_painter(available_size, egui::Sense::click_and_drag());

        let rect = rect_response.rect;
        let piano_key_width = 60.0;

        self.handle_input(ui, rect.size());

        let visible_semitones = rect.height() / self.state.vertical_zoom;
        let min_visible_pitch = self.state.pan_y.floor() as u8;
        let max_visible_pitch = (self.state.pan_y + visible_semitones).ceil() as u8;

        let visible_beats = (rect.width() - piano_key_width) / self.state.horizontal_zoom;
        let min_visible_beat = self.state.pan_x;
        let max_visible_beat = self.state.pan_x + visible_beats;

        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(30, 30, 30));

        self.draw_piano_keys(
            &painter,
            rect,
            piano_key_width,
            min_visible_pitch,
            max_visible_pitch,
        );

        let grid_rect = egui::Rect::from_min_size(
            egui::Pos2::new(rect.left() + piano_key_width, rect.top()),
            egui::Vec2::new(rect.width() - piano_key_width, rect.height()),
        );
        painter.rect_filled(grid_rect, 0.0, egui::Color32::from_rgb(40, 40, 40));

        self.draw_grid(
            &painter,
            rect,
            piano_key_width,
            min_visible_pitch,
            max_visible_pitch,
            min_visible_beat,
            max_visible_beat,
        );

        let modification = self.draw_notes(
            &painter,
            &rect_response,
            rect,
            piano_key_width,
            min_visible_pitch,
            max_visible_pitch,
        );
        if modification.is_some() {
            response.modified = true;
        }

        response
    }

    fn handle_input(&mut self, ui: &egui::Ui, available_size: egui::Vec2) {
        let modifiers = ui.input(|i| i.modifiers);

        ui.input(|i| {
            let scroll_delta = i.smooth_scroll_delta;

            if modifiers.alt {
                if modifiers.shift {
                    let zoom_factor = 1.0 + scroll_delta.y * 0.01;
                    self.state.horizontal_zoom =
                        (self.state.horizontal_zoom * zoom_factor).clamp(10.0, 200.0);
                } else {
                    let zoom_factor = 1.0 + scroll_delta.y * 0.01;
                    self.state.vertical_zoom =
                        (self.state.vertical_zoom * zoom_factor).clamp(5.0, 100.0);
                }
            } else {
                self.state.pan_x -= scroll_delta.x / self.state.horizontal_zoom;
                self.state.pan_x = self.state.pan_x.max(0.0);

                self.state.pan_y -= scroll_delta.y / self.state.vertical_zoom;
                self.state.pan_y = self
                    .state
                    .pan_y
                    .clamp(0.0, 127.0 - available_size.y / self.state.vertical_zoom);
            }
        });
    }

    fn draw_piano_keys(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        piano_key_width: f32,
        min_pitch: u8,
        max_pitch: u8,
    ) {
        for pitch in min_pitch..=max_pitch {
            let y = self.pitch_to_screen_y(pitch, rect);
            let key_rect = egui::Rect::from_min_size(
                egui::Pos2::new(rect.left(), y),
                egui::Vec2::new(piano_key_width, self.state.vertical_zoom),
            );

            if key_rect.bottom() < rect.top() || key_rect.top() > rect.bottom() {
                continue;
            }

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

            // Label C notes
            if pitch % 12 == 0 {
                let octave = (pitch / 12) as i32 - 1;
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
    }

    fn draw_grid(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        piano_key_width: f32,
        min_pitch: u8,
        max_pitch: u8,
        min_beat: f32,
        max_beat: f32,
    ) {
        let time_signature = self.pattern.time_signature;
        let beats_per_bar = time_signature.0 as f32;

        let start_beat = min_beat.floor() as i32;
        let end_beat = max_beat.ceil() as i32;

        for beat in start_beat..=end_beat {
            let x = self.beat_to_screen_x(beat as f32, rect, piano_key_width);

            if x < rect.left() + piano_key_width || x > rect.right() {
                continue;
            }

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

            if is_bar_line && beat >= 0 {
                let bar_num = (beat as f32 / beats_per_bar) as i32 + 1;
                painter.text(
                    egui::Pos2::new(x + 5.0, rect.top() + 10.0),
                    egui::Align2::LEFT_TOP,
                    format!("Bar {}", bar_num),
                    egui::FontId::proportional(10.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        }

        for pitch in min_pitch..=max_pitch {
            let y = self.pitch_to_screen_y(pitch, rect);

            if y < rect.top() || y > rect.bottom() {
                continue;
            }

            painter.line_segment(
                [
                    egui::Pos2::new(rect.left() + piano_key_width, y),
                    egui::Pos2::new(rect.right(), y),
                ],
                egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 60)),
            );
        }
    }

    fn draw_notes(
        &mut self,
        painter: &egui::Painter,
        response: &egui::Response,
        rect: egui::Rect,
        piano_key_width: f32,
        min_pitch: u8,
        max_pitch: u8,
    ) -> Option<NoteModification> {
        let mut modification = None;
        let mut note_to_delete: Option<usize> = None;

        for (idx, note) in self.pattern.notes.iter().enumerate() {
            if note.pitch < min_pitch || note.pitch > max_pitch {
                continue;
            }

            let note_x = self.beat_to_screen_x(note.start_beat, rect, piano_key_width);
            let note_width = note.duration_beats * self.state.horizontal_zoom;
            let note_y = self.pitch_to_screen_y(note.pitch, rect);

            let note_rect = egui::Rect::from_min_size(
                egui::Pos2::new(note_x, note_y),
                egui::Vec2::new(note_width, self.state.vertical_zoom),
            );

            if note_rect.right() < rect.left() + piano_key_width || note_rect.left() > rect.right()
            {
                continue;
            }

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
            self.pattern.notes.remove(idx);
            modification = Some(NoteModification::Deleted);
        }

        if response.clicked() {
            if let Some(click_pos) = response.interact_pointer_pos() {
                if click_pos.x > rect.left() + piano_key_width {
                    let pitch = self.screen_y_to_pitch(click_pos.y, rect);
                    let beat = self.screen_x_to_beat(click_pos.x, rect, piano_key_width);

                    if pitch >= min_pitch && pitch <= max_pitch && beat >= 0.0 {
                        let snapped_beat = beat.round();

                        let time_signature = self.pattern.time_signature;
                        let beats_per_bar = time_signature.0 as f32;
                        let total_beats = beats_per_bar * self.pattern.duration_bars as f32;

                        if snapped_beat < total_beats {
                            let note_exists = self.pattern.notes.iter().any(|n| {
                                n.pitch == pitch && (n.start_beat - snapped_beat).abs() < 0.1
                            });

                            if !note_exists {
                                self.pattern.notes.push(Note {
                                    pitch,
                                    velocity: 100,
                                    start_beat: snapped_beat,
                                    duration_beats: 1.0,
                                });
                                modification = Some(NoteModification::Added);
                            }
                        }
                    }
                }
            }
        }

        modification
    }

    fn pitch_to_screen_y(&self, pitch: u8, rect: egui::Rect) -> f32 {
        let pitches_from_bottom = pitch as f32 - self.state.pan_y;
        rect.bottom() - (pitches_from_bottom * self.state.vertical_zoom)
    }

    fn screen_y_to_pitch(&self, y: f32, rect: egui::Rect) -> u8 {
        let relative_y = rect.bottom() - y;
        let pitch = self.state.pan_y + (relative_y / self.state.vertical_zoom);
        pitch.clamp(0.0, 127.0) as u8
    }

    fn beat_to_screen_x(&self, beat: f32, rect: egui::Rect, piano_key_width: f32) -> f32 {
        let beats_from_left = beat - self.state.pan_x;
        rect.left() + piano_key_width + (beats_from_left * self.state.horizontal_zoom)
    }

    fn screen_x_to_beat(&self, x: f32, rect: egui::Rect, piano_key_width: f32) -> f32 {
        let relative_x = x - rect.left() - piano_key_width;
        self.state.pan_x + (relative_x / self.state.horizontal_zoom)
    }
}

pub struct PianoRollResponse {
    pub modified: bool,
}

enum NoteModification {
    Added,
    Deleted,
}
