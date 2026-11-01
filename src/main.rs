#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod cli;
mod output;
mod runtime;
mod source;
mod svg;

use anyhow::{Result, ensure};
use clap::Parser;
use cli::{Cli, Command};
use source::{Sample, Source};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

fn run() -> Result<()> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let started = Instant::now();
    let command = Cli::parse().command;
    let (source_arguments, output_arguments, extension) = match &command {
        Command::Setup { skip_browser } => return runtime::setup(*skip_browser),
        Command::Doctor => return runtime::doctor(),
        Command::Render { source, output, .. } => (source, output, "mp4"),
        Command::Frame { source, output, .. } | Command::ContactSheet { source, output, .. } => {
            (source, output, "png")
        }
    };
    runtime::ensure_extension(&output_arguments.output, extension)?;
    output::validate_output(output_arguments, &source_arguments.input)?;
    let mut source = Source::open(source_arguments, cancelled.clone())?;
    let setup_seconds = started.elapsed().as_secs_f64();
    let video = source.video.clone();
    let processing = Instant::now();
    let mut encoder_name = None;
    let samples;
    let sample_times;
    let (output_width, output_height);

    match command {
        Command::Render {
            ref output,
            encoder,
            crf,
            bitrate,
            ..
        } => {
            let batch_size = source.batch_size() as u64;
            let mut encoder = output::VideoEncoder::start(
                &video,
                &output.output,
                encoder,
                crf,
                bitrate,
                batch_size as usize,
                source.buffers.clone(),
            )?;
            encoder_name = Some(encoder.name);
            let total = video.frames();
            let batch_at = |start: u64| -> Vec<_> {
                (start..(start + batch_size).min(total))
                    .map(|frame| Sample {
                        time: frame as f64 / video.fps,
                        frame,
                    })
                    .collect()
            };
            source.request(&batch_at(0))?;
            eprintln!(
                "Rendering {total} frames through {} → {}",
                source.backend, encoder.name
            );
            for start in (0..total).step_by(batch_size as usize) {
                ensure!(!cancelled.load(Ordering::Relaxed), "Cancelled");
                // Generate the next batch while this one renders and encodes.
                if start + batch_size < total {
                    source.request(&batch_at(start + batch_size))?;
                }
                source.collect(&batch_at(start), &mut |frame| encoder.write(frame))?;
                if start == 0
                    || (start / batch_size).is_multiple_of(10)
                    || start + batch_size >= total
                {
                    eprintln!("{}/{} frames", (start + batch_size).min(total), total);
                }
            }
            encoder.finish(output)?;
            samples = total;
            sample_times = None;
            (output_width, output_height) = (video.width, video.height);
        }
        Command::Frame {
            ref output, time, ..
        } => {
            ensure!(
                time.is_finite() && time >= 0.0 && time <= video.duration,
                "Time must be between 0 and {} seconds",
                video.duration
            );
            let sample = Sample {
                time,
                frame: (time * video.fps).floor() as u64,
            };
            source.request(&[sample])?;
            source.collect(&[sample], &mut |pixels| {
                output::save_png(
                    &output.output,
                    output.force,
                    video.width,
                    video.height,
                    &pixels,
                )
            })?;
            samples = 1;
            sample_times = Some(vec![time]);
            (output_width, output_height) = (video.width, video.height);
        }
        Command::ContactSheet {
            ref output,
            count,
            columns,
            thumbnail_width,
            ..
        } => {
            let columns = columns.min(count);
            let rows = count.div_ceil(columns);
            let thumbnail_height = ((video.height as f64 / video.width as f64)
                * thumbnail_width as f64)
                .round()
                .max(1.0) as u32;
            (output_width, output_height) = (columns * thumbnail_width, rows * thumbnail_height);
            ensure!(
                u64::from(output_width) * u64::from(output_height) <= 67_108_864,
                "Contact sheet exceeds 64 megapixels; reduce --thumbnail-width or --count"
            );
            let mut sheet = vec![0; output_width as usize * output_height as usize * 4];
            for pixel in sheet.as_chunks_mut::<4>().0 {
                pixel[3] = 255;
            }
            let positions: Vec<_> = (0..count)
                .map(|index| {
                    let frame = if count == 1 {
                        0
                    } else {
                        ((video.frames() - 1) as f64 * index as f64 / (count - 1) as f64).round()
                            as u64
                    };
                    Sample {
                        time: frame as f64 / video.fps,
                        frame,
                    }
                })
                .collect();
            sample_times = Some(
                positions
                    .iter()
                    .map(|sample| sample.time)
                    .collect::<Vec<_>>(),
            );
            let batch_size = source.batch_size();
            for (batch_index, batch) in positions.chunks(batch_size).enumerate() {
                let first = batch_index * batch_size;
                let mut offset = 0;
                source.request(batch)?;
                source.collect(batch, &mut |pixels| {
                    let index = first + offset;
                    offset += 1;
                    let left = index as u32 % columns * thumbnail_width;
                    let top = index as u32 / columns * thumbnail_height;
                    let tile =
                        resvg::tiny_skia::PixmapRef::from_bytes(&pixels, video.width, video.height)
                            .ok_or_else(|| anyhow::anyhow!("Invalid thumbnail pixels"))?;
                    let mut canvas = resvg::tiny_skia::PixmapMut::from_bytes(
                        &mut sheet,
                        output_width,
                        output_height,
                    )
                    .ok_or_else(|| anyhow::anyhow!("Invalid contact sheet dimensions"))?;
                    let paint = resvg::tiny_skia::PixmapPaint {
                        quality: resvg::tiny_skia::FilterQuality::Bilinear,
                        ..Default::default()
                    };
                    let transform = resvg::tiny_skia::Transform::from_scale(
                        thumbnail_width as f32 / video.width as f32,
                        thumbnail_height as f32 / video.height as f32,
                    )
                    .post_translate(left as f32, top as f32);
                    canvas.draw_pixmap(0, 0, tile, &paint, transform, None);
                    Ok(())
                })?;
            }
            output::save_png(
                &output.output,
                output.force,
                output_width,
                output_height,
                &sheet,
            )?;
            samples = u64::from(count);
        }
        _ => unreachable!(),
    }
    let processing_seconds = processing.elapsed().as_secs_f64();
    let report = serde_json::json!({
        "status":"ok", "version":env!("CARGO_PKG_VERSION"), "backend":source.backend,
        "input":std::fs::canonicalize(&source_arguments.input)?, "output":std::fs::canonicalize(&output_arguments.output)?,
        "video":video,"output_width":output_width,"output_height":output_height,
        "frames":samples,"sample_times":sample_times,
        "output_duration_seconds":encoder_name.map(|_| samples as f64 / video.fps),
        "encoder":encoder_name,"setup_seconds":setup_seconds,
        "processing_seconds":processing_seconds,"elapsed_seconds":started.elapsed().as_secs_f64(),
        "processing_fps":samples as f64 / processing_seconds
    });
    output::report(output_arguments, &report)?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("framejet: {error:#}");
        std::process::exit(1);
    }
}
