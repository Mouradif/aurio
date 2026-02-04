use crate::{Project, audio, events, scripting, timing};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam::channel::{Receiver, Sender};
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone)]
pub enum EngineCommand {
    LoadProject(PathBuf),
    ReloadProject(Project),
    Play,
    Pause,
    Stop,
    SetVariable { name: String, value: f64 },
}

#[derive(Debug, Clone)]
pub enum EngineUpdate {
    ProjectLoaded { project: Project },
    CurrentNodes { track_nodes: Vec<(usize, String)> },
    PlaybackState { playing: bool },
    Error { message: String },
}

pub struct EngineHandle {
    pub command_tx: Sender<EngineCommand>,
    pub update_rx: Receiver<EngineUpdate>,
}

pub fn spawn_engine() -> EngineHandle {
    let (command_tx, command_rx) = crossbeam::channel::unbounded();
    let (update_tx, update_rx) = crossbeam::channel::unbounded();

    std::thread::spawn(move || {
        engine_thread(command_rx, update_tx);
    });

    EngineHandle {
        command_tx,
        update_rx,
    }
}

struct EngineState {
    project: Option<Project>,
    track_configs: Option<Arc<ArcSwap<Vec<audio::TrackConfig>>>>,
    sample_counter: Option<Arc<AtomicU64>>,
    lua_runtime: Option<scripting::LuaRuntime>,
    audio_stream: Option<cpal::Stream>,
    playing: bool,
}

fn engine_thread(command_rx: Receiver<EngineCommand>, update_tx: Sender<EngineUpdate>) {
    let mut state = EngineState {
        project: None,
        track_configs: None,
        sample_counter: None,
        lua_runtime: None,
        audio_stream: None,
        playing: false,
    };

    loop {
        match command_rx.recv() {
            Ok(EngineCommand::LoadProject(path)) => match Project::load(&path) {
                Ok(project) => {
                    println!("Project loaded successfully");

                    state.audio_stream = None;
                    state.playing = false;

                    let _ = update_tx.send(EngineUpdate::ProjectLoaded {
                        project: project.clone(),
                    });

                    state.project = Some(project);
                }
                Err(e) => {
                    let _ = update_tx.send(EngineUpdate::Error {
                        message: format!("Failed to load project: {}", e),
                    });
                }
            },
            Ok(EngineCommand::ReloadProject(project)) => {
                println!("Reloading project with updated sequences");

                if let Some(ref track_configs) = state.track_configs {
                    let new_configs: Vec<audio::TrackConfig> = project
                        .tracks
                        .iter()
                        .map(|track_data| {
                            let mut config = audio::TrackConfig::new(
                                track_data.id,
                                track_data.instrument.clone(),
                                track_data.adsr.clone(),
                            );
                            config.volume = track_data.volume;
                            config.pan = track_data.pan;
                            config
                        })
                        .collect();

                    track_configs.store(Arc::new(new_configs));
                    println!("Hot-swapped track configs");
                }

                state.project = Some(project);
            }
            Ok(EngineCommand::Play) => {
                if let Some(ref project) = state.project {
                    if state.audio_stream.is_none() {
                        match setup_audio(project) {
                            Ok((stream, configs, counter, lua)) => {
                                state.audio_stream = Some(stream);
                                state.track_configs = Some(configs);
                                state.sample_counter = Some(counter);
                                state.lua_runtime = Some(lua);
                                state.playing = true;

                                let _ =
                                    update_tx.send(EngineUpdate::PlaybackState { playing: true });
                            }
                            Err(e) => {
                                let _ = update_tx.send(EngineUpdate::Error {
                                    message: format!("Failed to start audio: {}", e),
                                });
                            }
                        }
                    } else {
                        state.playing = true;
                        let _ = update_tx.send(EngineUpdate::PlaybackState { playing: true });
                    }
                }
            }

            Ok(EngineCommand::Pause) => {
                state.playing = false;
                let _ = update_tx.send(EngineUpdate::PlaybackState { playing: false });
            }

            Ok(EngineCommand::Stop) => {
                state.audio_stream = None;
                state.track_configs = None;
                state.sample_counter = None;
                state.playing = false;
                let _ = update_tx.send(EngineUpdate::PlaybackState { playing: false });
                let _ = update_tx.send(EngineUpdate::CurrentNodes {
                    track_nodes: vec![],
                });
            }

            Ok(EngineCommand::SetVariable { .. }) => {
                // TODO
            }

            Err(crossbeam::channel::RecvError) => break,
        }
    }
}

struct TimingState {
    graphs: Vec<timing::StateGraph>,
    current_nodes: Vec<String>,
    sequence_end_samples: Vec<u64>,
}

struct AudioState {
    playback_states: Vec<audio::PlaybackState>,
    pending_event: Option<events::ScheduledEvent>,
    consumer: HeapCons<events::ScheduledEvent>,
    track_configs: Arc<ArcSwap<Vec<audio::TrackConfig>>>,
    sample_rate: f32,
    num_channels: usize,
}

fn setup_audio(
    project: &Project,
) -> Result<
    (
        cpal::Stream,
        Arc<ArcSwap<Vec<audio::TrackConfig>>>,
        Arc<AtomicU64>,
        scripting::LuaRuntime,
    ),
    Box<dyn std::error::Error>,
> {
    let lua_runtime = scripting::LuaRuntime::new()?;

    let track_configs: Vec<audio::TrackConfig> = project
        .tracks
        .iter()
        .map(|track_data| {
            let mut config = audio::TrackConfig::new(
                track_data.id,
                track_data.instrument.clone(),
                track_data.adsr.clone(),
            );
            config.volume = track_data.volume;
            config.pan = track_data.pan;
            config
        })
        .collect();

    let track_configs = Arc::new(ArcSwap::from_pointee(track_configs));
    let sample_counter = Arc::new(AtomicU64::new(0));

    let bpm = project.bpm;
    let sample_rate = project.sample_rate as f32;

    let ring_buffer = HeapRb::<events::ScheduledEvent>::new(4096);
    let (mut producer, consumer) = ring_buffer.split();

    let mut timing_state = TimingState {
        graphs: project.tracks.iter().map(|t| t.graph.clone()).collect(),
        current_nodes: project
            .tracks
            .iter()
            .map(|t| t.initial_node.clone())
            .collect(),
        sequence_end_samples: Vec::new(),
    };

    for (track_id, (graph, current_node)) in timing_state
        .graphs
        .iter()
        .zip(timing_state.current_nodes.iter())
        .enumerate()
    {
        if let Some(node) = graph.get_node(current_node) {
            let _ = timing::schedule_sequence_events(
                &node.sequence,
                track_id,
                0,
                bpm,
                sample_rate,
                &mut producer,
                Some(&lua_runtime),
            );
            let duration = node.sequence.duration_samples(bpm, sample_rate);
            timing_state.sequence_end_samples.push(duration as u64);
        } else {
            timing_state.sequence_end_samples.push(u64::MAX);
        }
    }

    let counter_timing = sample_counter.clone();
    let lua_timing = scripting::LuaRuntime::new()?;

    std::thread::spawn(move || {
        timing_thread(
            timing_state,
            producer,
            counter_timing,
            bpm,
            sample_rate,
            lua_timing,
        );
    });

    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("No output device")?;
    let config = device.default_output_config()?;
    let stream_config: cpal::StreamConfig = config.into();

    let num_channels = stream_config.channels as usize;
    println!(
        "Audio output: {} channels, {} Hz",
        num_channels, sample_rate
    );

    let configs_snapshot = track_configs.load();
    let playback_states: Vec<audio::PlaybackState> = configs_snapshot
        .iter()
        .map(|_| audio::PlaybackState::new())
        .collect();

    let mut audio_state = AudioState {
        playback_states,
        pending_event: None,
        consumer,
        track_configs: track_configs.clone(),
        sample_rate,
        num_channels,
    };

    let counter_audio = sample_counter.clone();

    let stream = device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            audio_callback(data, &mut audio_state, &counter_audio);
        },
        |err| eprintln!("Audio error: {}", err),
        None,
    )?;

    stream.play()?;

    Ok((stream, track_configs, sample_counter, lua_runtime))
}

fn timing_thread(
    mut state: TimingState,
    mut producer: HeapProd<events::ScheduledEvent>,
    sample_counter: Arc<AtomicU64>,
    bpm: f32,
    sample_rate: f32,
    lua_runtime: scripting::LuaRuntime,
) {
    loop {
        let current_sample = sample_counter.load(Ordering::Relaxed);

        for track_id in 0..state.graphs.len() {
            let end_sample = state.sequence_end_samples[track_id];
            if current_sample >= end_sample {
                let current_node = &state.current_nodes[track_id];
                let graph = &state.graphs[track_id];
                let edges = graph.get_outgoing_edges(current_node);

                let next_node = if let Some(edge) = edges.first() {
                    edge.to.clone()
                } else {
                    current_node.clone()
                };

                println!(
                    "Track {}: transitioning from {} to {}",
                    track_id, current_node, next_node
                );

                let _ = producer.try_push(events::ScheduledEvent {
                    sample_timestamp: current_sample,
                    event: events::Event::StopAllNotes { track_id },
                });

                state.current_nodes[track_id] = next_node.clone();

                if let Some(node) = graph.get_node(&next_node) {
                    let _ = timing::schedule_sequence_events(
                        &node.sequence,
                        track_id,
                        current_sample,
                        bpm,
                        sample_rate,
                        &mut producer,
                        Some(&lua_runtime),
                    );

                    let duration = node.sequence.duration_samples(bpm, sample_rate);
                    state.sequence_end_samples[track_id] = current_sample + duration as u64;
                }
            }
        }
    }
}

fn audio_callback(data: &mut [f32], state: &mut AudioState, sample_counter: &Arc<AtomicU64>) {
    let num_frames = data.len() / state.num_channels;
    let current_sample = sample_counter.load(Ordering::Relaxed);
    let buffer_end = current_sample + num_frames as u64;

    let configs = state.track_configs.load();
    let mut events: Vec<events::ScheduledEvent> = Vec::with_capacity(64);
    if let Some(ev) = state.pending_event.take() {
        if ev.sample_timestamp < buffer_end {
            events.push(ev);
        } else {
            state.pending_event = Some(ev);
        }
    }

    while state.pending_event.is_none() {
        match state.consumer.try_pop() {
            Some(ev) if ev.sample_timestamp < buffer_end => events.push(ev),
            Some(ev) => {
                state.pending_event = Some(ev);
                break;
            }
            None => break,
        }
    }

    events.sort_by_key(|e| e.sample_timestamp);
    data.fill(0.0);

    let mut frame = 0;
    let mut event_idx = 0;

    while frame < num_frames {
        while event_idx < events.len() {
            let event_frame = events[event_idx]
                .sample_timestamp
                .saturating_sub(current_sample) as usize;
            if event_frame > frame {
                break;
            }
            process_event(&mut state.playback_states, &configs, &events[event_idx]);
            event_idx += 1;
        }

        render_frame(
            &mut data[frame * state.num_channels..(frame + 1) * state.num_channels],
            &mut state.playback_states,
            &configs,
            state.sample_rate,
        );
        frame += 1;
    }

    sample_counter.fetch_add(num_frames as u64, Ordering::Relaxed);
}

fn process_event(
    playback_states: &mut [audio::PlaybackState],
    configs: &[audio::TrackConfig],
    event: &events::ScheduledEvent,
) {
    match &event.event {
        events::Event::MidiEvent { track_id, message } => {
            if *track_id >= playback_states.len() {
                return;
            }
            match message {
                events::MidiMessage::NoteOn { pitch, velocity } => {
                    let num_oscs = configs.get(*track_id).map_or(0, |c| c.num_oscillators());
                    playback_states[*track_id].note_on(*pitch, *velocity, num_oscs);
                }
                events::MidiMessage::NoteOff { pitch } => {
                    playback_states[*track_id].note_off(*pitch);
                }
            }
        }
        events::Event::StopAllNotes { track_id } => {
            if *track_id < playback_states.len() {
                playback_states[*track_id].stop_all();
            }
        }
        events::Event::NodeTransition { .. } => {}
    }
}

fn render_frame(
    output: &mut [f32],
    states: &mut [audio::PlaybackState],
    configs: &[audio::TrackConfig],
    sample_rate: f32,
) {
    for (state, config) in states.iter_mut().zip(configs.iter()) {
        let sample = state.render_sample(config, sample_rate);

        let (l_gain, r_gain) = pan_to_gains(config.pan);

        let left = sample * l_gain * config.volume;
        let right = sample * r_gain * config.volume;

        if output.len() >= 2 {
            output[0] += left;
            output[1] += right;
        } else if !output.is_empty() {
            output[0] += sample * config.volume;
        }
    }
}

fn pan_to_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4; // 0 to PI/2
    let l_gain = angle.cos();
    let r_gain = angle.sin();
    (l_gain, r_gain)
}
