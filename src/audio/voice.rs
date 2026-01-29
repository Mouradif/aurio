use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ADSRConfig {
    /// Seconds
    pub attack: f32,
    /// Seconds
    pub decay: f32,
    /// 0.0 -> 1.0
    pub sustain: f32,
    /// Seconds
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

pub fn calculate_envelope(state: &NoteState, adsr: &ADSRConfig) -> f32 {
    match &state.envelope_state {
        EnvelopeState::Attack { time } => {
            if adsr.attack == 0.0 {
                1.0
            } else {
                (time / adsr.attack).min(1.0)
            }
        }
        EnvelopeState::Decay { time } => {
            let decay_progress = if adsr.decay == 0.0 {
                1.0
            } else {
                (time / adsr.decay).min(1.0)
            };
            1.0 - (1.0 - adsr.sustain) * decay_progress
        }
        EnvelopeState::Sustain => adsr.sustain,
        EnvelopeState::Release { time } => {
            let release_progress = if adsr.release == 0.0 {
                1.0
            } else {
                (time / adsr.release).min(1.0)
            };
            state.envelope_level * (1.0 - release_progress)
        }
    }
}

pub fn advance_envelope(
    state: &mut NoteState,
    adsr: &ADSRConfig,
    sample_rate: f32,
    buffer_size: usize,
) {
    let dt = buffer_size as f32 / sample_rate;

    match &mut state.envelope_state {
        EnvelopeState::Attack { time } => {
            *time += dt;
            if *time >= adsr.attack {
                state.envelope_state = EnvelopeState::Decay { time: 0.0 };
                state.envelope_level = 1.0;
            } else {
                state.envelope_level = calculate_envelope(state, adsr);
            }
        }
        EnvelopeState::Decay { time } => {
            *time += dt;
            if *time >= adsr.decay {
                state.envelope_state = EnvelopeState::Sustain;
                state.envelope_level = adsr.sustain;
            } else {
                state.envelope_level = calculate_envelope(state, adsr);
            }
        }
        EnvelopeState::Sustain => {
            state.envelope_level = adsr.sustain;
        }
        EnvelopeState::Release { time } => {
            *time += dt;
            state.envelope_level = calculate_envelope(state, adsr);
        }
    }
}
