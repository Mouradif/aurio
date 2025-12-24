use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().expect("no default config");

    match config.sample_format() {
        cpal::SampleFormat::F32 => run::<f32>(&device, &config.into()),
        cpal::SampleFormat::I16 => run::<i16>(&device, &config.into()),
        cpal::SampleFormat::U16 => run::<u16>(&device, &config.into()),
        _ => panic!("unsupported format"),
    }
}

fn run<T: cpal::SizedSample + cpal::FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
) {
    let sample_rate = config.sample_rate as f32;
    let channels = config.channels as usize;

    let sample_count = Arc::new(AtomicU32::new(0));
    let sample_count_clone = sample_count.clone();

    // Frequency schedule: (freq_hz, duration_ms)
    let schedule: Vec<(f32, u32)> = vec![(440.0, 150), (660.0, 150), (880.0, 100)];

    // Convert to sample boundaries
    let boundaries: Vec<(f32, u32)> = schedule
        .iter()
        .scan(0u32, |acc, &(freq, ms)| {
            *acc += (sample_rate * ms as f32 / 1000.0) as u32;
            Some((freq, *acc))
        })
        .collect();

    let total_samples = boundaries.last().unwrap().1;
    let mut phase = 0.0f32;

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    let count = sample_count.fetch_add(1, Ordering::Relaxed);

                    let freq = boundaries
                        .iter()
                        .find(|(_, end)| count < *end)
                        .map(|(f, _)| *f)
                        .unwrap_or(0.0);

                    let sample = if freq > 0.0 {
                        let out = (phase * std::f32::consts::TAU).sin() * 0.3;
                        phase = (phase + freq / sample_rate) % 1.0;
                        out
                    } else {
                        0.0
                    };

                    let sample: T = T::from_sample(sample);
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

    // Wait until done
    while sample_count_clone.load(Ordering::Relaxed) < total_samples {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

