use assert_cmd::Command;
use predicates::prelude::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn sample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mp4")
}

fn bin(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env("ACSM_CONFIG", root.join("config.toml"));
    cmd
}

#[test]
fn compression_effort_invalid_cli_fails_before_config_or_rendering() {
    let dir = TempDir::new().unwrap();
    for values in [vec!["7"], vec!["-1"], vec!["1.5"], vec!["fast"], vec![]] {
        bin(dir.path())
            .args(["animated-sheet", "missing.mp4", "--compression-effort"])
            .args(values)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("error:"));
        assert!(!dir.path().join("config.toml").exists());
    }
    bin(dir.path())
        .args(["reel", "missing.mp4", "--compression-effort", "1"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn compression_effort_invalid_config_is_not_rescued_by_cli() {
    let dir = TempDir::new().unwrap();
    for value in ["7", "-1", "1.5", "\"fast\""] {
        let body = format!("[animated_sheet]\ncompression_effort = {value}\n");
        fs::write(dir.path().join("config.toml"), &body).unwrap();
        let result = bin(dir.path())
            .args(["animated-sheet", "missing.mp4", "--compression-effort", "0"])
            .assert()
            .code(2)
            .stderr(predicate::str::contains("compression_effort"));
        if value == "7" {
            result.stderr(predicate::str::contains(
                "config: animated_sheet.compression_effort = 7 out of range (0..=6)",
            ));
        }
        assert_eq!(
            fs::read_to_string(dir.path().join("config.toml")).unwrap(),
            body
        );
    }
}

#[test]
fn compression_effort_help_and_completions_are_tool_free() {
    let dir = TempDir::new().unwrap();
    for help in ["-h", "--help"] {
        bin(dir.path())
            .env("PATH", "")
            .args(["animated-sheet", help])
            .assert()
            .success()
            .stdout(predicate::str::contains("--compression-effort <EFFORT>"))
            .stdout(predicate::str::contains("0 = lowest, not auto"))
            .stdout(predicate::str::contains("independent of quality"))
            .stdout(predicate::str::contains("Default: 4 unless configured"));
    }
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        bin(dir.path())
            .env("PATH", "")
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains("compression-effort"));
    }
    bin(dir.path())
        .env("PATH", "")
        .arg("manpage")
        .assert()
        .success();
    assert!(!dir.path().join("config.toml").exists());
}

fn render(root: &Path, config: Option<u32>, cli: &[&str], expected: u32) -> Vec<u8> {
    let body = config
        .map(|value| format!("[animated_sheet]\ncompression_effort = {value}\n"))
        .unwrap_or_default();
    let config_path = root.join("config.toml");
    fs::write(&config_path, &body).unwrap();
    bin(root)
        .args([
            "animated-sheet",
            "--cols",
            "2",
            "--rows",
            "1",
            "--width",
            "320",
            "--clip-length",
            "1",
            "--fps",
            "4",
            "--no-header",
            "--no-timestamps",
            "--verbose",
            "--overwrite-existing",
            "-o",
        ])
        .arg(root)
        .args(cli)
        .arg(sample())
        .assert()
        .success()
        .stdout(format!(
            "{}\n",
            root.join("sample_animated_sheet.webp").display()
        ))
        .stderr(predicate::str::contains(format!(
            "-compression_level {expected} "
        )))
        .stderr(predicate::str::contains("unknown key").not());
    assert_eq!(fs::read_to_string(config_path).unwrap(), body);
    fs::read(root.join("sample_animated_sheet.webp")).unwrap()
}

// Read actual RIFF chunks rather than matching chunk-like bytes inside frame data.
fn animation_metadata(bytes: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WEBP");
    assert_eq!(
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8,
        bytes.len()
    );
    let mut canvas_and_loop = Vec::new();
    let mut frames = Vec::new();
    let mut pos = 12;
    while pos < bytes.len() {
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let payload = &bytes[pos + 8..pos + 8 + len];
        match &bytes[pos..pos + 4] {
            b"VP8X" | b"ANIM" => canvas_and_loop.extend_from_slice(payload),
            b"ANMF" => frames.push(payload[..16].to_vec()),
            _ => {}
        }
        pos += 8 + len + len % 2;
    }
    assert_eq!(pos, bytes.len());
    assert_eq!(frames.len(), 4);
    (canvas_and_loop, frames)
}

#[test]
fn compression_effort_renders_all_values_and_preserves_default_precedence_and_timing() {
    let dir = TempDir::new().unwrap();
    let default = render(dir.path(), None, &[], 4);
    let metadata = animation_metadata(&default);
    for effort in 0..=6 {
        let value = effort.to_string();
        let bytes = render(dir.path(), None, &["--compression-effort", &value], effort);
        assert_eq!(animation_metadata(&bytes), metadata);
        if effort == 4 {
            assert_eq!(bytes, default, "implicit default must equal explicit 4");
        }
        // The same explicit effort reached through config must produce identical bytes.
        assert_eq!(render(dir.path(), Some(effort), &[], effort), bytes);
    }
    let zero = render(dir.path(), None, &["--compression-effort=0"], 0);
    assert_eq!(
        render(dir.path(), Some(6), &["--compression-effort", "0"], 0),
        zero
    );
    assert_eq!(
        render(dir.path(), Some(0), &["--compression-effort", "4"], 4),
        default
    );

    // A changed effort still skips an existing exact name unless overwrite is explicit.
    bin(dir.path())
        .args([
            "animated-sheet",
            "--compression-effort",
            "1",
            "--verbose",
            "-o",
        ])
        .arg(dir.path())
        .arg(sample())
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("skipped (output exists:"))
        .stderr(predicate::str::contains("-compression_level").not());
    assert_eq!(
        fs::read(dir.path().join("sample_animated_sheet.webp")).unwrap(),
        default
    );
}
