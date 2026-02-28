use crate::graph::{AudioGraph, NodeId, NodeKind, PortKind};
use crate::{Project, audio, events, scripting, timing};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam::channel::{Receiver, Sender};
use ringbuf::{
    HeapCons, HeapProd, HeapRb,
    traits::{Consumer, Producer, Split},
};
use std::collections::HashMap;
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
    CurrentNodes { active_patterns: HashMap<NodeId, NodeId> },
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

/// Runtime config for a single instrument node, hot-swappable via ArcSwap.
#[derive(Debug, Clone)]
pub struct InstrumentConfig {
    pub node_id: NodeId,
    pub instrument: audio::Instrument,
    pub adsr: audio::ADSRConfig,
    pub volume: f32,
    pub pan: f32,
}

impl InstrumentConfig {
    pub fn num_oscillators(&self) -> usize {
        match &self.instrument {
            audio::Instrument::MultiOsc { oscillators } => oscillators.len(),
            audio::Instrument::Sampler { .. } => 0,
        }
    }
}

struct EngineState {
    project: Option<Project>,
    instrument_configs: Option<Arc<ArcSwap<Vec<InstrumentConfig>>>>,
    sample_counter: Option<Arc<AtomicU64>>,
    lua_runtime: Option<scripting::LuaRuntime>,
    audio_stream: Option<cpal::Stream>,
    playing: bool,
}

fn engine_thread(command_rx: Receiver<EngineCommand>, update_tx: Sender<EngineUpdate>) {
    let mut state = EngineState {
        project: None,
        instrument_configs: None,
        sample_counter: None,
        lua_runtime: None,
        audio_stream: None,
        playing: false,
    };

    loop {
        match command_rx.recv() {
            Ok(EngineCommand::LoadProject(path)) => match Project::load(&path) {
                Ok(project) => {
                    tracing::info!("Project loaded successfully");

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
                tracing::debug!("Reloading project with updated data");

                if let Some(ref instrument_configs) = state.instrument_configs {
                    let new_configs = build_instrument_configs(&project.graph);
                    instrument_configs.store(Arc::new(new_configs));
                    tracing::debug!("Hot-swapped instrument configs");
                }

                state.project = Some(project);
            }
            Ok(EngineCommand::Play) => {
                if let Some(ref project) = state.project {
                    if state.audio_stream.is_none() {
                        match setup_audio(project) {
                            Ok((stream, configs, counter, lua)) => {
                                state.audio_stream = Some(stream);
                                state.instrument_configs = Some(configs);
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
                state.instrument_configs = None;
                state.sample_counter = None;
                state.playing = false;
                let _ = update_tx.send(EngineUpdate::PlaybackState { playing: false });
                let _ = update_tx.send(EngineUpdate::CurrentNodes {
                    active_patterns: HashMap::new(),
                });
            }

            Ok(EngineCommand::SetVariable { .. }) => {
                // TODO
            }

            Err(crossbeam::channel::RecvError) => break,
        }
    }
}

fn build_instrument_configs(graph: &AudioGraph) -> Vec<InstrumentConfig> {
    let mut configs = Vec::new();

    // Find mixer to get per-channel volume/pan
    let mixer_node = graph.nodes.iter().find(|n| matches!(n.kind, NodeKind::Mixer { .. }));
    let mixer_channels = match mixer_node {
        Some(n) => match &n.kind {
            NodeKind::Mixer { channels } => channels.clone(),
            _ => vec![],
        },
        None => vec![],
    };

    // Build lookup: synth node id → mixer channel index (from audio edges)
    let mut synth_to_mixer_port: HashMap<&NodeId, usize> = HashMap::new();
    if let Some(mixer) = mixer_node {
        for edge in &graph.edges {
            if edge.to_node == mixer.id && edge.kind == PortKind::Audio {
                synth_to_mixer_port.insert(&edge.from_node, edge.to_port);
            }
        }
    }

    for node in &graph.nodes {
        if let NodeKind::Synth { instrument, adsr } = &node.kind {
            let (volume, pan) = synth_to_mixer_port
                .get(&node.id)
                .and_then(|port| mixer_channels.get(*port))
                .map(|ch| (ch.volume, ch.pan))
                .unwrap_or((1.0, 0.0));

            configs.push(InstrumentConfig {
                node_id: node.id.clone(),
                instrument: instrument.clone(),
                adsr: adsr.clone(),
                volume,
                pan,
            });
        }
    }

    configs
}

struct TimingState {
    graph: AudioGraph,
    /// For each initial_pattern_node entry, track the current SM state
    current_sm_states: Vec<NodeId>,
    sequence_end_samples: Vec<u64>,
    /// pattern_node_id → target instrument node_id
    pattern_to_instrument: HashMap<NodeId, NodeId>,
}

struct AudioState {
    /// instrument node_id → PlaybackState
    playback_states: HashMap<NodeId, audio::PlaybackState>,
    pending_event: Option<events::ScheduledEvent>,
    consumer: HeapCons<events::ScheduledEvent>,
    instrument_configs: Arc<ArcSwap<Vec<InstrumentConfig>>>,
    /// For mixer: which input port maps to which instrument node
    mixer_inputs: Vec<(NodeId, usize)>,
    sample_rate: f32,
    num_channels: usize,
}

type AudioSetupResult = (
    cpal::Stream,
    Arc<ArcSwap<Vec<InstrumentConfig>>>,
    Arc<AtomicU64>,
    scripting::LuaRuntime,
);

fn setup_audio(project: &Project) -> Result<AudioSetupResult, Box<dyn std::error::Error>> {
    let lua_runtime = scripting::LuaRuntime::new()?;

    let configs = build_instrument_configs(&project.graph);
    let instrument_configs = Arc::new(ArcSwap::from_pointee(configs));
    let sample_counter = Arc::new(AtomicU64::new(0));

    let bpm = project.bpm;
    let sample_rate = project.sample_rate as f32;

    let ring_buffer = HeapRb::<events::ScheduledEvent>::new(4096);
    let (mut producer, consumer) = ring_buffer.split();

    // Build pattern → instrument lookup
    let mut pattern_to_instrument: HashMap<NodeId, NodeId> = HashMap::new();
    for edge in &project.graph.edges {
        if edge.kind == PortKind::Event {
            pattern_to_instrument.insert(edge.from_node.clone(), edge.to_node.clone());
        }
    }

    // Initialize timing state from initial_pattern_nodes
    let mut current_sm_states = Vec::new();
    let mut sequence_end_samples = Vec::new();

    for initial_node_id in &project.graph.initial_pattern_nodes {
        current_sm_states.push(initial_node_id.clone());

        if let Some(node) = project.graph.get_node(initial_node_id) {
            if let Some(sequence) = node.kind.sequence() {
                let target = pattern_to_instrument.get(initial_node_id);
                if let Some(target_node) = target {
                    let _ = timing::schedule_sequence_events(
                        &sequence,
                        target_node,
                        0,
                        bpm,
                        sample_rate,
                        &mut producer,
                        Some(&lua_runtime),
                    );
                }
                let duration = sequence.duration_samples(bpm, sample_rate);
                sequence_end_samples.push(duration as u64);
            } else {
                sequence_end_samples.push(u64::MAX);
            }
        } else {
            sequence_end_samples.push(u64::MAX);
        }
    }

    let timing_state = TimingState {
        graph: project.graph.clone(),
        current_sm_states,
        sequence_end_samples,
        pattern_to_instrument: pattern_to_instrument.clone(),
    };

    let counter_timing = sample_counter.clone();
    let lua_timing = scripting::LuaRuntime::new()?;

    std::thread::spawn(move || {
        timing_thread(timing_state, producer, counter_timing, bpm, sample_rate, lua_timing);
    });

    // Build mixer input mapping
    let mut mixer_inputs = Vec::new();
    for node in &project.graph.nodes {
        if matches!(node.kind, NodeKind::Mixer { .. }) {
            for edge in &project.graph.edges {
                if edge.to_node == node.id && edge.kind == PortKind::Audio {
                    mixer_inputs.push((edge.from_node.clone(), edge.to_port));
                }
            }
            break;
        }
    }

    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("No output device")?;
    let config = device.default_output_config()?;
    let stream_config: cpal::StreamConfig = config.into();

    let num_channels = stream_config.channels as usize;

    // Create playback states for each instrument node
    let mut playback_states: HashMap<NodeId, audio::PlaybackState> = HashMap::new();
    for node in &project.graph.nodes {
        if matches!(node.kind, NodeKind::Synth { .. }) {
            playback_states.insert(node.id.clone(), audio::PlaybackState::new());
        }
    }

    let mut audio_state = AudioState {
        playback_states,
        pending_event: None,
        consumer,
        instrument_configs: instrument_configs.clone(),
        mixer_inputs,
        sample_rate,
        num_channels,
    };

    let counter_audio = sample_counter.clone();

    let stream = device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            audio_callback(data, &mut audio_state, &counter_audio);
        },
        |err| tracing::error!(err = %err, "Audio error"),
        None,
    )?;

    stream.play()?;

    Ok((stream, instrument_configs, sample_counter, lua_runtime))
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

        for i in 0..state.current_sm_states.len() {
            let end_sample = state.sequence_end_samples[i];
            if current_sample >= end_sample {
                let current_node_id = &state.current_sm_states[i];
                let sm_edges = state.graph.get_sm_outgoing(current_node_id);

                let next_node_id = if let Some(edge) = sm_edges.first() {
                    edge.to.clone()
                } else {
                    current_node_id.clone()
                };

                tracing::debug!(
                    current_node = %current_node_id,
                    next_node = %next_node_id,
                    "Sequence transition"
                );

                // Stop all notes on the target instrument
                if let Some(target) = state.pattern_to_instrument.get(current_node_id) {
                    let _ = producer.try_push(events::ScheduledEvent {
                        sample_timestamp: current_sample,
                        event: events::Event::StopAllNotes {
                            target_node: target.clone(),
                        },
                    });
                }

                state.current_sm_states[i] = next_node_id.clone();

                if let Some(node) = state.graph.get_node(&next_node_id)
                    && let Some(sequence) = node.kind.sequence()
                {
                    if let Some(target) = state.pattern_to_instrument.get(&next_node_id) {
                        let _ = timing::schedule_sequence_events(
                            &sequence,
                            target,
                            current_sample,
                            bpm,
                            sample_rate,
                            &mut producer,
                            Some(&lua_runtime),
                        );
                    }

                    let duration = sequence.duration_samples(bpm, sample_rate);
                    state.sequence_end_samples[i] = current_sample + duration as u64;
                }
            }
        }
    }
}

fn audio_callback(data: &mut [f32], state: &mut AudioState, sample_counter: &Arc<AtomicU64>) {
    let num_frames = data.len() / state.num_channels;
    let current_sample = sample_counter.load(Ordering::Relaxed);
    let buffer_end = current_sample + num_frames as u64;

    let configs = state.instrument_configs.load();

    // Build a quick lookup: node_id → config index
    let config_map: HashMap<&NodeId, usize> = configs
        .iter()
        .enumerate()
        .map(|(i, c)| (&c.node_id, i))
        .collect();

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
            process_event(&mut state.playback_states, &configs, &config_map, &events[event_idx]);
            event_idx += 1;
        }

        render_frame(
            &mut data[frame * state.num_channels..(frame + 1) * state.num_channels],
            &mut state.playback_states,
            &configs,
            &config_map,
            &state.mixer_inputs,
            state.sample_rate,
        );
        frame += 1;
    }

    sample_counter.fetch_add(num_frames as u64, Ordering::Relaxed);
}

fn process_event(
    playback_states: &mut HashMap<NodeId, audio::PlaybackState>,
    configs: &[InstrumentConfig],
    config_map: &HashMap<&NodeId, usize>,
    event: &events::ScheduledEvent,
) {
    match &event.event {
        events::Event::MidiEvent { target_node, message } => {
            if let Some(state) = playback_states.get_mut(target_node) {
                match message {
                    events::MidiMessage::NoteOn { pitch, velocity } => {
                        let num_oscs = config_map
                            .get(target_node)
                            .and_then(|&i| configs.get(i))
                            .map_or(0, |c| c.num_oscillators());
                        state.note_on(*pitch, *velocity, num_oscs);
                    }
                    events::MidiMessage::NoteOff { pitch } => {
                        state.note_off(*pitch);
                    }
                }
            }
        }
        events::Event::StopAllNotes { target_node } => {
            if let Some(state) = playback_states.get_mut(target_node) {
                state.stop_all();
            }
        }
        events::Event::NodeTransition { .. } => {}
    }
}

fn render_frame(
    output: &mut [f32],
    states: &mut HashMap<NodeId, audio::PlaybackState>,
    configs: &[InstrumentConfig],
    config_map: &HashMap<&NodeId, usize>,
    mixer_inputs: &[(NodeId, usize)],
    sample_rate: f32,
) {
    // Render each instrument and sum through the mixer
    for (synth_node_id, _mixer_port) in mixer_inputs {
        if let Some(state) = states.get_mut(synth_node_id)
            && let Some(&config_idx) = config_map.get(synth_node_id)
        {
            let config = &configs[config_idx];
            let sample = state.render_sample_with_config(config, sample_rate);

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
}

fn pan_to_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4; // 0 to PI/2
    let l_gain = angle.cos();
    let r_gain = angle.sin();
    (l_gain, r_gain)
}
