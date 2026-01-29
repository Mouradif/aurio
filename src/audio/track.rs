use std::collections::HashMap;
use super::{Instrument, Wave, ADSRConfig, NoteState, voice::calculate_envelope, voice::advance_envelope, midi_to_freq};
use crate::timing::StateGraph;

pub struct Track {
    pub id: usize,
    pub instrument: Instrument,
    pub adsr: ADSRConfig,
    pub volume: f32,
    pub pan: f32,
    pub graph: StateGraph,
    pub current_node: String,
    pub active_notes: HashMap<u8, NoteState>,
}

impl Track {
    pub fn new(id: usize, instrument: Instrument, adsr: ADSRConfig) -> Self {
        Self {
            id,
            instrument,
            adsr,
            volume: 1.0,
            pan: 0.0,
            graph: StateGraph::new(),
            current_node: String::new(),
            active_notes: HashMap::new(),
        }
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        let num_oscs = match &self.instrument {
            Instrument::MultiOsc { oscillators } => oscillators.len(),
            Instrument::Sampler { .. } => 0,
        };

        self.active_notes.insert(pitch, NoteState::new(velocity, num_oscs));
    }

    pub fn note_off(&mut self, pitch: u8) {
        if let Some(state) = self.active_notes.get_mut(&pitch) {
            state.envelope_state = super::EnvelopeState::Release { time: 0.0 };
        }
    }

    pub fn stop_all_notes(&mut self) {
        self.active_notes.clear();
    }

    pub fn render_audio(&mut self, output: &mut [f32], sample_rate: f32) {
        output.fill(0.0);

        for (pitch, state) in &mut self.active_notes {
            let envelope = calculate_envelope(state, &self.adsr);
            let velocity_scale = state.velocity as f32 / 127.0;

            match &self.instrument {
                Instrument::MultiOsc { oscillators } => {
                    for (i, osc) in oscillators.iter().enumerate() {
                        let note = (*pitch as i8 + osc.semitone) as u8;
                        let freq = midi_to_freq(note);

                        for j in 0..output.len() {
                            let phase = state.oscillator_phases[i];

                            let sample = match osc.wave {
                                Wave::Sine => (phase * 2.0 * std::f32::consts::PI).sin(),
                                Wave::Square => if phase < 0.5 { -1.0 } else { 1.0 },
                                Wave::Saw => phase * 2.0 - 1.0,
                            };

                            output[j] += sample * envelope * velocity_scale * osc.gain;

                            state.oscillator_phases[i] += freq / sample_rate;
                            if state.oscillator_phases[i] >= 1.0 {
                                state.oscillator_phases[i] -= 1.0;
                            }
                        }
                    }
                }
                Instrument::Sampler { .. } => {
                    // TODO: Implement sampler rendering
                }
            }

            advance_envelope(state, &self.adsr, sample_rate, output.len());
        }

        for sample in output.iter_mut() {
            *sample *= self.volume;
        }

        self.active_notes.retain(|_, state| {
            !matches!(state.envelope_state, super::EnvelopeState::Release { time }
                if time > self.adsr.release)
        });
    }
}
