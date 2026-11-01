# CSS/SMIL export speed

Measured on an Apple M4 Pro (14 logical CPUs, 24 GiB RAM), with the renderer code at `9dd6046` (`0.2.0-dev`), using full headless Chromium with CSS timeline freezing and explicit resource/paint synchronization. All exports use x264 fast at CRF 18, 30 fps, and four seconds of video: 120 frames. Times include the entire CLI process, Chromium/Node startup, frame capture, encoding, and shutdown.

Each case had one warmup and three measured runs. Case order alternated each round; jobs ran sequentially. These are warmed measurements on one normally used Mac, not cold-install timings or a guarantee for other scenes.

| CSS/SMIL example | Median export time | Whole-command throughput |
| --- | ---: | ---: |
| 960×540 | 8.754 s | 13.71 frames/s |
| 1920×1080 | 14.577 s | 8.23 frames/s |

This is the actual [CSS/SMIL example](../examples/css-smil.svg), including gradients, text, fading, SMIL rotation, and a progress bar. Browser SVG currently uses PNG screenshots. It provides declarative animation compatibility, and is significantly slower than native frame-by-frame SVG generation. Higher resolution raises capture, PNG compression, transfer, and decode work.

## Matched native/browser control

The simpler [animated SVG fixture](../tests/fixtures/animated.svg) was also compared with a [JavaScript generator](animated-native.svg.mjs) that explicitly computes the same four shapes and timing. Both were exported at 960×540, with the same 120-frame count and encoder settings.

| Engine | Median export time | Whole-command throughput |
| --- | ---: | ---: |
| Native SVG, generated states | 0.134 s | 898.77 frames/s |
| Browser SVG, CSS/SMIL | 6.687 s | 17.95 frames/s |

All 120 decoded output frames matched exactly between these two control videos. This is a small flat-shape fixture, not a prediction of native speed for a complex scene. Native SVG does not directly execute CSS/SMIL; it needs explicitly sampled markup. Existing native SVG and Three.js outputs remain on their existing paths.

Every timed output was checked with ffprobe for H.264, exact dimensions, 30 fps, 120 decoded frames, and a four-second duration. Full timings, input/binary hashes, and the control parity result are in [results/svg-animation-speed.json](results/svg-animation-speed.json).

Reproduce from a built and configured checkout:

```sh
node scripts/svg-speed.mjs target/release/framejet artifacts/svg-speed
```

Choose a fresh output directory. The script does not rebuild, install dependencies, or run concurrent exports while timing.
