use crate::{
    cli::{Encoder, OutputArgs},
    source::{Frame, FramePool, Video},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::JoinHandle,
};
use tempfile::{NamedTempFile, TempPath};

pub fn validate_output(arguments: &OutputArgs, input: &Path) -> Result<()> {
    let absolute =
        |path: &Path| -> Result<std::path::PathBuf> {
            let parent = path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create {}", parent.display()))?;
            Ok(fs::canonicalize(parent)?
                .join(path.file_name().context("Output requires a file name")?))
        };
    let input = fs::canonicalize(input)?;
    let output = absolute(&arguments.output)?;
    ensure!(
        output != input,
        "Output cannot replace the animation source"
    );
    for path in std::iter::once(&arguments.output).chain(arguments.report.iter()) {
        ensure!(
            arguments.force || !path.exists(),
            "{} already exists; use --force to replace it",
            path.display()
        );
    }
    if let Some(report) = &arguments.report {
        let report = absolute(report)?;
        ensure!(
            report != input && report != output,
            "Report must differ from the input and output"
        );
    }
    Ok(())
}

fn temporary(output: &Path) -> Result<NamedTempFile> {
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    NamedTempFile::new_in(parent).context("Cannot create temporary output")
}

fn persist(file: NamedTempFile, output: &Path, force: bool) -> Result<()> {
    if force {
        file.persist(output).map_err(|error| error.error)?;
    } else {
        file.persist_noclobber(output)
            .map_err(|error| error.error)?;
    }
    Ok(())
}

pub fn save_png(output: &Path, force: bool, width: u32, height: u32, pixels: &[u8]) -> Result<()> {
    let mut file = temporary(output)?;
    {
        let mut encoder = png::Encoder::new(file.as_file_mut(), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(pixels)?;
        writer.finish()?;
    }
    persist(file, output, force)
}

pub fn report(arguments: &OutputArgs, value: &serde_json::Value) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    if let Some(path) = &arguments.report {
        let mut file = temporary(path)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        persist(file, path, arguments.force)?;
    }
    println!("{json}");
    Ok(())
}

fn hardware_works() -> bool {
    // Probe the actual encoder, rather than trusting a compiled-in encoder listing.
    Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:r=1",
            "-frames:v",
            "1",
            "-c:v",
            "h264_videotoolbox",
            "-allow_sw",
            "0",
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Starts the encoder probe alongside runtime startup, which takes longer anyway.
pub fn probe_hardware(requested: Encoder) -> JoinHandle<bool> {
    std::thread::spawn(move || match requested {
        Encoder::Software => false,
        Encoder::Auto => cfg!(target_os = "macos") && hardware_works(),
        Encoder::Videotoolbox => hardware_works(),
    })
}

pub struct VideoEncoder {
    child: Child,
    frames: Option<mpsc::SyncSender<Frame>>,
    writer: Option<JoinHandle<io::Result<()>>>,
    errors: File,
    temporary: Option<TempPath>,
    pub name: &'static str,
}

impl VideoEncoder {
    pub fn start(
        video: &Video,
        output: &Path,
        requested: Encoder,
        hardware: JoinHandle<bool>,
        crf: u8,
        bitrate: u32,
        queue: usize,
        buffers: FramePool,
    ) -> Result<Self> {
        ensure!(
            video.width.is_multiple_of(2) && video.height.is_multiple_of(2),
            "H.264 MP4 requires even width and height"
        );
        let hardware = hardware.join().unwrap_or(false);
        ensure!(
            hardware || requested != Encoder::Videotoolbox,
            "VideoToolbox hardware encoding is unavailable; use --encoder software"
        );
        let name = if hardware {
            "h264_videotoolbox"
        } else {
            "libx264"
        };
        let temporary = temporary(output)?.into_temp_path();
        let errors = tempfile::tempfile()?;
        let mut command = Command::new("ffmpeg");
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{}x{}", video.width, video.height),
            "-framerate",
            &video.fps.to_string(),
            "-i",
            "pipe:0",
            "-an",
            "-c:v",
            name,
        ]);
        if hardware {
            command.args(["-allow_sw", "0", "-b:v", &format!("{bitrate}M")]);
        } else {
            command.args(["-preset", "fast", "-crf", &crf.to_string()]);
        }
        command
            .args([
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ])
            .arg(&temporary)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(errors.try_clone()?));
        let mut child = command
            .spawn()
            .context("FFmpeg is required; install ffmpeg and try again")?;
        let mut input = child.stdin.take().context("Cannot open encoder input")?;
        // A separate writer lets rendering continue while FFmpeg consumes frames.
        let (frames, queued) = mpsc::sync_channel::<Frame>(queue);
        let writer = std::thread::spawn(move || {
            for frame in queued {
                input.write_all(&frame)?;
                buffers.recycle(frame);
            }
            Ok(())
        });
        Ok(Self {
            child,
            frames: Some(frames),
            writer: Some(writer),
            errors,
            temporary: Some(temporary),
            name,
        })
    }

    pub fn write(&mut self, frame: Frame) -> Result<()> {
        let sender = self.frames.as_ref().context("Encoder input is closed")?;
        if sender.send(frame).is_err() {
            self.frames.take();
            let error = self.join_writer();
            let _ = self.child.wait();
            anyhow::bail!("FFmpeg rejected a frame: {error}. {}", self.error_text());
        }
        Ok(())
    }

    fn join_writer(&mut self) -> String {
        match self.writer.take().map(JoinHandle::join) {
            Some(Ok(Err(error))) => error.to_string(),
            Some(Err(_)) => "encoder writer panicked".to_owned(),
            _ => String::new(),
        }
    }

    fn error_text(&mut self) -> String {
        let mut text = String::new();
        let _ = self.errors.rewind();
        let _ = (&self.errors).take(64 * 1024).read_to_string(&mut text);
        text
    }

    pub fn finish(mut self, output: &OutputArgs) -> Result<()> {
        self.frames.take();
        let writer_error = self.join_writer();
        let status = self.child.wait()?;
        ensure!(
            status.success() && writer_error.is_empty(),
            "FFmpeg encoding failed: {writer_error} {}",
            self.error_text()
        );
        let temporary = self
            .temporary
            .take()
            .context("Temporary video is missing")?;
        if output.force {
            temporary
                .persist(&output.output)
                .map_err(|error| error.error)?;
        } else {
            temporary
                .persist_noclobber(&output.output)
                .map_err(|error| error.error)?;
        }
        Ok(())
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        self.frames.take();
        // Killing FFmpeg also releases a writer blocked on a full pipe.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.join_writer();
    }
}
