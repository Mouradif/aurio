use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midir::MidiInput;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

fn main() {
    let host = cpal::default_host();

    let input_device = host.default_input_device().expect("no input device");
    let output_device = host.default_output_device().expect("no output device");

    let config: cpal::StreamConfig = input_device
        .default_input_config()
        .expect("no default input config")
        .into();

    let sample_rate = config.sample_rate as usize;
    let max_delay_samples = sample_rate;

    let ctrl_time = Arc::new(AtomicU8::new(127));
    let ctrl_feedback = Arc::new(AtomicU8::new(64));
    let ctrl_damping = Arc::new(AtomicU8::new(0));

    let ctrl_time_midi = ctrl_time.clone();
    let ctrl_feedback_midi = ctrl_feedback.clone();
    let ctrl_damping_midi = ctrl_damping.clone();

    let ctrl_time_audio = ctrl_time.clone();
    let ctrl_feedback_audio = ctrl_feedback.clone();
    let ctrl_damping_audio = ctrl_damping.clone();

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
                if msg[0] == 0xB0 {
                    match msg[1] {
                        48 => {
                            ctrl_time_midi.store(msg[2], Ordering::Relaxed);
                            println!("Time: {}%", msg[2] as u32 * 100 / 127);
                        }
                        49 => {
                            ctrl_feedback_midi.store(msg[2], Ordering::Relaxed);
                            println!("Feedback: {}%", msg[2] as u32 * 100 / 127);
                        }
                        50 => {
                            ctrl_damping_midi.store(msg[2], Ordering::Relaxed);
                            println!("Damping: {}%", msg[2] as u32 * 100 / 127);
                        }
                        _ => {}
                    }
                }
            },
            (),
        )
        .expect("failed to connect MIDI");

    let mut delay_buf = vec![0.0f32; max_delay_samples];
    let mut write_pos = 0usize;
    let mut current_delay = max_delay_samples as f32;
    let mut lowpass_state = 0.0f32;

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

    let output_stream = output_device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let target_delay = (ctrl_time_audio.load(Ordering::Relaxed) as f32 + 1.0) / 128.0
                    * max_delay_samples as f32;
                let feedback = ctrl_feedback_audio.load(Ordering::Relaxed) as f32 / 127.0 * 0.95;
                let damping = ctrl_damping_audio.load(Ordering::Relaxed) as f32 / 127.0;

                for sample in data {
                    current_delay += (target_delay - current_delay) * 0.0001;
                    let clamped_delay = current_delay.clamp(1.0, max_delay_samples as f32);

                    let read_pos_f = (write_pos as f32 + max_delay_samples as f32 - clamped_delay)
                        % max_delay_samples as f32;
                    let read_pos_0 = read_pos_f.floor() as usize % max_delay_samples;
                    let read_pos_1 = (read_pos_0 + 1) % max_delay_samples;
                    let frac = read_pos_f.fract();

                    let delayed =
                        delay_buf[read_pos_0] * (1.0 - frac) + delay_buf[read_pos_1] * frac;

                    lowpass_state += (delayed - lowpass_state) * (1.0 - damping * 0.9);

                    let input_sample = consumer.try_pop().unwrap_or(0.0);

                    delay_buf[write_pos] = input_sample + lowpass_state * feedback;

                    *sample = input_sample + delayed;

                    write_pos = (write_pos + 1) % max_delay_samples;
                }
            },
            |err| eprintln!("output error: {err}"),
            None,
        )
        .expect("failed to build output stream");

    input_stream.play().expect("failed to start input");
    output_stream.play().expect("failed to start output");

    println!("Delay running:");
    println!("  CC 48 = Time");
    println!("  CC 49 = Feedback");
    println!("  CC 50 = Damping");
    println!("Press Enter to quit.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
