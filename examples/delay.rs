use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midir::{Ignore, MidiInput};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() {
    let host = cpal::default_host();

    let input_device = host.default_input_device().expect("no input device");
    let output_device = host.default_output_device().expect("no output device");

    let config: cpal::StreamConfig = input_device
        .default_input_config()
        .expect("no default input config")
        .into();

    let sample_rate = config.sample_rate as usize;
    let max_delay_samples = sample_rate; // 1 second max

    // Shared delay control
    let delay_samples = Arc::new(AtomicUsize::new(max_delay_samples));
    let delay_samples_midi = delay_samples.clone();
    let delay_samples_audio = delay_samples.clone();

    // MIDI setup
    let midi_in = MidiInput::new("aurio").expect("failed to create MIDI input");
    let ports = midi_in.ports();
    let port = ports
        .iter()
        .find(|p| midi_in.port_name(p).unwrap_or_default().contains("APC"))
        .or_else(|| ports.first())
        .expect("no MIDI input found");

    println!("MIDI: {}", midi_in.port_name(port).unwrap_or_default());

    let _midi_conn = midi_in
        .connect(
            port,
            "aurio-input",
            move |_, msg, _| {
                // CC message on channel 0, controller 48
                if msg[0] == 0xB0 && msg[1] == 48 {
                    let val = msg[2] as usize;
                    let new_delay =
                        (max_delay_samples * (val + 1) / 128).clamp(1, max_delay_samples);
                    delay_samples_midi.store(new_delay, Ordering::Relaxed);
                    println!("Delay: {}ms", new_delay * 1000 / sample_rate);
                }
            },
            (),
        )
        .expect("failed to connect MIDI");

    // Delay buffer - fixed size circular buffer
    let mut delay_buf = vec![0.0f32; max_delay_samples];
    let mut write_pos = 0usize;

    // Audio I/O ring buffer
    let rb = HeapRb::<f32>::new(8192);
    let (mut producer, mut consumer) = rb.split();

    let input_stream = input_device
        .build_input_stream(
            &config,
            move |data: &[f32], _| {
                for &sample in data {
                    let _ = producer.try_push(sample);
                }
            },
            |err| eprintln!("input error: {err}"),
            None,
        )
        .expect("failed to build input stream");

    let mut current_delay = max_delay_samples as f32;
    let output_stream = output_device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let target_delay = delay_samples_audio
                    .load(Ordering::Relaxed)
                    .clamp(1, max_delay_samples) as f32;

                for sample in data {
                    // Slew toward target delay (adjust 0.01 for faster/slower response)
                    current_delay += (target_delay - current_delay) * 0.0001;

                    // Write incoming audio to delay buffer
                    if let Some(input_sample) = consumer.try_pop() {
                        delay_buf[write_pos] = input_sample;
                    }

                    // Fractional read position for smooth interpolation
                    let read_pos_f = (write_pos as f32 + max_delay_samples as f32 - current_delay)
                        % max_delay_samples as f32;

                    let read_pos_0 = read_pos_f.floor() as usize % max_delay_samples;
                    let read_pos_1 = (read_pos_0 + 1) % max_delay_samples;
                    let frac = read_pos_f.fract();

                    // Linear interpolation between samples
                    *sample = delay_buf[read_pos_0] * (1.0 - frac) + delay_buf[read_pos_1] * frac;

                    write_pos = (write_pos + 1) % max_delay_samples;
                }
            },
            |err| eprintln!("output error: {err}"),
            None,
        )
        .expect("failed to build output stream");
    input_stream.play().expect("failed to start input");
    output_stream.play().expect("failed to start output");

    println!("Delay running. Turn knob (CC 48) to adjust. Press Enter to quit.");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
