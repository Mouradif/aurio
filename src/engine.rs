use crate::{Project, audio, events, scripting, timing};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam::channel::{Receiver, Sender};
use crossbeam::queue::ArrayQueue;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone)]
pub enum EngineCommand {
    LoadProject(PathBuf),
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
    tracks: Option<Arc<RwLock<Vec<audio::Track>>>>,
    event_queue: Option<Arc<ArrayQueue<events::ScheduledEvent>>>,
    sample_counter: Option<Arc<AtomicU64>>,
    lua_runtime: Option<scripting::LuaRuntime>,
    audio_stream: Option<cpal::Stream>,
    playing: bool,
}

fn engine_thread(command_rx: Receiver<EngineCommand>, update_tx: Sender<EngineUpdate>) {
    let mut state = EngineState {
        project: None,
        tracks: None,
        event_queue: None,
        sample_counter: None,
        lua_runtime: None,
        audio_stream: None,
        playing: false,
    };

    loop {
        match command_rx.recv_timeout(std::time::Duration::from_millis(50)) {
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

            Ok(EngineCommand::Play) => {
                if let Some(ref project) = state.project {
                    if state.audio_stream.is_none() {
                        match setup_audio(project) {
                            Ok((stream, tracks, queue, counter, lua)) => {
                                state.audio_stream = Some(stream);
                                state.tracks = Some(tracks);
                                state.event_queue = Some(queue);
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
                state.tracks = None;
                state.event_queue = None;
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

            Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                // Timeout - continue to send updates
            }
            Err(crossbeam::channel::RecvTimeoutError::Disconnected) => break,
        }

        if state.playing {
            if let Some(ref tracks) = state.tracks {
                let tracks_read = tracks.read();
                let current_nodes: Vec<(usize, String)> = tracks_read
                    .iter()
                    .map(|t| (t.id, t.current_node.clone()))
                    .collect();
                let _ = update_tx.send(EngineUpdate::CurrentNodes {
                    track_nodes: current_nodes,
                });
            }
        }
    }
}

fn setup_audio(
    project: &Project,
) -> Result<
    (
        cpal::Stream,
        Arc<RwLock<Vec<audio::Track>>>,
        Arc<ArrayQueue<events::ScheduledEvent>>,
        Arc<AtomicU64>,
        scripting::LuaRuntime,
    ),
    Box<dyn std::error::Error>,
> {
    let lua_runtime = scripting::LuaRuntime::new()?;

    let mut tracks_vec = Vec::new();
    for track_data in &project.tracks {
        let mut track = audio::Track::new(
            track_data.id,
            track_data.instrument.clone(),
            track_data.adsr.clone(),
        );
        track.volume = track_data.volume;
        track.pan = track_data.pan;
        track.graph = track_data.graph.clone();
        track.current_node = track_data.initial_node.clone();
        tracks_vec.push(track);
    }

    let tracks = Arc::new(RwLock::new(tracks_vec));
    let event_queue = Arc::new(ArrayQueue::new(1024));
    let sample_counter = Arc::new(AtomicU64::new(0));

    let bpm = project.bpm;
    let sample_rate = project.sample_rate as f32;

    {
        let tracks_read = tracks.read();
        for (track_id, track) in tracks_read.iter().enumerate() {
            let node = track.graph.get_node(&track.current_node).unwrap();
            timing::schedule_sequence_events(
                &node.sequence,
                track_id,
                0,
                bpm,
                sample_rate,
                &event_queue,
                Some(&lua_runtime),
            );
        }
    }

    let tracks_timing = tracks.clone();
    let queue_timing = event_queue.clone();
    let counter_timing = sample_counter.clone();
    let lua_timing = scripting::LuaRuntime::new()?;

    std::thread::spawn(move || {
        timing_thread(
            tracks_timing,
            queue_timing,
            counter_timing,
            bpm,
            sample_rate,
            lua_timing,
        );
    });

    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("No output device")?;
    let config = device.default_output_config()?;

    let tracks_audio = tracks.clone();
    let queue_audio = event_queue.clone();
    let counter_audio = sample_counter.clone();

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            audio_callback(
                data,
                &tracks_audio,
                &queue_audio,
                &counter_audio,
                sample_rate,
            );
        },
        |err| eprintln!("Audio error: {}", err),
        None,
    )?;

    stream.play()?;

    Ok((stream, tracks, event_queue, sample_counter, lua_runtime))
}

fn timing_thread(
    tracks: Arc<RwLock<Vec<audio::Track>>>,
    event_queue: Arc<ArrayQueue<events::ScheduledEvent>>,
    sample_counter: Arc<AtomicU64>,
    bpm: f32,
    sample_rate: f32,
    lua_runtime: scripting::LuaRuntime,
) {
    let mut sequence_end_samples: Vec<Option<u64>> = vec![None; 10]; // Max 10 tracks for now

    {
        let tracks_read = tracks.read();
        for (track_id, track) in tracks_read.iter().enumerate() {
            let node = track.graph.get_node(&track.current_node).unwrap();
            let duration = node.sequence.duration_samples(bpm, sample_rate);
            sequence_end_samples[track_id] = Some(duration as u64);
        }
    }

    loop {
        std::thread::sleep(std::time::Duration::from_millis(1));

        let current_sample = sample_counter.load(Ordering::Relaxed);
        let mut tracks_write = tracks.write();

        for track_id in 0..tracks_write.len() {
            if let Some(end_sample) = sequence_end_samples[track_id] {
                if current_sample >= end_sample {
                    let track = &mut tracks_write[track_id];
                    let current_node = track.current_node.clone();
                    let edges = track.graph.get_outgoing_edges(&current_node);

                    if let Some(edge) = edges.first() {
                        let next_node = edge.to.clone();

                        println!("Transitioning from {} to {}", current_node, next_node);

                        track.stop_all_notes();
                        track.current_node = next_node.clone();

                        let node = track.graph.get_node(&track.current_node).unwrap();
                        let new_start = current_sample;

                        timing::schedule_sequence_events(
                            &node.sequence,
                            track_id,
                            new_start,
                            bpm,
                            sample_rate,
                            &event_queue,
                            Some(&lua_runtime),
                        );

                        let duration = node.sequence.duration_samples(bpm, sample_rate);
                        sequence_end_samples[track_id] = Some(new_start + duration as u64);
                    } else {
                        let node = track.graph.get_node(&track.current_node).unwrap();
                        let new_start = current_sample;

                        timing::schedule_sequence_events(
                            &node.sequence,
                            track_id,
                            new_start,
                            bpm,
                            sample_rate,
                            &event_queue,
                            Some(&lua_runtime),
                        );

                        let duration = node.sequence.duration_samples(bpm, sample_rate);
                        sequence_end_samples[track_id] = Some(new_start + duration as u64);
                    }
                }
            }
        }
    }
}

fn audio_callback(
    data: &mut [f32],
    tracks: &Arc<RwLock<Vec<audio::Track>>>,
    event_queue: &Arc<ArrayQueue<events::ScheduledEvent>>,
    sample_counter: &Arc<AtomicU64>,
    sample_rate: f32,
) {
    let current_sample = sample_counter.load(Ordering::Relaxed);
    let mut tracks_write = tracks.write();

    while let Some(event) = event_queue.pop() {
        if event.sample_timestamp >= current_sample + data.len() as u64 {
            let _ = event_queue.push(event);
            break;
        }

        match event.event {
            events::Event::MidiEvent {
                track_id,
                pitch,
                velocity,
                is_note_on,
            } => {
                if track_id < tracks_write.len() {
                    if is_note_on {
                        tracks_write[track_id].note_on(pitch, velocity);
                    } else {
                        tracks_write[track_id].note_off(pitch);
                    }
                }
            }
            events::Event::NodeTransition { .. } => {}
        }
    }

    data.fill(0.0);

    for track in tracks_write.iter_mut() {
        track.render_audio(data, sample_rate);
    }

    sample_counter.fetch_add(data.len() as u64, Ordering::Relaxed);
}
