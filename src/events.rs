use crate::graph::NodeId;

#[derive(Debug, Clone)]
pub struct ScheduledEvent {
    pub sample_timestamp: u64,
    pub event: Event,
}

#[derive(Debug, Clone)]
pub enum MidiMessage {
    NoteOn { pitch: u8, velocity: u8 },
    NoteOff { pitch: u8 },
}

#[derive(Debug, Clone)]
pub enum Event {
    MidiEvent {
        target_node: NodeId,
        message: MidiMessage,
    },
    StopAllNotes {
        target_node: NodeId,
    },
    NodeTransition {
        pattern_node: NodeId,
        new_sm_state: NodeId,
    },
}
