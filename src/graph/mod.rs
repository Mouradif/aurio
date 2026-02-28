pub mod topo;

use crate::audio::{ADSRConfig, Instrument};
use crate::timing::{GeneratedPattern, Sequence, StaticPattern};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        NodeId(s.to_string())
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        NodeId(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub position: (f32, f32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    PianoRoll { pattern: StaticPattern },
    PatternScript { script: GeneratedPattern },
    Synth { instrument: Instrument, adsr: ADSRConfig },
    Mixer { channels: Vec<MixerChannel> },
    Output { device: Option<String> },
}

impl NodeKind {
    pub fn event_inputs(&self) -> usize {
        match self {
            NodeKind::Synth { .. } => 1,
            _ => 0,
        }
    }

    pub fn event_outputs(&self) -> usize {
        match self {
            NodeKind::PianoRoll { .. } | NodeKind::PatternScript { .. } => 1,
            _ => 0,
        }
    }

    pub fn audio_inputs(&self) -> Option<usize> {
        match self {
            NodeKind::Mixer { channels } => Some(channels.len()),
            NodeKind::Output { .. } => Some(1),
            _ => None,
        }
    }

    pub fn audio_outputs(&self) -> usize {
        match self {
            NodeKind::Synth { .. } | NodeKind::Mixer { .. } => 1,
            _ => 0,
        }
    }

    pub fn is_pattern(&self) -> bool {
        matches!(self, NodeKind::PianoRoll { .. } | NodeKind::PatternScript { .. })
    }

    pub fn sequence(&self) -> Option<Sequence> {
        match self {
            NodeKind::PianoRoll { pattern } => Some(Sequence::Static(pattern.clone())),
            NodeKind::PatternScript { script } => Some(Sequence::Generated(script.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixerChannel {
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

impl Default for MixerChannel {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortKind {
    Event,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from_node: NodeId,
    pub from_port: usize,
    pub to_node: NodeId,
    pub to_port: usize,
    pub kind: PortKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachineEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub condition: String,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub state_machine_edges: Vec<StateMachineEdge>,
    pub initial_pattern_nodes: Vec<NodeId>,
}

impl AudioGraph {
    pub fn get_node(&self, id: &NodeId) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    pub fn get_node_mut(&mut self, id: &NodeId) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| &n.id == id)
    }

    pub fn get_sm_outgoing(&self, from: &NodeId) -> Vec<&StateMachineEdge> {
        let mut edges: Vec<&StateMachineEdge> = self
            .state_machine_edges
            .iter()
            .filter(|e| &e.from == from)
            .collect();
        edges.sort_by_key(|e| e.priority);
        edges
    }

    pub fn generate_node_id(&self, prefix: &str) -> NodeId {
        let mut counter = 0;
        loop {
            let candidate = NodeId(format!("{}-{}", prefix, counter));
            if !self.nodes.iter().any(|n| n.id == candidate) {
                return candidate;
            }
            counter += 1;
        }
    }

    pub fn get_connected_target(&self, pattern_node: &NodeId) -> Option<&NodeId> {
        self.edges
            .iter()
            .find(|e| &e.from_node == pattern_node && e.kind == PortKind::Event)
            .map(|e| &e.to_node)
    }
}
