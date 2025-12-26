use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

pub mod parser;

const SAMPLE_RATE: f32 = 44000.0;

pub enum OscillatorType {
    Sine,
    Square,
    Saw,
}

pub struct OscillatorState {
    pub osc_type: OscillatorType,
    pub freq: f32,
    pub phase: AtomicU32,
}

impl OscillatorState {
    pub fn process(&self, output: &mut [f32]) {
        let mut phase = f32::from_bits(self.phase.load(Ordering::Relaxed));
        for i in 0..output.len() {
            match self.osc_type {
                OscillatorType::Sine => output[i] = (phase * 2.0 * std::f32::consts::PI).sin(),
                OscillatorType::Square => output[i] = if phase < 0.5 { -1.0 } else { 1.0 },
                OscillatorType::Saw => output[i] = phase,
            }

            phase += self.freq / SAMPLE_RATE;
            if phase > 1.0 {
                phase -= 1.0;
            }
        }
        self.phase.store(phase.to_bits(), Ordering::Relaxed);
    }
}

pub struct GainState {
    pub value: f32,
}

impl GainState {
    pub fn process(&self, inputs: &[&[f32]], output: &mut [f32]) {
        output.fill(0.0);
        for i in 0..inputs.len() {
            let input = inputs[i];
            for j in 0..input.len() {
                if j >= output.len() {
                    break;
                }
                output[j] += input[j] * self.value;
            }
        }
    }
}

pub struct OutputState {}

impl OutputState {
    pub fn process(&self, inputs: &[&[f32]], outputs: &mut [f32]) {
        let len = outputs.len();
        for i in 0..inputs.len() {
            let input = inputs[i];
            if input.len() != len {
                continue; // Weird input with mismatched length, right? Or should we allow it?
            }
            for j in 0..input.len() {
                if j == 0 {
                    outputs[j] = 0.0;
                }
                outputs[j] += input[j];
            }
        }
    }
}

pub enum NodeState {
    Oscillator(OscillatorState),
    Gain(GainState),
    Output(OutputState),
}

pub struct Node {
    pub id: u32,
    pub inner: NodeState,
}

impl Node {
    fn process(&self, inputs: &[&[f32]], output: &mut [f32]) {
        match &self.inner {
            NodeState::Oscillator(state) => state.process(output),
            NodeState::Gain(state) => state.process(inputs, output),
            NodeState::Output(state) => state.process(inputs, output),
        }
    }
}

pub struct Wire {
    pub from_node_id: u32,
    pub from_output_idx: usize,
    pub to_node_id: u32,
    pub to_input_idx: usize,
}

pub struct AudioGraph {
    pub nodes: Vec<Node>,
    pub wires: Vec<Wire>,
    pub is_sorted: bool,
    pub buffers: Mutex<Vec<Vec<f32>>>,
}

impl AudioGraph {
    pub fn process(&self, output: &mut [f32]) {
        if !self.is_sorted {
            panic!("Graph must be sorted before being used");
        }
        let mut buffers = self.buffers.lock().unwrap();
        if buffers.len() != self.nodes.len() {
            *buffers = vec![vec![0.0; output.len()]; self.nodes.len()];
        } else {
            for buf in &mut *buffers {
                buf.fill(0.0);
            }
        }
        for i in 0..self.nodes.len() {
            let node_id = self.nodes[i].id;

            let input_indices: Vec<usize> = self
                .wires
                .iter()
                .filter(|w| w.to_node_id == node_id)
                .map(|w| {
                    self.nodes
                        .iter()
                        .position(|n| n.id == w.from_node_id)
                        .unwrap()
                })
                .collect();

            let (before, rest) = buffers.split_at_mut(i);
            let (current, after) = rest.split_first_mut().unwrap();

            let mut inputs: Vec<&[f32]> = vec![];
            for &idx in &input_indices {
                if idx == i {
                    // self-feedback
                    continue;
                } else if idx < i {
                    inputs.push(&before[idx]);
                } else {
                    inputs.push(&after[idx - i - 1]);
                }
            }

            self.nodes[i].process(&inputs, current);
            if let NodeState::Output(_) = self.nodes[i].inner {
                output.copy_from_slice(current);
            }
        }
    }

    fn sort(&mut self) -> Result<(), String> {
        let mut in_degree: HashMap<u32, usize> = HashMap::new();

        for node in &self.nodes {
            in_degree.insert(node.id, 0);
        }

        for wire in &self.wires {
            *in_degree.get_mut(&wire.to_node_id).unwrap() += 1;
        }

        let mut queue: Vec<u32> = in_degree
            .iter()
            .filter(|&(_, deg)| *deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted_ids = Vec::new();

        while let Some(node_id) = queue.pop() {
            sorted_ids.push(node_id);

            for wire in &self.wires {
                if wire.from_node_id == node_id {
                    let deg = in_degree.get_mut(&wire.to_node_id).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(wire.to_node_id);
                    }
                }
            }
        }

        if sorted_ids.len() != self.nodes.len() {
            return Err("Cycle detected".into());
        }
        let mut sorted_nodes: Vec<Node> = Vec::with_capacity(self.nodes.len());

        for id in sorted_ids {
            let idx = self
                .nodes
                .iter()
                .position(|n| n.id == id)
                .ok_or(format!("Couldn't find node id {}", id))?;
            sorted_nodes.push(self.nodes.remove(idx));
        }

        self.nodes = sorted_nodes;
        self.is_sorted = true;
        Ok(())
    }
}
