use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use maudio::{
    audio::sample_rate::SampleRate,
    device::{
        device_builder::{DeviceBuilder, DeviceBuilderOps},
        Device, DeviceOps,
    },
};

const CHANNELS: u32 = 2;
const QUEUE_CAPACITY: usize = 16_384;

pub struct Audio {
    samples: Arc<ArrayQueue<f32>>,
    _device: Option<Device<f32>>,
}

impl Audio {
    pub fn new() -> Self {
        let samples = Arc::new(ArrayQueue::new(QUEUE_CAPACITY));
        let callback_samples = Arc::clone(&samples);

        let device = DeviceBuilder::playback()
            .f32()
            .playback_channels(CHANNELS)
            .sample_rate(SampleRate::Sr48000)
            .with_callback(move |_device, output| {
                mix_mono_to_stereo(output, || callback_samples.pop());
            });

        let device = match device {
            Ok(mut device) => match device.device_start() {
                Ok(()) => Some(device),
                Err(error) => {
                    eprintln!("Audio playback disabled: could not start miniaudio device: {error}");
                    None
                }
            },
            Err(error) => {
                eprintln!("Audio playback disabled: could not initialize miniaudio: {error}");
                None
            }
        };

        Self {
            samples,
            _device: device,
        }
    }

    pub fn sample_queue(&self) -> Arc<ArrayQueue<f32>> {
        Arc::clone(&self.samples)
    }
}

impl Default for Audio {
    fn default() -> Self {
        Self::new()
    }
}

pub fn enqueue_sound(queue: &ArrayQueue<f32>, name: &str) {
    let (frequency, duration, volume) = match name {
        "bounce" => (520.0, 0.045, 0.18),
        "brick" => (760.0, 0.085, 0.20),
        "life_lost" => (180.0, 0.18, 0.24),
        _ => return,
    };

    for sample in synthesize_tone(frequency, duration, volume) {
        if queue.push(sample).is_err() {
            break;
        }
    }
}

fn synthesize_tone(frequency: f32, duration_seconds: f32, volume: f32) -> Vec<f32> {
    const SAMPLE_RATE: f32 = 48_000.0;
    let frame_count = (SAMPLE_RATE * duration_seconds).round() as usize;
    (0..frame_count)
        .map(|frame| {
            let phase = std::f32::consts::TAU * frequency * frame as f32 / SAMPLE_RATE;
            let envelope = 1.0 - frame as f32 / frame_count.max(1) as f32;
            (phase.sin() * envelope * volume).clamp(-1.0, 1.0)
        })
        .collect()
}

fn mix_mono_to_stereo(output: &mut [f32], mut next_sample: impl FnMut() -> Option<f32>) {
    let complete_len = output.len() - output.len() % CHANNELS as usize;
    for frame in output[..complete_len].chunks_exact_mut(CHANNELS as usize) {
        let sample = next_sample().unwrap_or(0.0);
        frame[0] = sample;
        frame[1] = sample;
    }
    output[complete_len..].fill(0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_effect_samples_are_finite_and_bounded() {
        for (name, expected_count, maximum) in [
            ("bounce", 2160_usize, 0.18_f32),
            ("brick", 4080_usize, 0.20_f32),
            ("life_lost", 8640_usize, 0.24_f32),
        ] {
            let queue = ArrayQueue::new(QUEUE_CAPACITY);
            enqueue_sound(&queue, name);
            let mut count = 0;

            while let Some(sample) = queue.pop() {
                assert!(sample.is_finite(), "{name}");
                assert!(sample.abs() <= maximum, "{name}");
                count += 1;
            }

            assert_eq!(count, expected_count, "{name}");
        }
    }

    #[test]
    fn callback_mixes_mono_samples_to_stereo_without_allocating() {
        let mut source = [0.25, -0.5].into_iter();
        let mut output = [9.0; 5];

        mix_mono_to_stereo(&mut output, || source.next());

        assert_eq!(output, [0.25, 0.25, -0.5, -0.5, 0.0]);
    }

    #[test]
    fn unknown_effect_does_not_write_to_audio_queue() {
        let queue = ArrayQueue::new(64);
        enqueue_sound(&queue, "not-a-sound");
        assert!(queue.is_empty());
    }
}
