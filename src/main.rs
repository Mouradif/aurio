use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

fn main() {
    let host = cpal::default_host();

    let input_device = host.default_input_device().expect("no input device");
    let output_device = host.default_output_device().expect("no output device");

    let config: cpal::StreamConfig = input_device
        .default_input_config()
        .expect("no default input config")
        .into();

    let sample_rate = config.sample_rate as usize;
    let delay_samples = sample_rate; // 1 second delay

    let buffer = Arc::new(Mutex::new(DelayBuffer::new(delay_samples)));
    let buffer_in = buffer.clone();
    let buffer_out = buffer.clone();

    let input_stream = input_device
        .build_input_stream(
            &config,
            move |data: &[f32], _| {
                let mut buf = buffer_in.lock().unwrap();
                for &sample in data {
                    buf.write(sample);
                }
            },
            |err| eprintln!("input error: {err}"),
            None,
        )
        .expect("failed to build input stream");

    let output_stream = output_device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let mut buf = buffer_out.lock().unwrap();
                for sample in data {
                    *sample = buf.read();
                }
            },
            |err| eprintln!("output error: {err}"),
            None,
        )
        .expect("failed to build output stream");

    input_stream.play().expect("failed to start input");
    output_stream.play().expect("failed to start output");

    println!("Delay effect running (1 second). Press Enter to quit.");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

struct DelayBuffer {
    data: Vec<f32>,
    write_pos: usize,
    read_pos: usize,
}

impl DelayBuffer {
    fn new(delay_samples: usize) -> Self {
        let size = delay_samples + 8192;
        Self {
            data: vec![0.0; size],
            write_pos: delay_samples, // Write starts ahead
            read_pos: 0,              // Read starts at 0
        }
    }

    fn write(&mut self, sample: f32) {
        self.data[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % self.data.len();
    }

    fn read(&mut self) -> f32 {
        let sample = self.data[self.read_pos];
        self.read_pos = (self.read_pos + 1) % self.data.len();
        sample
    }
}

