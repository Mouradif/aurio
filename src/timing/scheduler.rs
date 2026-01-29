use super::Sequence;
use crate::events::{Event, ScheduledEvent};
use crossbeam::queue::ArrayQueue;
use std::sync::Arc;

pub fn schedule_sequence_events(
    sequence: &Sequence,
    track_id: usize,
    start_sample: u64,
    bpm: f32,
    sample_rate: f32,
    queue: &Arc<ArrayQueue<ScheduledEvent>>,
    lua_runtime: Option<&crate::scripting::LuaRuntime>,
) {
    let notes = match sequence {
        Sequence::Static(pattern) => pattern.notes.clone(),
        Sequence::Generated(_pattern) => sequence.get_notes(lua_runtime),
    };

    let samples_per_beat = (60.0 / bpm) * sample_rate;
    let sequence_duration = sequence.duration_samples(bpm, sample_rate) as u64;

    for note in notes {
        let note_on_sample = start_sample + (note.start_beat * samples_per_beat) as u64;

        if note_on_sample < start_sample + sequence_duration {
            let _ = queue.push(ScheduledEvent {
                sample_timestamp: note_on_sample,
                event: Event::MidiEvent {
                    track_id,
                    pitch: note.pitch,
                    velocity: note.velocity,
                    is_note_on: true,
                },
            });
        }

        let note_off_sample =
            start_sample + ((note.start_beat + note.duration_beats) * samples_per_beat) as u64;

        if note_off_sample <= start_sample + sequence_duration {
            let _ = queue.push(ScheduledEvent {
                sample_timestamp: note_off_sample,
                event: Event::MidiEvent {
                    track_id,
                    pitch: note.pitch,
                    velocity: note.velocity,
                    is_note_on: false,
                },
            });
        }
    }
}
