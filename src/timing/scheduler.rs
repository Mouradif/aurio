use super::Sequence;
use crate::events::{Event, ScheduledEvent};
use ringbuf::traits::Producer;

pub type EventProducer = ringbuf::HeapProd<ScheduledEvent>;

pub fn schedule_sequence_events(
    sequence: &Sequence,
    track_id: usize,
    start_sample: u64,
    bpm: f32,
    sample_rate: f32,
    producer: &mut EventProducer,
    lua_runtime: Option<&crate::scripting::LuaRuntime>,
) -> Result<(), SchedulerError> {
    let notes = match sequence {
        Sequence::Static(pattern) => pattern.notes.clone(),
        Sequence::Generated(_pattern) => sequence.get_notes(lua_runtime),
    };

    let samples_per_beat = (60.0 / bpm) * sample_rate;
    let sequence_duration = sequence.duration_samples(bpm, sample_rate) as u64;

    let mut events: Vec<ScheduledEvent> = Vec::with_capacity(notes.len() * 2);

    for note in notes {
        let note_on_sample = start_sample + (note.start_beat * samples_per_beat) as u64;

        if note_on_sample < start_sample + sequence_duration {
            events.push(ScheduledEvent {
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
            events.push(ScheduledEvent {
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

    events.sort_by_key(|e| e.sample_timestamp);
    for event in events {
        if producer.try_push(event).is_err() {
            return Err(SchedulerError::BufferFull);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub enum SchedulerError {
    BufferFull,
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulerError::BufferFull => write!(f, "Event buffer is full"),
        }
    }
}

impl std::error::Error for SchedulerError {}
