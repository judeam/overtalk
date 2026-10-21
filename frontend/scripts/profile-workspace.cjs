// Production-only, synthetic workload. Never opens recordings or the user's database.
// node scripts/profile-workspace.cjs [playwright-module] [base-url]
const { chromium } = require(process.argv[2] || 'playwright');
const { installFixture } = require('./guide/fixture.cjs');
const { supplement, updatedFixture } = require('./guide/workspace-fixture.cjs');
const base = process.argv[3] || 'http://127.0.0.1:3118';

function workload({ lines, meetings }) {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  window.__perfCalls = {};
  const people = ['spk_lena', 'spk_marco', 'spk_priya'];
  const rows = Array.from({ length: meetings }, (_, i) => ({ id: `m${i + 1}`, title: `Meeting ${i + 1}`, created_at: '2026-10-12T09:30:00Z', duration: lines * 3, speakers: [], pattern: [] }));
  const transcripts = Array.from({ length: lines }, (_, i) => ({ id: `t${i}`, text: `Transcript line ${i}. We discussed the proposed project milestones and agreed to review progress next week.`, timestamp: '', audio_start_time: i * 3, audio_end_time: i * 3 + 2, speaker_id: people[i % people.length], speaker_name: ['Lena', 'Marco', 'Priya'][i % 3] }));
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    window.__perfCalls[cmd] = (window.__perfCalls[cmd] || 0) + 1;
    if (cmd === 'api_get_meetings' || cmd === 'meeting_overviews') return rows;
    if (cmd === 'workspace_meeting_states') return [];
    if (cmd === 'api_get_meeting_transcripts') return { transcripts, total_count: lines, has_more: false };
    if (cmd === 'meeting_timeline') {
      const original = await invoke(cmd, args);
      return { ...original, duration: lines * 3, turns: transcripts.map(t => ({ speaker_id: t.speaker_id, start: t.audio_start_time, end: t.audio_end_time })), sources: transcripts.map(t => [t.audio_start_time, t.audio_end_time]) };
    }
    if (cmd === 'meeting_waveform') return [];
    return invoke(cmd, args);
  };
}

async function main() {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  try {
    const results = [];
    for (const lines of [30, 5000]) {
      const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.addInitScript(installFixture);
      await page.addInitScript(supplement);
      await page.addInitScript(updatedFixture);
      await page.addInitScript(workload, { lines, meetings: 2000 });
      const cdp = await page.context().newCDPSession(page);
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: 4 });
      await cdp.send('Performance.enable');
      const start = performance.now();
      await page.goto(`${base}/meeting-details?id=m1`, { waitUntil: 'networkidle' });
      await page.locator('.ot-line-text').first().waitFor();
      await page.waitForTimeout(500);
      const readyMs = Math.round(performance.now() - start);
      const metrics = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map(m => [m.name, m.value]));
      await cdp.send('HeapProfiler.collectGarbage');
      const before = await metrics();
      await page.waitForTimeout(1500);
      const idle = await metrics();
      const frames = await page.evaluate(async () => {
        const gaps = [];
        const body = document.querySelector('.ot-sheet-body');
        const start = performance.now();
        let previous = start;
        await new Promise(resolve => {
          function frame(now) {
            gaps.push(now - previous); previous = now;
            body.scrollTop = ((now - start) / 1800) * Math.max(0, body.scrollHeight - body.clientHeight);
            if (now - start < 1800) requestAnimationFrame(frame); else resolve();
          }
          requestAnimationFrame(frame);
        });
        gaps.sort((a, b) => a - b);
        return { p95Ms: +gaps[Math.floor(gaps.length * .95)].toFixed(1), over50Ms: gaps.filter(n => n > 50).length };
      });
      results.push({ lines, cpuSlowdown: 4, readyMs, heapMiB: +(before.JSHeapUsedSize / 1048576).toFixed(1), nodes: before.Nodes, idleCpuMs: +((idle.TaskDuration - before.TaskDuration) * 1000).toFixed(1), scroll: frames, ...await page.evaluate(() => ({ clips: document.querySelectorAll('.ot-clip').length, transcriptRows: document.querySelectorAll('.ot-line').length, calls: window.__perfCalls })), errors });
      await page.close();
    }
    console.log(JSON.stringify(results, null, 2));
  } finally { await browser.close(); }
}
module.exports = { workload };
if (require.main === module) main().catch(error => { console.error(error); process.exitCode = 1; });
