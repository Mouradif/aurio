use crate::parser::parse_file;
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{env, fs};

mod parser;

const SAMPLE_RATE: f32 = 44000.0;

pub enum Wave {
    Sine,
    Square,
    Saw,
}

pub struct OscillatorState {
    pub osc_type: Wave,
    pub freq: f32,
    pub phase: AtomicU32,
}

impl OscillatorState {
    pub fn process(&self, output: &mut [f32]) {
        let mut phase = f32::from_bits(self.phase.load(Ordering::Relaxed));
        for i in 0..output.len() {
            match self.osc_type {
                Wave::Sine => output[i] = (phase * 2.0 * std::f32::consts::PI).sin(),
                Wave::Square => output[i] = if phase < 0.5 { -1.0 } else { 1.0 },
                Wave::Saw => output[i] = phase,
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
                continue;
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
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <file.au>", args[0]);
        std::process::exit(1);
    }

    let filepath = &args[1];

    let content = fs::read_to_string(filepath).expect("failed to read file");
    let initial_graph = parse_file(&content).expect("failed to parse initial file");

    let graph = Arc::new(ArcSwap::from_pointee(initial_graph));
    let graph_clone = graph.clone();

    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().expect("no default config");

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let current = graph_clone.load_full();
                current.process(data);
            },
            |err| eprintln!("Stream error: {}", err),
            None,
        )
        .expect("failed to build stream");

    stream.play().expect("failed to play");

    let graph_for_watcher = graph.clone();
    let filepath_owned = filepath.to_string();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| match res {
            Ok(event) => {
                if event.kind.is_modify() {
                    println!("File changed, reloading...");
                    match fs::read_to_string(&filepath_owned) {
                        Ok(content) => match parse_file(&content) {
                            Ok(new_graph) => {
                                graph_for_watcher.store(Arc::new(new_graph));
                                println!("Graph updated successfully");
                            }
                            Err(e) => eprintln!("Parse error: {}", e),
                        },
                        Err(e) => eprintln!("Read error: {}", e),
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(Path::new(filepath), RecursiveMode::NonRecursive)
        .expect("failed to watch file");

    println!("Watching {} - edit and save to update audio", filepath);
    println!("Press Ctrl+C to stop");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
