import { createInterface } from 'node:readline';
import { once } from 'node:events';
import { format } from 'node:util';
import { readFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { createServer } from 'node:http';
import { randomBytes } from 'node:crypto';

// stdout is a binary protocol; animation logs belong on stderr.
for (const method of ['log', 'info', 'debug', 'warn', 'error']) {
  console[method] = (...values) => process.stderr.write(`${format(...values)}\n`);
}

async function packet(kind, value) {
  const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value);
  const header = Buffer.alloc(5);
  header[0] = kind;
  header.writeUInt32LE(bytes.length, 1);
  if (!process.stdout.write(header)) await once(process.stdout, 'drain');
  if (!process.stdout.write(bytes)) await once(process.stdout, 'drain');
}

function metadata(video, overrides) {
  const result = { width: 1280, height: 720, fps: 30, duration: 3, ...video };
  for (const key of ['width', 'height', 'fps', 'duration']) {
    if (overrides[key] !== null && overrides[key] !== undefined) result[key] = overrides[key];
  }
  if (!Number.isInteger(result.width) || !Number.isInteger(result.height)
    || result.width < 1 || result.height < 1 || result.width * result.height > 33554432
    || !Number.isFinite(result.fps) || result.fps <= 0 || result.fps > 240
    || !Number.isFinite(result.duration) || result.duration <= 0
    || Math.ceil(result.duration * result.fps) > 10000000) {
    throw new Error('Invalid video metadata: positive dimensions (at most 32 megapixels), fps <= 240, positive duration, at most 10 million frames required');
  }
  return result;
}

let browser;
let server;
let streamServer;
let session;
let render;
let dispose;

async function close() {
  await dispose?.();
  await browser?.close();
  if (streamServer) {
    for (const client of streamServer.clients) client.terminate();
    await new Promise(resolve => streamServer.close(resolve));
  }
  server?.closeAllConnections();
  if (server) await new Promise(resolve => server.close(resolve));
}

async function startThree(configuration) {
  let chromium;
  let WebSocketServer;
  try {
    ({ chromium } = await import('playwright'));
    ({ WebSocketServer } = await import('ws'));
  }
  catch { throw new Error('Three.js runtime is missing. Run: framejet setup'); }
  const project = path.dirname(configuration.input);
  const dependencyRoot = fileURLToPath(new URL('./node_modules/', import.meta.url));
  const mime = { '.mjs': 'text/javascript', '.js': 'text/javascript', '.json': 'application/json',
    '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.svg': 'image/svg+xml',
    '.glb': 'model/gltf-binary', '.gltf': 'model/gltf+json', '.bin': 'application/octet-stream',
    '.woff2': 'font/woff2', '.hdr': 'application/octet-stream' };
  server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, 'http://localhost');
      if (url.pathname === '/') {
        response.setHeader('Content-Type', 'text/html');
        response.end(`<base href="/project/"><script type="importmap">{"imports":{"three":"/dependencies/three/build/three.module.js","three/addons/":"/dependencies/three/examples/jsm/"}}</script>`);
        return;
      }
      const dependencies = url.pathname.startsWith('/dependencies/');
      const prefix = dependencies ? '/dependencies/' : '/project/';
      if (!url.pathname.startsWith(prefix)) { response.writeHead(404).end(); return; }
      const root = await realpath(dependencies ? dependencyRoot : project);
      const requested = path.resolve(root, decodeURIComponent(url.pathname.slice(prefix.length)));
      const resolved = await realpath(requested);
      if (!resolved.startsWith(`${root}${path.sep}`)) { response.writeHead(403).end(); return; }
      response.setHeader('Content-Type', mime[path.extname(resolved)] || 'application/octet-stream');
      response.end(await readFile(resolved));
    } catch (error) {
      console.error(error.message);
      if (!response.headersSent) response.writeHead(500);
      response.end('Framejet could not serve this resource');
    }
  });
  const streamPath = `/frames/${randomBytes(24).toString('hex')}`;
  streamServer = new WebSocketServer({ server, path: streamPath, maxPayload: 134217728, perMessageDeflate: false });
  streamServer.on('connection', socket => {
    socket.on('error', error => console.error(`Frame stream: ${error.message}`));
    socket.on('message', async (bytes, binary) => {
      try {
        if (!binary) throw new Error('Expected a binary frame');
        await packet(1, bytes);
        socket.send('ok');
      } catch (error) { socket.send(`error: ${error.message}`); }
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const arguments_ = ['--enable-webgl', '--ignore-gpu-blocklist'];
  if (configuration.softwareRendering) arguments_.push('--use-angle=swiftshader');
  else if (process.platform === 'darwin') arguments_.push('--use-angle=metal');
  browser = await chromium.launch({ headless: true, args: arguments_ });
  const page = await browser.newPage();
  page.on('console', message => console.error(`[browser] ${message.text()}`));
  page.on('pageerror', error => console.error(`[browser] ${error.message}`));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const info = await page.evaluate(async ({ input, overrides, streamUrl }) => {
    const animation = await import(`/project/${encodeURIComponent(input)}`);
    const video = { width: 1280, height: 720, fps: 30, duration: 3, ...animation.video };
    for (const key of ['width', 'height', 'fps', 'duration']) {
      if (overrides[key] != null) video[key] = overrides[key];
    }
    // Validate before allocating a canvas or GPU buffers.
    if (!Number.isInteger(video.width) || !Number.isInteger(video.height)
      || video.width < 1 || video.height < 1 || video.width * video.height > 33554432
      || !Number.isFinite(video.fps) || video.fps <= 0 || video.fps > 240
      || !Number.isFinite(video.duration) || video.duration <= 0
      || Math.ceil(video.duration * video.fps) > 10000000) throw new Error('Invalid video metadata');
    if (typeof animation.setup !== 'function') throw new Error('Export setup({ THREE, renderer, width, height }) from your Three.js module');
    const THREE = await import('three');
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, preserveDrawingBuffer: true });
    renderer.debug.onShaderError = (context, program, vertex, fragment) => {
      throw new Error(`Shader compilation failed: ${context.getProgramInfoLog(program)} ${context.getShaderInfoLog(vertex)} ${context.getShaderInfoLog(fragment)}`);
    };
    renderer.setPixelRatio(1);
    renderer.setSize(video.width, video.height);
    renderer.setClearColor(0x000000, 1);
    document.body.append(renderer.domElement);
    const state = await animation.setup({ THREE, renderer, ...video });
    if (!state?.scene || !state?.camera) throw new Error('setup() must return { scene, camera, update?, render?, dispose? }');
    await renderer.compileAsync(state.scene, state.camera);
    const context = renderer.getContext();
    const extension = context.getExtension('WEBGL_debug_renderer_info');
    const gpu = extension ? context.getParameter(extension.UNMASKED_RENDERER_WEBGL) : context.getParameter(context.RENDERER);
    const socket = new WebSocket(streamUrl);
    await new Promise((resolve, reject) => {
      socket.onopen = resolve;
      socket.onerror = () => reject(new Error('Cannot connect to frame stream'));
    });
    window.framejet = { renderer, state, video, context, socket };
    return { ...video, gpu };
  }, { input: path.basename(configuration.input), overrides: configuration.overrides,
    streamUrl: `ws://127.0.0.1:${server.address().port}${streamPath}` });
  if (/swiftshader|llvmpipe|softpipe|software/i.test(info.gpu)) {
    console.warn(`Three.js is using software rendering: ${info.gpu}`);
  }
  render = async samples => {
    await page.evaluate(async samples => {
      const { renderer, state, video, context, socket } = window.framejet;
      const pixels = new Uint8Array(video.width * video.height * 4);
      const flipped = new Uint8Array(pixels.length);
      const stride = video.width * 4;
      for (const sample of samples) {
        try {
          await state.update?.({ ...sample, ...video });
          if (state.render) await state.render({ renderer, ...sample, ...video });
          else renderer.render(state.scene, state.camera);
          renderer.setRenderTarget(null);
          context.readPixels(0, 0, video.width, video.height, context.RGBA, context.UNSIGNED_BYTE, pixels);
          if (context.isContextLost()) throw new Error('WebGL context was lost');
          const error = context.getError();
          if (error !== context.NO_ERROR) throw new Error(`WebGL readback failed: ${error}`);
          for (let row = 0; row < video.height; row++) {
            flipped.set(pixels.subarray(row * stride, (row + 1) * stride), (video.height - 1 - row) * stride);
          }
          await new Promise((resolve, reject) => {
            const cleanup = () => { socket.onmessage = null; socket.onerror = null; socket.onclose = null; };
            socket.onmessage = event => {
              cleanup();
              if (event.data === 'ok') resolve();
              else reject(new Error(`Frame transfer failed: ${event.data}`));
            };
            socket.onerror = socket.onclose = () => { cleanup(); reject(new Error('Frame stream disconnected')); };
            socket.send(flipped);
          });
        } catch (error) { throw new Error(`Frame ${sample.frame} at ${sample.time}s: ${error.message}`); }
      }
    }, samples);
  };
  dispose = async () => {
    await page.evaluate(async () => {
      await window.framejet?.state.dispose?.();
      window.framejet?.socket.close();
      window.framejet?.renderer.dispose();
    }).catch(() => {});
  };
  return metadata(info, {});
}

async function main() {
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    const command = JSON.parse(line);
    if (!session) {
      if (command.backend === 'three') session = await startThree(command);
      else {
        const animation = await import(pathToFileURL(command.input).href);
        session = metadata(animation.video, command.overrides);
        if (typeof animation.render !== 'function') throw new Error('Export render({ time, frame, width, height, fps, duration }) returning an SVG string');
        render = async samples => {
          for (const sample of samples) {
            try {
              const svg = await animation.render({ ...session, ...sample });
              if (typeof svg !== 'string') throw new Error('render() must return an SVG string');
              await packet(1, svg);
            } catch (error) { throw new Error(`Frame ${sample.frame} at ${sample.time}s: ${error.message}`); }
          }
        };
        dispose = animation.dispose;
      }
      await packet(0, JSON.stringify(session));
    } else if (command.command === 'frames') await render(command.samples);
    else if (command.command === 'close') break;
    else throw new Error(`Unknown command: ${command.command}`);
  }
}

process.on('SIGTERM', () => { close().finally(() => process.exit(143)); });
process.on('SIGINT', () => { close().finally(() => process.exit(130)); });
try { await main(); }
catch (error) { await packet(2, error.stack || error.message); process.exitCode = 1; }
finally { await close(); }
