use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::audio::{ADSRConfig, Instrument};
use crate::graph::{
    AudioGraph, GraphEdge, GraphNode, MixerChannel, NodeId, NodeKind, PortKind,
    StateMachineEdge,
};
use crate::timing::StateGraph;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleRef {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub bpm: f32,
    pub sample_rate: u32,
    pub sample_library: Vec<SampleRef>,
    pub graph: AudioGraph,
}

// Legacy types for migration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyTrackData {
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
struct LegacyProject {
    pub name: String,
    pub version: String,
    pub bpm: f32,
    pub sample_rate: u32,
    pub sample_library: Vec<SampleRef>,
    pub tracks: Vec<LegacyTrackData>,
}

impl Project {
    pub fn new_empty(name: String) -> Self {
        Self {
            name,
            version: "0.1.0".to_string(),
            bpm: 120.0,
            sample_rate: 44100,
            sample_library: Vec::new(),
            graph: AudioGraph {
                nodes: Vec::new(),
                edges: Vec::new(),
                state_machine_edges: Vec::new(),
                initial_pattern_nodes: Vec::new(),
            },
        }
    }

    fn from_legacy(legacy: LegacyProject) -> Self {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut sm_edges = Vec::new();
        let mut initial_pattern_nodes = Vec::new();
        let mut mixer_channels = Vec::new();

        let mut synth_node_ids = Vec::new();

        for track in &legacy.tracks {
            let synth_id = NodeId(format!("synth-{}", track.id));

            // Create pattern nodes from the track's state graph
            for (i, sm_node) in track.graph.nodes.iter().enumerate() {
                let pattern_id = NodeId(sm_node.id.clone());
                let x = 100.0 + (i as f32 * 200.0);
                let y = 100.0 + (track.id as f32 * 300.0);

                let kind = match &sm_node.sequence {
                    crate::timing::Sequence::Static(pattern) => NodeKind::PianoRoll {
                        pattern: pattern.clone(),
                    },
                    crate::timing::Sequence::Generated(script) => NodeKind::PatternScript {
                        script: script.clone(),
                    },
                };

                nodes.push(GraphNode {
                    id: pattern_id.clone(),
                    name: sm_node.id.clone(),
                    kind,
                    position: (x, y),
                });

                // Connect pattern → synth via event edge
                edges.push(GraphEdge {
                    from_node: pattern_id,
                    from_port: 0,
                    to_node: synth_id.clone(),
                    to_port: 0,
                    kind: PortKind::Event,
                });
            }

            // State machine edges
            for sm_edge in &track.graph.edges {
                sm_edges.push(StateMachineEdge {
                    from: NodeId(sm_edge.from.clone()),
                    to: NodeId(sm_edge.to.clone()),
                    condition: sm_edge.condition.clone(),
                    priority: 0,
                });
            }

            // Initial pattern node
            initial_pattern_nodes.push(NodeId(track.initial_node.clone()));

            // Create Synth node
            let synth_x = 500.0;
            let synth_y = 200.0 + (track.id as f32 * 300.0);
            nodes.push(GraphNode {
                id: synth_id.clone(),
                name: track.name.clone(),
                kind: NodeKind::Synth {
                    instrument: track.instrument.clone(),
                    adsr: track.adsr.clone(),
                },
                position: (synth_x, synth_y),
            });

            synth_node_ids.push(synth_id);

            mixer_channels.push(MixerChannel {
                volume: track.volume,
                pan: track.pan,
                mute: false,
                solo: false,
            });
        }

        // Create Mixer node
        let mixer_id = NodeId("mixer".to_string());
        nodes.push(GraphNode {
            id: mixer_id.clone(),
            name: "Main Mixer".to_string(),
            kind: NodeKind::Mixer {
                channels: mixer_channels,
            },
            position: (700.0, 300.0),
        });

        // Connect all synths → mixer
        for (i, synth_id) in synth_node_ids.iter().enumerate() {
            edges.push(GraphEdge {
                from_node: synth_id.clone(),
                from_port: 0,
                to_node: mixer_id.clone(),
                to_port: i,
                kind: PortKind::Audio,
            });
        }

        // Create Output node
        let output_id = NodeId("output".to_string());
        nodes.push(GraphNode {
            id: output_id.clone(),
            name: "Output".to_string(),
            kind: NodeKind::Output { device: None },
            position: (900.0, 300.0),
        });

        // Connect mixer → output
        edges.push(GraphEdge {
            from_node: mixer_id,
            from_port: 0,
            to_node: output_id,
            to_port: 0,
            kind: PortKind::Audio,
        });

        Project {
            name: legacy.name,
            version: legacy.version,
            bpm: legacy.bpm,
            sample_rate: legacy.sample_rate,
            sample_library: legacy.sample_library,
            graph: AudioGraph {
                nodes,
                edges,
                state_machine_edges: sm_edges,
                initial_pattern_nodes,
            },
        }
    }

    pub fn save(&self, project_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        fs::create_dir_all(project_path)?;

        let samples_dir = project_path.join("samples");
        fs::create_dir_all(&samples_dir)?;

        let ron_path = project_path.join("manifest.ron");
        let ron_string = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        fs::write(ron_path, ron_string)?;

        Ok(())
    }

    pub fn load(project_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        // Try new format first
        let manifest_path = project_path.join("manifest.ron");
        if manifest_path.exists() {
            let ron_string = fs::read_to_string(manifest_path)?;
            let project: Project = ron::from_str(&ron_string)?;
            return Ok(project);
        }

        // Fall back to legacy format
        let legacy_path = project_path.join("project.ron");
        if legacy_path.exists() {
            let ron_string = fs::read_to_string(legacy_path)?;
            let legacy: LegacyProject = ron::from_str(&ron_string)?;
            tracing::info!("Migrating legacy project format to new graph format");
            return Ok(Project::from_legacy(legacy));
        }

        Err("No project file found (expected manifest.ron or project.ron)".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_manifest() {
        let project = Project::load(Path::new("TestProject.aurio")).unwrap();
        assert_eq!(project.name, "Test Project");
        assert_eq!(project.graph.nodes.len(), 5);
        assert_eq!(project.graph.edges.len(), 4);
        assert_eq!(project.graph.state_machine_edges.len(), 2);
        assert_eq!(project.graph.initial_pattern_nodes.len(), 1);
    }

    #[test]
    fn test_load_legacy_migration() {
        // Create a temp dir with only project.ron (legacy format)
        let temp_dir = std::env::temp_dir().join("aurio_test_legacy");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        // Copy legacy project.ron
        fs::copy(
            "TestProject.aurio/project.ron",
            temp_dir.join("project.ron"),
        )
        .unwrap();

        let project = Project::load(&temp_dir).unwrap();
        assert_eq!(project.name, "Test Project");
        // Legacy had 1 track → should produce: 2 patterns + 1 synth + 1 mixer + 1 output = 5 nodes
        assert_eq!(project.graph.nodes.len(), 5);
        assert!(project.graph.initial_pattern_nodes.len() == 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
