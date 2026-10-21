// SPDX-License-Identifier: Apache-2.0
//! Bounded, CPU-only voice embeddings and conservative open-set matching.
//! A similarity score is not a calibrated identity probability.
use ort::{execution_providers::CPUExecutionProvider, session::Session, value::Tensor};
use realfft::RealFftPlanner;
use std::path::Path;
pub mod consolidation;

pub const DIMENSIONS: usize = 192;
pub const MODEL_ID: &str = "campplus-cn-common-f682b514-v1";
pub const MODEL_SHA256: &str = "f682b514c05d947ee3fa91cd6ec6c5c7543479a128373fa29b1faedccd21fd11";
pub const MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx";
pub const MAX_SAMPLES: usize = 4 * 16000;

pub struct Encoder(Session);
impl Encoder {
    pub fn open(path: &Path) -> Result<Self, String> {
        let session = Session::builder()
            .and_then(|b| b.with_execution_providers([CPUExecutionProvider::default().build()]))
            .and_then(|b| b.with_intra_threads(2))
            .and_then(|b| b.with_inter_threads(1))
            .and_then(|b| b.with_parallel_execution(false))
            .and_then(|b| b.commit_from_file(path))
            .map_err(|e| e.to_string())?;
        Ok(Self(session))
    }

    pub fn encode(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
        let features = fbank(samples)?;
        let frames = features.len() / 80;
        let input = Tensor::from_array(([1usize, frames, 80], features.into_boxed_slice()))
            .map_err(|e| e.to_string())?;
        let outputs = self.0.run(ort::inputs![input]).map_err(|e| e.to_string())?;
        let (_, vector) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        normalize(vector)
    }
}

pub fn normalize(vector: &[f32]) -> Result<Vec<f32>, String> {
    if vector.len() != DIMENSIONS || vector.iter().any(|x| !x.is_finite()) {
        return Err("Invalid voice embedding".into());
    }
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if !norm.is_finite() || norm < 1e-8 {
        return Err("Empty voice embedding".into());
    }
    Ok(vector.iter().map(|x| x / norm).collect())
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != DIMENSIONS || b.len() != DIMENSIONS {
        return -1.;
    }
    let value = a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
    if value.is_finite() {
        value.clamp(-1., 1.)
    } else {
        -1.
    }
}

pub fn consistent(vectors: &[Vec<f32>]) -> bool {
    vectors.len() >= 2
        && vectors
            .iter()
            .enumerate()
            .all(|(i, a)| vectors[i + 1..].iter().all(|b| cosine(a, b) >= 0.55))
}

/// Require two independently sampled clips to agree. Reject a close runner-up,
/// low-quality samples and a weak match even if only one person is enrolled.
pub fn identify(samples: &[Vec<f32>], profiles: &[Vec<Vec<f32>>]) -> Option<(usize, f32)> {
    if !consistent(samples) {
        return None;
    }
    let mut scores: Vec<_> = profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| consistent(p))
        .map(|(i, p)| {
            let score = samples
                .iter()
                .map(|sample| {
                    p.iter()
                        .map(|reference| cosine(sample, reference))
                        .fold(-1f32, f32::max)
                })
                .fold(1f32, f32::min);
            (i, score)
        })
        .collect();
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    let &(index, score) = scores.first()?;
    let runner = scores.get(1).map_or(-1., |r| r.1);
    (score >= 0.65 && score - runner >= 0.12).then_some((index, score))
}

/// Find three clean, separated samples without loading the whole recording.
/// Subtract cross-talk instead of assuming a diarization label is exclusive.
pub fn sample_spans(speaker: &str, turns: &[(String, f64, f64)]) -> Vec<(f64, f64)> {
    sample_spans_count(speaker, turns, 3)
}

pub fn sample_spans_count(
    speaker: &str,
    turns: &[(String, f64, f64)],
    count: usize,
) -> Vec<(f64, f64)> {
    let mut own: Vec<_> = turns
        .iter()
        .filter(|(id, a, b)| id == speaker && a.is_finite() && b.is_finite() && *a >= 0. && b > a)
        .map(|(_, a, b)| (*a, *b))
        .collect();
    own.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut joined: Vec<(f64, f64)> = Vec::new();
    for (a, b) in own {
        if let Some(last) = joined.last_mut().filter(|last| a - last.1 <= 0.25) {
            last.1 = last.1.max(b);
        } else {
            joined.push((a, b));
        }
    }
    let mut candidates = Vec::new();
    for (start, end) in &joined {
        let mut spans = vec![(*start + 0.15, *end - 0.15)];
        for (other, left, right) in turns {
            if other == speaker {
                continue;
            }
            spans = spans
                .into_iter()
                .flat_map(|(a, b)| {
                    if *right <= a || *left >= b {
                        return vec![(a, b)];
                    }
                    [(a, b.min(*left - 0.10)), (a.max(*right + 0.10), b)]
                        .into_iter()
                        .filter(|(a, b)| b - a >= 2.0)
                        .collect()
                })
                .collect();
        }
        for (a, b) in spans {
            let mut a = a;
            while b - a >= 2.0 {
                let end = b.min(a + 3.0);
                candidates.push((a, end));
                a = end + 0.35;
                if candidates.len() >= 512 {
                    break;
                }
            }
            if candidates.len() >= 512 {
                break;
            }
        }
        if candidates.len() >= 512 {
            break;
        }
    }
    candidates.sort_by(|a, b| (b.1 - b.0).total_cmp(&(a.1 - a.0)));
    let mut chosen: Vec<(f64, f64)> = Vec::new();
    // Prefer separated utterances before taking a second window from a short
    // fragment. Otherwise one unusual delivery can dominate the voice model.
    for gap in [1.0, 0.3] {
        for &span in &candidates {
            if chosen
                .iter()
                .all(|s| span.0 >= s.1 + gap || span.1 + gap <= s.0)
            {
                chosen.push(span);
                if chosen.len() == count.clamp(1, 4) {
                    return chosen;
                }
            }
        }
    }
    chosen
}

/// Kaldi-compatible 16-kHz, 80-bin log filterbanks (no dither, Povey window,
/// 25-ms frames, 10-ms shift, per-utterance mean normalization).
pub fn fbank(samples: &[f32]) -> Result<Vec<f32>, String> {
    if samples.len() < 32000
        || samples.len() > MAX_SAMPLES
        || samples.iter().any(|x| !x.is_finite())
    {
        return Err("Voice samples must contain 2–4 seconds of finite 16-kHz mono audio".into());
    }
    let rms = (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt();
    if rms < 0.001 {
        return Err("This sample is too quiet to remember reliably".into());
    }
    let mel = |hz: f32| 1127. * (1. + hz / 700.).ln();
    let low = mel(20.);
    let step = (mel(8000.) - low) / 81.;
    let mut weights = vec![[0f32; 257]; 80];
    for (i, filter) in weights.iter_mut().enumerate() {
        for (bin, w) in filter.iter_mut().enumerate() {
            let x = (mel(bin as f32 * 16000. / 512.) - low) / step;
            *w = (1. - (x - (i + 1) as f32).abs()).max(0.);
        }
    }
    let window: Vec<f32> = (0..400)
        .map(|i| (0.5 - 0.5 * (2. * std::f32::consts::PI * i as f32 / 399.).cos()).powf(0.85))
        .collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(512);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let frames = 1 + (samples.len() - 400) / 160;
    let mut features = Vec::with_capacity(frames * 80);
    for frame in 0..frames {
        let source = &samples[frame * 160..frame * 160 + 400];
        let mean = source.iter().sum::<f32>() / 400.;
        for i in 0..400 {
            let current = source[i] - mean;
            let previous = source[i.saturating_sub(1)] - mean;
            input[i] = (current - 0.97 * previous) * window[i];
        }
        input[400..].fill(0.);
        fft.process_with_scratch(&mut input, &mut output, &mut scratch)
            .map_err(|e| e.to_string())?;
        for filter in &weights {
            let energy = filter
                .iter()
                .zip(&output)
                .map(|(w, c)| w * c.norm_sqr())
                .sum::<f32>();
            features.push(energy.max(f32::EPSILON).ln());
        }
    }
    for bin in 0..80 {
        let mean = features.iter().skip(bin).step_by(80).sum::<f32>() / frames as f32;
        for index in (bin..features.len()).step_by(80) {
            features[index] -= mean;
        }
    }
    Ok(features)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(x: usize) -> Vec<f32> {
        let mut v = vec![0.; 192];
        v[x] = 1.;
        v
    }
    #[test]
    fn no_forced_identity_or_ambiguous_matches() {
        let a = vec![v(0), v(0)];
        let b = vec![v(1), v(1)];
        assert_eq!(identify(&a, &[a.clone(), b.clone()]), Some((0, 1.)));
        assert_eq!(identify(&b, std::slice::from_ref(&a)), None);
        assert_eq!(identify(&a, &[a.clone(), a.clone()]), None);
        assert_eq!(identify(&[v(0), v(1)], std::slice::from_ref(&a)), None);
        assert_eq!(identify(&[v(0)], &[a]), None);
    }
    #[test]
    fn sample_only_nonoverlapping_real_speech() {
        let turns = vec![("a".into(), 0., 15.), ("b".into(), 2., 6.)];
        let selected = sample_spans("a", &turns);
        assert!(selected.len() >= 2);
        assert!(selected.iter().all(|(a, b)| *a >= 6.1 && *b <= 14.85));
        assert!(selected.iter().enumerate().all(|(i, a)| selected[i + 1..]
            .iter()
            .all(|b| a.0 >= b.1 + 0.3 || b.0 >= a.1 + 0.3)));
        assert!(sample_spans("b", &turns).is_empty());
    }
    #[test]
    fn adjacent_fragments_yield_clean_samples_without_crossing_other_voices() {
        let fragments = vec![
            ("a".into(), 0., 1.6),
            ("a".into(), 1.7, 3.2),
            ("a".into(), 4., 5.6),
            ("a".into(), 5.7, 7.2),
        ];
        assert_eq!(sample_spans("a", &fragments).len(), 2);
        let mut with_crosstalk = fragments;
        with_crosstalk.push(("b".into(), 1., 6.));
        assert!(sample_spans("a", &with_crosstalk).is_empty());
    }
    #[test]
    fn silence_and_invalid_embeddings_rejected() {
        assert!(fbank(&vec![0.; 48000]).is_err());
        assert!(normalize(&vec![f32::NAN; 192]).is_err());
        assert!(normalize(&vec![0.; 192]).is_err());
    }
}
