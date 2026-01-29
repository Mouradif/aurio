#[derive(Debug, Clone)]
pub struct ScheduledEvent {
    pub sample_timestamp: u64,
    pub event: Event,
}

#[derive(Debug, Clone)]
pub enum Event {
    MidiEvent {
        track_id: usize,
        pitch: u8,
        velocity: u8,
        is_note_on: bool,
    },
    NodeTransition {
        track_id: usize,
        new_node_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum MidiMessage {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
}
