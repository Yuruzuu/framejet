use std::{
    fs,
    io::BufReader,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(arguments: &[&str], directory: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_framejet"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap()
}

fn success(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn pixels(path: &Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    (
        info.width,
        info.height,
        bytes[..info.buffer_size()].to_vec(),
    )
}

#[test]
fn static_svg_preserves_orientation_and_composites_alpha() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("scene.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="4" fill="red"/><rect y="4" width="8" height="4" fill="blue" opacity="0.5"/></svg>"#).unwrap();
    let report = success(run(
        &["frame", "scene.svg", "-o", "image.png"],
        directory.path(),
    ));
    assert_eq!(report["frames"], 1);
    let (width, height, bytes) = pixels(&directory.path().join("image.png"));
    assert_eq!((width, height), (8, 8));
    assert_eq!(&bytes[..4], &[255, 0, 0, 255]);
    assert_eq!(&bytes[7 * 8 * 4..7 * 8 * 4 + 4], &[0, 0, 128, 255]);
}

#[test]
fn unsupported_animation_fails_without_creating_output() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("scene.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><animate attributeName="x" to="10"/></svg>"#).unwrap();
    let output = run(
        &[
            "frame",
            "scene.svg",
            "-o",
            "image.png",
            "--svg-renderer",
            "native",
        ],
        directory.path(),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("CSS/SMIL animation requires"));
    assert!(!directory.path().join("image.png").exists());
}

#[test]
fn static_svg_animation_labels_stay_native() {
    let directory = TempDir::new().unwrap();
    let markup = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><!-- <animate> @keyframes --><style>.animation { fill: lime; }</style><rect class="animation" width="16" height="16"/><text y="100">animation: static label</text></svg>"#;
    fs::write(directory.path().join("scene.svg"), markup).unwrap();
    fs::write(directory.path().join("scene.mjs"), format!("export const video = {{ width:16, height:16 }}; export function render() {{ return {}; }}", serde_json::to_string(markup).unwrap())).unwrap();
    for (input, output) in [("scene.svg", "static.png"), ("scene.mjs", "module.png")] {
        success(run(&["frame", input, "-o", output], directory.path()));
        assert_eq!(
            &pixels(&directory.path().join(output)).2[..4],
            &[0, 255, 0, 255]
        );
    }
}

#[test]
fn existing_output_is_preserved_and_force_is_explicit() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("scene.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>"#,
    )
    .unwrap();
    fs::write(directory.path().join("image.png"), "keep me").unwrap();
    let output = run(&["frame", "scene.svg", "-o", "image.png"], directory.path());
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(directory.path().join("image.png")).unwrap(),
        "keep me"
    );
    success(run(
        &["frame", "scene.svg", "-o", "image.png", "--force"],
        directory.path(),
    ));
    assert_eq!(pixels(&directory.path().join("image.png")).0, 8);
}

#[test]
fn svg_module_uses_exact_time_and_keeps_logs_off_stdout() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("scene with spaces.mjs"), r#"
export const video = { width: 8, height: 8, fps: 10, duration: 2 };
console.log('animation loaded');
export function render({ time, frame }) {
  if (time !== 1.25 || frame !== 12) throw new Error('Incorrect sample time');
  return '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="lime"/></svg>';
}"#).unwrap();
    let report = success(run(
        &[
            "frame",
            "scene with spaces.mjs",
            "--time",
            "1.25",
            "-o",
            "image.png",
        ],
        directory.path(),
    ));
    assert_eq!(report["backend"], "svg");
    assert_eq!(
        &pixels(&directory.path().join("image.png")).2[..4],
        &[0, 255, 0, 255]
    );
}

#[test]
fn successful_module_exits_normally() {
    let directory = TempDir::new().unwrap();
    fs::write(directory.path().join("scene.mjs"), r#"
import { writeFileSync } from 'node:fs';
process.on('exit', () => writeFileSync(new URL('exited.txt', import.meta.url), 'normal exit'));
export const video = { width: 8, height: 8 };
export function render() { return '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>'; }
"#).unwrap();
    success(run(
        &["frame", "scene.mjs", "-o", "image.png"],
        directory.path(),
    ));
    assert_eq!(
        fs::read_to_string(directory.path().join("exited.txt")).unwrap(),
        "normal exit"
    );
}

#[test]
fn module_failure_preserves_existing_output_and_cleans_temporary_files() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("scene.mjs"),
        r#"export function render() { throw new Error('intentional failure'); }"#,
    )
    .unwrap();
    fs::write(directory.path().join("image.png"), "keep me").unwrap();
    let output = run(
        &["frame", "scene.mjs", "-o", "image.png", "--force"],
        directory.path(),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("intentional failure"));
    assert_eq!(
        fs::read_to_string(directory.path().join("image.png")).unwrap(),
        "keep me"
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn stalled_module_times_out() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("scene.mjs"),
        "export function render() { return new Promise(() => {}); }",
    )
    .unwrap();
    let output = run(
        &["frame", "scene.mjs", "-o", "image.png", "--timeout", "1"],
        directory.path(),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("timed out"));
    assert!(!directory.path().join("image.png").exists());
}

#[test]
fn invalid_metadata_and_report_collisions_are_rejected() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("scene.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>"#,
    )
    .unwrap();
    for arguments in [
        vec!["frame", "scene.svg", "-o", "image.png", "--fps", "0"],
        vec![
            "frame",
            "scene.svg",
            "-o",
            "image.png",
            "--report",
            "scene.svg",
            "--force",
        ],
        vec![
            "frame",
            "scene.svg",
            "-o",
            "image.png",
            "--report",
            "image.png",
        ],
    ] {
        assert!(!run(&arguments, directory.path()).status.success());
    }
    assert!(
        fs::read_to_string(directory.path().join("scene.svg"))
            .unwrap()
            .starts_with("<svg")
    );
    assert!(!directory.path().join("image.png").exists());
}

#[test]
fn contact_sheet_has_expected_dimensions() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("scene.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"/>"#,
    )
    .unwrap();
    let report = success(run(
        &[
            "contact-sheet",
            "scene.svg",
            "-o",
            "sheet.png",
            "--count",
            "5",
            "--columns",
            "3",
            "--thumbnail-width",
            "40",
        ],
        directory.path(),
    ));
    assert_eq!(report["frames"], 5);
    let (width, height, _) = pixels(&directory.path().join("sheet.png"));
    assert_eq!((width, height), (120, 40));
}
