# Performance: v0.1.1 → v0.1.2

Median **whole-command wall times** on an Apple M4 Pro (14 logical CPUs, 24 GiB RAM), Node 24.13.1, FFmpeg 9.0.2. Each workload had one warmup per binary and five paired repetitions alternating order, against the frozen v0.1.1 binary. An identical-binary control differed by at most 0.4%. The machine was in normal use, so runs waited for a quiet CPU first.

| Workload | Frames / resolution | VideoToolbox: v0.1.1 → v0.1.2 | Speedup | x264: v0.1.1 → v0.1.2 | Speedup |
| --- | --- | ---: | ---: | ---: | ---: |
| SVG titles | 180 / 1920×1080 | 1.124 → 1.079 s | 1.04× | 0.873 → 0.477 s | 1.83× |
| SVG titles 4K | 90 / 3840×2160 | 2.156 → 1.812 s | 1.19× | 2.093 → 1.001 s | 2.09× |
| SVG particles | 120 / 1280×720 | 0.620 → 0.553 s | 1.12× | 0.454 → 0.347 s | 1.31× |
| Static SVG | 180 / 1920×1080 | 1.078 → 1.055 s | 1.02× | 0.484 → 0.338 s | 1.43× |
| Three.js orbit | 180 / 1280×720 | 2.069 → 0.905 s | 2.29× | 1.966 → 0.793 s | 2.48× |
| Three.js heavy | 120 / 1920×1080 | 3.033 → 1.108 s | 2.74× | 3.096 → 1.469 s | 2.11× |

Full samples, parity results and rejected candidates are in [results/m4-pro-v0.1.2.json](results/m4-pro-v0.1.2.json).

## What limits speed now

VideoToolbox has one encode engine on this machine: about 216 fps at 1080p in total, however many sessions run. SVG on VideoToolbox barely improves because it was already at that limit. `--encoder auto` still chooses VideoToolbox on macOS, because x264 is not faster everywhere:

| Encoder-only test | VideoToolbox 12 Mbps | x264 fast, CRF 18 |
| --- | ---: | ---: |
| SVG titles (time, size, SSIM) | 0.93 s, 0.93 MB, 0.9986 | 0.39 s, 0.15 MB, 0.9986 |
| SVG particles | 0.42 s, 5.99 MB, 0.9963 | 0.25 s, 1.34 MB, 0.9942 |
| Three.js orbit | 0.54 s, 3.79 MB, 0.9986 | 0.29 s, 0.57 MB, 0.9976 |
| Worst-case noise, 4K, 60 frames | 1.20 s, 5.17 MB, 0.337 | 6.93 s, 186 MB, 0.640 |

For clean motion graphics, `--encoder software` is the fastest choice and produces far smaller files, with slightly lower PSNR on particles and orbit. For heavy or noisy content, VideoToolbox is faster.

## What changed

1. **Direct Three.js stream.** Chromium sends pixels straight to a token-protected local WebSocket in the CLI. Previously Node unmasked every frame in JavaScript and re-piped it. This removes the `ws` dependency.
2. **Frames in flight.** Three transfer buffers and fenced GPU readback let the page prepare the next frame while the GPU and transfer finish earlier ones. Rows are flipped in Rust, off the page's main thread.
3. **Encoder pipeline.** A writer thread and one batch of lookahead run generation, rendering and encoding concurrently, with recycled frame buffers.
4. **Socket pair to FFmpeg.** Large socket buffers avoid a context switch per 64 KiB pipe chunk on Unix.
5. **Shared font mappings.** fontdb otherwise opens and maps the font file for every glyph, about 60% of active samples in title rendering. System fonts also load in the background, and SVGs without text do not wait for them.
6. **Larger batches at high resolution.** The batch budget rose from 64 to 256 MiB; 4K previously rendered one frame at a time. Peak frame memory can be higher at 4K and above.
7. **Startup and exit.** The hardware probe runs during runtime startup, the browser driver's bytecode is cached, the runtime depends on `playwright-core` alone, and Node skips its teardown.
8. **Build profile.** Fat LTO, one codegen unit and mimalloc.

## Output and failure checks

Against v0.1.1: 28 lossless PNG samples and all 168 decoded frames from matched x264 videos match exactly, across SVG text and gradients, particles, both Three.js scenes, software rendering, custom render callbacks and letterboxed static SVG. Frames at an odd height also match. Ten tests pass. Both backend smoke checks pass, including failures after encoded batches: nonzero exit, no partial MP4 and an existing destination preserved. VideoToolbox files are not claimed to be byte-identical. Linux CI passed; it checks correctness, not equivalent speedups. The non-Unix pipe fallback has not been compiled. Timings apply to this Mac and these workloads.

Upgrading requires `framejet setup`, because runtime dependencies changed.

# Earlier: v0.1.0 → v0.1.1

Measured on an Apple M4 Pro, 14 logical CPUs, 24 GiB RAM, Node 24.13.1, and FFmpeg 9.0.2. Three.js used ANGLE Metal; all timed exports used VideoToolbox H.264 at the same 12 Mbps setting.

These are median **whole-command wall times**, including startup, encoding, and process cleanup. Each workload had one warmup per binary, followed by five paired repetitions alternating baseline/candidate order. Dimensions, frame times, frame count, codec, bitrate, and source files were held constant. An identical-binary control differed by roughly 0–1.5%.

| Workload | Frames / resolution | v0.1.0 | v0.1.1 | Less time | Speedup |
| --- | --- | ---: | ---: | ---: | ---: |
| SVG titles | 180 / 1920×1080 | 1.669 s | 1.151 s | 31.1% | 1.45× |
| SVG particles | 120 / 1280×720 | 1.174 s | 0.637 s | 45.8% | 1.84× |
| Three.js orbit | 180 / 1280×720 | 4.828 s | 2.120 s | 56.1% | 2.28× |

All sample times, warmups, binary/source hashes, machine details, experiment summaries, and parity results are in [results/m4-pro-v0.1.1.json](results/m4-pro-v0.1.1.json). The baseline is commit `46108ba`; the measured candidate code is commit `655297a`.

## What changed

1. **Normal runtime shutdown.** Rust now closes the JavaScript runtime's stdin after sending its close command. Previously, the open input pipe kept Node alive until a half-second forced-kill deadline. This is a fixed per-job saving for module inputs, not faster SVG rasterization. Static `.svg` inputs do not start Node and do not gain from this change. The percentage benefit shrinks on longer jobs.
2. **Three.js frame transport.** A shared worker owns the binary WebSocket and lends one transferable pixel buffer to the page at a time. The page's Playwright DevTools network session previously mirrored each multi-megabyte frame as a second base64 payload. The new path avoids that duplicate traffic, JSON parsing, and associated allocations. Drawing, sampling, row orientation, and encoder settings are unchanged. The bounded one-frame acknowledgement protocol is preserved.

Median SVG processing times were essentially unchanged (titles: 1.044 → 1.067 s; particles: 0.557 → 0.549 s). Three.js processing time fell from 3.885 to 1.720 s. These processing figures include generation, transfer, and encoding; they do not isolate GPU drawing time.

Three other candidates were screened and removed: consuming Three.js frames earlier, generating batches on an extra thread, and reusing the validated SVG XML tree. Their measured gains were approximately 0–2%, too small or inconsistent to justify retaining the changes. The experiment results remain in the JSON ledger.

## Output and failure checks

Against the frozen original binary:

- 24 lossless PNG samples matched exactly, including arbitrary times and animation endpoints.
- All 144 decoded frames from matched x264 videos matched exactly.
- Cases cover SVG text/gradients, particles, Three.js geometry, local textures, software rendering, custom render callbacks, and static SVG letterboxing.
- Nine integration tests and both backend smoke checks passed. Failure checks throw after earlier batches have encoded and verify that failed jobs publish no partial MP4 and preserve an existing destination.

Pixel parity was checked using PNGs and software-encoded videos. No claim is made that separately encoded hardware MP4 files are byte-identical. Timings apply to these workloads and this Mac; Linux CI checks correctness, not equivalent speedups. Cold installation and first-time browser downloads are excluded.

## Reproduce

Build both revisions into separate locations, and install their runtimes before measuring:

```sh
cargo install --git https://github.com/Yuruzuu/framejet --tag v0.1.0 --locked --root /tmp/framejet-baseline
cargo build --release --locked
/tmp/framejet-baseline/bin/framejet setup
./target/release/framejet setup

node scripts/bench.mjs /tmp/framejet-baseline/bin/framejet target/release/framejet \
  --rounds 5 --encoder videotoolbox --output artifacts/comparison
node scripts/parity.mjs /tmp/framejet-baseline/bin/framejet target/release/framejet artifacts/parity-check
```

Use `--encoder software` when VideoToolbox is unavailable. `--workload three-orbit` (repeatable) selects individual benchmark cases. `--encoder auto` skips the encoder check. Choose a fresh results directory for each run. Keep other heavy work idle while timing; compare wall time and processing time separately.
