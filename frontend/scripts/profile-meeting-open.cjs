// Measure click-to-transcript, never network-idle (which adds a fixed 500 ms).
const assert = require('node:assert/strict');
const { installFixture } = require('./guide/fixture.cjs');
const { supplement, updatedFixture } = require('./guide/workspace-fixture.cjs');
const base = process.argv[3] || 'http://127.0.0.1:3118';

function meetingFixture({ summaryDelay }) {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const rows = Array.from({ length: 8 }, (_, i) => ({ id: `m${i + 1}`, title: `Open benchmark ${i + 1}`, created_at: '2026-10-14T12:00:00Z', updated_at: '2026-10-14T12:00:00Z', duration: 300, folder_path: '/synthetic', speakers: [], pattern: [] }));
  const metadata = id => rows.find(row => row.id === id);
  const transcripts = id => ({ transcripts: Array.from({ length: 100 }, (_, i) => ({ id: `${id}-t${i}`, text: `Content for ${id}, line ${i}`, timestamp: '', audio_start_time: i * 3, audio_end_time: i * 3 + 2, speaker_id: 'spk_lena', speaker_name: 'Lena' })), total_count: 100, has_more: false });
  window.__openCalls = [];
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const call = { cmd, meetingId: args?.meetingId, start: performance.now() };
    window.__openCalls.push(call);
    try {
      if (cmd === 'api_get_meetings' || cmd === 'meeting_overviews') return rows;
      if (cmd === 'workspace_meeting_states') return [];
      if (cmd === 'api_get_meeting_metadata') return metadata(args.meetingId);
      if (cmd === 'api_get_meeting_transcripts') return transcripts(args.meetingId);
      if (cmd === 'api_get_summary') await new Promise(resolve => setTimeout(resolve, summaryDelay));
      return await invoke(cmd, args);
    } finally { call.end = performance.now(); }
  };
}

async function measureMeetingOpen(id) {
  const button = [...document.querySelectorAll('.ot-meeting-open')].find(el => el.textContent.includes(`Open benchmark ${id.slice(1)}`));
  const start = performance.now();
  button.click();
  await new Promise((resolve, reject) => {
    const deadline = setTimeout(() => { observer.disconnect(); reject(new Error('Meeting did not render')); }, 5000);
    const observer = new MutationObserver(check);
    function check() {
      if (![...document.querySelectorAll('.ot-line-text')].some(el => el.textContent.includes(`Content for ${id}, line 0`))) return;
      observer.disconnect(); clearTimeout(deadline);
      requestAnimationFrame(() => resolve());
    }
    observer.observe(document.querySelector('.ot-main'), { childList: true, subtree: true, characterData: true });
    check();
  });
  return {
    ms: Math.round((performance.now() - start) * 10) / 10,
    routeRequests: performance.getEntriesByType('resource').filter(entry => entry.startTime >= start && entry.name.includes('/meeting-details.txt')).length,
  };
}

async function main() {
  const { chromium } = require(process.argv[2] || 'playwright');
  const browser = await chromium.launch({ headless: true, executablePath: process.env.BROWSER_EXECUTABLE, args: ['--no-sandbox'] });
  try {
    const results = [];
    for (const summaryDelay of [0, 500]) {
      const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
      await page.addInitScript(installFixture);
      await page.addInitScript(supplement);
      await page.addInitScript(updatedFixture);
      await page.addInitScript(meetingFixture, { summaryDelay });
      await page.goto(base, { waitUntil: 'domcontentloaded' });
      await page.locator('.ot-meeting-open').filter({ hasText: 'Open benchmark 1' }).waitFor();
      const measurements = [];
      for (const id of ['m1', 'm2', 'm3', 'm1', 'm2', 'm3']) {
        const measured = await page.evaluate(measureMeetingOpen, id);
        if (measurements.length) assert.equal(measured.routeRequests, 0, 'Switching meetings should not refetch the static route');
        measurements.push({ id, ...measured });
      }
      await page.goBack();
      await page.locator('.ot-line-text').filter({ hasText: 'Content for m2, line 0' }).waitFor();
      await page.goForward();
      await page.locator('.ot-line-text').filter({ hasText: 'Content for m3, line 0' }).waitFor();
      await page.getByRole('button', { name: 'Play', exact: true }).click();
      await page.getByRole('button', { name: 'Pause', exact: true }).waitFor();
      await page.getByRole('button', { name: 'Pause', exact: true }).click();
      results.push({ summaryDelay, measurements, calls: await page.evaluate(() => window.__openCalls) });
      await page.close();
    }
    console.log(JSON.stringify(results, null, 2));
    if (process.env.ASSERT_INSTANT === '1') {
      for (const result of results) for (const open of result.measurements.slice(1)) assert.ok(open.ms < 100, `${open.id}: ${open.ms} ms (summary delay ${result.summaryDelay})`);
    }
  } finally { await browser.close(); }
}
module.exports = { meetingFixture, measureMeetingOpen, installFixture, supplement, updatedFixture };
if (require.main === module) main().catch(error => { console.error(error); process.exitCode = 1; });
