use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ADSRConfig {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnvelopeState {
    Attack { time: f32 },
    Decay { time: f32 },
    Sustain,
    Release { time: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteState {
    pub velocity: u8,
    pub envelope_state: EnvelopeState,
    pub envelope_level: f32,
    pub oscillator_phases: Vec<f32>,
    pub sample_position: f32,
}

impl NoteState {
    pub fn new(velocity: u8, num_oscillators: usize) -> Self {
        Self {
            velocity,
            envelope_state: EnvelopeState::Attack { time: 0.0 },
            envelope_level: 0.0,
            oscillator_phases: vec![0.0; num_oscillators],
            sample_position: 0.0,
        }
    }
}
