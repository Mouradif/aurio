use arc_swap::ArcSwap;
use aurio::parser::parse_file;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::Arc;
use std::{env, fs};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <file.au>", args[0]);
        std::process::exit(1);
    }

    let filepath = &args[1];

    // Load initial graph
    let content = fs::read_to_string(filepath).expect("failed to read file");
    let initial_graph = parse_file(&content).expect("failed to parse initial file");

    let graph = Arc::new(ArcSwap::from_pointee(initial_graph));
    let graph_clone = graph.clone();

    // Setup cpal
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().expect("no default config");

    let stream = device
        .build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let current = graph_clone.load_full();
                current.process(data);
            },
            |err| eprintln!("Stream error: {}", err),
            None,
        )
        .expect("failed to build stream");

    stream.play().expect("failed to play");

    // Setup file watcher
    let graph_for_watcher = graph.clone();
    let filepath_owned = filepath.to_string();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| match res {
            Ok(event) => {
                if event.kind.is_modify() {
                    println!("File changed, reloading...");
                    match fs::read_to_string(&filepath_owned) {
                        Ok(content) => match parse_file(&content) {
                            Ok(new_graph) => {
                                graph_for_watcher.store(Arc::new(new_graph));
                                println!("Graph updated successfully");
                            }
                            Err(e) => eprintln!("Parse error: {}", e),
                        },
                        Err(e) => eprintln!("Read error: {}", e),
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(Path::new(filepath), RecursiveMode::NonRecursive)
        .expect("failed to watch file");

    println!("Watching {} - edit and save to update audio", filepath);
    println!("Press Ctrl+C to stop");

    // Keep running
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
