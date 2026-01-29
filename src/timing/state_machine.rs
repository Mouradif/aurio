use super::Sequence;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Hook {
    OnEnter,
    OnLeave,
    OnStart,
    OnEnd,
    OnLoop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransitionTiming {
    Immediate,
    NextBeat,
    NextBar,
    FinishSequence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub sequence: Sequence,
    pub hooks: Vec<(Hook, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub condition: String,
    pub timing: TransitionTiming,
    pub inlet_hook: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl StateGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn get_outgoing_edges(&self, node_id: &str) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.from == node_id).collect()
    }
}
