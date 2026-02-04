use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Sequence {
    Static(StaticPattern),
    Generated(GeneratedPattern),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticPattern {
    pub duration_bars: u32,
    pub time_signature: (u32, u32),
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f32,
    pub duration_beats: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedPattern {
    pub duration_bars: u32,
    pub time_signature: (u32, u32),
    pub function: String,
}

/// Takes a Vec<Notes> where there may be overlap between notes of the same
/// pitch and returns a normalized Vec<Notes> with the guarantee that there
/// will be no such overlap (overlapping notes will be merged)
fn normalize_notes(mut notes: Vec<Note>) -> Vec<Note> {
    let mut by_pitch: HashMap<u8, Vec<Note>> = HashMap::new();
    for note in notes.drain(..) {
        by_pitch.entry(note.pitch).or_default().push(note);
    }

    let mut result = Vec::new();

    for (_pitch, mut group) in by_pitch {
        group.sort_by(|a, b| a.start_beat.partial_cmp(&b.start_beat).unwrap());

        let mut current = group[0].clone();

        for note in group.into_iter().skip(1) {
            let current_end = current.start_beat + current.duration_beats;
            let note_end = note.start_beat + note.duration_beats;

            if note.start_beat <= current_end {
                let new_end = current_end.max(note_end);
                current.duration_beats = new_end - current.start_beat;
                current.velocity = current.velocity.max(note.velocity);
            } else {
                result.push(current);
                current = note;
            }
        }

        result.push(current);
    }

    result
}

impl Sequence {
    pub fn duration_samples(&self, bpm: f32, sample_rate: f32) -> usize {
        let (bars, time_sig) = match self {
            Sequence::Static(p) => (p.duration_bars, p.time_signature),
            Sequence::Generated(p) => (p.duration_bars, p.time_signature),
        };

        let beats_per_bar = time_sig.0 as f32;
        let beat_unit = time_sig.1 as f32;

        let total_quarter_notes = (beats_per_bar * bars as f32) * (4.0 / beat_unit);
        let samples_per_quarter = (60.0 / bpm) * sample_rate;

        (total_quarter_notes * samples_per_quarter) as usize
    }

    pub fn get_notes(&self, lua_runtime: Option<&crate::scripting::LuaRuntime>) -> Vec<Note> {
        let notes = match self {
            Sequence::Static(pattern) => pattern.notes.clone(),
            Sequence::Generated(pattern) => {
                if let Some(runtime) = lua_runtime {
                    runtime
                        .execute_pattern(&pattern.function)
                        .unwrap_or_else(|e| {
                            eprintln!("Lua error: {}", e);
                            Vec::new()
                        })
                } else {
                    Vec::new()
                }
            }
        };

        normalize_notes(notes)
    }
}
