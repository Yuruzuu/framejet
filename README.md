# Framejet

A Rust CLI that turns SVG and Three.js animations into MP4 videos, PNG frames, and contact sheets.

- **SVG:** JavaScript calculates each frame's SVG; Rust rasterizes it in parallel with [resvg](https://github.com/linebender/resvg). Static `.svg` inputs work too.
- **Three.js:** one persistent Chromium session loads the scene once. Fenced GPU readback and a binary WebSocket in a shared worker send raw pixels directly to the CLI, with a few frames in flight, avoiding PNG capture and Node in the data path.
- **Video:** frames stream to FFmpeg with bounded batches. On macOS, `auto` probes VideoToolbox hardware encoding and uses x264 if unavailable. A dedicated writer thread keeps FFmpeg fed while the next batch renders.
- **Agents:** exact frame times, JSON results on stdout, progress/errors on stderr, and review images before a full export.

Inspired by [FFrames](https://github.com/dmtrKovalenko/fframes). Framejet uses its own JavaScript animation adapters; it does not import FFrames projects or replace Three.js with a native 3D engine.

See [benchmarks and parity checks](bench/README.md) for measured changes against v0.1.0 and commands to reproduce them.

## Install

Requires **Node.js 20+**, **npm**, and **FFmpeg** on `PATH`. Static SVG → PNG needs neither Node nor FFmpeg. Building from source also requires Rust.

The [GitHub release](https://github.com/Yuruzuu/framejet/releases/latest) also provides a macOS Apple Silicon binary: unpack it and place `framejet` on your `PATH`, then run `framejet setup`.

```sh
cargo install --git https://github.com/Yuruzuu/framejet --locked
framejet setup
framejet doctor
```

`setup` installs pinned Three.js/Playwright packages and Chromium into user caches, outside your animation project. Use `framejet setup --skip-browser` to install packages without downloading Chromium. On Linux, Playwright may also need system browser libraries: follow its installation error or run the matching Playwright CLI with `install --with-deps chromium`.

For development:

```sh
git clone https://github.com/Yuruzuu/framejet
cd framejet
cargo build --release --locked
./target/release/framejet setup
./target/release/framejet render examples/hello.svg.mjs -o hello.mp4
./target/release/framejet render examples/orbit.three.mjs -o orbit.mp4
```

## Use

```sh
framejet frame animation.svg.mjs --time 1.5 -o frame.png
framejet contact-sheet animation.three.mjs --count 8 -o sheet.png
framejet render animation.three.mjs -o video.mp4 --report render.json
```

All three commands accept `--width`, `--height`, `--fps`, and `--duration` overrides. Use even dimensions for MP4. `--duration` changes the total animation duration, not playback speed. Samples occur at `frame / fps`; video contains `ceil(duration * fps)` frames. A frame export can also sample the exact endpoint.

Other options:

- `--backend svg|three` overrides filename detection. `*.three.mjs` selects Three.js; other `.mjs` files select SVG.
- `--font ./Brand.ttf` loads an additional SVG font; repeat for multiple fonts.
- `--threads 4` controls native SVG rendering parallelism.
- `--software-rendering` forces Three.js to use SwiftShader. macOS requests Metal by default; other systems use Chromium's default backend.
- `render --encoder software --crf 18` selects x264. `--encoder videotoolbox --bitrate 12` requires Apple's hardware encoder and sets Mbps. Hardware/software quality settings are different. VideoToolbox has one encode engine, so it is often not the fastest choice: x264 is usually faster and much smaller for clean motion graphics, while VideoToolbox stays faster for heavy or noisy content. See [the benchmarks](bench/README.md).
- `--timeout 120` limits the wait for each JavaScript runtime response.
- `--force` explicitly replaces an existing output or report. A failed render leaves an existing output intact.

Files are finalized only after successful rendering/encoding. JSON reports include input/output paths, dimensions, frame count, actual output duration, encoder, startup time, processing time, throughput, and the reported GPU for Three.js. Frame and contact-sheet reports also include sample times. Processing time includes frame generation, transfer, encoding, and encoder finalization; it is not isolated GPU time. Contact-sheet tiles run left to right, top to bottom, from the first to the last video frame.

### Smaller video files

For clean motion graphics, use quality-based software encoding:

```sh
framejet render animation.svg.mjs -o video.mp4 --encoder software --crf 18
```

When file size matters more, try `--crf 20`, then review before increasing to `22`. Higher CRF trades detail for fewer bytes. Keep the authored resolution and frame rate. Dense or noisy 3D can still be larger with software encoding; compare against the hardware output rather than assuming one setting always wins. See the [measured file sizes and quality checks](bench/encoding-size.md).

## SVG animation API

Create `animation.svg.mjs`:

```js
export const video = { width: 1280, height: 720, fps: 30, duration: 3 };

export function render({ time, frame, width, height, fps, duration }) {
  const x = 100 + time / duration * (width - 200);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">
    <rect width="100%" height="100%" fill="#101827"/>
    <circle cx="${x}" cy="${height / 2}" r="50" fill="#5cf0cb"/>
  </svg>`;
}
```

`render` may be async and must return a complete, static SVG string for that time. Calculate motion from the supplied `time`; avoid wall-clock time, unseeded randomness, and incremental state. Imports use normal Node ESM resolution. Load large assets once at module scope. Optional `dispose()` runs when the session closes.

Relative SVG image references resolve from the animation file's directory. Embed remote images or download them locally first. Use local/system fonts; `--font` provides explicit font files. SVG scales proportionally and centers within overridden dimensions. The canvas is opaque black unless your SVG paints a background.

See [examples/hello.svg.mjs](examples/hello.svg.mjs).

## Three.js animation API

Create `animation.three.mjs`:

```js
export const video = { width: 1280, height: 720, fps: 30, duration: 3 };

export function setup({ THREE, renderer, width, height }) {
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(45, width / height, 0.1, 100);
  camera.position.z = 4;
  const cube = new THREE.Mesh(
    new THREE.BoxGeometry(),
    new THREE.MeshNormalMaterial(),
  );
  scene.add(cube);

  return {
    scene,
    camera,
    update({ time }) {
      cube.rotation.set(time * 0.4, time, 0);
    },
  };
}
```

`setup` runs once and may be async. Await texture/model loads before returning. It receives the pinned `THREE` module, the renderer, and video metadata. It must return `scene` and `camera`. Optional `update({ time, frame, width, height, fps, duration })` runs before each sampled frame. Optional `dispose()` releases application resources.

For postprocessing, return `render({ renderer, time, frame, ...video })` to draw the final image to the renderer's canvas instead of the default `renderer.render(scene, camera)` call. The canvas is opaque; the tool controls its pixel size. Avoid `requestAnimationFrame`, real-time timers, and frame-to-frame simulation assumptions: frame and contact-sheet commands can sample arbitrary times.

Browser ESM imports `three`, `three/addons/...`, and relative local modules are supported. Other bare imports need bundling first. Assets and relative module imports must stay inside the animation's directory; a short-lived HTTP server serves only that directory and runtime dependencies on loopback. Relative asset URLs resolve from that directory. Sources are executable code: use animations you trust.

See [examples/orbit.three.mjs](examples/orbit.three.mjs).

## Version-one limits

- Native SVG is CPU rendered. Static markup is parsed for every generated frame; it does not have FFrames' compile-time subtree caching or Skia GPU backend.
- Existing HTML/CSS/SMIL animations need an adapter that samples their state. Embedded SVG animation, scripts, and `foreignObject` are rejected.
- Three.js uses WebGL2, synchronous pixel readback, and a local browser. Actual GPU/software rendering depends on Chromium and the machine; the JSON result records its renderer string.
- MP4 is silent H.264 with an opaque background. Audio, alpha video, TypeScript compilation, arbitrary web-page recording, and WebGPU scenes are outside this release.
- First-time installation and browser startup can dominate short jobs. No speedup over another exporter is guaranteed; benchmark the same scene, dimensions, frame rate, and quality.

## Claude and Codex skill

The shared [Framejet skill](skills/framejet/SKILL.md) contains the adapter contract and export workflow. From this checkout (or the unpacked release), install it for both agents:

```sh
mkdir -p ~/.codex/skills/framejet ~/.claude/skills/framejet
cp skills/framejet/SKILL.md ~/.codex/skills/framejet/SKILL.md
cp skills/framejet/SKILL.md ~/.claude/skills/framejet/SKILL.md
```

Start a new agent session to discover the skill, then ask it to render an SVG or Three.js animation as a video.

## Checks

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
./target/release/framejet setup
node scripts/smoke.mjs
```

The smoke test requires `ffprobe` as well as FFmpeg. It checks both backends' H.264 output, exact frame count and duration, visible motion, raw PNG colors/orientation, and cleanup after a partial render failure.

MIT licensed. Runtime dependencies retain their own licenses.
