---
name: framejet
description: Render SVG or Three.js animations as MP4 videos with Framejet, and export PNG frames or contact sheets for review. Use for video exports from SVG/Three.js code, not ordinary static image creation or arbitrary web-page recording.
---

# Framejet

Use `framejet` for SVG and Three.js video exports. Run `framejet doctor` to check dependencies. Install the current source build for CSS/SMIL with `cargo install --git https://github.com/Yuruzuu/framejet --locked`. Node.js 20+, npm, and FFmpeg must be on PATH; `framejet setup` installs the runtime and Chromium. Native static SVG does not need Chromium; CSS/SMIL and Three.js do.

Create a small `.mjs` adapter next to existing animation assets, preserving the requested design and original source. Animation comes from explicit `time` in seconds, not timers or real-time playback. Frame/contact-sheet requests can sample times out of order. Await asset loads before rendering.

**SVG:** existing `.svg` files with CSS keyframes or timed SMIL render directly: `framejet render animation.svg --duration 4 -o video.mp4`. Auto mode samples CSS/SMIL in Chromium and keeps static SVG native. `--svg-renderer browser|native|auto` selects explicitly. For JavaScript-generated motion, name the adapter `animation.svg.mjs` and export metadata plus a function returning complete SVG markup at each time:

```js
export const video = { width: 1280, height: 720, fps: 30, duration: 3 };
export function render({ time, frame, width, height, fps, duration }) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">
    <circle cx="${100 + time * 150}" cy="360" r="50" fill="cyan"/>
  </svg>`;
}
```

**Three.js:** name it `animation.three.mjs`. Export the same `video` metadata and `setup({ THREE, renderer, width, height })`, returning `{ scene, camera, update({ time, frame }) }`. Build geometry once in `setup`; pose it in `update`. `setup` may be async. Browser imports support `three`, `three/addons/...`, and relative modules/assets inside the adapter's directory. Other packages need bundling. For postprocessing, return `render({ renderer, time, frame })` that draws to the canvas. See the [complete example](https://github.com/Yuruzuu/framejet/blob/v0.1.2/examples/orbit.three.mjs).

```sh
framejet frame animation.svg.mjs --time 1 -o frame.png
framejet contact-sheet animation.three.mjs --count 8 -o sheet.png
framejet render animation.three.mjs -o video.mp4 --encoder software --crf 18 --report render.json
```

For clean motion graphics, prefer `--encoder software --crf 18` for small, high-quality H.264 files. When smaller files are requested, start at `--crf 20`; inspect text, gradients, and motion before trying `22`. Dense/noisy 3D can produce larger software files, so compare sizes with the hardware output. Preserve resolution and frame rate unless asked otherwise. Compression changes are lossy; do not claim decoded pixel parity across quality settings.

Inspect useful frames/contact sheets before the final export. Both inputs support `--width`, `--height`, `--fps`, and `--duration`; MP4 needs even dimensions. Use `--font Brand.ttf` for SVG fonts. Existing files require explicit `--force`. Commands print JSON on stdout and diagnostics on stderr; report failures rather than treating a partial file as success. Deliver the actual output path.

Limits: CSS/SMIL export controls the SVG document's time-based timelines; interaction triggers, transition histories, scroll timelines, scripts, `foreignObject`, and independently animated images are unsupported. Browser SVG uses system fonts and CSS `@font-face`; `--font` is native-only. Three.js uses WebGL2 in Chromium, with Metal requested on macOS. `--software-rendering` forces SwiftShader; `--encoder software` selects x264 independently. MP4 is silent and opaque. TypeScript compilation, WebGPU scenes, audio, and arbitrary HTML recording are unsupported. Do not replace animation with a static snapshot or promise a speedup without a matched benchmark.
