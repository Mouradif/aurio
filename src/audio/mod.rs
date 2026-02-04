mod instrument;
mod track;
mod voice;

pub use instrument::{Instrument, OscConfig, Wave};
pub use track::{NotePlaybackState, PlaybackState, TrackConfig};
pub use voice::{ADSRConfig, EnvelopeState, NoteState};

pub fn midi_to_freq(note: u8) -> f32 {
    // Multiplying any frequency by 2 makes it one octave (12 semi-tones) higher
    // so we can find the frequency that is exactly 1 semi-tone higher by
    // multiplying by 2 raised to the power 1/12. More generally, starting from
    // any given frequency, we can find the frequency that is exactly X semi-
    // tones higher by multiplying is by 2.pow(X / 12).
    //
    // This works in both directions: if we want to find a frequency that is X
    // semi-tones lower we can multiply by 2.pow(-X / 12)
    //
    // by starting from A4 (semi-tone 69 in MIDI and with a frequency of 440Hz)
    // we can find the frequency of the midi note `note` using this formula:

    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}
