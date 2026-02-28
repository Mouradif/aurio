use super::{AudioGraph, GraphEdge, NodeId, PortKind};
use std::collections::{HashMap, VecDeque};

#[derive(Debug)]
pub enum CycleError {
    CycleDetected,
}

impl std::fmt::Display for CycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CycleError::CycleDetected => write!(f, "Cycle detected in audio graph"),
        }
    }
}

impl std::error::Error for CycleError {}

#[derive(Debug)]
pub enum EdgeError {
    NodeNotFound(NodeId),
    InvalidPortKind { expected: PortKind, got: PortKind },
    PortOutOfRange { node: NodeId, port: usize, max: usize },
}

impl std::fmt::Display for EdgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeError::NodeNotFound(id) => write!(f, "Node not found: {}", id),
            EdgeError::InvalidPortKind { expected, got } => {
                write!(f, "Invalid port kind: expected {:?}, got {:?}", expected, got)
            }
            EdgeError::PortOutOfRange { node, port, max } => {
                write!(f, "Port {} out of range for node {} (max {})", port, node, max)
            }
        }
    }
}

impl std::error::Error for EdgeError {}

pub fn topological_order(graph: &AudioGraph) -> Result<Vec<NodeId>, CycleError> {
    let mut in_degree: HashMap<&NodeId, usize> = HashMap::new();
    let mut adjacency: HashMap<&NodeId, Vec<&NodeId>> = HashMap::new();

    for node in &graph.nodes {
        in_degree.entry(&node.id).or_insert(0);
        adjacency.entry(&node.id).or_default();
    }

    for edge in &graph.edges {
        *in_degree.entry(&edge.to_node).or_insert(0) += 1;
        adjacency.entry(&edge.from_node).or_default().push(&edge.to_node);
    }

    let mut queue: VecDeque<&NodeId> = VecDeque::new();
    for (node_id, &degree) in &in_degree {
        if degree == 0 {
            queue.push_back(node_id);
        }
    }

    let mut result = Vec::new();
    while let Some(node_id) = queue.pop_front() {
        result.push(node_id.clone());
        if let Some(neighbors) = adjacency.get(node_id) {
            for neighbor in neighbors {
                if let Some(degree) = in_degree.get_mut(neighbor) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    if result.len() != graph.nodes.len() {
        return Err(CycleError::CycleDetected);
    }

    Ok(result)
}

pub fn validate_edge(graph: &AudioGraph, edge: &GraphEdge) -> Result<(), EdgeError> {
    let from_node = graph
        .get_node(&edge.from_node)
        .ok_or_else(|| EdgeError::NodeNotFound(edge.from_node.clone()))?;
    let to_node = graph
        .get_node(&edge.to_node)
        .ok_or_else(|| EdgeError::NodeNotFound(edge.to_node.clone()))?;

    match edge.kind {
        PortKind::Event => {
            let max_out = from_node.kind.event_outputs();
            if edge.from_port >= max_out {
                return Err(EdgeError::PortOutOfRange {
                    node: edge.from_node.clone(),
                    port: edge.from_port,
                    max: max_out,
                });
            }
            let max_in = to_node.kind.event_inputs();
            if edge.to_port >= max_in {
                return Err(EdgeError::PortOutOfRange {
                    node: edge.to_node.clone(),
                    port: edge.to_port,
                    max: max_in,
                });
            }
        }
        PortKind::Audio => {
            let max_out = from_node.kind.audio_outputs();
            if edge.from_port >= max_out {
                return Err(EdgeError::PortOutOfRange {
                    node: edge.from_node.clone(),
                    port: edge.from_port,
                    max: max_out,
                });
            }
            let max_in = to_node.kind.audio_inputs().unwrap_or(0);
            if edge.to_port >= max_in {
                return Err(EdgeError::PortOutOfRange {
                    node: edge.to_node.clone(),
                    port: edge.to_port,
                    max: max_in,
                });
            }
        }
    }

    Ok(())
}
