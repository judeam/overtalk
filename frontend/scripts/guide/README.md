# Guide screenshots

The guide (`src/components/guide/`) shows real screens of the app, captured with synthetic demo data.
After changing a screen the guide describes, recapture and convert:

```sh
pnpm dev                                    # serves http://localhost:3118
node scripts/guide/capture-guide.cjs <path to puppeteer> /tmp/guide-shots src/components/guide/markers.json
# then convert /tmp/guide-shots/*.png to public/guide/*.webp at 1344x896
bun test tests/lib/guide.test.ts            # every step must point at a control on its screenshot
```

`fixture.cjs` replaces the Tauri bridge with fixed demo meetings (Lena, Marco and Priya); nothing is read from a real database.

`workspace-fixture.cjs` supplies the current settings and stateful workspace commands. The browser regression check is `node scripts/check-ui.cjs /path/to/playwright [base-url]`. It also exercises large virtual lists and persistent notes.
