use anyhow::{Result, ensure};
use cuerithm_media::{AudioClock, PreparedAudio};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const WINDOW: usize = 4096;
// Spectral analysis cadence; finer waveforms reuse the nearest analysed point instead of one FFT per bin.
const MAX_ANALYSIS_POINTS: usize = 4096;
const SAMPLE_RATE: f32 = 44100.;
// Band edges and weights follow Lexicon's RGB waveform: bass, mids and highs map to red, green and blue.
const BANDS: [(f32, f32, f32); 3] = [(0., 150., 1.2), (150., 1500., 1.), (1500., 6000., 1.)];
// Lexicon's three-band waveform draws these ranges as stacked shapes: low, mid and high. Each is
// a band-pass centred on the geometric mean (Q = centre / width) followed by a low-pass at the
// upper edge, both second order, so neighbouring bands leak into each other and nest.
const LEVEL_BANDS: [(f32, f32); 3] = [(60., 120.), (120., 500.), (900., 1500.)];
const LOW_PASS_Q: f32 = 1.122; // 1 dB resonance, the Web Audio default

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveBin {
    pub min: f32,
    pub max: f32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waveform {
    pub duration_seconds: f64,
    pub clock: AudioClock,
    pub bins: Vec<WaveBin>,
    /// Per-bin red/green/blue weights from bass, mid and high energy, strongest band at 255.
    /// Empty in caches written before colour was added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bands: Vec<[u8; 3]>,
    /// Per-bin average (RMS) level over every frame in the bin, 0 to 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rms: Vec<f32>,
    /// Per-bin RMS level inside the low (60-120 Hz), mid (120-500 Hz) and high (900-1500 Hz) ranges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<[f32; 3]>,
}

struct Analysis {
    colour: [u8; 3],
    levels: [f32; 3],
}
fn round4(value: f32) -> f32 {
    (value * 10_000.).round() / 10_000.
}

struct BandAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    taper: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    ranges: [(usize, usize); 3],
    level_weights: [Vec<f32>; 3],
}
impl BandAnalyzer {
    fn new() -> Self {
        let hz = SAMPLE_RATE / WINDOW as f32;
        let last = WINDOW / 2 - 1;
        let edge = |hz_edge: f32| ((hz_edge / hz).floor() as usize).min(last);
        Self {
            fft: FftPlanner::new().plan_fft_forward(WINDOW),
            taper: (0..WINDOW)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos())
                .collect(),
            buffer: vec![Complex::new(0., 0.); WINDOW],
            ranges: BANDS.map(|(low, high, _)| (edge(low), edge(high))),
            level_weights: LEVEL_BANDS.map(|(low, high)| {
                let centre = (low * high).sqrt();
                let q = centre / (high - low);
                (0..=last)
                    .map(|k| {
                        let f = k as f32 * hz;
                        let x = f / centre;
                        let band_pass = 1. / (1. + q * q * (x - 1. / x).powi(2));
                        let y = f / high;
                        let low_pass = 1. / ((1. - y * y).powi(2) + (y / LOW_PASS_Q).powi(2));
                        if k == 0 { 0. } else { band_pass * low_pass }
                    })
                    .collect()
            }),
        }
    }
    fn analyze(&mut self, mono: &[f32]) -> Analysis {
        for (i, slot) in self.buffer.iter_mut().enumerate() {
            *slot = Complex::new(mono.get(i).map_or(0., |v| v * self.taper[i]), 0.);
        }
        self.fft.process(&mut self.buffer);
        let mut level = [0f32; 3];
        for (band, (first, last)) in self.ranges.iter().enumerate() {
            let power: f32 = self.buffer[*first..=*last]
                .iter()
                .map(|c| c.norm_sqr())
                .sum();
            level[band] = (power / (last - first + 1) as f32).sqrt() * BANDS[band].2;
        }
        let strongest = level.iter().cloned().fold(0., f32::max);
        let colour = if strongest <= f32::EPSILON {
            [0; 3]
        } else {
            level.map(|v| (v / strongest * 255.).round() as u8)
        };
        // Parseval: mean square of the windowed signal is 2 * sum |X|^2 / (N * sum w^2) per band.
        let window_power: f32 = self.taper[..mono.len().min(WINDOW)]
            .iter()
            .map(|w| w * w)
            .sum();
        let levels = self.level_weights.each_ref().map(|weights| {
            let power: f32 = weights
                .iter()
                .zip(&self.buffer)
                .map(|(weight, c)| weight * c.norm_sqr())
                .sum();
            if window_power <= f32::EPSILON {
                0.
            } else {
                round4((2. * power / (WINDOW as f32 * window_power)).sqrt())
            }
        });
        Analysis { colour, levels }
    }
}

pub fn waveform(audio: &PreparedAudio, bins: usize) -> Result<Waveform> {
    ensure!((1..=16384).contains(&bins), "Invalid waveform resolution");
    let mut output = vec![WaveBin { min: 0., max: 0. }; bins];
    let points = bins.min(MAX_ANALYSIS_POINTS);
    let mut point_bands = vec![[0u8; 3]; points];
    let mut point_levels = vec![[0f32; 3]; points];
    let mut energy = vec![(0f64, 0u64); bins];
    let mut analyzer = BandAnalyzer::new();
    // Mono samples from the first frame still needed by an unanalysed bin.
    let (mut mono, mut base, mut next) = (Vec::<f32>::new(), 0u64, 0usize);
    let start_of = |point: usize| (point as u64 * audio.frames).div_ceil(points as u64);
    let mut pos = 0;
    while pos < audio.frames {
        let samples = audio.read_frames(pos, 65536)?;
        for (i, pair) in samples.as_chunks::<2>().0.iter().enumerate() {
            let bin = ((pos + i as u64) * bins as u64 / audio.frames) as usize;
            output[bin].min = output[bin].min.min(pair[0]).min(pair[1]);
            output[bin].max = output[bin].max.max(pair[0]).max(pair[1]);
            energy[bin].0 += f64::from(pair[0] * pair[0] + pair[1] * pair[1]) / 2.;
            energy[bin].1 += 1;
            if pos + i as u64 >= base + mono.len() as u64 {
                mono.push((pair[0] + pair[1]) / 2.);
            }
        }
        pos += samples.len() as u64 / 2;
        let end = base + mono.len() as u64;
        while next < points {
            let from = start_of(next);
            if from + WINDOW as u64 > end && end < audio.frames {
                break;
            }
            let at = (from - base) as usize;
            let analysis = analyzer.analyze(&mono[at..(at + WINDOW).min(mono.len())]);
            point_bands[next] = analysis.colour;
            point_levels[next] = analysis.levels;
            next += 1;
        }
        let keep = if next < points { start_of(next) } else { end };
        if keep >= end {
            mono.clear();
        } else {
            mono.drain(..(keep - base) as usize);
        }
        base = keep;
    }
    // Each bin takes the analysed point at its centre.
    let nearest = |bin: usize| (2 * bin + 1) * points / (2 * bins);
    let bands = (0..bins).map(|bin| point_bands[nearest(bin)]).collect();
    let levels = (0..bins).map(|bin| point_levels[nearest(bin)]).collect();
    Ok(Waveform {
        duration_seconds: audio.clock.duration_seconds(),
        clock: audio.clock.clone(),
        bins: output,
        bands,
        rms: energy
            .iter()
            .map(|(sum, count)| round4((sum / (*count).max(1) as f64).sqrt() as f32))
            .collect(),
        levels,
    })
}

/// The resolutions the player and the preview ask for (`apps/desktop/src/App.tsx`).
pub const CACHED_RESOLUTIONS: [usize; 2] = [240, 16384];
/// Name (without `.json`) of the cache file for one source file, decoder and resolution.
pub fn waveform_cache_key(source_sha256: &str, decoder_identity: &str, bins: usize) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(format!("{source_sha256}:{decoder_identity}:{bins}"))
    )
}
pub fn cached_waveform(
    audio: &PreparedAudio,
    bins: usize,
    cache: &std::path::Path,
) -> Result<Waveform> {
    use std::io::{Read, Write};
    ensure!((1..=16384).contains(&bins), "Invalid waveform resolution");
    let key = waveform_cache_key(&audio.source_sha256, &audio.decoder_identity, bins);
    std::fs::create_dir_all(cache)?;
    let path = cache.join(format!("{key}.json"));
    if let Ok(file) = std::fs::File::open(&path)
        && let Ok(value) = serde_json::from_reader::<_, Waveform>(file.take(4 * 1024 * 1024))
        && value.clock.frames == audio.frames
        && value.clock.sample_rate == 44100
        && value.duration_seconds == audio.clock.duration_seconds()
        && value.bins.len() == bins
        && value.bands.len() == bins
        && value.rms.len() == bins
        && value.levels.len() == bins
        && value
            .bins
            .iter()
            .all(|b| b.min.is_finite() && b.max.is_finite() && b.min <= b.max)
    {
        // Used now: the cache limit drops the files nobody asked for first.
        crate::cache_limits::touch(&path);
        return Ok(value);
    }
    let value = waveform(audio, bins)?;
    let mut temp = tempfile::NamedTempFile::new_in(cache)?;
    temp.write_all(&serde_json::to_vec(&value)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tone(hz: f32) -> Vec<f32> {
        (0..WINDOW)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / SAMPLE_RATE).sin())
            .collect()
    }
    #[test]
    fn bass_mids_and_highs_map_to_red_green_and_blue() {
        let mut analyzer = BandAnalyzer::new();
        for (hz, channel) in [(60., 0), (700., 1), (3500., 2)] {
            let colour = analyzer.analyze(&tone(hz)).colour;
            assert_eq!(
                colour[channel], 255,
                "{hz} Hz should peak channel {channel}"
            );
            for other in (0..3).filter(|c| *c != channel) {
                assert!(
                    colour[other] < 60,
                    "{hz} Hz leaked into channel {other}: {colour:?}"
                );
            }
        }
    }
    #[test]
    fn band_levels_follow_the_filtered_band_and_nest_through_leakage() {
        let mut analyzer = BandAnalyzer::new();
        // A unit sine has RMS 0.71. Inside its own band it reads close to that; the neighbours see
        // only filter skirts, so the right band must clearly lead.
        for (hz, band) in [(90., 0), (250., 1), (1200., 2)] {
            let levels = analyzer.analyze(&tone(hz)).levels;
            assert!(
                (0.5..1.1).contains(&levels[band]),
                "{hz} Hz read {levels:?}"
            );
            for other in (0..3).filter(|b| *b != band) {
                assert!(
                    levels[other] * 1.8 < levels[band],
                    "{hz} Hz did not lead: {levels:?}"
                );
            }
        }
    }
    #[test]
    fn silence_has_no_colour() {
        assert_eq!(
            BandAnalyzer::new().analyze(&vec![0.; WINDOW]).colour,
            [0; 3]
        );
    }
}
