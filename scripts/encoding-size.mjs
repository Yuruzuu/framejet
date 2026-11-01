import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = fileURLToPath(new URL('../', import.meta.url));
const { values, positionals } = parseArgs({ allowPositionals:true, options:{
  direct:{ type:'boolean', default:false }, workload:{ type:'string', multiple:true },
} });
const directory = path.resolve(positionals[0] || 'artifacts/encoding-size');
assert.equal(existsSync(directory), false, 'Choose a fresh output directory');
mkdirSync(directory, { recursive: true });
const executable = path.join(root, 'target/release/framejet');
const workloads = [
  { name: 'svg-titles', input: 'examples/hello.svg.mjs', width:1280, height:720 },
  { name: 'svg-particles', input: 'bench/particles.svg.mjs', width:1280, height:720 },
  { name: 'three-heavy', input: 'bench/heavy.three.mjs', width:1280, height:720 },
  { name: 'three-orbit', input: 'examples/orbit.three.mjs', width:1280, height:720 },
  { name: 'three-heavy-1080p', input: 'bench/heavy.three.mjs', width:1920, height:1080 },
].filter(workload => !values.workload || values.workload.includes(workload.name));
assert.ok(workloads.length, 'Unknown workload');
const settings = [
  { name: 'hardware-12mbps', framejet:['--encoder','videotoolbox','--bitrate','12'], arguments: ['-c:v', 'h264_videotoolbox', '-allow_sw', '0', '-b:v', '12M'] },
  { name: 'hardware-6mbps', framejet:['--encoder','videotoolbox','--bitrate','6'], arguments: ['-c:v', 'h264_videotoolbox', '-allow_sw', '0', '-b:v', '6M'] },
  { name: 'fast-crf18', framejet:['--encoder','software','--crf','18'], arguments: ['-c:v', 'libx264', '-preset', 'fast', '-crf', '18'] },
  { name: 'fast-crf20', framejet:['--encoder','software','--crf','20'], arguments: ['-c:v', 'libx264', '-preset', 'fast', '-crf', '20'] },
  { name: 'fast-crf22', framejet:['--encoder','software','--crf','22'], arguments: ['-c:v', 'libx264', '-preset', 'fast', '-crf', '22'] },
  { name: 'medium-crf18', arguments: ['-c:v', 'libx264', '-preset', 'medium', '-crf', '18'] },
  { name: 'medium-crf20', arguments: ['-c:v', 'libx264', '-preset', 'medium', '-crf', '20'] },
  { name: 'slow-crf20', arguments: ['-c:v', 'libx264', '-preset', 'slow', '-crf', '20'] },
].filter(setting => !values.direct || setting.framejet);
const records = [];

function run(program, arguments_) {
  const started = process.hrtime.bigint();
  const result = spawnSync(program, arguments_, { encoding: 'utf8', timeout: 120000, maxBuffer: 4 * 1024 * 1024 });
  assert.equal(result.status, 0, `${program}: ${result.error || result.stderr}`);
  return { stdout: result.stdout, stderr: result.stderr, seconds: Number(process.hrtime.bigint() - started) / 1e9 };
}

for (const workload of workloads) {
  const reference = path.join(directory, `${workload.name}-reference.mp4`);
  const dimensions = ['--width', String(workload.width), '--height', String(workload.height), '--duration', '3', '--fps', '30'];
  run(executable, ['render', path.join(root, workload.input), '-o', reference, ...dimensions, '--encoder', 'software', '--crf', '0']);
  for (const setting of settings) {
    const output = path.join(directory, `${workload.name}-${setting.name}.mp4`);
    const encoding = values.direct
      ? run(executable, ['render', path.join(root, workload.input), '-o', output, ...dimensions, ...setting.framejet])
      : run('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-i', reference,
        ...setting.arguments, '-an', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', output]);
    const probe = JSON.parse(run('ffprobe', ['-v', 'error', '-select_streams', 'v:0', '-count_frames',
      '-show_entries', 'stream=codec_name,width,height,nb_read_frames,r_frame_rate,duration', '-of', 'json', output]).stdout).streams[0];
    assert.equal(probe.codec_name, 'h264');
    assert.equal(probe.width, workload.width);
    assert.equal(probe.height, workload.height);
    assert.equal(Number(probe.nb_read_frames), 90);
    assert.equal(probe.r_frame_rate, '30/1');
    assert.equal(Number(probe.duration), 3);
    const quality = run('ffmpeg', ['-hide_banner', '-i', output, '-i', reference,
      '-lavfi', '[0:v][1:v]ssim=stats_file=-', '-f', 'null', '-']);
    const ssim = Number(quality.stderr.match(/All:([\d.]+)/)?.[1]);
    assert.ok(Number.isFinite(ssim), 'SSIM result missing');
    const frames = quality.stdout.trim().split('\n').filter(line => line.startsWith('n:'));
    assert.equal(frames.length, 90, 'Quality comparison must include every frame');
    const record = { workload: workload.name, width:workload.width, height:workload.height, setting: setting.name, bytes: statSync(output).size,
      wall_seconds: encoding.seconds, ssim, worst_frame_ssim: Math.min(...frames.map(line => Number(line.match(/All:([\d.]+)/)[1]))) };
    records.push(record);
    console.log(JSON.stringify(record));
    writeFileSync(path.join(directory, 'results.json'), JSON.stringify({ mode:values.direct?'direct':'encoding-only', fps:30, frames:90,
      reference: 'Lossless x264 CRF 0 after the existing RGB-to-YUV420 conversion. Direct timings cover the whole render; encoding-only timings include reference decoding.', records }, null, 2) + '\n');
  }
}
