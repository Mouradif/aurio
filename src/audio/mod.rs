mod instrument;
mod track;
mod voice;

pub use instrument::{Instrument, OscConfig, Wave};
pub use track::Track;
pub use voice::{ADSRConfig, EnvelopeState, NoteState};

pub fn midi_to_freq(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}
