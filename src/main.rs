use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

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

    let mut phase = 0.0f32;
    let freq = 440.0; // A4 note
    let phase_inc = freq / sample_rate;

    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for frame in data.chunks_mut(channels) {
                    let sample = (phase * std::f32::consts::TAU).sin() * 0.3;
                    phase = (phase + phase_inc) % 1.0;

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

    std::thread::sleep(std::time::Duration::from_millis(400));
}
