use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use tungstenite::{
    Message, WebSocket,
    handshake::server::{ErrorResponse, Request, Response},
    protocol::WebSocketConfig,
};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub fn directory() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .context("Cannot locate the cache directory")?
        .join("framejet")
        .join(env!("CARGO_PKG_VERSION")))
}

pub fn prepare() -> Result<PathBuf> {
    let directory = directory()?;
    fs::create_dir_all(&directory)?;
    for (name, content) in [
        ("worker.mjs", include_str!("../runtime/worker.mjs")),
        (
            "frame-transport.js",
            include_str!("../runtime/frame-transport.js"),
        ),
        ("package.json", include_str!("../runtime/package.json")),
        (
            "package-lock.json",
            include_str!("../runtime/package-lock.json"),
        ),
    ] {
        let path = directory.join(name);
        if fs::read_to_string(&path).ok().as_deref() != Some(content) {
            let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
            temporary.write_all(content.as_bytes())?;
            temporary.persist(path).map_err(|error| error.error)?;
        }
    }
    Ok(directory)
}

pub fn setup(skip_browser: bool) -> Result<()> {
    let directory = prepare()?;
    for (program, arguments) in [
        ("npm", vec!["ci", "--no-audit", "--no-fund"]),
        (
            "node",
            vec!["node_modules/playwright-core/cli.js", "install", "chromium"],
        ),
    ] {
        if skip_browser && program == "node" {
            continue;
        }
        let status = Command::new(program)
            .args(arguments)
            .current_dir(&directory)
            .stdout(Stdio::from(std::io::stderr()))
            .status()
            .with_context(|| format!("Cannot run {program}"))?;
        ensure!(status.success(), "Runtime installation failed: {program}");
    }
    println!(
        "{}",
        serde_json::json!({"status":"ok", "runtime":directory,"browser_installation_skipped":skip_browser})
    );
    Ok(())
}

/// Accepts one WebSocket holding the frame token and forwards its binary frames.
/// The browser writes straight to Rust; Node only drives the page.
fn serve_frames(listener: TcpListener, token: String, frames: mpsc::SyncSender<Result<Vec<u8>, String>>) {
    let expected = format!("/frames/{token}");
    let configuration = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES))
        .read_buffer_size(1024 * 1024);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = stream.set_nodelay(true);
        let authorize = |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
            if request.uri().path() == expected {
                Ok(response)
            } else {
                let mut rejection = ErrorResponse::new(None);
                *rejection.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
                Err(rejection)
            }
        };
        if let Ok(socket) = tungstenite::accept_hdr_with_config(stream, authorize, Some(configuration)) {
            forward_frames(socket, &frames);
            return;
        }
    }
}

fn forward_frames(mut socket: WebSocket<TcpStream>, frames: &mpsc::SyncSender<Result<Vec<u8>, String>>) {
    loop {
        let result = match socket.read() {
            Ok(Message::Close(_)) => Err(tungstenite::Error::ConnectionClosed),
            Ok(Message::Binary(bytes)) => {
                if frames.send(Ok(Vec::from(bytes))).is_err() {
                    return;
                }
                socket.send(Message::text("ok"))
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        };
        if result.is_err() {
            // Frames in flight would otherwise leave the CLI waiting for its timeout.
            // During normal shutdown nobody is listening, so this must not block.
            let _ = frames.try_send(Err("Frame stream disconnected".to_owned()));
            return;
        }
    }
}

const MAX_FRAME_BYTES: usize = 134_217_728;

pub struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Result<Vec<u8>, String>>,
    timeout: Duration,
    cancelled: Arc<AtomicBool>,
}

impl Worker {
    pub fn start(
        configuration: &mut Value,
        frame_stream: bool,
        timeout: u64,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self> {
        let directory = prepare()?;
        let mut child = Command::new("node")
            .arg(directory.join("worker.mjs"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Node.js 20+ is required; install Node.js and try again")?;
        let input = child.stdin.take().context("Cannot open runtime input")?;
        let output = child.stdout.take().context("Cannot open runtime output")?;
        let (sender, messages) = mpsc::sync_channel(1);
        if frame_stream {
            let listener = TcpListener::bind(("127.0.0.1", 0))
                .context("Cannot open the local frame stream")?;
            let mut random = [0u8; 24];
            getrandom::fill(&mut random).map_err(|error| anyhow::anyhow!("{error}"))?;
            let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            configuration["streamUrl"] = format!(
                "ws://127.0.0.1:{}/frames/{token}",
                listener.local_addr()?.port()
            )
            .into();
            let frames = sender.clone();
            std::thread::spawn(move || serve_frames(listener, token, frames));
        }
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let packet = (|| -> Result<Vec<u8>> {
                    let mut header = [0u8; 5];
                    reader.read_exact(&mut header)?;
                    let length = u32::from_le_bytes(header[1..].try_into()?) as usize;
                    ensure!(
                        length <= 256 * 1024 * 1024,
                        "Runtime response exceeds 256 MiB"
                    );
                    let mut bytes = vec![0; length];
                    reader.read_exact(&mut bytes)?;
                    if header[0] == 2 {
                        bail!("{}", String::from_utf8_lossy(&bytes));
                    }
                    ensure!(header[0] <= 1, "Invalid runtime packet");
                    Ok(bytes)
                })()
                .map_err(|error| format!("{error:#}"));
                let failed = packet.is_err();
                if sender.send(packet).is_err() || failed {
                    break;
                }
            }
        });
        let mut worker = Self {
            child,
            input: Some(input),
            messages,
            timeout: Duration::from_secs(timeout),
            cancelled,
        };
        worker.send(configuration)?;
        Ok(worker)
    }

    pub fn send(&mut self, value: &Value) -> Result<()> {
        let input = self.input.as_mut().context("Runtime input is closed")?;
        serde_json::to_writer(&mut *input, value)?;
        input
            .write_all(b"\n")
            .context("Runtime stopped accepting commands")?;
        input.flush()?;
        Ok(())
    }

    pub fn receive(&mut self) -> Result<Vec<u8>> {
        let started = Instant::now();
        loop {
            ensure!(!self.cancelled.load(Ordering::Relaxed), "Cancelled");
            match self.messages.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result.map_err(|error| anyhow::anyhow!(error)),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("Animation runtime exited unexpectedly; see stderr")
                }
                Err(mpsc::RecvTimeoutError::Timeout) => ensure!(
                    started.elapsed() < self.timeout,
                    "Animation runtime timed out after {} seconds",
                    self.timeout.as_secs()
                ),
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(mut input) = self.input.take() {
            let _ = input.write_all(b"{\"command\":\"close\"}\n");
        }
        for _ in 0..500 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn version(program: &str, argument: &str) -> Option<String> {
    let output = Command::new(program).arg(argument).output().ok()?;
    if !output.status.success() {
        return None;
    }
    BufReader::new(output.stdout.as_slice())
        .lines()
        .next()?
        .ok()
}

pub fn doctor() -> Result<()> {
    let directory = directory()?;
    let browser = Command::new("node").args(["--input-type=module", "-e",
        "import {chromium} from 'playwright-core'; import fs from 'node:fs'; const p=chromium.executablePath(); console.log(JSON.stringify({installed:fs.existsSync(p),path:p}))"])
        .current_dir(&directory).output().ok().filter(|output| output.status.success())
        .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok());
    let node = version("node", "--version");
    let ffmpeg = version("ffmpeg", "-version");
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "version":env!("CARGO_PKG_VERSION"), "node":node,"ffmpeg":ffmpeg,
            "runtime":directory,"three":browser,"svg_ready":node.is_some(),
            "hint":"Run framejet setup to install the pinned Three.js runtime and Chromium."
        }))?
    );
    Ok(())
}

pub fn ensure_extension(path: &Path, expected: &str) -> Result<()> {
    ensure!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(expected)),
        "Output must end in .{expected}"
    );
    Ok(())
}
