//! Speaker filtering uses merged recording intervals, never transcript display rows.
#[derive(Default)]
pub(crate) struct Filter {
    intervals: Option<Vec<(f64, f64)>>,
}
impl Filter {
    pub(crate) fn clear(&mut self) {
        self.intervals = None;
    }
    pub(crate) fn set(&mut self, mut intervals: Vec<(f64, f64)>) {
        intervals.retain(|(start, end)| {
            start.is_finite() && end.is_finite() && *start >= 0.0 && end > start
        });
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (start, end) in intervals {
            if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        self.intervals = Some(merged);
    }
    /// None means playback has reached the end of the enabled speech.
    pub(crate) fn next_position(&self, position: f64) -> Option<f64> {
        let Some(intervals) = &self.intervals else {
            return Some(position);
        };
        let index = intervals.partition_point(|(_, end)| *end <= position);
        intervals.get(index).map(|(start, _)| position.max(*start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_skips_gaps_merges_overlaps_and_stops_at_end() {
        let mut filter = Filter::default();
        assert_eq!(filter.next_position(13.0), Some(13.0));
        filter.set(vec![(10.0, 12.0), (2.0, 4.0), (3.0, 7.0), (f64::NAN, 20.0)]);
        assert_eq!(filter.next_position(0.0), Some(2.0));
        assert_eq!(filter.next_position(4.0), Some(4.0));
        assert_eq!(filter.next_position(7.0), Some(10.0));
        assert_eq!(filter.next_position(12.0), None);
        filter.set(vec![]);
        assert_eq!(filter.next_position(0.0), None);
        filter.clear();
        assert_eq!(filter.next_position(0.0), Some(0.0));
    }
}

/// Transcript pages are returned in recording order. In gaps retain the last passage.
pub(crate) fn passage_start(line: &serde_json::Value) -> f64 {
    line["audio_start_time"]
        .as_f64()
        .or_else(|| line["start_time"].as_f64())
        .unwrap_or(0.0)
}
pub(crate) fn passage_at(lines: &[serde_json::Value], position: f64) -> Option<usize> {
    if !position.is_finite() {
        return None;
    }
    lines
        .partition_point(|line| passage_start(line) <= position)
        .checked_sub(1)
}

/// A display-only reading guide for recordings that contain segment timestamps,
/// not forced-aligned word timestamps. Never changes transcript text or evidence.
/// UTF-8 ranges are cached once per active passage; ticks use binary search.
pub(crate) struct WordTrack {
    spans: Vec<(usize, usize, f64)>,
    start: f64,
    end: f64,
}
impl WordTrack {
    pub(crate) fn covers(&self, start: f64, end: f64) -> bool {
        self.start == start && self.end == end
    }
    pub(crate) fn new(text: &str, start: f64, end: f64) -> Self {
        let mut ranges = Vec::new();
        let mut begin = None;
        for (offset, ch) in text.char_indices() {
            let cjk = matches!(ch as u32, 0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af | 0x20000..=0x2fa1f);
            if ch.is_whitespace() || cjk {
                if let Some(first) = begin.take() {
                    ranges.push((first, offset));
                }
                if cjk {
                    ranges.push((offset, offset + ch.len_utf8()));
                }
            } else if begin.is_none() {
                begin = Some(offset);
            }
        }
        if let Some(first) = begin {
            ranges.push((first, text.len()));
        }
        let mut weight = 0.0;
        let spans = ranges
            .into_iter()
            .map(|(first, last)| {
                // Length and punctuation provide a restrained approximation of rhythm.
                let token = &text[first..last];
                weight += token.chars().count().max(2) as f64
                    + if token.ends_with(['.', ',', '?', '!', '。', '、']) {
                        2.0
                    } else {
                        0.0
                    };
                (first, last, weight)
            })
            .collect::<Vec<_>>();
        let spans = spans
            .into_iter()
            .map(|(a, b, w)| {
                (
                    a,
                    b,
                    if weight > 0.0 {
                        start + (end - start) * w / weight
                    } else {
                        end
                    },
                )
            })
            .collect();
        Self { spans, start, end }
    }
    pub(crate) fn at(&self, position: f64) -> Option<(usize, usize)> {
        if !position.is_finite()
            || position < self.start
            || position >= self.end
            || self.end <= self.start
        {
            return None;
        }
        let index = self
            .spans
            .partition_point(|(_, _, until)| *until <= position);
        self.spans.get(index).map(|&(a, b, _)| (a, b))
    }
}

#[cfg(test)]
mod word_tests {
    use super::*;
    #[test]
    fn estimated_tracking_respects_gaps_seeks_and_unicode_boundaries() {
        let text = "Hello café नमस्ते 日本語";
        let track = WordTrack::new(text, 3.0, 12.0);
        assert_eq!(track.at(2.0), None);
        assert_eq!(track.at(12.0), None);
        assert_eq!(track.at(f64::NAN), None);
        assert_eq!(track.at(3.0).map(|(a, b)| &text[a..b]), Some("Hello"));
        for step in 0..90 {
            let (a, b) = track.at(3.0 + step as f64 / 10.0).unwrap();
            assert!(!text[a..b].is_empty());
        }
        assert_eq!(track.at(3.0).map(|(a, b)| &text[a..b]), Some("Hello"));
        assert_eq!(WordTrack::new("", 0.0, 4.0).at(2.0), None);
        assert_eq!(WordTrack::new("hello", 4.0, 4.0).at(4.0), None);
    }
}

#[cfg(test)]
mod passage_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn follows_timestamps_gaps_and_backwards_seeks() {
        let lines = vec![
            json!({"audio_start_time":3}),
            json!({"audio_start_time":10}),
            json!({"start_time":60}),
        ];
        assert_eq!(passage_at(&lines, 0.0), None);
        assert_eq!(passage_at(&lines, 3.0), Some(0));
        assert_eq!(passage_at(&lines, 59.0), Some(1));
        assert_eq!(passage_at(&lines, 60.0), Some(2));
        assert_eq!(passage_at(&lines, 12.0), Some(1));
        assert_eq!(passage_at(&[], 12.0), None);
    }
}
