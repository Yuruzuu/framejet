use crate::{
    cli::{Backend, SourceArgs},
    runtime::Worker,
    svg::SvgRenderer,
};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Deserialize, Serialize)]
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
}

impl Video {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0
                && self.height > 0
                && u64::from(self.width) * u64::from(self.height) <= 33_554_432,
            "Dimensions must be positive and at most 32 megapixels"
        );
        ensure!(
            self.fps.is_finite() && self.fps > 0.0 && self.fps <= 240.0,
            "FPS must be greater than zero and at most 240"
        );
        ensure!(
            self.duration.is_finite() && self.duration > 0.0,
            "Duration must be positive and finite"
        );
        ensure!(
            self.frames() <= 10_000_000,
            "Animation exceeds 10 million frames"
        );
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        (self.fps * self.duration).ceil() as u64
    }
    pub fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
}

#[derive(Clone, Copy, Serialize)]
pub struct Sample {
    pub time: f64,
    pub frame: u64,
}

pub struct Source {
    pub video: Video,
    pub backend: &'static str,
    pub threads: usize,
    worker: Option<Worker>,
    svg: Option<SvgRenderer>,
    static_pixels: Option<Vec<u8>>,
    pool: rayon::ThreadPool,
    cancelled: Arc<AtomicBool>,
}

impl Source {
    pub fn open(arguments: &SourceArgs, cancelled: Arc<AtomicBool>) -> Result<Self> {
        let input = fs::canonicalize(&arguments.input)
            .with_context(|| format!("Cannot open {}", arguments.input.display()))?;
        let extension = input
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        ensure!(
            matches!(extension, "mjs" | "svg"),
            "Input must be a .mjs animation module or a static .svg file"
        );
        let backend = match arguments.backend {
            Backend::Auto if input.to_string_lossy().ends_with(".three.mjs") => "three",
            Backend::Three => "three",
            _ => "svg",
        };
        ensure!(
            extension != "svg" || backend == "svg",
            "A .svg file requires the SVG backend"
        );
        let threads = arguments
            .threads
            .map(|value| value as usize)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(usize::from)
                    .unwrap_or(1)
                    .min(8)
            });
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?;
        let svg = if backend == "svg" {
            Some(SvgRenderer::new(&input, &arguments.font)?)
        } else {
            None
        };
        let mut worker = None;
        let mut static_pixels = None;
        let video = if extension == "svg" {
            let markup = fs::read_to_string(&input)?;
            let renderer = svg.as_ref().context("SVG renderer is missing")?;
            let (width, height) = renderer.dimensions(&markup)?;
            let video = Video {
                width: arguments.width.unwrap_or(width),
                height: arguments.height.unwrap_or(height),
                fps: arguments.fps.unwrap_or(30.0),
                duration: arguments.duration.unwrap_or(3.0),
                gpu: None,
            };
            video.validate()?;
            static_pixels = Some(renderer.render(&markup, video.width, video.height)?);
            video
        } else {
            let configuration = serde_json::json!({"input":input,"backend":backend,"softwareRendering":arguments.software_rendering,"overrides":{
                "width":arguments.width,"height":arguments.height,"fps":arguments.fps,"duration":arguments.duration
            }});
            let mut runtime = Worker::start(&configuration, arguments.timeout, cancelled.clone())?;
            let video: Video = serde_json::from_slice(&runtime.receive()?)
                .context("Invalid animation metadata")?;
            worker = Some(runtime);
            video
        };
        video.validate()?;
        Ok(Self {
            video,
            backend,
            threads,
            worker,
            svg,
            static_pixels,
            pool,
            cancelled,
        })
    }

    pub fn batch_size(&self) -> usize {
        (64 * 1024 * 1024 / self.video.frame_bytes()).clamp(1, self.threads * 2)
    }

    pub fn frames(&mut self, samples: &[Sample]) -> Result<Vec<Vec<u8>>> {
        ensure!(!self.cancelled.load(Ordering::Relaxed), "Cancelled");
        if let Some(pixels) = &self.static_pixels {
            return Ok(samples.iter().map(|_| pixels.clone()).collect());
        }
        let worker = self
            .worker
            .as_mut()
            .context("Animation runtime is missing")?;
        worker.send(&serde_json::json!({"command":"frames","samples":samples}))?;
        let bytes = samples
            .iter()
            .map(|_| worker.receive())
            .collect::<Result<Vec<_>>>()?;
        if let Some(renderer) = &self.svg {
            self.pool.install(|| {
                bytes
                    .into_par_iter()
                    .zip(samples.par_iter())
                    .map(|(bytes, sample)| {
                        let markup = String::from_utf8(bytes).context("SVG is not valid UTF-8")?;
                        renderer
                            .render(&markup, self.video.width, self.video.height)
                            .with_context(|| format!("Frame {} at {}s", sample.frame, sample.time))
                    })
                    .collect()
            })
        } else {
            for (pixels, sample) in bytes.iter().zip(samples) {
                ensure!(
                    pixels.len() == self.video.frame_bytes(),
                    "Frame {} has {} bytes; expected {}",
                    sample.frame,
                    pixels.len(),
                    self.video.frame_bytes()
                );
            }
            Ok(bytes)
        }
    }
}
