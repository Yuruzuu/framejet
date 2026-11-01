import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const executable = path.resolve(process.argv[2] || path.join(root, 'target/release/framejet'));
const directory = mkdtempSync(path.join(tmpdir(), 'framejet-svg-'));
let serial = 0;

function run(program, arguments_) {
  const result = spawnSync(program, arguments_, {
    encoding: null,
    timeout: 30000,
    maxBuffer: 16 * 1024 * 1024,
  });
  assert.equal(result.status, 0, `${program}: ${result.error || result.stderr}`);
  return result.stdout;
}
const fixture = (name) => path.join(root, 'tests/fixtures', name);
const pixel = (pixels, x, y, width = 96) => [
  ...pixels.subarray((y * width + x) * 4, (y * width + x) * 4 + 4),
];
const red = [255, 0, 0, 255],
  green = [0, 255, 0, 255],
  blue = [0, 0, 255, 255],
  black = [0, 0, 0, 255];

function frame(input, time, extra = []) {
  const output = path.join(directory, `${serial++}.png`);
  run(executable, ['frame', input, '--time', String(time), '-o', output, ...extra]);
  return run('ffmpeg', ['-v', 'error', '-i', output, '-f', 'rawvideo', '-pix_fmt', 'rgba', '-']);
}
function checkCombined(pixels, time) {
  const x = Math.min(64, time * 32) + 4;
  assert.deepEqual(pixel(pixels, x, 4), red, `CSS position at ${time}s`);
  assert.deepEqual(pixel(pixels, x, 28), green, `SMIL x at ${time}s`);
  assert.deepEqual(pixel(pixels, x, 52), blue, `SMIL transform at ${time}s`);
  assert.deepEqual(
    pixel(pixels, 4, 76),
    time >= 1 && time < 1.5 ? [255, 255, 0, 255] : [128, 0, 128, 255],
    `SMIL set at ${time}s`,
  );
  if (x > 20) assert.deepEqual(pixel(pixels, 4, 4), black, 'Earlier CSS position must be empty');
}

async function checkRewind() {
  const doctor = JSON.parse(run(executable, ['doctor']));
  const child = spawn('node', [path.join(doctor.runtime, 'worker.mjs')], {
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let pending = Buffer.alloc(0);
  const packets = [];
  let waiter;
  let errors = '';
  child.stderr.on('data', (bytes) => {
    errors += bytes;
  });
  child.stdout.on('data', (bytes) => {
    pending = Buffer.concat([pending, bytes]);
    while (pending.length >= 5 && pending.length >= pending.readUInt32LE(1) + 5) {
      const length = pending.readUInt32LE(1);
      packets.push({ kind: pending[0], bytes: pending.subarray(5, 5 + length) });
      pending = pending.subarray(5 + length);
    }
    waiter?.();
  });
  const closed = new Promise((resolve) => child.on('close', resolve));
  let ended = false;
  child.on('close', () => {
    ended = true;
    waiter?.();
  });
  const next = async () => {
    while (!packets.length) {
      if (ended) throw new Error(`Worker stopped: ${errors}`);
      await new Promise((resolve, reject) => {
        const timeout = setTimeout(() => reject(new Error('SVG worker response timed out')), 15000);
        waiter = () => {
          clearTimeout(timeout);
          resolve();
        };
      });
    }
    const packet = packets.shift();
    assert.notEqual(packet.kind, 2, packet.bytes.toString());
    return packet;
  };
  try {
    child.stdin.write(
      JSON.stringify({
        backend: 'svg-browser',
        input: fixture('animated.svg'),
        video: { width: 96, height: 96, fps: 4, duration: 3 },
        overrides: {},
      }) + '\n',
    );
    assert.equal((await next()).kind, 0);
    const times = [2, 0, 1, 0.5, 1.25, 2];
    child.stdin.write(
      JSON.stringify({
        command: 'frames',
        samples: times.map((time) => ({ time, frame: Math.floor(time * 4) })),
      }) + '\n',
    );
    for (const time of times) {
      const packet = await next();
      assert.equal(packet.kind, 3);
      const file = path.join(directory, `${serial++}.png`);
      writeFileSync(file, packet.bytes);
      checkCombined(
        run('ffmpeg', ['-v', 'error', '-i', file, '-f', 'rawvideo', '-pix_fmt', 'rgba', '-']),
        time,
      );
    }
    child.stdin.end('{"command":"close"}\n');
    assert.equal(await closed, 0, errors);
  } finally {
    if (!ended) child.kill();
  }
}

try {
  for (const time of [0, 1, 1.25, 2]) checkCombined(frame(fixture('animated.svg'), time), time);
  await checkRewind();
  console.log(
    'CSS + SMIL: exact positions, transforms, set timing, endpoints and backward seeks passed',
  );

  for (const input of ['animated-module.svg.mjs', 'animated-mixed.svg.mjs'])
    checkCombined(frame(fixture(input), 1), 1);
  const mixedVideo = path.join(directory, 'mixed.mp4');
  run(executable, [
    'render',
    fixture('animated-mixed.svg.mjs'),
    '-o',
    mixedVideo,
    '--encoder',
    'software',
  ]);
  const mixed = run('ffmpeg', [
    '-v',
    'error',
    '-i',
    mixedVideo,
    '-f',
    'rawvideo',
    '-pix_fmt',
    'rgba',
    '-',
  ]);
  assert.equal(mixed.length, 96 * 96 * 4 * 8);
  console.log('SVG modules: automatic detection and mixed native/browser packet batches passed');

  for (const time of [0.25, 1, 1.75]) {
    const pixels = frame(fixture('animated-css.svg'), time);
    const x = (time <= 0.5 ? 0 : time <= 1.5 ? (time - 0.5) * 64 : (2.5 - time) * 64) + 4;
    assert.deepEqual(pixel(pixels, x, 4), red, 'CSS delay/repeat/direction');
    assert.deepEqual(pixel(pixels, 4, 28), green, 'Authored paused animation remains paused');
    assert.deepEqual(pixel(pixels, time * 32 + 4, 52), blue, 'SMIL animateMotion');
  }
  assert.deepEqual(
    pixel(frame(fixture('animated-external.svg'), 1), 36, 4),
    red,
    'Relative external stylesheet',
  );
  const letterbox = frame(fixture('animated.svg'), 1, ['--width', '192', '--height', '96']);
  assert.deepEqual(pixel(letterbox, 84, 4, 192), red, 'Proportional scaling and centering');
  assert.deepEqual(pixel(letterbox, 4, 4, 192), black, 'Letterbox background');
  console.log('CSS delays/repeats/pause, SMIL motion, relative CSS and output resizing passed');

  const shortStart = frame(fixture('animated-timing.svg'), 0);
  const shortMiddle = frame(fixture('animated-timing.svg'), 0.0005);
  const shortEnd = frame(fixture('animated-timing.svg'), 1);
  assert.deepEqual(
    pixel(shortStart, 4, 4),
    black,
    'Short animation resets even after navigation finishes',
  );
  assert.ok(
    Math.abs(pixel(shortMiddle, 4, 4)[0] - 128) <= 1,
    'Sub-frame CSS time is sampled exactly',
  );
  assert.deepEqual(pixel(shortEnd, 4, 4), red);
  assert.deepEqual(pixel(shortStart, 36, 28), green, 'Negative CSS delay');
  assert.deepEqual(pixel(shortEnd, 68, 28), green, 'Negative delay endpoint');
  assert.deepEqual(pixel(shortStart, 4, 52), blue, 'Stepped CSS animation starts at zero');
  assert.deepEqual(pixel(shortEnd, 36, 52), blue, 'Stepped CSS animation switches at one second');
  console.log('Very short CSS animations, sub-frame times, negative delays and steps passed');

  const sheet = path.join(directory, 'sheet.png');
  run(executable, [
    'contact-sheet',
    fixture('animated.svg'),
    '-o',
    sheet,
    '--duration',
    '2',
    '--fps',
    '4',
    '--count',
    '3',
    '--columns',
    '3',
    '--thumbnail-width',
    '96',
  ]);
  const sheetPixels = run('ffmpeg', [
    '-v',
    'error',
    '-i',
    sheet,
    '-f',
    'rawvideo',
    '-pix_fmt',
    'rgba',
    '-',
  ]);
  assert.deepEqual(pixel(sheetPixels, 4, 4, 288), red);
  assert.deepEqual(pixel(sheetPixels, 96 + 36, 4, 288), red);
  assert.deepEqual(pixel(sheetPixels, 192 + 60, 4, 288), red);
  const movie = path.join(directory, 'movie.mp4');
  run(executable, [
    'render',
    fixture('animated.svg'),
    '-o',
    movie,
    '--duration',
    '2',
    '--fps',
    '4',
    '--encoder',
    'software',
  ]);
  const probe = JSON.parse(
    run('ffprobe', ['-v', 'error', '-count_frames', '-show_streams', '-of', 'json', movie]),
  ).streams[0];
  assert.equal(Number(probe.nb_read_frames), 8);
  assert.equal(Number(probe.duration), 2);
  assert.equal(probe.width, 96);
  assert.equal(probe.height, 96);
  console.log('Animated SVG contact sheets and MP4 frame count/duration passed');

  const missing = path.join(directory, 'missing.svg');
  writeFileSync(
    missing,
    '<?xml-stylesheet type="text/css" href="missing.css"?><svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"/>',
  );
  const failure = path.join(directory, 'failure.png');
  const result = spawnSync(executable, ['frame', missing, '-o', failure], {
    encoding: 'utf8',
    timeout: 30000,
  });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /resources failed to load/);
  assert.equal(existsSync(failure), false);
  console.log('Missing animated SVG resources fail without a partial output');

  const failingModule = path.join(directory, 'failing.svg.mjs');
  writeFileSync(
    failingModule,
    `export const video={width:96,height:96,fps:4,duration:5}; export function render({frame}) { return (frame>=18?'<?xml-stylesheet type="text/css" href="missing.css"?>':'')+'<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="16" height="16" fill="red"><animate attributeName="x" from="0" to="64" dur="2s" fill="freeze"/></rect></svg>'; }`,
  );
  const existingMovie = path.join(directory, 'existing.mp4');
  writeFileSync(existingMovie, 'existing video');
  const failedRender = spawnSync(
    executable,
    ['render', failingModule, '-o', existingMovie, '--force', '--encoder', 'software'],
    { encoding: 'utf8', timeout: 30000 },
  );
  assert.equal(failedRender.status, 1);
  assert.match(failedRender.stderr, /resources failed to load/);
  assert.equal(readFileSync(existingMovie, 'utf8'), 'existing video');
  console.log('Browser SVG failure after earlier batches preserves the existing video');
} finally {
  rmSync(directory, { recursive: true, force: true });
}
