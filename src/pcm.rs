use anyhow::{ensure, Result};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;
pub struct Pcm {
    channels: usize,
    pending: VecDeque<f32>,
    resampler: Option<SincFixedIn<f32>>,
}
impl Pcm {
    pub fn new(rate: u32, channels: u16) -> Result<Self> {
        ensure!(rate > 0 && channels > 0, "Invalid microphone format");
        let resampler = if rate == 48000 {
            None
        } else {
            Some(SincFixedIn::new(
                48000.0 / rate as f64,
                1.0,
                SincInterpolationParameters {
                    sinc_len: 128,
                    f_cutoff: 0.95,
                    interpolation: SincInterpolationType::Cubic,
                    oversampling_factor: 256,
                    window: WindowFunction::BlackmanHarris2,
                },
                960,
                1,
            )?)
        };
        Ok(Self {
            channels: channels as usize,
            pending: VecDeque::new(),
            resampler,
        })
    }
    pub fn convert(&mut self, input: &[f32]) -> Result<Vec<u8>> {
        ensure!(
            input.len().is_multiple_of(self.channels),
            "Incomplete input audio frame"
        );
        self.pending.extend(
            input
                .chunks_exact(self.channels)
                .map(|frame| frame.iter().sum::<f32>() / self.channels as f32),
        );
        let mut pcm = Vec::new();
        while self.pending.len() >= 960 {
            let mono: Vec<_> = self.pending.drain(..960).collect();
            let samples = match self.resampler.as_mut() {
                Some(r) => r.process(&[mono], None)?.remove(0),
                None => mono,
            };
            for sample in samples {
                pcm.extend_from_slice(
                    &((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes(),
                );
            }
        }
        Ok(pcm)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stereo_downmix_and_clipping() {
        let mut pcm = Pcm::new(48000, 2).unwrap();
        assert!(pcm.convert(&[1.0, 1.0]).unwrap().is_empty());
        let mut input = vec![0.5; 1918];
        input[0] = 4.0;
        input[1] = 4.0;
        let bytes = pcm.convert(&input).unwrap();
        assert_eq!(bytes.len(), 1920);
        assert_eq!(i16::from_le_bytes([bytes[0], bytes[1]]), 32767);
        assert_eq!(i16::from_le_bytes([bytes[2], bytes[3]]), 32767);
        assert!((i16::from_le_bytes([bytes[4], bytes[5]]) - 16384).abs() <= 1);
    }
    #[test]
    fn resamples_44100_hz_tone_without_changing_pitch() {
        let mut pcm = Pcm::new(44100, 1).unwrap();
        let mut out = Vec::new();
        for start in (0..44100).step_by(441) {
            let block: Vec<_> = (start..start + 441)
                .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin() * 0.2)
                .collect();
            out.extend(pcm.convert(&block).unwrap());
        }
        let samples: Vec<_> = out
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        // The final incomplete 960-frame block stays buffered; the sinc filter
        // also has startup latency. Neither should change the tone's pitch.
        let expected = (44100 / 960 * 960) as f64 * 48000.0 / 44100.0;
        assert!(
            (samples.len() as f64 - expected).abs() < 256.0,
            "{} samples; expected {expected}",
            samples.len()
        );
        let crossings = samples[1000..]
            .windows(2)
            .filter(|s| s[0] <= 0 && s[1] > 0)
            .count();
        let hz = crossings as f64 * 48000.0 / (samples.len() - 1000) as f64;
        assert!((hz - 440.0).abs() < 2.0, "{hz}");
    }
}
