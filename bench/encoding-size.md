# Smaller rendered videos

The v0.1.2 speed changes did not increase file sizes in the six matched workloads retained from the v0.1.1 comparison: all six had equal file sizes with VideoToolbox and with x264. The x264 files were also byte-identical. Hardware files are not claimed to be byte-identical.

For clean motion graphics, existing Framejet options already produce much smaller files:

```sh
framejet render animation.svg.mjs -o video.mp4 --encoder software --crf 18
```

When smaller output is requested, start at CRF 20 and inspect the result. CRF 22 compresses more aggressively. Keep resolution and frame rate unchanged unless those changes are explicitly requested. Dense/noisy 3D needs a comparison: software CRF 18 can be larger than the current hardware output because it preserves more detail.

## Measured output

All clips below are 3 seconds at 30 fps, generated directly by the released Framejet 0.1.2 binary on an Apple M4 Pro. The first three use 1280×720; the heavy scene uses 1920×1080. MB is decimal. The baseline is VideoToolbox at its existing 12 Mbps target. These are compression changes, so decoded pixel parity is not expected.

| Scene | Baseline size | Smaller setting | Smaller size | Reduction | SSIM: baseline → smaller |
| --- | ---: | --- | ---: | ---: | ---: |
| SVG titles | 0.431 MB | x264 fast, CRF 20 | 0.077 MB | 82.2% | 0.999465 → 0.998986 |
| SVG particles | 4.553 MB | x264 fast, CRF 20 | 0.779 MB | 82.9% | 0.999280 → 0.995727 |
| Three.js orbit | 1.294 MB | x264 fast, CRF 20 | 0.307 MB | 76.3% | 0.999458 → 0.998285 |
| Dense Three.js | 4.669 MB | x264 fast, CRF 22 | 4.063 MB | 13.0% | 0.986575 → 0.986882 |

SSIM compares every decoded frame with a CRF 0 reference after the existing RGB-to-YUV420 conversion. It measures similarity, not a percentage of perceived quality. The dense 3D result has similar aggregate SSIM at CRF 22, but that does not guarantee indistinguishable quality for other scenes. Text and geometry detail crops were also visually inspected. The lossless source rendering and the encoded video's quality are separate concerns.

The ordinary software CRF 18 setting is a conservative starting point: title, particle, and orbit files measured 0.088, 1.030, and 0.373 MB respectively. At 1080p, dense 3D grew to 6.198 MB at CRF 18, versus 4.669 MB on hardware. CRF 20 was still slightly larger for that scene; CRF 22 reduced size while achieving similar SSIM to its hardware baseline.

Medium and slow x264 presets were screened too. They generally saved only a few additional percent at the tested quality settings and took longer to encode, so the existing fast preset remains the recommendation. Halving hardware bitrate to 6 Mbps reduced the dense scene to 2.348 MB but lowered its SSIM to 0.970048, a substantially greater quality tradeoff.

The direct runs took approximately 0.18–0.61 seconds for the three clean scenes at CRF 20, versus 0.50–0.73 seconds on hardware. Dense 3D at CRF 22 took 1.28 seconds versus 0.97 seconds. These are single-run timings, not repeated performance benchmarks; they show the size/speed tradeoff rather than a guaranteed speedup.

Full per-frame minima, all tested options, and binary/source provenance are in [results/encoding-size-v0.1.2.json](results/encoding-size-v0.1.2.json).

## Reproduce

The script requires FFmpeg with x264, VideoToolbox, and the SSIM filter, plus a built and configured Framejet runtime. On the tested Mac:

```sh
node scripts/encoding-size.mjs artifacts/size-check --direct \
  --workload svg-titles --workload svg-particles \
  --workload three-orbit --workload three-heavy-1080p
```

Omit `--direct` to screen additional medium/slow preset options by encoding the same reference video. Those timings include reference decoding and exclude rendering. Choose a fresh output directory each time. This workflow does not recompress an already-lossy user export: final exports should render again from their animation source using the selected settings.
