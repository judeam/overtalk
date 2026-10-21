#!/usr/bin/env node
/** Detect acceleration once, then invoke the Tauri CLI without a shell. */
const { execFileSync, spawnSync } = require('node:child_process');
const path = require('node:path');

const command = process.argv[2];
if (!['dev', 'build'].includes(command)) {
  console.error('Usage: node tauri-auto.js dev|build');
  process.exit(1);
}
const cwd = path.resolve(__dirname, '..');
let feature = process.env.TAURI_GPU_FEATURE || '';
if (!feature) {
  try {
    feature = execFileSync(process.execPath, [path.join(__dirname, 'auto-detect-gpu.js')], {
      cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'],
    }).trim();
  } catch {
    // Detection is optional; use the platform default if it fails.
  }
}
const supported = new Set(['', 'none', 'cuda', 'vulkan', 'metal', 'coreml', 'openblas', 'openmp', 'hipblas']);
if (!supported.has(feature)) {
  console.error('Unsupported TAURI_GPU_FEATURE');
  process.exit(2);
}
const env = { ...process.env };
if (process.platform === 'linux' && feature === 'cuda') {
  env.CMAKE_CUDA_ARCHITECTURES ||= 'native';
  env.CMAKE_CUDA_STANDARD = '17';
  env.CMAKE_POSITION_INDEPENDENT_CODE = 'ON';
}
const args = [require.resolve('@tauri-apps/cli/tauri.js'), command];
if (feature && feature !== 'none') args.push('--', '--features', feature);
console.log(`Running Tauri ${command} with ${feature || 'platform default'} acceleration`);
const result = spawnSync(process.execPath, args, { cwd, stdio: 'inherit', env });
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
