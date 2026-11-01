import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, existsSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const executable = path.resolve(process.argv[2] || path.join(root, 'target/release/framejet'));
const temporary = mkdtempSync(path.join(tmpdir(), 'framejet-smoke-'));

function command(program, arguments_, binary = false) {
  const result = spawnSync(program, arguments_, { encoding: binary ? null : 'utf8', timeout: 60000, maxBuffer: 32 * 1024 * 1024 });
  assert.equal(result.status, 0, `${program} failed: ${result.error || result.stderr}`);
  return result.stdout;
}

try {
  for (const backend of ['svg', 'three']) {
    const input = path.join(root, `tests/fixtures/bands.${backend}.mjs`);
    const output = path.join(temporary, `${backend}.mp4`);
    const report = JSON.parse(command(executable, ['render', input, '-o', output, '--encoder', 'software']));
    assert.equal(report.status, 'ok');
    assert.equal(report.frames, 4);
    const probe = JSON.parse(command('ffprobe', ['-v', 'error', '-select_streams', 'v:0', '-count_frames', '-show_streams', '-of', 'json', output]));
    assert.equal(probe.streams[0].codec_name, 'h264');
    assert.equal(probe.streams[0].width, 64);
    assert.equal(probe.streams[0].height, 64);
    assert.equal(Number(probe.streams[0].nb_read_frames), 4);
    assert.equal(Number(probe.streams[0].duration), 1);

    const decoded = command('ffmpeg', ['-v', 'error', '-i', output, '-f', 'rawvideo', '-pix_fmt', 'rgba', '-'], true);
    const frameBytes = 64 * 64 * 4;
    assert.equal(decoded.length, frameBytes * 4);
    assert.notDeepEqual(decoded.subarray(0, frameBytes), decoded.subarray(frameBytes * 3));
    // Lossy H.264 is allowed small color differences; orientation must remain correct.
    assert.ok(decoded[0] > 240 && decoded[2] < 15, 'Top of video must be red');
    const bottom = 63 * 64 * 4;
    assert.ok(decoded[bottom + 2] > 240 && decoded[bottom] < 15, 'Bottom of video must be blue');

    const frame = path.join(temporary, `${backend}.png`);
    command(executable, ['frame', input, '--time', '0.5', '-o', frame]);
    const pixels = command('ffmpeg', ['-v', 'error', '-i', frame, '-f', 'rawvideo', '-pix_fmt', 'rgba', '-'], true);
    assert.deepEqual([...pixels.subarray(0, 4)], [255, 0, 0, 255]);
    assert.deepEqual([...pixels.subarray(bottom, bottom + 4)], [0, 0, 255, 255]);
    console.log(`${backend}: H.264, dimensions, frame count, duration, motion, pixel colors and orientation passed`);
  }

  const failing = path.join(temporary, 'failure.mjs');
  const output = path.join(temporary, 'failure.mp4');
  writeFileSync(failing, `export const video = {width:64,height:64,fps:4,duration:1}; export function render({frame}) { if(frame===2) throw new Error('smoke failure'); return '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"/>'; }`);
  const result = spawnSync(executable, ['render', failing, '-o', output, '--encoder', 'software'], { encoding: 'utf8', timeout: 30000 });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /smoke failure/);
  assert.equal(existsSync(output), false, 'Failed renders must not leave a final MP4');
  console.log('partial render failure: nonzero exit and no published output passed');
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
