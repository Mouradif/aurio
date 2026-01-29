use std::{collections::HashSet, sync::atomic::AtomicU32};

use crate::{AudioGraph, GainState, Node, NodeState, OscillatorState, OutputState, Wave, Wire};

fn strip_comment(s: &str) -> &str {
    s.split('#').next().unwrap_or("")
}

fn parse_node(line: &str) -> Result<Node, String> {
    let end = line.find(']').ok_or("missing ']'")?;
    let id: u32 = line[1..end].trim().parse().map_err(|_| "invalid node id")?;

    let rest = line[end + 1..].trim();
    let mut parts = rest.split_whitespace();

    let inner = match parts.next().ok_or("missing node type")? {
        "Osc" => {
            let osc_type = match parts.next().ok_or("missing wave type")? {
                "Sine" => Wave::Sine,
                "Square" => Wave::Square,
                "Saw" => Wave::Saw,
                other => return Err(format!("unknown wave '{other}'")),
            };

            let freq: f32 = parts
                .next()
                .ok_or("missing frequency")?
                .parse()
                .map_err(|_| "invalid frequency")?;

            NodeState::Oscillator(OscillatorState {
                osc_type,
                freq,
                phase: AtomicU32::new(0),
            })
        }

        "Gain" => {
            let value: f32 = parts
                .next()
                .ok_or("missing gain")?
                .parse()
                .map_err(|_| "invalid gain")?;

            NodeState::Gain(GainState { value })
        }

        "Out" => NodeState::Output(OutputState {}),

        other => return Err(format!("unknown node type '{other}'")),
    };

    Ok(Node { id, inner })
}

fn parse_wires(line: &str) -> Result<Vec<Wire>, String> {
    let mut wires = Vec::new();

    for part in line.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (from, to) = part
            .split_once("->")
            .ok_or("invalid wire syntax, expected a->b")?;

        let from_node_id: u32 = from.trim().parse().map_err(|_| "invalid wire source")?;
        let to_node_id: u32 = to.trim().parse().map_err(|_| "invalid wire destination")?;

        wires.push(Wire {
            from_node_id,
            from_output_idx: 0,
            to_node_id,
            to_input_idx: 0,
        });
    }

    Ok(wires)
}

fn validate_wires(nodes: &[Node], wires: &[Wire]) -> Result<(), String> {
    let ids: HashSet<u32> = nodes.iter().map(|n| n.id).collect();

    for wire in wires {
        if !ids.contains(&wire.from_node_id) {
            return Err(format!(
                "wire references unknown source node {}",
                wire.from_node_id
            ));
        }
        if !ids.contains(&wire.to_node_id) {
            return Err(format!(
                "wire references unknown destination node {}",
                wire.to_node_id
            ));
        }
    }

    Ok(())
}

pub fn parse_file(content: &str) -> Result<AudioGraph, String> {
    let mut nodes = Vec::new();
    let mut wires = Vec::new();

    for line in content.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            nodes.push(parse_node(line)?);
        } else {
            wires.extend(parse_wires(line)?);
        }
    }

    validate_wires(&nodes, &wires)?;

    let mut graph = AudioGraph {
        nodes,
        wires,
        is_sorted: false,
        buffers: vec![].into(),
    };
    graph.sort()?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_nodes() {
        let input = r#"
            [0] Osc Sine 330.0
            [1] Osc Saw 220.0
            [2] Gain 0.2
            [3] Out
        "#;

        let graph = parse_file(input).unwrap();
        assert_eq!(graph.nodes.len(), 4);

        for node in graph.nodes {
            match node.inner {
                NodeState::Oscillator(state) => match state.osc_type {
                    Wave::Sine => assert_eq!(state.freq, 330.0),
                    Wave::Saw => assert_eq!(state.freq, 220.0),
                    _ => panic!("Expected Sine or Saw"),
                },
                NodeState::Gain(state) => assert_eq!(state.value, 0.2),
                _ => {}
            }
        }
    }

    #[test]
    fn sorts_nodes() {
        let input = r#"
            [0] Osc Sine 330.0
            [1] Osc Saw 220.0
            [2] Gain 0.2
            [3] Out

            0->2,
            1->2,
            2->3,
        "#;

        let graph = parse_file(input).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.wires.len(), 3);

        match &graph.nodes[0].inner {
            NodeState::Oscillator(state) => match state.osc_type {
                Wave::Sine => assert_eq!(state.freq, 330.0),
                Wave::Saw => assert_eq!(state.freq, 220.0),
                _ => panic!("Expected Sine or Saw"),
            },
            _ => panic!("Expected Osc"),
        }
        match &graph.nodes[1].inner {
            NodeState::Oscillator(state) => match state.osc_type {
                Wave::Sine => assert_eq!(state.freq, 330.0),
                Wave::Saw => assert_eq!(state.freq, 220.0),
                _ => panic!("Expected Sine or Saw"),
            },
            _ => panic!("Expected Osc"),
        }
    }

    #[test]
    fn valid_wires_pass_validation() {
        let input = r#"
        [0] Out
        [1] Out
        0->1
    "#;

        let graph = parse_file(input).unwrap();
        assert_eq!(graph.wires.len(), 1);
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.wires[0].from_node_id, 0);
        assert_eq!(graph.wires[0].to_node_id, 1);

        match &graph.nodes[0].inner {
            NodeState::Output(_) => {}
            _ => panic!("Expected Out"),
        }
        match &graph.nodes[1].inner {
            NodeState::Output(_) => {}
            _ => panic!("Expected Out"),
        }
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let input = r#"
            # full line comment

            [0] Gain 0.5
            [1] Out  # trailing comment

            0->1, # wire comment
        "#;

        let graph = parse_file(input).unwrap();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.wires.len(), 1);
    }

    #[test]
    fn errors_on_unknown_node_type() {
        let input = "[0] Foo 123";

        let err = parse_file(input).err().unwrap();
        assert!(err.contains("unknown node type"));
    }

    #[test]
    fn errors_on_invalid_wire() {
        let input = "0=>1";

        let err = parse_file(input).err().unwrap();
        assert!(err.contains("invalid wire syntax"));
    }

    #[test]
    fn errors_on_missing_osc_params() {
        let input = "[0] Osc Sine";

        let err = parse_file(input).err().unwrap();
        assert!(err.contains("missing frequency"));
    }
}
