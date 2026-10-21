#!/usr/bin/env node
// Capture only synthetic meetings from the existing web UI; never attach to user data.
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const { installFixture } = require(path.join(root, 'frontend/scripts/guide/fixture.cjs'));
const { supplement, updatedFixture } = require(path.join(root, 'frontend/scripts/guide/workspace-fixture.cjs'));
const output = path.resolve(process.argv[2] || '/tmp/overtalk-parity');
const sizes = (process.argv[3] || '1280x720,1920x1080,3840x2160,7680x4320').split(',').map(value => value.split('x').map(Number));
fs.mkdirSync(output, { recursive: true });
(async () => {
  const browser = await chromium.launch({
    ...(process.env.CHROMIUM_EXECUTABLE ? { executablePath: process.env.CHROMIUM_EXECUTABLE } : {}),
    args: ['--no-sandbox'],
  });
  try {
    let fixture;
    for (const [width, height] of sizes) {
      const page = await browser.newPage({ viewport: { width, height }, colorScheme: 'dark', timezoneId: 'Europe/Berlin' });
      await page.addInitScript(() => {
        const OriginalDate = Date;
        const instant = OriginalDate.parse('2026-10-14T07:30:00Z');
        window.Date = class extends OriginalDate {
          constructor(...args) { super(...(args.length ? args : [instant])); }
          static now() { return instant; }
        };
      });
      for (const initialize of [installFixture, supplement, updatedFixture]) await page.addInitScript(initialize);
      for (const [name, route] of [['meeting', '/meeting-details?id=m1'], ['library', '/library'], ['settings', '/settings?tab=general'], ['home', '/']]) {
        await page.goto((process.env.OVERTALK_WEB_URL || 'http://127.0.0.1:3118') + route, { waitUntil: 'networkidle' });
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: path.join(output, `web-${name}-${width}.png`) });
        if (!fixture && name === 'meeting') {
          fixture = await page.evaluate(async () => Object.fromEntries(await Promise.all([
            'meeting_overviews', 'api_get_meeting_metadata', 'api_get_meeting_transcripts', 'api_get_summary',
            'meeting_timeline', 'meeting_waveform', 'get_recording_preferences', 'get_audio_devices',
            'api_get_model_config', 'api_get_transcript_config', 'workspace_search',
          ].map(async command => [command, await window.__TAURI_INTERNALS__.invoke(command, { meetingId: 'm1', request: { query: '', limit: 100 } })]))));
        }
        if (name === 'meeting') {
          await page.getByRole('tab', { name: 'Notes', exact: true }).click();
          await page.locator('[contenteditable="true"]').first().waitFor();
          await page.screenshot({ path: path.join(output, `web-notes-${width}.png`) });
        }
        if (name === 'home') {
          fixture.visual = await page.evaluate(() => ({ recording_title: document.querySelector('.ot-title-input')?.value, recording_date: '14 Oct' }));
        }
      }
      await page.close();
      console.log(`Web reference: ${width} × ${height}`);
    }
    for (const row of fixture.meeting_overviews) delete row.pattern;
    fs.writeFileSync(path.join(output, 'fixture.json'), JSON.stringify(fixture));
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
