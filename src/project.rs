use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::{
    audio::{ADSRConfig, Instrument},
    timing::StateGraph,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleRef {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackData {
    pub id: usize,
    pub name: String,
    pub instrument: Instrument,
    pub adsr: ADSRConfig,
    pub volume: f32,
    pub pan: f32,
    pub initial_node: String,
    pub graph: StateGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub bpm: f32,
    pub sample_rate: u32,
    pub sample_library: Vec<SampleRef>,
    pub tracks: Vec<TrackData>,
}

impl Project {
    pub fn save(&self, project_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(project_path)?;

        let samples_dir = project_path.join("samples");
        fs::create_dir_all(&samples_dir)?;

        let ron_path = project_path.join("project.ron");
        let ron_string = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        fs::write(ron_path, ron_string)?;

        Ok(())
    }

    pub fn load(project_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let ron_path = project_path.join("project.ron");
        let ron_string = fs::read_to_string(ron_path)?;
        let project: Project = ron::from_str(&ron_string)?;

        Ok(project)
    }
}
