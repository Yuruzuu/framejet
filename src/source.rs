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
    ops::Deref,
    sync::{
        Arc, Mutex,
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

/// WebGL rows run bottom-up; video and PNG rows run top-down.
fn flip_rows(pixels: &mut [u8], stride: usize) {
    let middle = pixels.len() / stride / 2 * stride;
    let (top, bottom) = pixels.split_at_mut(middle);
    // With an odd height, `zip` stops before the middle row, which stays in place.
    top.chunks_exact_mut(stride)
        .zip(bottom.chunks_exact_mut(stride).rev())
        .for_each(|(upper, lower)| upper.swap_with_slice(lower));
}

/// Rendered RGBA pixels: owned by one frame, or shared by every static frame.
pub enum Frame {
    Owned(Vec<u8>),
    Shared(Arc<Vec<u8>>),
}

impl Deref for Frame {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Self::Owned(pixels) => pixels,
            Self::Shared(pixels) => pixels,
        }
    }
}

/// Recycles frame allocations, avoiding fresh zeroed pages for every frame.
#[derive(Clone, Default)]
pub struct FramePool(Arc<Mutex<Vec<Vec<u8>>>>);

impl FramePool {
    pub fn take(&self, length: usize) -> Vec<u8> {
        self.0
            .lock()
            .ok()
            .and_then(|mut buffers| buffers.pop())
            .filter(|buffer| buffer.len() == length)
            .unwrap_or_else(|| vec![0; length])
    }

    pub fn recycle(&self, frame: Frame) {
        if let (Frame::Owned(buffer), Ok(mut buffers)) = (frame, self.0.lock()) {
            buffers.push(buffer);
        }
    }
}

pub struct Source {
    pub video: Video,
    pub backend: &'static str,
    pub threads: usize,
    worker: Option<Worker>,
    svg: Option<SvgRenderer>,
    static_pixels: Option<Arc<Vec<u8>>>,
    pub buffers: FramePool,
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
            static_pixels = Some(Arc::new(renderer.render(
                &markup,
                video.width,
                video.height,
                vec![0; video.frame_bytes()],
            )?));
            video
        } else {
            let mut configuration = serde_json::json!({"input":input,"backend":backend,"softwareRendering":arguments.software_rendering,"overrides":{
                "width":arguments.width,"height":arguments.height,"fps":arguments.fps,"duration":arguments.duration
            }});
            let mut runtime = Worker::start(
                &mut configuration,
                backend == "three",
                arguments.timeout,
                cancelled.clone(),
            )?;
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
            buffers: FramePool::default(),
            pool,
            cancelled,
        })
    }

    pub fn batch_size(&self) -> usize {
        (256 * 1024 * 1024 / self.video.frame_bytes()).clamp(1, self.threads * 2)
    }

    /// Asks the runtime for a batch, which it may generate ahead of `collect`.
    pub fn request(&mut self, samples: &[Sample]) -> Result<()> {
        ensure!(!self.cancelled.load(Ordering::Relaxed), "Cancelled");
        if self.static_pixels.is_some() {
            return Ok(());
        }
        self.worker
            .as_mut()
            .context("Animation runtime is missing")?
            .send(&serde_json::json!({"command":"frames","samples":samples}))
    }

    /// Delivers a requested batch to `sink`, in order.
    pub fn collect(
        &mut self,
        samples: &[Sample],
        sink: &mut dyn FnMut(Frame) -> Result<()>,
    ) -> Result<()> {
        ensure!(!self.cancelled.load(Ordering::Relaxed), "Cancelled");
        if let Some(pixels) = &self.static_pixels {
            return samples
                .iter()
                .try_for_each(|_| sink(Frame::Shared(pixels.clone())));
        }
        let worker = self
            .worker
            .as_mut()
            .context("Animation runtime is missing")?;
        if let Some(renderer) = &self.svg {
            let markups = samples
                .iter()
                .map(|_| worker.receive())
                .collect::<Result<Vec<_>>>()?;
            let buffers = &self.buffers;
            let video = &self.video;
            let frames: Vec<Vec<u8>> = self.pool.install(|| {
                markups
                    .into_par_iter()
                    .zip(samples.par_iter())
                    .map(|(bytes, sample)| {
                        let markup = String::from_utf8(bytes).context("SVG is not valid UTF-8")?;
                        renderer
                            .render(
                                &markup,
                                video.width,
                                video.height,
                                buffers.take(video.frame_bytes()),
                            )
                            .with_context(|| format!("Frame {} at {}s", sample.frame, sample.time))
                    })
                    .collect::<Result<_>>()
            })?;
            frames
                .into_iter()
                .try_for_each(|pixels| sink(Frame::Owned(pixels)))
        } else {
            for sample in samples {
                let mut pixels = worker.receive()?;
                ensure!(
                    pixels.len() == self.video.frame_bytes(),
                    "Frame {} has {} bytes; expected {}",
                    sample.frame,
                    pixels.len(),
                    self.video.frame_bytes()
                );
                flip_rows(&mut pixels, self.video.width as usize * 4);
                sink(Frame::Owned(pixels))?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::flip_rows;

    #[test]
    fn flips_even_and_odd_heights() {
        let mut even = vec![1, 1, 2, 2, 3, 3, 4, 4];
        flip_rows(&mut even, 2);
        assert_eq!(even, [4, 4, 3, 3, 2, 2, 1, 1]);
        let mut odd = vec![1, 1, 2, 2, 3, 3];
        flip_rows(&mut odd, 2);
        assert_eq!(odd, [3, 3, 2, 2, 1, 1]);
        let mut single = vec![1, 2];
        flip_rows(&mut single, 2);
        assert_eq!(single, [1, 2]);
    }
}
