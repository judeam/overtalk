// Capture the Overtalk session screen with a synthetic Tauri IPC fixture carrying comp C's content.
// Usage: node capture-hero.cjs <puppeteer-module-path> <out-file.png> [base-url] [path] [WxH]
const out = process.argv[3];
const base = process.argv[4] || 'http://localhost:3118';
const route = process.argv[5] || '/meeting-details?id=m1&at=189';
const [W, H] = (process.argv[6] || '1536x1024').split('x').map(Number);

// Runs in the page before any app code. Everything here is synthetic demo data.
function installFixture() {
  let seed = 7;
  const rand = () => { seed = (seed * 16807) % 2147483647; return (seed - 1) / 2147483646; };
  const day = (d) => `2026-10-${String(d).padStart(2, '0')}T09:30:00Z`;
  const meetings = [
    { id: 'm1', title: 'Product sync: Q4 roadmap', created_at: day(12), duration: 462 },
    { id: 'm2', title: 'Design review', created_at: day(10), duration: 2597 },
    { id: 'm3', title: 'Growth planning', created_at: day(8), duration: 1686 },
    { id: 'm4', title: 'Weekly standup', created_at: day(6), duration: 924 },
    { id: 'm5', title: 'Customer feedback', created_at: day(3), duration: 2272 },
    { id: 'm6', title: 'Engineering sync', created_at: day(1), duration: 3138 },
  ];
  const people = [{ id: 'spk_lena', name: 'Lena' }, { id: 'spk_marco', name: 'Marco' }, { id: 'spk_priya', name: 'Priya' }];
  const pattern = () => {
    const bins = [];
    let speaker = 0;
    while (bins.length < 96) {
      const run = 18 + Math.floor(rand() * 16);
      for (let i = 0; i < run && bins.length < 96; i++) bins.push({ speaker, level: 0.15 + rand() * 0.75 * (rand() < 0.15 ? 1.3 : 0.8) });
      speaker = (speaker + 1) % 3;
    }
    return bins;
  };
  const overviews = meetings.map((m) => ({ id: m.id, title: m.title, created_at: m.created_at, duration: m.duration, speakers: people, pattern: pattern() }));
  const turns = [
    ['spk_lena', 64.3, 97.5], ['spk_marco', 98.3, 149.3], ['spk_priya', 133.6, 167.2], ['spk_lena', 162.6, 219.0],
    ['spk_marco', 227.3, 292.4], ['spk_priya', 301.1, 346.3], ['spk_lena', 331.4, 391.1],
  ].map(([speaker_id, start, end]) => ({ speaker_id, start, end }));
  const lines = [
    ['spk_lena', 'Lena', 72, 80, 'Search is the most requested thing by far.'],
    ['spk_marco', 'Marco', 110, 120, 'Export is blocked on the PDF layout for long tables.'],
    ['spk_priya', 'Priya', 151, 160, 'I can take the PDF tables, two or three days.'],
    ['spk_lena', 'Lena', 189, 213, 'Then search goes first, end of October.'],
  ];
  const transcripts = lines.map(([speaker_id, speaker_name, a, b, text], i) => ({
    id: 't' + i, text, timestamp: '09:3' + i + ':00', sequence_id: i, audio_start_time: a, audio_end_time: b, duration: b - a,
    speaker: 'system', speaker_id, speaker_name, speaker_confidence: 0.9,
  }));
  const peaks = [];
  for (let k = 0; k < 4620; k++) {
    const t = k / 10;
    const talking = turns.some((x) => t >= x.start && t < x.end);
    // Syllable-like envelope: a slow phrase swell times a faster syllable beat, plus grain.
    const swell = 0.55 + 0.45 * Math.sin(t * 0.9 + 1.3) * Math.sin(t * 0.37);
    const beat = 0.35 + 0.65 * Math.abs(Math.sin(t * 7.1 + Math.sin(t * 1.7)));
    const spike = rand() < 0.04 ? 1.6 : 1;
    peaks.push(talking ? Math.round(255 * Math.min(1, Math.max(0.1, 0.62 * swell * beat * (0.7 + rand() * 0.6) * spike))) : Math.round(rand() * 5));
  }
  const summary = {
    markdown: [
      '# Product sync: Q4 roadmap', '',
      '## Decisions', '- Search ships before export, first version end of October. [S3][S4]', '',
      '## Action items', '- **Priya:** PDF tables, two or three days [01:40]',
      '- **Marco:** release note, once search is on the test machines [05:10]',
    ].join('\n'),
  };
  // 462 s of silence at a low sample rate, so seeking behaves like a real recording.
  const wav = (() => {
    const rate = 3000, bytes = 462 * rate, body = bytes;
    const buf = new Uint8Array(44 + body); const v = new DataView(buf.buffer);
    const s = (o, str) => [...str].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
    s(0, 'RIFF'); v.setUint32(4, 36 + bytes, true); s(8, 'WAVE'); s(12, 'fmt '); v.setUint32(16, 16, true);
    v.setUint16(20, 1, true); v.setUint16(22, 1, true); v.setUint32(24, rate, true); v.setUint32(28, rate, true);
    v.setUint16(32, 1, true); v.setUint16(34, 8, true); s(36, 'data'); v.setUint32(40, bytes, true); buf.fill(128, 44);
    let bin = '';
    for (let i = 0; i < buf.length; i += 8192) bin += String.fromCharCode.apply(null, buf.subarray(i, i + 8192));
    return 'data:audio/wav;base64,' + btoa(bin);
  })();
  const meta = (id) => {
    const m = meetings.find((x) => x.id === id) || meetings[0];
    return { id: m.id, title: m.title, created_at: m.created_at, updated_at: m.created_at, folder_path: '/home/demo/Recordings/' + m.id, transcripts };
  };
  try { if (!/tour/.test(window.name)) localStorage.setItem('overtalk.tour.seen', 'home,review'); } catch (e) {}
  const LIVE = window.name === 'live';
  const liveLines = [
    ['spk_lena', 'Lena', 60, 95, 'Search is the most requested thing by far.'],
    ['spk_marco', 'Marco', 100, 128, 'Export is blocked on the PDF layout for long tables.'],
    ['spk_priya', 'Priya', 142, 156, 'I can take the PDF tables, two or three days.'],
    ['spk_lena', 'Lena', 167, 210, 'Then search goes first, end of October.'],
    ['spk_marco', 'Marco', 185, 215, 'Fine by me, as long as the release note goes out with it.'],
    ['spk_priya', 'Priya', 216, 238, 'I will pair with Marco on the export tests next week.'],
    ['spk_lena', 'Lena', 225, 252, 'Good. Let us check the German transcripts before we close.'],
    [null, null, 238, 246, 'Sorry, can you hear me now?'],
  ];
  if (LIVE) {
    try { sessionStorage.setItem('overtalk.pendingMarks', JSON.stringify([{ at: 151, label: 'Priya takes PDF tables' }, { at: 189, label: 'Decision: search first' }])); } catch (e) {}
  }
  const R = {
    is_recording: LIVE,
    get_recording_state: { is_recording: LIVE, is_paused: false, is_active: LIVE, recording_duration: LIVE ? 252 : null, active_duration: LIVE ? 252 : null },
    get_transcript_history: liveLines.map(([speaker_id, speaker_name, a, b, text], i) => ({ id: 'live' + i, text, display_time: '', sequence_id: i, is_partial: false, confidence: 0.9, audio_start_time: a, audio_end_time: b, duration: b - a, speaker_id, speaker_name, speaker_confidence: 0.8, overlap: false })),
    api_get_meetings: meetings.map(({ id, title }) => ({ id, title })),
    meeting_overviews: overviews,
    api_search_transcripts: (a) => (String(a && a.query || '').toLowerCase().includes('export') ? [{ id: 'm1', title: meetings[0].title, matchContext: 'Export is blocked on the PDF layout for long tables.', timestamp: '01:50' }] : []),
    meeting_timeline: { duration: 462, speakers: people, turns, sources: lines.map(([, , a, b]) => [a, b]) },
    meeting_waveform: peaks,
    workspace_audio_source: wav,
    api_get_meeting: (a) => meta(a?.meetingId),
    api_get_meeting_metadata: (a) => ({ ...meta(a?.meetingId), duration: 462, transcript_count: transcripts.length }),
    api_get_meeting_transcripts: { transcripts, total_count: transcripts.length, has_more: false },
    api_get_summary: (a) => ({ status: 'completed', meetingName: meetings[0].title, meeting_id: a?.meetingId || 'm1', start: day(12), end: day(12), data: summary, error: null }),
    list_meeting_speakers: people.map((p) => ({ ...p, segment_count: 2 })),
    check_first_launch: false,
    get_onboarding_status: /^onboard\d/.test(window.name) ? { completed: false, current_step: Number(window.name.slice(7)), model_status: { parakeet: 'downloaded', summary: 'downloaded' } } : window.name === 'onboard' ? null : { completed: true, current_step: 5, model_status: {} },
    get_recording_preferences: { save_folder: '/home/demo/Recordings', auto_save: true, file_format: 'mp4', preferred_mic_device: null, preferred_system_device: null },
    get_transcription_status: { chunks_in_queue: 0, is_processing: false, last_activity_ms: 0 },
    integrations_get_settings: { cli_enabled: false, mcp_enabled: false, webhooks: [] },
    get_audio_devices: [{ name: 'Built-in microphone', device_type: 'Input' }, { name: 'Speakers (monitor)', device_type: 'Output' }],
    parakeet_has_available_models: true, whisper_has_available_models: true, builtin_ai_is_model_ready: true,
    api_get_model_config: { provider: 'builtin-ai', model: 'local-summary-model', whisperModel: 'large-v3-turbo' },
    api_get_transcript_config: { provider: 'localWhisper', model: 'large-v3-turbo' },
    workspace_search: { sources: meetings.slice(0, 3).flatMap((meeting) => lines.slice(0, 3).map((line, i) => ({
      id: `transcript:${meeting.id}:${i}`, meeting_id: meeting.id, source_kind: 'transcript', source_id: 't' + i,
      title: meeting.title, body: line[4], start_time: line[2], end_time: line[3], recorded_at: meeting.created_at, fingerprint: 'demo',
    }))), mode: 'full-text', warnings: [] }, workspace_tags: [], workspace_meeting_tags: [], workspace_answers: [], workspace_tasks: [], workspace_calendar_events: [],
    'plugin:app|version': '0.1.0', 'plugin:app|name': 'Overtalk', 'plugin:os|platform': 'linux', 'plugin:os|os_type': 'linux',
    'plugin:os|arch': 'x86_64', 'plugin:os|version': '6.0', 'plugin:os|locale': 'en-GB', 'plugin:os|family': 'unix',
  };
  let next = 1;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    plugins: {},
    transformCallback(cb) { const id = next++; window['_' + id] = cb; return id; },
    unregisterCallback(id) { delete window['_' + id]; },
    convertFileSrc(p) { return p; },
    async invoke(cmd, args) {
      if (cmd === 'plugin:event|listen') {
        (window.__fixtureListeners = window.__fixtureListeners || []).push({ event: args && args.event, handler: args && args.handler });
        if (LIVE && args && args.event === 'recording-levels') {
          const handler = args.handler;
          setTimeout(() => {
            for (let start = 0; start < 252; start += 0.5) {
              const values = [];
              for (let i = 0; i < 5; i++) {
                const t = start + i / 10;
                const talking = liveLines.some(([, , a, b]) => t >= a && t < b);
                const swell = 0.55 + 0.45 * Math.sin(t * 0.9 + 1.3) * Math.sin(t * 0.37);
                const beat = 0.35 + 0.65 * Math.abs(Math.sin(t * 7.1 + Math.sin(t * 1.7)));
                values.push(talking ? Math.round(255 * Math.min(1, 0.62 * swell * beat * (0.7 + rand() * 0.6))) : Math.round(rand() * 5));
              }
              const cb = window['_' + handler];
              if (cb) cb({ event: 'recording-levels', id: 1, payload: { start, peaks: values } });
            }
          }, 400);
        }
        return next++;
      }
      const v = R[cmd];
      if (v === undefined) return /(models|devices|list|templates|speakers|tasks|tags|events|answers|providers|history|backups|languages|webhooks|sources|meetings|folders)/.test(cmd) ? [] : null;
      return typeof v === 'function' ? v(args) : v;
    },
  };
}

module.exports = { installFixture };
if (require.main === module) (async () => {
  const puppeteer = require(process.argv[2]);
  const browser = await puppeteer.launch({ headless: true, args: ['--no-sandbox', '--autoplay-policy=no-user-gesture-required'] });
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e.message).slice(0, 200)));
  page.on('console', (m) => { if (m.type() === 'error') errors.push('console: ' + m.text().slice(0, 200)); });
  await page.setViewport({ width: W, height: H, deviceScaleFactor: 1 });
  await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: process.env.SCHEME || 'light' }]);
  if (process.env.LIVE) await page.evaluateOnNewDocument(() => { window.name = 'live'; });
  if (process.env.ONBOARD) await page.evaluateOnNewDocument((n) => { window.name = n; }, process.env.ONBOARD === '1' ? 'onboard' : 'onboard' + process.env.ONBOARD);
  if (process.env.TOUR) await page.evaluateOnNewDocument((t) => { window.name = (window.name || '') + 'tour'; if (t === 'home') { try { localStorage.setItem('overtalk.tour.pending', 'home'); localStorage.setItem('overtalk.tour.seen', 'review'); } catch (e) {} } }, process.env.TOUR);
  await page.evaluateOnNewDocument(installFixture);
  try {
    await page.goto(base + route, { waitUntil: 'networkidle2', timeout: 90000 });
  } catch (e) { errors.push('goto: ' + e.message); }
  await new Promise((r) => setTimeout(r, 3000));
  if (process.env.CLICK) { try { await page.click(process.env.CLICK); } catch (e) { errors.push('click: ' + e.message); } await new Promise((r) => setTimeout(r, 1500)); }
  await page.addStyleTag({ content: '*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}' });
  await page.evaluate(() => document.activeElement && document.activeElement.blur && document.activeElement.blur());
  await new Promise((r) => setTimeout(r, 300));
  await page.screenshot({ path: out });
  console.log(out, errors.length ? 'ERRORS:\n  ' + [...new Set(errors)].slice(0, 6).join('\n  ') : 'no errors');
  await browser.close();
})();
