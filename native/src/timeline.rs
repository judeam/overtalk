use serde_json::Value;
pub(crate) struct Note {
    pub id: String,
    pub label: String,
    pub body: String,
    pub start: f64,
    pub end: f64,
    pub kind: i32,
}
pub(crate) fn note_clips(markdown: &str, sources: &Value) -> Vec<Note> {
    let mut kind = 0;
    let mut notes = Vec::new();
    for (index, line) in markdown.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with("**") && !line.starts_with("- ") {
            let heading = line
                .trim_matches(|c| c == '#' || c == '*' || c == ' ' || c == ':')
                .to_lowercase();
            kind = if heading.contains("decision") || heading.contains("decisões") {
                1
            } else if heading.contains("action") || heading.contains("tarefa") {
                2
            } else {
                0
            };
            continue;
        }
        if kind == 0 || !(line.starts_with("- ") || line.starts_with("* ")) {
            continue;
        }
        let raw = &line[2..];
        let mut spans = Vec::new();
        let mut timestamp = None;
        for token in raw
            .split('[')
            .skip(1)
            .filter_map(|s| s.split_once(']').map(|(s, _)| s))
        {
            if let Some(id) = token
                .strip_prefix('S')
                .and_then(|s| s.parse::<usize>().ok())
            {
                if id > 0 {
                    if let Some(span) = sources.get(id - 1).and_then(Value::as_array) {
                        if let (Some(a), Some(b)) = (
                            span.first().and_then(Value::as_f64),
                            span.get(1).and_then(Value::as_f64),
                        ) {
                            spans.push((a, b));
                        }
                    }
                }
            } else {
                let numbers: Option<Vec<f64>> =
                    token.split(':').map(|s| s.parse::<f64>().ok()).collect();
                if let Some(numbers) = numbers {
                    if (2..=3).contains(&numbers.len()) {
                        timestamp = Some(numbers.iter().fold(0.0, |acc, n| acc * 60.0 + n));
                    }
                }
            }
        }
        let (start, end) = if !spans.is_empty() {
            (
                spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min),
                spans.iter().map(|s| s.1).fold(0.0, f64::max),
            )
        } else if let Some(at) = timestamp {
            (at, at)
        } else {
            continue;
        };
        let mut body = String::new();
        let mut inside = false;
        for c in raw.chars() {
            if c == '[' {
                inside = true;
            } else if c == ']' {
                inside = false;
            } else if !inside && c != '*' {
                body.push(c);
            }
        }
        let body = body.trim().to_owned();
        let label = body
            .split([',', ';'])
            .next()
            .unwrap_or(&body)
            .trim_end_matches('.')
            .to_owned();
        notes.push(Note {
            id: format!("note-{kind}-{index}"),
            label,
            body,
            start,
            end,
            kind,
        });
    }
    notes
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notes_require_real_source_or_written_time() {
        let notes=note_clips("## Decisions\n- Ship first [S1]\n- No source\n## Actions\n- **Priya:** Review tables [01:40]",&serde_json::json!([[72,80]]));
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].start, 72.0);
        assert_eq!(notes[1].start, 100.0);
        assert_eq!(notes[1].label, "Priya: Review tables");
    }
}

/// Cluster labels in screen space, including clamping at the right edge. Every note
/// remains selectable in the cluster menu; no event is hidden behind another card.
pub(crate) struct NoteGroup<'a> {
    pub notes: Vec<&'a Note>,
    pub position: f64,
    pub span: f64,
}
pub(crate) fn group_notes(
    notes: &[Note],
    kind: i32,
    duration: f64,
    width: f64,
    scale: f64,
) -> Vec<NoteGroup<'_>> {
    let width = width.max(1.0);
    let card = (192.0 * scale).min(width);
    let gap = 8.0 * scale;
    let mut ordered: Vec<_> = notes
        .iter()
        .filter(|n| n.kind == kind && n.start.is_finite())
        .collect();
    ordered.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut groups: Vec<NoteGroup<'_>> = Vec::new();
    for note in ordered {
        let x = (note.start / duration.max(1.0) * width).clamp(0.0, width - card);
        if let Some(last) = groups
            .last_mut()
            .filter(|g| x < g.position * width + card + gap)
        {
            last.notes.push(note);
        } else {
            groups.push(NoteGroup {
                notes: vec![note],
                position: x / width,
                span: card / width,
            });
        }
    }
    groups
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    fn note(start: f64) -> Note {
        Note {
            id: format!("{start}"),
            label: "Decision".into(),
            body: "Full decision".into(),
            start,
            end: start + 3.0,
            kind: 1,
        }
    }
    #[test]
    fn crowded_notes_and_right_edge_never_overlap_or_lose_members() {
        let notes = vec![
            note(0.0),
            note(38.0),
            note(40.0),
            note(42.0),
            note(86.0),
            note(97.0),
        ];
        for width in [320.0, 720.0, 1600.0, 7000.0] {
            let groups = group_notes(&notes, 1, 98.0, width, 1.0);
            assert_eq!(
                groups.iter().map(|g| g.notes.len()).sum::<usize>(),
                notes.len()
            );
            for group in &groups {
                assert!(group.position >= 0.0 && group.position + group.span <= 1.00001);
            }
            for pair in groups.windows(2) {
                assert!(pair[0].position + pair[0].span <= pair[1].position);
            }
        }
    }
}
