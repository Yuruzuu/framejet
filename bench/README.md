# Performance: v0.1.0 → v0.1.1

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

Use `--encoder software` when VideoToolbox is unavailable. `--workload three-orbit` (repeatable) selects individual benchmark cases. Choose a fresh results directory for each run. Keep other heavy work idle while timing; compare wall time and processing time separately.
