// Requires a running frontend and Playwright; all native data is synthetic.
// node scripts/check-ui.cjs [path-to-playwright] [base-url]
const assert = require('node:assert/strict');
const { chromium } = require(process.argv[2] || 'playwright');
const { installFixture } = require('./guide/fixture.cjs');
const { supplement, updatedFixture } = require('./guide/workspace-fixture.cjs');
const base = process.argv[3] || 'http://127.0.0.1:3118';
function largeLibrary() {
  const invoke = window.__TAURI_INTERNALS__.invoke;
  const rows = Array.from({ length: 2000 }, (_, i) => ({ id: `m${i + 1}`, title: `Meeting ${i + 1}`, created_at: '2026-10-12T09:30:00Z', duration: 500, speakers: [], pattern: [] }));
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    if (cmd === 'api_get_meetings' || cmd === 'meeting_overviews') return rows;
    if (cmd === 'workspace_meeting_states') return rows.map(r => ({ meeting_id: r.id, pinned: false, archived: false, trashed_at: null }));
    if (cmd === 'api_get_meeting_transcripts') return { transcripts: Array.from({ length: 1000 }, (_, i) => ({ id: `t${i}`, text: `Transcript line ${i}`, timestamp: '', audio_start_time: i, audio_end_time: i + 1, speaker_id: 'spk_lena', speaker_name: 'Lena' })), total_count: 1000, has_more: false };
    if (cmd === 'meeting_timeline') return { duration: 6000, speakers: [{ id: 'spk_lena', name: 'Lena' }], turns: Array.from({ length: 1000 }, (_, i) => ({ speaker_id: 'spk_lena', start: i * 6, end: i * 6 + 2 })), sources: [] };
    return invoke(cmd, args);
  };
}
(async () => {
  const browser = await chromium.launch({ headless: true, executablePath: process.env.BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const errors = [];
  try {
    async function open(width, height, large = false, route = '/meeting-details?id=m1') {
      const page = await browser.newPage({ viewport: { width, height } });
      page.on('pageerror', error => errors.push(error.message));
      await page.addInitScript(installFixture);
      await page.addInitScript(() => localStorage.setItem('overtalk.timeline-open', 'false'));
      await page.addInitScript(supplement);
      await page.addInitScript(updatedFixture);
      if (large) await page.addInitScript(largeLibrary);
      await page.goto(`${base}${route}`, { waitUntil: 'networkidle' });
      await page.locator(route.startsWith('/meeting-details') ? '.ot-line-text' : route === '/settings' ? '.settings-content-header' : '.library-search').first().waitFor();
      return page;
    }
    for (const [width, height] of [[390,844], [800,700], [1280,720], [1920,1080], [3840,2160], [7680,4320]]) {
      const page = await open(width, height);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `overflow at ${width}`);
      assert.equal(await page.locator('.ot-app').evaluate(el => Math.round(el.getBoundingClientRect().width)), width, `full window at ${width}`);
      assert.equal(await page.getByRole('button', { name: 'Timeline', exact: true }).getAttribute('aria-expanded'), 'true');
      if (width >= 3840) {
        assert.ok(await page.locator('.ot-line-text').first().evaluate(el => parseFloat(getComputedStyle(el).fontSize)) >= 24);
        assert.ok(await page.locator('.ot-arrangement').evaluate(el => el.scrollHeight <= el.clientHeight + 2), 'large screen fits every timeline lane');
      }
      assert.ok(await page.locator('.ot-sheet-body').evaluate(el => el.clientHeight) > 180, `reading area at ${width}`);
      await page.close();
    }
    let page = await open(1280, 720, true);
    const count = await page.locator('.ot-meeting').count();
    assert.ok(count > 0 && count < 60, `2,000 meetings should render a small window; got ${count}`);
    assert.ok(await page.locator('.ot-line').count() < 60, 'long transcripts should be windowed');
    assert.equal(await page.locator('.ot-dense-clips').count(), 1, 'dense timeline uses one interactive canvas per speaker');
    assert.equal(await page.locator('.ot-clip').count(), 0, 'dense timeline does not create a button per passage');
    await page.locator('.ot-dense-clips').press('End');
    assert.ok((await page.locator('.ot-dense-clips').getAttribute('aria-label')).includes('99:54') || (await page.locator('.ot-dense-clips').getAttribute('aria-label')).includes('1:39:54'), 'keyboard can select the last speech passage');
    await page.locator('.ot-sheet-body').evaluate(el => { el.scrollTop = el.scrollHeight; });
    await page.waitForFunction(() => document.body.innerText.includes('Transcript line 999'));
    await page.getByRole('button', { name: 'Edit transcript at 16:39' }).click();
    await page.getByLabel('Transcript text', { exact: true }).fill('Last line corrected');
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await page.locator('.ot-rail-list').evaluate(el => { el.scrollTop = el.scrollHeight; });
    await page.waitForFunction(() => document.body.innerText.includes('Meeting 2000'));
    const start = Date.now();
    await page.getByLabel('Search meeting titles and transcripts').fill('Meeting 1999');
    await page.waitForFunction(() => document.querySelectorAll('.ot-meeting').length < 5);
    console.log(`2,000 meetings: ${count} rows rendered; search ${Date.now() - start} ms`);
    await page.close();
    page = await open(1280, 720);
    await page.getByRole('tab', { name: 'Notes', exact: true }).click();
    const editor = page.locator('[contenteditable=true]').first();
    await editor.click(); await page.keyboard.press('Control+End'); await page.keyboard.type(' AUTOSAVE CHECK');
    await page.waitForFunction(() => JSON.stringify(window.__savedNotes || '').includes('AUTOSAVE CHECK'));
    await page.getByRole('tab', { name: 'Transcript', exact: true }).click();
    await page.getByRole('tab', { name: 'Notes', exact: true }).click();
    assert.ok((await page.locator('body').innerText()).includes('AUTOSAVE CHECK'));
    await page.close();
    page = await open(1440, 900, false, '/library');
    assert.equal(await page.locator('.library-meeting-group').count(), 3);
    await page.locator('.library-passage').nth(1).click();
    assert.ok((await page.locator('.library-preview-body').innerText()).includes('Export is blocked'));
    assert.ok((await page.locator('.library-preview-actions a').getAttribute('href')).includes('at=110'));
    await page.setViewportSize({ width: 390, height: 844 });
    await page.getByRole('button', { name: 'Back to results' }).click();
    assert.equal(await page.locator('.library-results-list').isVisible(), true);
    await page.close();
    page = await open(1440, 900, false, '/settings');
    await page.getByRole('radio', { name: /^Extra large/ }).check();
    assert.equal(await page.evaluate(() => document.documentElement.dataset.readingSize), 'extra-large');
    await page.getByLabel('Find a setting').fill('microphone');
    await page.getByLabel('Find a setting').press('Enter');
    assert.equal(await page.getByRole('heading', { name: 'Recordings', exact: true }).isVisible(), true);
    await page.close();
    assert.deepEqual(errors, []);
    console.log('Responsive layout, virtual scrolling, editing, persistent notes, Library preview and settings search passed.');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
