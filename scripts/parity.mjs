import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const [baseline, candidate, destination = 'artifacts/parity'] = process.argv.slice(2);
assert.ok(baseline && candidate, 'Usage: node scripts/parity.mjs BASELINE CANDIDATE [OUTPUT_DIRECTORY]');
const directory = path.resolve(destination);
assert.equal(existsSync(path.join(directory, 'results.json')), false, 'Choose a fresh output directory');
mkdirSync(directory, { recursive: true });
const executables = [baseline, candidate].map(file => path.resolve(file));
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const results = { executables: executables.map(file => ({ path: file, sha256: hash(file) })), cases: [] };

function command(program, arguments_) {
  const result = spawnSync(program, arguments_, { encoding: 'utf8', timeout: 120000, maxBuffer: 4 * 1024 * 1024 });
  assert.equal(result.status, 0, `${program}: ${result.error || result.stderr}`);
  return result.stdout;
}

const cases = [
  { name: 'svg-title', input: 'examples/hello.svg.mjs', width: 1920, height: 1080 },
  { name: 'svg-particles', input: 'bench/particles.svg.mjs', width: 1280, height: 720 },
  { name: 'three-orbit', input: 'examples/orbit.three.mjs', width: 1280, height: 720 },
  { name: 'three-heavy', input: 'bench/heavy.three.mjs', width: 1280, height: 720 },
  { name: 'three-software', input: 'tests/fixtures/bands.three.mjs', width: 64, height: 64, software: true },
  { name: 'three-custom-render', input: 'tests/fixtures/custom-render.three.mjs', width: 64, height: 64 },
  { name: 'static-letterbox', input: 'tests/fixtures/bands.svg', width: 96, height: 64 },
];
for (const fixture of cases) {
  const source = path.join(root, fixture.input);
  const settings = ['--width', String(fixture.width), '--height', String(fixture.height), '--duration', '2', '--fps', '12'];
  if (fixture.software) settings.push('--software-rendering');
  const samples = [];
  for (const time of [0, 0.75, 23 / 12, 2]) {
    const checksums = executables.map((executable, variant) => {
      const output = path.join(directory, `${fixture.name}-${time}-${variant}.png`);
      command(executable, ['frame', source, ...settings, '--time', String(time), '-o', output]);
      return command('ffmpeg', ['-v', 'error', '-i', output, '-pix_fmt', 'rgba', '-f', 'framemd5', '-']);
    });
    assert.equal(checksums[1], checksums[0], `${fixture.name} at ${time}s differs`);
    samples.push({ time, rgba_checksum: checksums[0].trim().split('\n').at(-1) });
  }
  const movies = executables.map((executable, variant) => {
    const output = path.join(directory, `${fixture.name}-${variant}.mp4`);
    const report = JSON.parse(command(executable, ['render', source, ...settings, '--encoder', 'software', '-o', output]));
    assert.equal(report.frames, 24);
    return command('ffmpeg', ['-v', 'error', '-i', output, '-pix_fmt', 'rgba', '-f', 'framemd5', '-']);
  });
  assert.equal(movies[1], movies[0], `${fixture.name} decoded video frames differ`);
  results.cases.push({ ...fixture, input_sha256: hash(source), samples, exact_decoded_video_frames: 24 });
  writeFileSync(path.join(directory, 'results.json'), JSON.stringify(results, null, 2) + '\n');
  console.log(`${fixture.name}: four PNG samples and all 24 decoded video frames match exactly`);
}
