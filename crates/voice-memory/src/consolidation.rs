// SPDX-License-Identifier: Apache-2.0
//! Consolidate diarization slots using audio, never matching display names.
use crate::cosine;

#[derive(Clone)]
pub struct Cluster {
    pub id: String,
    pub confirmed_name: Option<String>,
    pub samples: Vec<Vec<f32>>,
    pub intervals: Vec<(f64, f64)>,
}

pub struct Merge {
    pub source: String,
    pub target: String,
    pub similarity: f32,
}

fn within(samples: &[Vec<f32>]) -> f32 {
    let mut total = 0.;
    let mut count = 0;
    for (i, a) in samples.iter().enumerate() {
        for b in &samples[i + 1..] {
            total += cosine(a, b);
            count += 1;
        }
    }
    if count == 0 {
        -1.
    } else {
        total / count as f32
    }
}

fn between(a: &Cluster, b: &Cluster) -> (f32, f32) {
    if a.samples.len() < 2 || b.samples.len() < 2 {
        return (-1., -1.);
    }
    let mut total = 0.;
    let mut minimum = 1f32;
    for left in &a.samples {
        for right in &b.samples {
            let score = cosine(left, right);
            total += score;
            minimum = minimum.min(score);
        }
    }
    (total / (a.samples.len() * b.samples.len()) as f32, minimum)
}

fn seconds(cluster: &Cluster) -> f64 {
    cluster.intervals.iter().map(|(a, b)| (b - a).max(0.)).sum()
}

fn simultaneous(a: &Cluster, b: &Cluster) -> bool {
    let (mut i, mut j, mut overlap) = (0, 0, 0.);
    while i < a.intervals.len() && j < b.intervals.len() {
        let (a0, a1) = a.intervals[i];
        let (b0, b1) = b.intervals[j];
        let shared = (a1.min(b1) - a0.max(b0)).max(0.);
        overlap += shared;
        // Tolerate tiny boundary jitter, never concurrent conversation.
        if shared >= 0.2 || overlap >= 0.4 {
            return true;
        }
        if a1 <= b1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    false
}

/// Re-evaluate all acoustic evidence after each merge. In particular, a tiny
/// orphan must agree with the entire combined voice, not just one lucky clip.
/// Similar scores, conflicting confirmed names and simultaneous speech abstain.
pub fn consolidate(mut clusters: Vec<Cluster>) -> Vec<Merge> {
    if clusters.len() > 32 {
        return Vec::new();
    }
    for cluster in &mut clusters {
        cluster.intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        if cluster.samples.len() > 4 {
            cluster.samples.truncate(4);
        }
    }
    let mut merges = Vec::new();
    loop {
        let mut best = None;
        for source in 0..clusters.len() {
            let a = &clusters[source];
            if a.samples.len() < 2 || within(&a.samples) < 0.45 {
                continue;
            }
            let mut candidates: Vec<_> = clusters
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != source)
                .map(|(i, b)| {
                    let (mean, minimum) = between(a, b);
                    (i, mean, minimum)
                })
                .collect();
            candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
            let Some(&(target, score, minimum)) = candidates.first() else {
                continue;
            };
            let b = &clusters[target];
            if within(&b.samples) < 0.45 {
                continue;
            }
            let runner = candidates.get(1).map_or(-1., |c| c.1);
            if seconds(a) > seconds(b) || score - runner < 0.10 || simultaneous(a, b) {
                continue;
            }
            if matches!((&a.confirmed_name, &b.confirmed_name), (Some(a), Some(b)) if a != b) {
                continue;
            }
            let coherent = score >= 0.60
                && minimum >= 0.48
                && score >= within(&a.samples).min(within(&b.samples)) - 0.045;
            let short_fragment = seconds(a) <= 10.
                && seconds(b) >= seconds(a) * 3.
                && score >= 0.55
                && minimum >= 0.50;
            if (coherent || short_fragment)
                && best.as_ref().is_none_or(|&(_, _, value)| score > value)
            {
                best = Some((source, target, score));
            }
        }
        let Some((source, target, similarity)) = best else {
            break;
        };
        let removed = clusters.remove(source);
        let destination = &mut clusters[if target > source { target - 1 } else { target }];
        merges.push(Merge {
            source: removed.id,
            target: destination.id.clone(),
            similarity,
        });
        destination.samples.extend(removed.samples);
        destination.intervals.extend(removed.intervals);
        destination.intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        if destination.confirmed_name.is_none() {
            destination.confirmed_name = removed.confirmed_name;
        }
    }
    merges
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cluster(id: &str, dimension: usize, start: f64) -> Cluster {
        let mut sample = vec![0.; crate::DIMENSIONS];
        sample[dimension] = 1.;
        Cluster {
            id: id.into(),
            confirmed_name: None,
            samples: vec![sample.clone(), sample],
            intervals: vec![(start, start + 8.)],
        }
    }
    #[test]
    fn same_name_cannot_merge_distinct_voices() {
        let mut a = cluster("a", 0, 0.);
        let mut b = cluster("b", 1, 20.);
        a.confirmed_name = Some("Ben".into());
        b.confirmed_name = a.confirmed_name.clone();
        assert!(consolidate(vec![a, b]).is_empty());
    }
    #[test]
    fn audio_can_reunite_an_anonymous_split() {
        let merges = consolidate(vec![
            cluster("a", 0, 0.),
            cluster("b", 0, 20.),
            cluster("c", 1, 40.),
        ]);
        assert_eq!(merges.len(), 1);
        assert_ne!(merges[0].source, "c");
        assert_ne!(merges[0].target, "c");
    }
    #[test]
    fn concurrent_or_ambiguous_voices_never_merge() {
        assert!(consolidate(vec![cluster("a", 0, 0.), cluster("b", 0, 1.)]).is_empty());
        assert!(consolidate(vec![
            cluster("a", 0, 0.),
            cluster("b", 0, 20.),
            cluster("c", 0, 40.)
        ])
        .is_empty());
    }
    #[test]
    fn confirmed_identity_conflicts_and_one_sample_abstain() {
        let mut a = cluster("a", 0, 0.);
        let mut b = cluster("b", 0, 20.);
        a.confirmed_name = Some("Anna".into());
        b.confirmed_name = Some("Priya".into());
        assert!(consolidate(vec![a.clone(), b.clone()]).is_empty());
        a.confirmed_name = None;
        b.confirmed_name = None;
        b.samples.truncate(1);
        assert!(consolidate(vec![a, b]).is_empty());
    }
    #[test]
    fn an_internally_mixed_target_is_not_used_as_a_voice_reference() {
        let a = cluster("a", 0, 0.);
        let mut b = cluster("mixed", 0, 20.);
        let offset = (1f32 - 0.65f32.powi(2)).sqrt();
        b.samples[0][0] = 0.65;
        b.samples[0][1] = offset;
        b.samples[1][0] = 0.65;
        b.samples[1][1] = -offset;
        b.intervals = vec![(20., 120.)];
        assert!(consolidate(vec![a, b]).is_empty());
    }
}
