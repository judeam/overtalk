import { describe, expect, test } from 'bun:test';
import { transcriptsFromSnapshot, TranscriptSnapshot } from '../../src/lib/transcriptSnapshot';

const snapshot = (text: string, partial = true): TranscriptSnapshot => ({
  session_id: 'meeting-a', revision: 1, is_partial: partial,
  segments: [{ id: 'live:a:SPEAKER_00:0', text, display_time: '[00:00]',
    audio_start_time: 0, audio_end_time: 2, duration: 2, sequence_id: 0,
    speaker_id: 'SPEAKER_00', confidence: null, overlap: false }],
});

describe('cumulative speaker snapshots', () => {
  test('revisions correct stable IDs without duplicate text or invented confidence', () => {
    const first = transcriptsFromSnapshot(snapshot('We should'));
    const final = transcriptsFromSnapshot(snapshot('We should ship tomorrow.', false));
    expect(first[0].id).toBe(final[0].id);
    expect(final).toHaveLength(1);
    expect(final[0].text).toBe('We should ship tomorrow.');
    expect(final[0].is_partial).toBe(false);
    expect(final[0].confidence).toBeUndefined();
    expect(final[0].speaker_id).toBe('SPEAKER_00');
  });
  test('keeps simultaneous speakers and real recording timestamps', () => {
    const value = snapshot('Yes.');
    value.segments[0].overlap = true;
    value.segments.push({ ...value.segments[0], id: 'live:a:SPEAKER_01:1', speaker_id: 'SPEAKER_01', audio_start_time: 1 });
    const result = transcriptsFromSnapshot(value);
    expect(result).toHaveLength(2);
    expect(result[1].audio_start_time).toBe(1);
    expect(result[1].overlap).toBe(true);
    expect(result[1].duration).toBe(1);
  });
  test('rejects invalid revisions atomically', () => {
    const value = snapshot('Bad.');
    value.segments.push({ ...value.segments[0] });
    expect(() => transcriptsFromSnapshot(value)).toThrow();
    value.segments.pop(); value.segments[0].audio_start_time = NaN;
    expect(() => transcriptsFromSnapshot(value)).toThrow();
  });
});
