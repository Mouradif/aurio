use super::voice::{ADSRConfig, EnvelopeState};
use super::{Instrument, Wave, midi_to_freq};

#[derive(Debug, Clone)]
pub struct TrackConfig {
    pub id: usize,
    pub instrument: Instrument,
    pub adsr: ADSRConfig,
    pub volume: f32,
    pub pan: f32,
}

impl TrackConfig {
    pub fn new(id: usize, instrument: Instrument, adsr: ADSRConfig) -> Self {
        Self {
            id,
            instrument,
            adsr,
            volume: 1.0,
            pan: 0.0,
        }
    }

    pub fn num_oscillators(&self) -> usize {
        match &self.instrument {
            Instrument::MultiOsc { oscillators } => oscillators.len(),
            Instrument::Sampler { .. } => 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NotePlaybackState {
    pub velocity: u8,
    pub envelope_state: EnvelopeState,
    pub envelope_level: f32,
    pub oscillator_phases: Vec<f32>,
    pub sample_position: f32,
}

impl NotePlaybackState {
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

pub struct PlaybackState {
    pub notes: [Option<NotePlaybackState>; 128],
}

impl PlaybackState {
    pub fn new() -> Self {
        Self {
            notes: std::array::from_fn(|_| None),
        }
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8, num_oscillators: usize) {
        self.notes[pitch as usize] = Some(NotePlaybackState::new(velocity, num_oscillators));
    }

    pub fn note_off(&mut self, pitch: u8) {
        if let Some(state) = &mut self.notes[pitch as usize] {
            state.envelope_state = EnvelopeState::Release { time: 0.0 };
        }
    }

    pub fn stop_all(&mut self) {
        for note in &mut self.notes {
            *note = None;
        }
    }

    pub fn render_sample(&mut self, config: &TrackConfig, sample_rate: f32) -> f32 {
        let mut output = 0.0;

        for pitch in 0..128u8 {
            let should_remove = if let Some(state) = &mut self.notes[pitch as usize] {
                let envelope = calculate_envelope_from_playback(state, &config.adsr);
                let velocity_scale = state.velocity as f32 / 127.0;

                match &config.instrument {
                    Instrument::MultiOsc { oscillators } => {
                        for (i, osc) in oscillators.iter().enumerate() {
                            let note = (pitch as i8 + osc.semitone) as u8;
                            let freq = midi_to_freq(note);

                            let phase = state.oscillator_phases[i];
                            let sample = match osc.wave {
                                Wave::Sine => (phase * 2.0 * std::f32::consts::PI).sin(),
                                Wave::Square => {
                                    if phase < 0.5 {
                                        -1.0
                                    } else {
                                        1.0
                                    }
                                }
                                Wave::Saw => phase * 2.0 - 1.0,
                            };

                            output += sample * envelope * velocity_scale * osc.gain;

                            state.oscillator_phases[i] += freq / sample_rate;
                            if state.oscillator_phases[i] >= 1.0 {
                                state.oscillator_phases[i] -= 1.0;
                            }
                        }
                    }
                    Instrument::Sampler { .. } => {
                        // TODO: Implement sampler rendering
                    }
                }

                advance_envelope_one_sample_playback(state, &config.adsr, sample_rate);
                matches!(state.envelope_state, EnvelopeState::Release { time } if time > config.adsr.release)
            } else {
                false
            };

            if should_remove {
                self.notes[pitch as usize] = None;
            }
        }

        output
    }
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::new()
    }
}

fn calculate_envelope_from_playback(state: &NotePlaybackState, adsr: &ADSRConfig) -> f32 {
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

fn advance_envelope_one_sample_playback(
    state: &mut NotePlaybackState,
    adsr: &ADSRConfig,
    sample_rate: f32,
) {
    let dt = 1.0 / sample_rate;

    match &mut state.envelope_state {
        EnvelopeState::Attack { time } => {
            *time += dt;
            if *time >= adsr.attack {
                state.envelope_state = EnvelopeState::Decay { time: 0.0 };
                state.envelope_level = 1.0;
            } else {
                state.envelope_level = if adsr.attack == 0.0 {
                    1.0
                } else {
                    (*time / adsr.attack).min(1.0)
                };
            }
        }
        EnvelopeState::Decay { time } => {
            *time += dt;
            if *time >= adsr.decay {
                state.envelope_state = EnvelopeState::Sustain;
                state.envelope_level = adsr.sustain;
            } else {
                let decay_progress = if adsr.decay == 0.0 {
                    1.0
                } else {
                    (*time / adsr.decay).min(1.0)
                };
                state.envelope_level = 1.0 - (1.0 - adsr.sustain) * decay_progress;
            }
        }
        EnvelopeState::Sustain => {
            state.envelope_level = adsr.sustain;
        }
        EnvelopeState::Release { time } => {
            *time += dt;
            let release_progress = if adsr.release == 0.0 {
                1.0
            } else {
                (*time / adsr.release).min(1.0)
            };
            state.envelope_level *= 1.0 - release_progress;
        }
    }
}
