// Module mocks in Bun share a process cache. Keep each regression suite isolated.
const { readdirSync } = require('node:fs');
const { join, relative } = require('node:path');
const { spawnSync } = require('node:child_process');

function testFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return testFiles(path);
    return entry.isFile() && /\.test\.(?:[cm]?[jt]s|[jt]sx)$/.test(entry.name) ? [path] : [];
  });
}
const files = testFiles(join(process.cwd(), 'tests')).sort();
if (!files.length) throw new Error('No regression suites found');
let failed = 0;
for (const file of files) {
  const result = spawnSync('bun', ['test', relative(process.cwd(), file)], {
    stdio: 'inherit', env: process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) failed++;
}
console.log(`Validated ${files.length} isolated suites; ${failed} failed.`);
process.exitCode = failed ? 1 : 0;
