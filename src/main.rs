use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::thread;

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().expect("no default config");

    let rb = HeapRb::<NoteEvent>::new(64);
    let (mut producer, mut consumer) = rb.split();

    // Audio thread state
    let sample_rate = config.sample_rate() as f32;
    let channels = config.channels();
    let mut phase = 0.0f32;
    let mut current_freq: Option<f32> = None;

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _| {
                // Drain events from ring buffer
                while let Some(event) = consumer.try_pop() {
                    match event {
                        NoteEvent::NoteOn(freq) => {
                            current_freq = Some(freq);
                            phase = 0.0; // Reset phase for clean attack
                        }
                        NoteEvent::NoteOff => current_freq = None,
                    }
                }

                for frame in data.chunks_mut(channels.into()) {
                    let sample = match current_freq {
                        Some(freq) => {
                            let out = (phase * std::f32::consts::TAU).sin() * 0.3;
                            phase = (phase + freq / sample_rate) % 1.0;
                            out
                        }
                        None => 0.0,
                    };

                    for s in frame {
                        *s = sample;
                    }
                }
            },
            |err| eprintln!("stream error: {err}"),
            None,
        )
        .expect("failed to build stream");

    stream.play().expect("failed to play");

    // Keyboard input thread
    let input_handle = thread::spawn(move || {
        enable_raw_mode().expect("failed to enable raw mode");
        println!("Piano ready! Press A-K for white keys, W/E/T/Y/U for black keys. ESC to quit.\r");

        loop {
            if let Ok(Event::Key(key_event)) = event::read() {
                if key_event.code == KeyCode::Esc {
                    break;
                }

                if let Some(freq) = key_to_freq(key_event.code) {
                    let event = match key_event.kind {
                        KeyEventKind::Press => NoteEvent::NoteOn(freq),
                        KeyEventKind::Release => NoteEvent::NoteOff,
                        _ => continue,
                    };
                    let _ = producer.try_push(event);
                }
            }
        }

        disable_raw_mode().expect("failed to disable raw mode");
    });

    input_handle.join().unwrap();
}

fn key_to_freq(code: KeyCode) -> Option<f32> {
    // C4 = 261.63 Hz, each semitone is * 2^(1/12)
    let semitone = |n: i32| 261.63 * 2.0_f32.powf(n as f32 / 12.0);

    match code {
        // White keys: C D E F G A B C
        KeyCode::Char('a') => Some(semitone(0)),  // C
        KeyCode::Char('s') => Some(semitone(2)),  // D
        KeyCode::Char('d') => Some(semitone(4)),  // E
        KeyCode::Char('f') => Some(semitone(5)),  // F
        KeyCode::Char('g') => Some(semitone(7)),  // G
        KeyCode::Char('h') => Some(semitone(9)),  // A
        KeyCode::Char('j') => Some(semitone(11)), // B
        KeyCode::Char('k') => Some(semitone(12)), // C5
        // Black keys: C# D# F# G# A#
        KeyCode::Char('w') => Some(semitone(1)),  // C#
        KeyCode::Char('e') => Some(semitone(3)),  // D#
        KeyCode::Char('t') => Some(semitone(6)),  // F#
        KeyCode::Char('y') => Some(semitone(8)),  // G#
        KeyCode::Char('u') => Some(semitone(10)), // A#
        _ => None,
    }
}

enum NoteEvent {
    NoteOn(f32),
    NoteOff,
}

