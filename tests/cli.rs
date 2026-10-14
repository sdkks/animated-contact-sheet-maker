// Integration tests for acsm via assert_cmd. Uses the shared
// fixture and writes outputs into TempDir-scoped directories so tests are hermetic.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn sample() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests").join("fixtures").join("sample.mp4")
}

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    // Isolate config file so real $HOME isn't touched.
    let tmp = std::env::temp_dir().join(format!("acsm-test-{}.toml", std::process::id()));
    cmd.env("ACSM_CONFIG", tmp);
    cmd
}

#[test]
fn product_identity_is_consistent() {
    assert_eq!(env!("CARGO_PKG_NAME"), "acsm");
    bin().arg("--version").assert().success()
        .stdout(format!("acsm {}\n", env!("CARGO_PKG_VERSION")));
    bin().arg("--help").assert().success()
        .stdout(predicate::str::contains("Usage: acsm"))
        .stdout(predicate::str::contains("~/.acsm.toml"))
        .stdout(predicate::str::contains("ACSM_CONFIG"))
        .stdout(predicate::str::contains("https://github.com/sdkks/animated-contact-sheet-maker"));
}

#[test]
fn default_config_is_created_in_isolated_home() {
    let home = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env_remove("ACSM_CONFIG")
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .arg("probe").arg(sample()).assert().success();
    let config = fs::read_to_string(home.path().join(".acsm.toml")).unwrap();
    assert!(config.starts_with("# ~/.acsm.toml — per-user defaults for acsm."));
}

#[test]
fn tool_free_commands_use_acsm_without_creating_config() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("unused.toml");
    for args in [
        vec!["--help"], vec!["--version"], vec!["manpage"],
        vec!["completions", "bash"], vec!["completions", "zsh"],
        vec!["completions", "fish"], vec!["completions", "powershell"],
        vec!["completions", "elvish"],
    ] {
        bin().env("ACSM_CONFIG", &config).env("PATH", "")
            .args(args).assert().success()
            .stdout(predicate::str::contains("acsm"));
    }
    assert!(!config.exists());
}

#[test]
fn probe_emits_ffprobe_json() {
    bin().args(["probe"]).arg(sample())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"duration_secs\""));
}

#[test]
fn probe_mediainfo_wraps_both() {
    bin().args(["probe", "--mediainfo"]).arg(sample())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ffprobe\""))
        .stdout(predicate::str::contains("\"mediainfo\""));
}

#[test]
fn screenshots_produces_expected_count() {
    let out = TempDir::new().unwrap();
    bin().args(["screenshots", "--count", "3", "-o"]).arg(out.path()).arg(sample())
        .assert().success();
    let pngs: Vec<_> = fs::read_dir(out.path()).unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("png"))
        .collect();
    assert_eq!(pngs.len(), 3);
}

#[test]
fn sheet_produces_nonempty_output() {
    let out = TempDir::new().unwrap();
    bin().args(["sheet", "--cols", "2", "--rows", "2", "-o"]).arg(out.path()).arg(sample())
        .assert().success();
    let entries: Vec<_> = fs::read_dir(out.path()).unwrap().collect();
    let file = entries.into_iter().map(|e| e.unwrap().path()).find(|p| p.is_file()).unwrap();
    assert!(fs::metadata(&file).unwrap().len() > 1024);
}

#[test]
fn reel_produces_webp() {
    let out = TempDir::new().unwrap();
    bin().args(["reel", "--count", "2", "--clip-length", "1", "-o"]).arg(out.path()).arg(sample())
        .assert().success();
    let webp = fs::read_dir(out.path()).unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().and_then(|x| x.to_str()) == Some("webp"));
    assert!(webp.is_some());
    // VP8X chunk indicates animated webp — "VP8X" appears early in the file.
    let bytes = fs::read(webp.unwrap().path()).unwrap();
    assert!(bytes.windows(4).any(|w| w == b"VP8X"));
}

#[test]
fn animated_sheet_produces_webp() {
    let out = TempDir::new().unwrap();
    bin().args(["animated-sheet", "--cols", "2", "--rows", "2", "--clip-length", "1", "-o"])
        .arg(out.path()).arg(sample())
        .assert().success();
    let ok = fs::read_dir(out.path()).unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("webp"));
    assert!(ok);
}

#[test]
fn config_file_provides_default() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join(".acsm.toml");
    fs::write(&cfg_path, "[sheet]\nsuffix = \"_fromconfig\"\n").unwrap();

    let out = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env("ACSM_CONFIG", &cfg_path)
        .args(["sheet", "--cols", "2", "--rows", "2", "-o"]).arg(out.path()).arg(sample())
        .assert().success();

    let names: Vec<_> = fs::read_dir(out.path()).unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    assert!(
        names.iter().any(|n| n.contains("_fromconfig")),
        "expected output filename with suffix from config, got {:?}", names
    );
}

#[test]
fn cli_flag_overrides_config() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join(".acsm.toml");
    fs::write(&cfg_path, "[sheet]\nsuffix = \"_fromconfig\"\n").unwrap();

    let out = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env("ACSM_CONFIG", &cfg_path)
        .args(["sheet", "--cols", "2", "--rows", "2", "--suffix", "_fromflag", "-o"])
        .arg(out.path()).arg(sample())
        .assert().success();

    let names: Vec<_> = fs::read_dir(out.path()).unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    assert!(
        names.iter().any(|n| n.contains("_fromflag")),
        "expected flag suffix to win, got {:?}", names
    );
    assert!(
        !names.iter().any(|n| n.contains("_fromconfig")),
        "config suffix should not appear when CLI flag is set, got {:?}", names
    );
}

#[test]
fn builtin_defaults_used_when_config_empty() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join(".acsm.toml");
    fs::write(&cfg_path, "# empty\n").unwrap();

    let out = TempDir::new().unwrap();
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env("ACSM_CONFIG", &cfg_path)
        .args(["sheet", "--cols", "2", "--rows", "2", "-o"]).arg(out.path()).arg(sample())
        .assert().success();

    let names: Vec<_> = fs::read_dir(out.path()).unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    assert!(
        names.iter().any(|n| n.contains("_sheet")),
        "expected built-in DEFAULT_SHEET_SUFFIX '_sheet' in filename, got {:?}", names
    );
}

#[test]
fn first_run_creates_config() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join(".acsm.toml");
    assert!(!cfg_path.exists());

    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env("ACSM_CONFIG", &cfg_path)
        .args(["probe"]).arg(sample())
        .assert().success();
    assert!(cfg_path.exists(), "config template should be auto-created on first run");
}

fn elementary_stream(dir: &TempDir) -> std::path::PathBuf {
    let out = dir.path().join("raw.m4v");
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"]).arg(sample())
        .args(["-an", "-c:v", "mpeg4", "-f", "m4v"]).arg(&out)
        .status().unwrap();
    assert!(status.success());
    out
}

#[test]
fn probe_estimates_duration_of_stream_without_container() {
    let dir = TempDir::new().unwrap();
    let assert = bin().args(["probe"]).arg(elementary_stream(&dir)).assert().success();
    let info: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert!(info["duration_secs"].as_f64().unwrap() > 0.0);
    assert_eq!(info["duration_source"], "frame_count");
}

#[test]
fn sheet_refuses_stream_without_timestamps() {
    let dir = TempDir::new().unwrap();
    let input = elementary_stream(&dir);
    bin().args(["sheet", "-o"]).arg(dir.path()).arg(&input)
        .assert()
        .failure()
        .stderr(predicate::str::contains("remux"));
}

#[test]
fn missing_input_exits_nonzero() {
    bin().args(["screenshots", "/does/not/exist.mp4"])
        .assert().failure();
}

#[test]
fn completions_zsh_emits_compdef() {
    bin()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("#compdef"));
}

#[test]
fn completions_bash_emits_complete_builtin() {
    bin()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -F"));
}

#[test]
fn completions_fish_emits_complete() {
    bin()
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -c acsm"));
}

#[test]
fn completions_powershell_emits_register_argumentcompleter() {
    bin()
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Register-ArgumentCompleter"));
}

#[test]
fn manpage_emits_th_header() {
    bin()
        .args(["manpage"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".TH"));
}
