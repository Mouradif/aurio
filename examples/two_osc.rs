use aurio::{
    AudioGraph, Node,
    NodeState::{Gain, Oscillator, Output},
    OscillatorType, OutputState, Wire,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::stdin;

fn main() {
    let osc_1 = Node {
        id: 0,
        inner: Oscillator(aurio::OscillatorState {
            osc_type: OscillatorType::Square,
            freq: 220.0,
            phase: 0.0,
        }),
    };
    let osc_2 = Node {
        id: 3,
        inner: Oscillator(aurio::OscillatorState {
            osc_type: OscillatorType::Sine,
            freq: 330.0,
            phase: 0.0,
        }),
    };

    let gain = Node {
        id: 1,
        inner: Gain(aurio::GainState { value: 0.2 }),
    };

    let output = Node {
        id: 2,
        inner: Output(OutputState {}),
    };

    let wires = vec![
        Wire {
            from_node_id: 0,
            from_output_idx: 0,
            to_node_id: 1,
            to_input_idx: 0,
        },
        Wire {
            from_node_id: 1,
            from_output_idx: 0,
            to_node_id: 2,
            to_input_idx: 0,
        },
        Wire {
            from_node_id: 3,
            from_output_idx: 0,
            to_node_id: 1,
            to_input_idx: 0,
        },
    ];

    let mut graph = AudioGraph {
        nodes: vec![osc_1, gain, output, osc_2],
        wires,
        is_sorted: false,
        buffers: vec![],
    };

    // Setup cpal
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().expect("no default config");

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                graph.process(data);
            },
            |err| eprintln!("Stream error: {}", err),
            None,
        )
        .expect("failed to build stream");

    stream.play().expect("failed to play");

    println!("Playing 440Hz sine wave. Press Enter to stop...");
    let mut input = String::new();
    stdin().read_line(&mut input).unwrap();
}
