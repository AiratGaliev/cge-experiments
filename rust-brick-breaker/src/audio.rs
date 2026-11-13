use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use maudio::{
    audio::sample_rate::SampleRate,
    device::{
        Device,
        device_builder::{DeviceBuilder, DeviceBuilderOps},
    },
};

const CHANNELS: u32 = 2;
const QUEUE_CAPACITY: usize = 32_768;
const SAMPLE_RATE: f32 = 48_000.0;

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

pub fn enqueue_tone(
    queue: &ArrayQueue<f32>,
    frequency: f32,
    duration_seconds: f32,
    volume: f32,
) -> bool {
    if !frequency.is_finite()
        || !(20.0..=20_000.0).contains(&frequency)
        || !duration_seconds.is_finite()
        || !(0.005..=2.0).contains(&duration_seconds)
        || !volume.is_finite()
        || !(0.0..=1.0).contains(&volume)
    {
        return false;
    }

    let frame_count = (SAMPLE_RATE * duration_seconds).round() as usize;
    if frame_count > queue.capacity().saturating_sub(queue.len()) {
        return false;
    }

    for sample in synthesize_tone(frequency, duration_seconds, volume) {
        if queue.push(sample).is_err() {
            return false;
        }
    }
    true
}

fn synthesize_tone(frequency: f32, duration_seconds: f32, volume: f32) -> Vec<f32> {
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
    let (frames, remainder) = output.as_chunks_mut::<2>();
    for frame in frames {
        let sample = next_sample().unwrap_or(0.0);
        frame[0] = sample;
        frame[1] = sample;
    }
    remainder.fill(0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_generation_is_finite_bounded_and_uses_the_requested_duration() {
        let queue = ArrayQueue::new(QUEUE_CAPACITY);
        assert!(enqueue_tone(&queue, 440.0, 0.05, 0.2));

        let mut count = 0;
        while let Some(sample) = queue.pop() {
            assert!(sample.is_finite());
            assert!(sample.abs() <= 0.2);
            count += 1;
        }
        assert_eq!(count, 2400);
    }

    #[test]
    fn invalid_tone_parameters_are_rejected() {
        let queue = ArrayQueue::new(64);
        assert!(!enqueue_tone(&queue, f32::NAN, 0.1, 0.5));
        assert!(!enqueue_tone(&queue, 440.0, 3.0, 0.5));
        assert!(!enqueue_tone(&queue, 440.0, 0.1, 1.5));
        assert!(queue.is_empty());
    }

    #[test]
    fn tone_is_rejected_without_partial_enqueue_when_queue_capacity_is_insufficient() {
        let queue = ArrayQueue::new(1024);
        assert!(!enqueue_tone(&queue, 440.0, 0.05, 0.2));
        assert!(queue.is_empty());
    }

    #[test]
    fn callback_mixes_mono_samples_to_stereo_without_allocating() {
        let mut source = [0.25, -0.5].into_iter();
        let mut output = [9.0; 5];

        mix_mono_to_stereo(&mut output, || source.next());

        assert_eq!(output, [0.25, 0.25, -0.5, -0.5, 0.0]);
    }
}
