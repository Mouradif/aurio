use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midir::MidiInput;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

const NUM_TRACKS: usize = 8;

#[derive(Clone, Copy, PartialEq, Debug)]
enum TrackState {
    Empty,
    Countdown,
    Recording,
    Playing,
    Stopped,
}

struct Track {
    state: TrackState,
    buffer: Vec<f32>,
    length: usize,
    pos: usize,
    bars: usize,
    beats_elapsed: usize,
    phase_in_beat: usize,
}

impl Track {
    fn new(max_samples: usize) -> Self {
        Self {
            state: TrackState::Empty,
            buffer: vec![0.0; max_samples],
            length: 0,
            pos: 0,
            bars: 1,
            beats_elapsed: 0,
            phase_in_beat: 0,
        }
    }
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
        buffer_size: cpal::BufferSize::Fixed(64),
    };

    let sample_rate = config.sample_rate as usize;
    let max_loop_samples = sample_rate * 60;

    let shutting_down = Arc::new(AtomicBool::new(false));
    let shutting_down_audio = shutting_down.clone();

    let ctrl_bars = Arc::new(AtomicU8::new(0));
    let ctrl_bars_midi = ctrl_bars.clone();
    let ctrl_bars_audio = ctrl_bars.clone();

    let first_tap_sample = Arc::new(AtomicU32::new(0));
    let tap_count = Arc::new(AtomicU8::new(0));
    let tempo_set = Arc::new(AtomicBool::new(false));
    let beat_samples_shared = Arc::new(AtomicU32::new(0));

    let first_tap_sample_midi = first_tap_sample.clone();
    let tap_count_midi = tap_count.clone();
    let tempo_set_midi = tempo_set.clone();
    let beat_samples_midi = beat_samples_shared.clone();

    let tempo_set_audio = tempo_set.clone();
    let beat_samples_audio = beat_samples_shared.clone();

    let global_sample_count = Arc::new(AtomicU32::new(0));
    let global_sample_count_midi = global_sample_count.clone();
    let global_sample_count_audio = global_sample_count.clone();

    // Per-track pending triggers (255 = no action, 0 = trigger pending)
    let track_triggers: Vec<Arc<AtomicBool>> = (0..NUM_TRACKS)
        .map(|_| Arc::new(AtomicBool::new(false)))
        .collect();
    let track_triggers_midi: Vec<Arc<AtomicBool>> =
        track_triggers.iter().map(|t| t.clone()).collect();
    let track_triggers_audio: Vec<Arc<AtomicBool>> =
        track_triggers.iter().map(|t| t.clone()).collect();

    let midi_in = MidiInput::new("aurio").expect("failed to create MIDI input");
    let ports = midi_in.ports();
    let port = ports
        .iter()
        .find(|p| midi_in.port_name(p).unwrap_or_default().contains("APC"))
        .or_else(|| ports.first())
        .expect("no MIDI input found");

    println!("MIDI: {}", midi_in.port_name(port).unwrap_or_default());
    println!("Bars: 1 (use CC 52 to change)");
    println!("Notes 0-7 = tracks. Tap twice on any note to set tempo.\n");

    let _midi_conn = midi_in
        .connect(
            port,
            "aurio-input",
            move |_, msg, _| {
                let status = msg[0] & 0xF0;
                let channel = msg[0] & 0x0F;

                if status == 0x90 && channel == 0 && msg[2] > 0 {
                    let note = msg[1];

                    if !tempo_set_midi.load(Ordering::Relaxed) {
                        let current = global_sample_count_midi.load(Ordering::Relaxed);
                        let taps = tap_count_midi.load(Ordering::Relaxed);

                        if taps == 0 {
                            first_tap_sample_midi.store(current, Ordering::Relaxed);
                            tap_count_midi.store(1, Ordering::Relaxed);
                            println!("First tap - waiting for second tap to set tempo...");
                        } else if taps == 1 {
                            let first = first_tap_sample_midi.load(Ordering::Relaxed);
                            let beat_samples = (current - first).max(1000) as usize;
                            beat_samples_midi.store(beat_samples as u32, Ordering::Relaxed);
                            tempo_set_midi.store(true, Ordering::Relaxed);
                            tap_count_midi.store(2, Ordering::Relaxed);
                            println!("Tempo set: {} BPM", 60 * sample_rate / beat_samples);

                            if (note as usize) < NUM_TRACKS {
                                track_triggers_midi[note as usize].store(true, Ordering::Relaxed);
                            }
                        }
                    } else if (note as usize) < NUM_TRACKS {
                        track_triggers_midi[note as usize].store(true, Ordering::Relaxed);
                        println!("Track {} triggered", note);
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

    let mut tracks: Vec<Track> = (0..NUM_TRACKS)
        .map(|_| Track::new(max_loop_samples))
        .collect();

    let mut global_phase_in_beat = 0usize;
    let mut click_phase = 0.0f32;

    let fadeout_samples = sample_rate / 100;
    let mut fadeout_pos = 0usize;
    let mut fading_out = false;

    // Latched triggers - set by MIDI, cleared on beat
    let mut pending_triggers = [false; NUM_TRACKS];

    let output_stream = output_device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _| {
                let bars = ctrl_bars_audio.load(Ordering::Relaxed) as usize / 8 + 1;
                let tempo_is_set = tempo_set_audio.load(Ordering::Relaxed);
                let beat_samples = beat_samples_audio.load(Ordering::Relaxed) as usize;

                if shutting_down_audio.load(Ordering::Relaxed) {
                    fading_out = true;
                }

                // Latch any new triggers from MIDI
                for i in 0..NUM_TRACKS {
                    if track_triggers_audio[i].swap(false, Ordering::Relaxed) {
                        pending_triggers[i] = true;
                    }
                }

                for sample in data {
                    global_sample_count_audio.fetch_add(1, Ordering::Relaxed);
                    let input_sample = consumer.try_pop().unwrap_or(0.0);

                    let is_beat_start =
                        tempo_is_set && beat_samples > 0 && global_phase_in_beat == 0;

                    // Process pending triggers at beat boundary
                    if is_beat_start {
                        for i in 0..NUM_TRACKS {
                            if pending_triggers[i] {
                                pending_triggers[i] = false;
                                let track = &mut tracks[i];

                                match track.state {
                                    TrackState::Empty => {
                                        track.state = TrackState::Countdown;
                                        track.bars = bars;
                                        track.beats_elapsed = 0;
                                        track.phase_in_beat = 0;
                                        println!("Track {} countdown ({} bars)", i, bars);
                                    }
                                    TrackState::Playing => {
                                        track.state = TrackState::Stopped;
                                        println!("Track {} stopped", i);
                                    }
                                    TrackState::Stopped => {
                                        track.state = TrackState::Playing;
                                        track.pos = 0;
                                        println!("Track {} playing", i);
                                    }
                                    TrackState::Countdown | TrackState::Recording => {
                                        // Can't interrupt recording
                                    }
                                }
                            }
                        }
                    }

                    // Generate click for any track in countdown or recording
                    let mut click = 0.0f32;
                    if let Some(track) = tracks.iter().find(|t| {
                        t.state == TrackState::Countdown || t.state == TrackState::Recording
                    }) {
                        if track.phase_in_beat < sample_rate / 50 {
                            let is_downbeat = track.beats_elapsed % 4 == 0;
                            let freq = if is_downbeat { 1000.0 } else { 800.0 };
                            click = (click_phase * std::f32::consts::TAU).sin() * 0.3;
                            click_phase = (click_phase + freq / sample_rate as f32) % 1.0;
                        } else {
                            click_phase = 0.0;
                        }
                    }

                    // Process each track
                    let mut loop_mix = 0.0f32;

                    for (i, track) in tracks.iter_mut().enumerate() {
                        match track.state {
                            TrackState::Countdown => {
                                track.phase_in_beat += 1;
                                if track.phase_in_beat >= beat_samples {
                                    track.phase_in_beat = 0;
                                    track.beats_elapsed += 1;
                                    println!(
                                        "Track {} countdown: {}",
                                        i,
                                        4 - track.beats_elapsed.min(4)
                                    );

                                    if track.beats_elapsed >= 4 {
                                        track.state = TrackState::Recording;
                                        track.beats_elapsed = 0;
                                        track.pos = 0;
                                        track.length = beat_samples * track.bars * 4;
                                        println!("Track {} recording ({} bars)...", i, track.bars);
                                    }
                                }
                            }

                            TrackState::Recording => {
                                if track.pos < track.buffer.len() {
                                    track.buffer[track.pos] = input_sample;
                                }
                                track.pos += 1;

                                track.phase_in_beat += 1;
                                if track.phase_in_beat >= beat_samples {
                                    track.phase_in_beat = 0;
                                    track.beats_elapsed += 1;
                                }

                                if track.pos >= track.length {
                                    track.state = TrackState::Playing;
                                    track.pos = 0;
                                    println!("Track {} playing!", i);
                                }
                            }

                            TrackState::Playing => {
                                let crossfade_samples = sample_rate / 100;
                                let looped = track.buffer[track.pos];

                                let out = if track.length > crossfade_samples * 2 {
                                    if track.pos < crossfade_samples {
                                        let t = track.pos as f32 / crossfade_samples as f32;
                                        let fade_out =
                                            (std::f32::consts::FRAC_PI_2 * (1.0 - t)).sin();
                                        let fade_in = (std::f32::consts::FRAC_PI_2 * t).sin();
                                        let from_end = track.buffer
                                            [track.length - crossfade_samples + track.pos];
                                        from_end * fade_out + looped * fade_in
                                    } else if track.pos >= track.length - crossfade_samples {
                                        let offset = track.pos - (track.length - crossfade_samples);
                                        let t = offset as f32 / crossfade_samples as f32;
                                        let fade_out =
                                            (std::f32::consts::FRAC_PI_2 * (1.0 - t)).sin();
                                        let fade_in = (std::f32::consts::FRAC_PI_2 * t).sin();
                                        let from_start = track.buffer[offset];
                                        looped * fade_out + from_start * fade_in
                                    } else {
                                        looped
                                    }
                                } else {
                                    looped
                                };

                                track.pos = (track.pos + 1) % track.length;
                                loop_mix += out;
                            }

                            _ => {}
                        }
                    }

                    // Advance global beat phase
                    if tempo_is_set && beat_samples > 0 {
                        global_phase_in_beat = (global_phase_in_beat + 1) % beat_samples;
                    }

                    let raw_output = input_sample + loop_mix + click;

                    if fading_out {
                        let t = fadeout_pos as f32 / fadeout_samples as f32;
                        let gain = (std::f32::consts::FRAC_PI_2 * (1.0 - t).max(0.0)).sin();
                        *sample = raw_output * gain;
                        fadeout_pos += 1;
                    } else {
                        *sample = raw_output;
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

    shutting_down.store(true, Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(15));
}
