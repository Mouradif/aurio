use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midir::MidiInput;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

#[derive(Clone, Copy, PartialEq)]
enum LooperState {
    Idle,
    WaitingSecond,
    Countdown,
    Recording,
    Playing,
}

fn main() {
    let host = cpal::default_host();

    let input_device = host.default_input_device().expect("no input device");
    let output_device = host.default_output_device().expect("no output device");

    let supported_config = input_device
        .default_input_config()
        .expect("no default input config");

    let config = cpal::StreamConfig {
        channels: supported_config.channels(),
        sample_rate: supported_config.sample_rate(),
        buffer_size: cpal::BufferSize::Fixed(64), // Request minimal buffer
    };

    let sample_rate = config.sample_rate as usize;
    let max_loop_samples = sample_rate * 60;

    // Controls
    let ctrl_bars = Arc::new(AtomicU8::new(0));
    let ctrl_bars_midi = ctrl_bars.clone();
    let ctrl_bars_audio = ctrl_bars.clone();

    let first_tap_sample = Arc::new(AtomicU32::new(0));
    let tap_count = Arc::new(AtomicU8::new(0));

    let first_tap_sample_midi = first_tap_sample.clone();
    let tap_count_midi = tap_count.clone();
    let first_tap_sample_audio = first_tap_sample.clone();
    let tap_count_audio = tap_count.clone();

    let global_sample_count = Arc::new(AtomicU32::new(0));
    let global_sample_count_midi = global_sample_count.clone();
    let global_sample_count_audio = global_sample_count.clone();

    // MIDI setup
    let midi_in = MidiInput::new("aurio").expect("failed to create MIDI input");
    let ports = midi_in.ports();
    let port = ports
        .iter()
        .find(|p| midi_in.port_name(p).unwrap_or_default().contains("APC"))
        .or_else(|| ports.first())
        .expect("no MIDI input found");

    println!("MIDI: {}", midi_in.port_name(port).unwrap_or_default());
    println!("Bars: 1 (use CC 52 to change)");
    println!("Tap any note twice to set tempo and start countdown.\n");

    let _midi_conn = midi_in
        .connect(
            port,
            "aurio-input",
            move |_, msg, _| {
                let status = msg[0] & 0xF0;
                let channel = msg[0] & 0x0F;

                if status == 0x90 && channel == 0 && msg[2] > 0 {
                    let current = global_sample_count_midi.load(Ordering::Relaxed);
                    let taps = tap_count_midi.load(Ordering::Relaxed);

                    if taps == 0 {
                        first_tap_sample_midi.store(current, Ordering::Relaxed);
                        tap_count_midi.store(1, Ordering::Relaxed);
                        println!("First tap - waiting for second tap...");
                    } else if taps == 1 {
                        tap_count_midi.store(2, Ordering::Relaxed);
                        println!("Second tap - starting countdown!");
                    }
                }

                if status == 0xB0 && channel == 0 && msg[1] == 52 {
                    ctrl_bars_midi.store(msg[2], Ordering::Relaxed);
                    let bars = msg[2] as u32 / 8 + 1;
                    println!("Bars: {}", bars);
                }
            },
            (),
        )
        .expect("failed to connect MIDI");

    // Small ring buffer for minimal latency
    let rb = HeapRb::<f32>::new(256);
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

    // Looper state
    let mut state = LooperState::Idle;
    let mut loop_buffer = vec![0.0f32; max_loop_samples];
    let mut loop_length = 0usize;
    let mut loop_pos = 0usize;
    let mut beat_samples = 0usize;
    let mut phase_in_beat = 0usize;
    let mut beats_elapsed = 0usize;
    let mut click_phase = 0.0f32;

    let output_stream = output_device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let bars = ctrl_bars_audio.load(Ordering::Relaxed) as usize / 8 + 1;
                let total_record_beats = bars * 4;
                let taps = tap_count_audio.load(Ordering::Relaxed);

                for sample in data {
                    let current_count = global_sample_count_audio.fetch_add(1, Ordering::Relaxed);
                    let input_sample = consumer.try_pop().unwrap_or(0.0);

                    match state {
                        LooperState::Idle => {
                            if taps == 1 {
                                state = LooperState::WaitingSecond;
                            }
                            *sample = input_sample;
                        }

                        LooperState::WaitingSecond => {
                            if taps == 2 {
                                let first = first_tap_sample_audio.load(Ordering::Relaxed) as usize;
                                beat_samples = (current_count as usize)
                                    .saturating_sub(first)
                                    .max(sample_rate / 8);
                                phase_in_beat = 0;
                                beats_elapsed = 0;
                                state = LooperState::Countdown;
                                println!("Tempo: {} BPM", 60 * sample_rate / beat_samples);
                            }
                            *sample = input_sample;
                        }

                        LooperState::Countdown => {
                            let click = if phase_in_beat < sample_rate / 50 {
                                let freq = if beats_elapsed == 0 { 1000.0 } else { 800.0 };
                                let out = (click_phase * std::f32::consts::TAU).sin() * 0.3;
                                click_phase = (click_phase + freq / sample_rate as f32) % 1.0;
                                out
                            } else {
                                click_phase = 0.0;
                                0.0
                            };

                            phase_in_beat += 1;
                            if phase_in_beat >= beat_samples {
                                phase_in_beat = 0;
                                beats_elapsed += 1;
                                println!("Countdown: {}", 4 - beats_elapsed.min(4));

                                if beats_elapsed >= 4 {
                                    beats_elapsed = 0;
                                    loop_pos = 0;
                                    loop_length = beat_samples * total_record_beats;
                                    state = LooperState::Recording;
                                    println!("Recording {} bars...", bars);
                                }
                            }

                            *sample = input_sample + click;
                        }

                        LooperState::Recording => {
                            let click = if phase_in_beat < sample_rate / 50 {
                                let freq = if phase_in_beat == 0 && beats_elapsed % 4 == 0 {
                                    1000.0
                                } else {
                                    800.0
                                };
                                let out = (click_phase * std::f32::consts::TAU).sin() * 0.3;
                                click_phase = (click_phase + freq / sample_rate as f32) % 1.0;
                                out
                            } else {
                                click_phase = 0.0;
                                0.0
                            };

                            if loop_pos < loop_buffer.len() {
                                loop_buffer[loop_pos] = input_sample;
                            }
                            loop_pos += 1;

                            phase_in_beat += 1;
                            if phase_in_beat >= beat_samples {
                                phase_in_beat = 0;
                                beats_elapsed += 1;
                            }

                            if loop_pos >= loop_length {
                                loop_pos = 0;
                                state = LooperState::Playing;
                                println!("Playing loop!");
                            }

                            *sample = input_sample + click;
                        }

                        LooperState::Playing => {
                            let looped = loop_buffer[loop_pos];
                            loop_pos = (loop_pos + 1) % loop_length;
                            *sample = input_sample + looped;
                        }
                    }
                }
            },
            |err| eprintln!("output error: {err}"),
            None,
        )
        .expect("failed to build output stream");

    input_stream.play().expect("failed to start input");
    output_stream.play().expect("failed to start output");

    println!("Looper ready. Press Enter to quit.");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}

