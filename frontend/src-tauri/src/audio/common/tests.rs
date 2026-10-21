use super::*;

#[test]
fn batch_preparation_reuses_short_buffers_and_preserves_timing() {
    let short = crate::audio::vad::SpeechSegment {
        samples: vec![0.25; 16000],
        start_timestamp_ms: 1250.,
        end_timestamp_ms: 2250.,
        confidence: 0.9,
    };
    let pointer = short.samples.as_ptr();
    let prepared = prepare_speech_segments(vec![short]);
    assert_eq!(prepared[0].samples.as_ptr(), pointer);
    assert_eq!(prepared[0].start_timestamp_ms, 1250.);
    assert_eq!(prepared[0].end_timestamp_ms, 2250.);
    assert_eq!(prepared[0].confidence, 0.9);
    assert!(prepare_speech_segments(vec![]).is_empty());
}

#[test]
fn batch_preparation_splits_long_audio_without_loss_or_reordering() {
    let samples: Vec<f32> = (0..60 * 16000).map(|i| ((i % 101) as f32) / 100.).collect();
    let prepared = prepare_speech_segments(vec![crate::audio::vad::SpeechSegment {
        samples: samples.clone(),
        start_timestamp_ms: 2000.,
        end_timestamp_ms: 62000.,
        confidence: 0.8,
    }]);
    assert!(prepared.len() > 1);
    assert_eq!(prepared[0].start_timestamp_ms, 2000.);
    assert_eq!(prepared.last().unwrap().end_timestamp_ms, 62000.);
    assert!(prepared
        .windows(2)
        .all(|pair| pair[0].end_timestamp_ms == pair[1].start_timestamp_ms));
    let joined: Vec<f32> = prepared
        .into_iter()
        .flat_map(|segment| segment.samples)
        .collect();
    assert_eq!(joined, samples);
}

#[cfg(unix)]
#[test]
fn transcript_checkpoint_does_not_follow_preexisting_temporary_link() {
    let directory = tempfile::tempdir().unwrap();
    let victim = directory.path().join("unrelated.txt");
    std::fs::write(&victim, b"keep this content").unwrap();
    std::os::unix::fs::symlink(&victim, directory.path().join(".transcripts.json.tmp")).unwrap();
    write_transcripts_json(directory.path(), &[]).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"keep this content");
    assert!(directory.path().join("transcripts.json").is_file());
}

#[test]
#[ignore = "manual transcript checkpoint performance measurement"]
fn benchmark_transcript_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = (0..20_000)
        .map(|i| {
            (
                "Synthetic transcript text. ".repeat(12),
                i as f64 * 1000.,
                (i + 1) as f64 * 1000.,
            )
        })
        .collect::<Vec<_>>();
    let segments = create_transcript_segments(&fixture);
    drop(fixture);
    let start = std::time::Instant::now();
    if std::env::var_os("OVERTALK_BENCH_LEGACY_CHECKPOINT").is_some() {
        // Baseline from 743a3df, confined to this disposable benchmark.
        let value = serde_json::json!({
            "version":"1.0", "last_updated":chrono::Utc::now().to_rfc3339(),
            "total_segments":segments.len(),
            "segments":segments.iter().enumerate().map(|(i,s)| serde_json::json!({
                "id":s.id,"text":s.text,"timestamp":s.timestamp,
                "audio_start_time":s.audio_start_time,"audio_end_time":s.audio_end_time,
                "duration":s.duration,"sequence_id":i
            })).collect::<Vec<_>>()
        });
        let output = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(directory.path().join("transcripts.json"), output).unwrap();
    } else {
        write_transcripts_json(directory.path(), &segments).unwrap();
    }
    eprintln!(
        "checkpoint_ms={:.3} bytes={}",
        start.elapsed().as_secs_f64() * 1000.,
        std::fs::metadata(directory.path().join("transcripts.json"))
            .unwrap()
            .len()
    );
}

#[test]
fn streamed_checkpoint_preserves_fields_and_unicode() {
    let directory = tempfile::tempdir().unwrap();
    let segments = create_transcript_segments(&[("हैलो \"world\"\nline".into(), 500., 1750.)]);
    write_transcripts_json(directory.path(), &segments).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("transcripts.json")).unwrap())
            .unwrap();
    assert_eq!(value["total_segments"], 1);
    assert_eq!(
        value["segments"][0],
        serde_json::json!({
            "id":segments[0].id,"text":segments[0].text,"timestamp":segments[0].timestamp,
            "audio_start_time":0.5,"audio_end_time":1.75,"duration":1.25,"sequence_id":0
        })
    );
}

#[tokio::test]
async fn test_engine_lifecycle_lock_serializes_acquirers() {
    let guard = acquire_engine_lifecycle_lock().await;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (acquired_tx, mut acquired_rx) = tokio::sync::oneshot::channel();
    let waiter = tokio::spawn(async {
        started_tx.send(()).unwrap();
        let _guard = acquire_engine_lifecycle_lock().await;
        acquired_tx.send(()).unwrap();
    });

    started_rx.await.unwrap();
    assert!(acquired_rx.try_recv().is_err());
    drop(guard);

    acquired_rx.await.unwrap();
    waiter.await.unwrap();
}
#[test]
fn test_create_transcript_segments_empty() {
    let transcripts: Vec<(String, f64, f64)> = vec![];
    let segments = create_transcript_segments(&transcripts);
    assert!(segments.is_empty());
}

#[test]
fn test_create_transcript_segments_single() {
    let transcripts = vec![
        ("Hello world".to_string(), 0.0, 1500.0), // 0-1.5 seconds
    ];
    let segments = create_transcript_segments(&transcripts);

    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, "Hello world");
    assert_eq!(segments[0].audio_start_time, Some(0.0));
    assert_eq!(segments[0].audio_end_time, Some(1.5));
    assert_eq!(segments[0].duration, Some(1.5));
}

#[test]
fn test_create_transcript_segments_multiple() {
    let transcripts = vec![
        ("First segment".to_string(), 0.0, 2000.0), // 0-2 seconds
        ("Second segment".to_string(), 3000.0, 5000.0), // 3-5 seconds
        ("Third segment".to_string(), 6500.0, 8000.0), // 6.5-8 seconds
    ];
    let segments = create_transcript_segments(&transcripts);

    assert_eq!(segments.len(), 3);

    // First segment
    assert_eq!(segments[0].text, "First segment");
    assert_eq!(segments[0].audio_start_time, Some(0.0));
    assert_eq!(segments[0].audio_end_time, Some(2.0));
    assert_eq!(segments[0].duration, Some(2.0));

    // Second segment
    assert_eq!(segments[1].text, "Second segment");
    assert_eq!(segments[1].audio_start_time, Some(3.0));
    assert_eq!(segments[1].audio_end_time, Some(5.0));
    assert_eq!(segments[1].duration, Some(2.0));

    // Third segment
    assert_eq!(segments[2].text, "Third segment");
    assert_eq!(segments[2].audio_start_time, Some(6.5));
    assert_eq!(segments[2].audio_end_time, Some(8.0));
    assert_eq!(segments[2].duration, Some(1.5));
}

#[test]
fn test_create_transcript_segments_trims_whitespace() {
    let transcripts = vec![("  Hello with spaces  ".to_string(), 0.0, 1000.0)];
    let segments = create_transcript_segments(&transcripts);

    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, "Hello with spaces");
}

#[test]
fn test_create_transcript_segments_generates_unique_ids() {
    let transcripts = vec![
        ("Segment one".to_string(), 0.0, 1000.0),
        ("Segment two".to_string(), 1000.0, 2000.0),
    ];
    let segments = create_transcript_segments(&transcripts);

    assert_eq!(segments.len(), 2);
    assert_ne!(segments[0].id, segments[1].id);
    assert!(segments[0].id.starts_with("transcript-"));
    assert!(segments[1].id.starts_with("transcript-"));
}

#[test]
fn test_split_segment_at_silence_short_segment() {
    // Segment shorter than max — returned as-is
    let segment = crate::audio::vad::SpeechSegment {
        samples: vec![0.1; 16000], // 1 second
        start_timestamp_ms: 0.0,
        end_timestamp_ms: 1000.0,
        confidence: 0.9,
    };
    let result = split_segment_at_silence(&segment, 25 * 16000);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].samples.len(), 16000);
}

#[test]
fn test_split_segment_at_silence_splits_long_segment() {
    // 60-second segment of low-level noise with a silent gap at ~25s
    let mut samples = vec![0.01f32; 60 * 16000];
    // Insert silence at 25 seconds (sample 400000)
    samples[(25 * 16000)..(25 * 16000 + 3200)].fill(0.0);
    let segment = crate::audio::vad::SpeechSegment {
        samples,
        start_timestamp_ms: 0.0,
        end_timestamp_ms: 60_000.0,
        confidence: 0.9,
    };

    let result = split_segment_at_silence(&segment, 25 * 16000);
    assert!(
        result.len() >= 2,
        "Should split into at least 2 segments, got {}",
        result.len()
    );

    // All sub-segments should have samples
    for (i, seg) in result.iter().enumerate() {
        assert!(!seg.samples.is_empty(), "Segment {} is empty", i);
        assert!(
            seg.start_timestamp_ms < seg.end_timestamp_ms,
            "Segment {} has invalid timestamps: {} >= {}",
            i,
            seg.start_timestamp_ms,
            seg.end_timestamp_ms
        );
    }
}

#[test]
fn test_split_segment_at_silence_no_silence_uses_overlap() {
    // Continuous speech (constant energy) — should still split with overlap
    let segment = crate::audio::vad::SpeechSegment {
        samples: vec![0.5f32; 60 * 16000], // 60 seconds of "speech"
        start_timestamp_ms: 0.0,
        end_timestamp_ms: 60_000.0,
        confidence: 0.9,
    };

    let result = split_segment_at_silence(&segment, 25 * 16000);
    assert!(result.len() >= 2);

    // Total samples should exceed input due to overlap
    let total_samples: usize = result.iter().map(|s| s.samples.len()).sum();
    assert!(
        total_samples >= 60 * 16000,
        "Overlap should not lose samples"
    );
}

#[test]
fn test_write_transcripts_json() {
    let dir = tempfile::tempdir().unwrap();
    let segments = vec![
        TranscriptSegment {
            id: "t-1".to_string(),
            text: "Hello world".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            audio_start_time: Some(0.0),
            audio_end_time: Some(1.5),
            speaker: None,
            speaker_id: None,
            speaker_name: None,
            speaker_confidence: None,
            overlap: false,
            is_partial: false,
            duration: Some(1.5),
        },
        TranscriptSegment {
            id: "t-2".to_string(),
            text: "Second segment".to_string(),
            timestamp: "2024-01-01T00:00:01Z".to_string(),
            audio_start_time: Some(2.0),
            audio_end_time: Some(3.5),
            speaker: None,
            speaker_id: None,
            speaker_name: None,
            speaker_confidence: None,
            overlap: false,
            is_partial: false,
            duration: Some(1.5),
        },
    ];

    let result = write_transcripts_json(dir.path(), &segments);
    assert!(
        result.is_ok(),
        "write_transcripts_json failed: {:?}",
        result
    );

    // Verify file exists and is valid JSON
    let path = dir.path().join("transcripts.json");
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed["total_segments"], 2);
    assert_eq!(parsed["version"], "1.0");
    assert_eq!(parsed["segments"][0]["text"], "Hello world");
    assert_eq!(parsed["segments"][1]["text"], "Second segment");
    assert_eq!(parsed["segments"][0]["sequence_id"], 0);
    assert_eq!(parsed["segments"][1]["sequence_id"], 1);

    // Verify temp file was cleaned up
    assert!(!dir.path().join(".transcripts.json.tmp").exists());
}
