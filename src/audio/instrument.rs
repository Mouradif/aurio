use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Wave {
    Sine,
    Square,
    Saw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OscConfig {
    pub wave: Wave,
    pub gain: f32,
    pub semitone: i8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instrument {
    MultiOsc { oscillators: Vec<OscConfig> },
    Sampler { sample_id: String, root_pitch: u8 },
}
