use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Render SVG and Three.js animations to MP4 or PNG")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Install the pinned JavaScript runtime and Chromium.
    Setup {
        /// Install packages without downloading Chromium (SVG-only use).
        #[arg(long)]
        skip_browser: bool,
    },
    /// Report dependency availability as JSON.
    Doctor,
    /// Export every frame to an H.264 MP4, without intermediate PNG files.
    Render {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long, value_enum, default_value = "auto")]
        encoder: Encoder,
        /// Software x264 quality, 0 (lossless) to 51 (lowest quality).
        #[arg(long, default_value_t = 18, value_parser = clap::value_parser!(u8).range(0..=51))]
        crf: u8,
        /// Hardware encoder bitrate in megabits per second.
        #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=1000))]
        bitrate: u32,
    },
    /// Export a PNG at an exact time in seconds.
    Frame {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long, default_value_t = 0.0)]
        time: f64,
    },
    /// Export evenly spaced frames as one PNG, in reading order.
    ContactSheet {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long, default_value_t = 12, value_parser = clap::value_parser!(u32).range(1..=64))]
        count: u32,
        #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..=16))]
        columns: u32,
        #[arg(long, default_value_t = 320, value_parser = clap::value_parser!(u32).range(16..=2048))]
        thumbnail_width: u32,
    },
}

#[derive(Args)]
pub struct SourceArgs {
    /// A .svg file or an animation .mjs module.
    pub input: PathBuf,
    /// Auto selects Three.js for *.three.mjs and SVG otherwise.
    #[arg(long, value_enum, default_value = "auto")]
    pub backend: Backend,
    /// SVG engine: auto uses Chromium for CSS/SMIL, native requires static markup.
    #[arg(long, value_enum, default_value = "auto")]
    pub svg_renderer: SvgRendering,
    #[arg(long)]
    pub width: Option<u32>,
    #[arg(long)]
    pub height: Option<u32>,
    #[arg(long)]
    pub fps: Option<f64>,
    /// Override the animation's total duration in seconds.
    #[arg(long)]
    pub duration: Option<f64>,
    /// Additional local font files for native SVG text. Repeat as needed.
    #[arg(long)]
    pub font: Vec<PathBuf>,
    /// Force Three.js to use SwiftShader instead of a hardware GPU.
    #[arg(long)]
    pub software_rendering: bool,
    /// Native SVG rendering threads. Defaults to at most eight.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=64))]
    pub threads: Option<u32>,
    /// Maximum seconds to wait for one runtime response.
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub timeout: u64,
}

#[derive(Args)]
pub struct OutputArgs {
    #[arg(short, long)]
    pub output: PathBuf,
    /// Replace an existing output/report only when explicitly requested.
    #[arg(long)]
    pub force: bool,
    /// Also save the JSON result to this path.
    #[arg(long)]
    pub report: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum Backend {
    Auto,
    Svg,
    Three,
}

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum SvgRendering {
    Auto,
    Native,
    Browser,
}

#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum Encoder {
    Auto,
    Software,
    Videotoolbox,
}
