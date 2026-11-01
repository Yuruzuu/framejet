import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { cpus, totalmem } from 'node:os';

const root = fileURLToPath(new URL('../', import.meta.url));
const { values, positionals } = parseArgs({ allowPositionals: true, options: {
  rounds: { type: 'string', default: '5' },
  encoder: { type: 'string', default: 'software' },
  output: { type: 'string', default: 'artifacts/benchmark' },
  workload: { type: 'string', multiple: true },
} });
if (positionals.length !== 2) throw new Error('Usage: node scripts/bench.mjs BASELINE CANDIDATE [--rounds 5] [--encoder software|videotoolbox] [--output DIRECTORY]');
const executables = positionals.map(executable => path.resolve(executable));
const directory = path.resolve(values.output);
const rounds = Number(values.rounds);
if (!Number.isInteger(rounds) || rounds < 1) throw new Error('Rounds must be a positive integer');
if (existsSync(path.join(directory, 'results.json'))) throw new Error('Results already exist; choose a fresh output directory');
mkdirSync(directory, { recursive: true });
const workloads = [
  { name: 'svg-titles', input: 'examples/hello.svg.mjs', width: 1920, height: 1080, duration: 6 },
  { name: 'svg-particles', input: 'bench/particles.svg.mjs', width: 1280, height: 720, duration: 4 },
  { name: 'three-orbit', input: 'examples/orbit.three.mjs', width: 1280, height: 720, duration: 6 },
].filter(workload => !values.workload || values.workload.includes(workload.name));
if (!workloads.length) throw new Error('No matching workloads');
const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex');
const manifest = {
  encoder: values.encoder, rounds, fps: 30,
  executables: executables.map(executable => ({ path: executable, sha256: hash(executable) })),
  workloads: workloads.map(workload => ({ ...workload, sha256: hash(path.join(root, workload.input)) })),
  node: process.version, platform: process.platform, architecture: process.arch,
  cpu: cpus()[0]?.model, logical_cpus: cpus().length, memory_bytes: totalmem(),
  ffmpeg: spawnSync('ffmpeg', ['-version'], { encoding: 'utf8' }).stdout.split('\n')[0],
};
const records = [];

function render(workload, variant, warmup, round) {
  const output = path.join(directory, `${workload.name}-${variant}.mp4`);
  const arguments_ = ['render', path.join(root, workload.input), '-o', output, '--force',
    '--width', String(workload.width), '--height', String(workload.height),
    '--fps', '30', '--duration', String(workload.duration), '--encoder', values.encoder];
  const started = process.hrtime.bigint();
  const result = spawnSync(executables[variant], arguments_, { encoding: 'utf8', timeout: 120000, maxBuffer: 4 * 1024 * 1024 });
  const wallSeconds = Number(process.hrtime.bigint() - started) / 1e9;
  if (result.status !== 0) throw new Error(`${workload.name} variant ${variant}: ${result.error || result.stderr}`);
  const report = JSON.parse(result.stdout);
  if (report.frames !== Math.ceil(workload.duration * 30) || report.encoder !== (values.encoder === 'software' ? 'libx264' : 'h264_videotoolbox')
    || report.video.width !== workload.width || report.video.height !== workload.height || report.video.fps !== 30) {
    throw new Error(`Work/encoder mismatch: ${JSON.stringify(report)}`);
  }
  const record = { workload: workload.name, variant, warmup, round, wall_seconds: wallSeconds, ...report };
  records.push(record);
  console.log(`${warmup ? 'warmup' : `round ${round + 1}`} ${workload.name} ${variant === 0 ? 'baseline' : 'candidate'}: ${wallSeconds.toFixed(3)}s wall, ${report.processing_seconds.toFixed(3)}s processing`);
  writeFileSync(path.join(directory, 'results.json'), JSON.stringify({ manifest, records }, null, 2) + '\n');
}

for (const workload of workloads) for (const variant of [0, 1]) render(workload, variant, true, -1);
for (let round = 0; round < rounds; round++) {
  for (const workload of workloads) {
    for (const variant of round % 2 ? [1, 0] : [0, 1]) render(workload, variant, false, round);
  }
}
const median = numbers => {
  const sorted = [...numbers].sort((first, second) => first - second);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};
const summary = workloads.map(workload => {
  const times = [0, 1].map(variant => records.filter(record => !record.warmup && record.variant === variant && record.workload === workload.name).map(record => record.wall_seconds));
  const baseline = median(times[0]);
  const candidate = median(times[1]);
  return { workload: workload.name, baseline_seconds: baseline, candidate_seconds: candidate,
    reduction_percent: (1 - candidate / baseline) * 100, speedup: baseline / candidate, samples: times };
});
writeFileSync(path.join(directory, 'summary.json'), JSON.stringify(summary, null, 2) + '\n');
console.log(JSON.stringify(summary, null, 2));
