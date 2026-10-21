// Capture the guide's screenshots from the app and record where each data-tour target sits.
// Usage: node capture-guide.cjs <puppeteer-module-path> <out-dir> <markers.json> [base-url]
process.argv[3] = process.argv[3] || '';
const puppeteer = require(process.argv[2]);
const fs = require('fs');
const { installFixture } = require('./fixture.cjs');
const { supplement, updatedFixture } = require('./workspace-fixture.cjs');
const out = process.argv[3];
const markersFile = process.argv[4];
const base = process.argv[5] || 'http://localhost:3118';
const review = '/meeting-details?id=m1&at=189';
const shots = [
  ['home', '/', {}],
  ['live', '/', { live: true }],
  ['review', review, {}],
  ['speakers', review, { click: 'button[data-tour="speakers-tab"]' }],
  ['notes', review, { click: 'button[data-tour="notes-button"]' }],
  ['search', review, { click: 'input[aria-label="Search meeting titles and transcripts"]', type: 'export' }],
  ['library', '/library', {}],
  ['settings', '/settings', {}],
];
fs.mkdirSync(out, { recursive: true });
(async () => {
  const browser = await puppeteer.launch({ headless: true, executablePath: process.env.BROWSER_EXECUTABLE || undefined, args: ['--no-sandbox'] });
  const markers = process.env.ONLY ? JSON.parse(fs.readFileSync(markersFile, 'utf8')) : {};
  for (const [name, route, opts] of shots.filter(([n]) => !process.env.ONLY || process.env.ONLY.split(',').includes(n))) {
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e.message).slice(0, 160)));
    await page.setViewport({ width: 1536, height: 1024, deviceScaleFactor: 1 });
    await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: 'light' }]);
    await page.evaluateOnNewDocument((live) => {
      if (live) window.name = 'live';
      try { localStorage.setItem('overtalk.tour.seen', 'home,review'); } catch (e) {}
    }, !!opts.live);
    await page.evaluateOnNewDocument(installFixture);
    await page.evaluateOnNewDocument(supplement);
    await page.evaluateOnNewDocument(updatedFixture);
    try { await page.goto(base + route, { waitUntil: 'networkidle2', timeout: 90000 }); } catch (e) { errors.push('goto ' + e.message); }
    await new Promise((r) => setTimeout(r, 2500));
    if (['review', 'search'].includes(name)) await page.evaluate(() => document.querySelector('[aria-expanded][class~="ot-tool"]') && Array.from(document.querySelectorAll('button')).find(b => b.textContent.trim() === 'Timeline' && b.getAttribute('aria-expanded') === 'false')?.click());
    if (name === 'live') await page.evaluate(() => Array.from(document.querySelectorAll('button')).find(b => b.textContent.includes('Show speaker timeline'))?.click());
    if (opts.click) { try { await page.click(opts.click); } catch (e) { errors.push('click ' + e.message); } await new Promise((r) => setTimeout(r, 1200)); }
    if (opts.type) { await page.keyboard.type(opts.type, { delay: 30 }); await new Promise((r) => setTimeout(r, 800)); }
    await page.addStyleTag({ content: '*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}' });
    if (!opts.type) await page.evaluate(() => document.activeElement && document.activeElement.blur && document.activeElement.blur());
    await new Promise((r) => setTimeout(r, 300));
    markers[name] = await page.evaluate(() => {
      const W = window.innerWidth, H = window.innerHeight, found = {};
      for (const el of document.querySelectorAll('[data-tour]')) {
        const r = el.getBoundingClientRect();
        if (r.width < 2 || r.height < 2 || r.bottom < 0 || r.top > H) continue;
        const key = el.getAttribute('data-tour');
        if (found[key]) continue;
        const top = Math.max(0, r.top), bottom = Math.min(H, r.bottom);
        found[key] = { x: +(r.left / W).toFixed(4), y: +(top / H).toFixed(4), w: +(r.width / W).toFixed(4), h: +((bottom - top) / H).toFixed(4) };
      }
      return found;
    });
    await page.screenshot({ path: `${out}/${name}.png` });
    console.log(name, Object.keys(markers[name]).length, 'targets', errors.length ? 'ERRORS ' + errors.join(' | ') : '');
    await page.close();
  }
  fs.writeFileSync(markersFile, JSON.stringify(markers, null, 1) + '\n');
  await browser.close();
})();
