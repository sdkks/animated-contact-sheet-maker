//! Exercise auto layout through the actual CLI, probe, extraction and publication path.
use assert_cmd::Command;
use std::{fs, path::Path};
use tempfile::TempDir;

fn command(config: &Path) -> Command {
    let temporary = config.parent().unwrap().join("temporary");
    fs::create_dir_all(&temporary).unwrap();
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.arg("--tmp-dir").arg(temporary).env("ACSM_CONFIG", config);
    cmd
}

fn fixture(path: &Path, seconds: u32) {
    fixture_at_rate(path, f64::from(seconds), 12);
}

fn fixture_at_rate(path: &Path, seconds: f64, fps: u32) {
    let tools = acsm::ffmpeg_test_hook_locate().unwrap();
    let output = std::process::Command::new(tools.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=s=160x90:r={fps}")).arg("-t")
        .arg(seconds.to_string()).args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn render(config: &Path, source: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    command(config).args(["animated-sheet", "--width", "320", "--gap", "4", "--clip-length", "5", "--fps", "12", "--compression-effort", "0"])
        .args(extra).arg("-o").arg(out).arg(source).assert().success().get_output().clone()
}

// Parse numeric fields, rather than pinning the plan's free-form wording.
fn plan_values(stderr: &str) -> (f64, (u32, u32), u32, f64, f64) {
    let line = stderr.lines().find(|line| line.contains(": plan · ")).expect("plan on stderr");
    let fields: Vec<_> = line.split(" · ").skip(1).collect();
    let number = |s: &str| s.split_whitespace().next().unwrap().trim_end_matches('s').parse::<f64>().unwrap();
    let grid: Vec<u32> = fields[1].split_whitespace().next().unwrap().split('x').map(|s| s.parse().unwrap()).collect();
    (number(fields[0]), (grid[0], grid[1]), number(fields[2]) as u32, number(fields[3]), number(fields[4]))
}

fn assert_animated(path: &Path) -> usize {
    let bytes = fs::read(path).unwrap();
    assert!(bytes.len() > 100);
    assert!(bytes.windows(4).any(|w| w == b"ANIM"));
    let frames = bytes.windows(4).filter(|w| *w == b"ANMF").count();
    assert!(frames > 1);
    frames
}

#[test]
fn animated_auto_renders_fractional_cells_and_preserves_static_precedence() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config.toml");
    fs::write(&config, "").unwrap();
    let long = root.path().join("long.mp4");
    let short = root.path().join("short.mp4");
    fixture(&long, 45);
    fixture(&short, 3);
    let out = root.path().join("out"); fs::create_dir(&out).unwrap();
    let output = render(&config, &long, &out, &["--auto", "--verbose"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    eprintln!("45-second acceptance render:\n{stderr}");
    let (duration, grid, cells, length, decoded) = plan_values(&stderr);
    assert_eq!((duration, grid, cells, length, decoded), (45., (3,3), 9, 3.89, 35.));
    let extraction_lines: Vec<_> = stderr.lines().filter(|l| l.starts_with("+ ") && l.contains(" -t ")).collect();
    assert_eq!(extraction_lines.len(), 9);
    let mut sum = 0.;
    let mut seeks = Vec::new();
    for line in extraction_lines {
        let args: Vec<_> = line.split_whitespace().collect();
        let value = |flag| args[args.iter().position(|a| *a == flag).unwrap() + 1].parse::<f64>().unwrap();
        let length = value("-t");
        assert!((length - 35. / 9.).abs() < 0.000001);
        seeks.push(value("-ss"));
        sum += length;
    }
    // Batch workers may start in any order; the cell geometry is ordered by seek.
    seeks.sort_by(f64::total_cmp);
    for (i, seek) in seeks.iter().enumerate() {
        assert!((seek - (5. + i as f64 * 35. / 9.)).abs() <= 0.0005);
    }
    assert!((sum - 35.).abs() < 0.00001);
    assert!(stderr.find(": plan · ").unwrap() < stderr.find(" -hide_banner").unwrap());
    let result = out.join("long_animated_sheet.webp");
    assert_eq!(assert_animated(&result), 47);
    let before = fs::read(&result).unwrap();
    let skipped = render(&config, &long, &out, &["--auto"]);
    assert!(!String::from_utf8_lossy(&skipped.stderr).contains(": plan · "));
    assert_eq!(fs::read(&result).unwrap(), before);
    let quiet = render(&config, &long, &out, &["--auto", "--quiet", "--overwrite-existing"]);
    assert!(quiet.stderr.is_empty());
    assert!(!quiet.stdout.is_empty());
    assert_animated(&result);

    // A static grid that would exceed both cell limits and fail the width check
    // must not influence auto mode, though ordinary config ranges still apply.
    fs::write(&config, "[animated_sheet]\nauto=true\ncols=32\nrows=32\n").unwrap();
    let output = render(&config, &short, &out, &[]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    eprintln!("3-second acceptance render:\n{stderr}");
    assert_eq!(plan_values(&stderr), (3., (3,1), 3, 0.5, 1.5));
    assert!(!stderr.contains("warning:"));
    assert!(!stderr.contains("too short"));
    assert_eq!(assert_animated(&out.join("short_animated_sheet.webp")), 6);

    let output = render(&config, &long, &out, &["--cols", "2", "--rows", "2", "--overwrite-existing"]);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(": plan · "));
    assert_animated(&result);
    for flag in ["--cols", "--rows"] {
        fs::write(&config, "[animated_sheet]\nauto=true\ncols=2\nrows=2\n").unwrap();
        let output = render(&config, &long, &out, &[flag, "2", "--overwrite-existing"]);
        assert!(!String::from_utf8_lossy(&output.stderr).contains(": plan · "));
    }
    // A different rule order still chooses the tightest ceiling, not the first.
    fs::write(&config, "[animated_sheet]\nauto=true\n[layout]\nrules=[{cols=4,rows=2},{max_seconds=120,cols=3,rows=3},{max_seconds=10,cols=3,rows=1}]\n").unwrap();
    let output = render(&config, &long, &out, &["--overwrite-existing"]);
    assert_eq!(plan_values(&String::from_utf8(output.stderr).unwrap()).1, (3,3));
    assert!(fs::read_dir(&out).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".acsm-")));
    let batch = root.path().join("batch"); fs::create_dir(&batch).unwrap();
    fs::write(&config, "[layout]\nrules=[{max_seconds=10,cols=2,rows=1},{cols=32,rows=1}]\n").unwrap();
    let output = command(&config).args(["animated-sheet", "--auto", "--width", "320", "--gap", "8", "--clip-length", "5", "--compression-effort", "0", "-o"])
        .arg(&batch).arg(&long).arg(&short).assert().code(1).get_output().clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("too narrow"));
    assert!(stderr.contains("1 done · 0 skipped · 1 failed"));
    assert!(!batch.join("long_animated_sheet.webp").exists());
    assert_animated(&batch.join("short_animated_sheet.webp"));
    // TempDir owns all input and output fixtures; verify cleanup rather than
    // leaving synthetic media or destination-local stages after the test.
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}

#[test]
fn subframe_auto_cells_publish_valid_webp_without_changing_nominal_plan() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config.toml");
    let out = root.path().join("out"); fs::create_dir(&out).unwrap();
    let tools = acsm::ffmpeg_test_hook_locate().unwrap();
    for (name, duration, source_fps, cols, rows) in [
        ("micro", 0.25, 12, 3, 1),
        ("dense", 3.0, 12, 8, 8),
        ("micro_faster_source", 0.25, 24, 3, 1),
    ] {
        let source = root.path().join(format!("{name}.mp4"));
        fixture_at_rate(&source, duration, source_fps);
        fs::write(&config, if cols == 8 { "[layout]\nrules=[{cols=8,rows=8}]\n" } else { "" }).unwrap();
        let output = render(&config, &source, &out, &["--auto", "--verbose"]);
        let stderr = String::from_utf8(output.stderr).unwrap();
        eprintln!("Sub-frame {name} render:\n{stderr}");
        let n = cols * rows;
        let nominal = duration / 2.0 / f64::from(n);
        let (d, grid, cells, length, decoded) = plan_values(&stderr);
        assert_eq!((d, grid, cells), (duration, (cols, rows), n));
        assert!((length - nominal).abs() <= 0.005001);
        assert!((decoded - duration / 2.0).abs() <= 0.005001);
        let extracts: Vec<_> = stderr.lines().filter(|l| l.starts_with("+ ") && l.contains(" -t ")).collect();
        assert_eq!(extracts.len(), n as usize);
        let mut seeks = Vec::new();
        for line in extracts {
            let args: Vec<_> = line.split_whitespace().collect();
            let value = |flag| args[args.iter().position(|a| *a == flag).unwrap() + 1];
            assert!((value("-t").parse::<f64>().unwrap() - nominal).abs() < 0.000001);
            assert_eq!(value("-map"), "0:0");
            assert_eq!(value("-r"), "12");
            assert!(line.contains(",setpts=PTS-STARTPTS -r 12 "));
            assert!(line.contains("drawtext="));
            seeks.push(value("-ss").parse::<f64>().unwrap());
        }
        seeks.sort_by(f64::total_cmp);
        for (i, seek) in seeks.iter().enumerate() {
            assert!((seek - (duration / 4.0 + i as f64 * nominal)).abs() <= 0.000500001);
        }
        assert!(stderr.contains(&format!(":d={nominal:.6}:r=12")));
        let result = out.join(format!("{name}_animated_sheet.webp"));
        let bytes = fs::read(&result).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        if bytes.windows(4).any(|w| w == b"ANIM") {
            assert_animated(&result);
        } else {
            // A single-frame sheet is a still image, not an invented animation.
            let probe = std::process::Command::new(&tools.ffprobe)
                .args(["-v", "error", "-count_frames", "-show_streams", "-of", "json"])
                .arg(&result).output().unwrap();
            assert!(probe.status.success(), "{}", String::from_utf8_lossy(&probe.stderr));
            let probe: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
            let streams = probe["streams"].as_array().unwrap();
            assert_eq!(streams.len(), 1);
            assert_eq!(streams[0]["codec_name"], "webp");
            assert_eq!(streams[0]["nb_read_frames"], "1");
            assert!(streams[0]["width"].as_u64().unwrap() > 0);
            assert!(streams[0]["height"].as_u64().unwrap() > 0);
        }
    }
    assert!(fs::read_dir(&out).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".acsm-")));
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}

#[tokio::test]
async fn auto_cancellation_preserves_destination_and_reaps_intermediates() {
    use acsm::{animated_sheet::{generate_auto, AnimatedSheetOptions}, auto_layout::{resolve, LayoutTable}, jobs::{OutputPolicy, PipelineContext, ProgressReporter}};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let root = TempDir::new().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mp4");
    let tools = acsm::ffmpeg_test_hook_locate().unwrap();
    let info = acsm::ffmpeg_test_hook_probe(&tools, source.to_str().unwrap()).await.unwrap();
    let plan = resolve(&LayoutTable {trim_start_secs: 5, trim_end_secs: 5, rules: acsm::defaults::layout::RULES.to_vec()}, info.sampling_duration_secs(), 320, 4, Some(5.0)).unwrap();
    let opts = AnimatedSheetOptions {
        cols: 32, rows: 32, width: 320, gap: 4, clip_length_secs: 5, fps: 12,
        quality: 75, compression_effort: 0, thumb_font_size: 14, header_font_size: 18,
        show_timestamps: false, show_header: false, suffix: String::new(), theme: Default::default(),
    };
    let out = root.path().join("sheet.webp");
    fs::write(&out, b"old destination").unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    // Cancel on a real cell-completion event, not after an arbitrary sleep.
    let emit = |_: u32, _: u32, _: &str| cancelled.store(true, Ordering::Relaxed);
    let reporter = ProgressReporter {emit: &emit};
    let ctx = PipelineContext {output_policy: OutputPolicy::OverwriteExisting, ffmpeg: &tools.ffmpeg,
        cancelled: cancelled.clone(), reporter: &reporter, has_zscale: false};
    let result = generate_auto(&source, &info, &out, &opts, &plan, Path::new("unused.ttf"), &ctx).await;
    assert!(matches!(result, Err(acsm::ffmpeg::RunError::Killed)), "{result:?}");
    assert_eq!(fs::read(&out).unwrap(), b"old destination");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}

fn still_render(config: &Path, source: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    command(config).args(["sheet", "--width", "320", "--gap", "4", "--format", "png", "--no-header", "--no-timestamps"])
        .args(extra).arg("-o").arg(out).arg(source).assert().success().get_output().clone()
}

fn png_dimensions(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()))
}

#[test]
fn still_auto_renders_centres_and_honours_mode_and_publication_policy() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config.toml");
    // This legal stored grid is too narrow at 320px, but auto ignores it.
    fs::write(&config, "[sheet]\nauto=true\ncols=32\nrows=32\n").unwrap();
    let source = root.path().join("long.mp4");
    fixture(&source, 45);
    let out = root.path().join("out"); fs::create_dir(&out).unwrap();
    let output = still_render(&config, &source, &out, &["--auto", "--verbose"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    eprintln!("Still 45-second acceptance render:\n{stderr}");
    assert!(!stderr.contains("warning:"));
    let line = stderr.lines().find(|l| l.contains(": plan · ")).unwrap();
    let fields: Vec<_> = line.split(" · ").skip(1).collect();
    let number = |s: &str| s.split_whitespace().next().unwrap().trim_end_matches('s').parse::<f64>().unwrap();
    let grid: Vec<u32> = fields[1].split_whitespace().next().unwrap().split('x').map(|s| s.parse().unwrap()).collect();
    assert_eq!((number(fields[0]), grid, number(fields[2]), number(fields[3])), (45., vec![3,3], 9., 35.));
    let extracts: Vec<_> = stderr.lines().filter(|l| l.starts_with("+ ") && l.contains("thumb_") && !l.contains(" -framerate ")).collect();
    assert_eq!(extracts.len(), 9);
    let mut seeks: Vec<f64> = extracts.iter().map(|line| {
        let args: Vec<_> = line.split_whitespace().collect();
        assert_eq!(args[args.iter().position(|a| *a == "-map").unwrap() + 1], "0:0");
        args[args.iter().position(|a| *a == "-ss").unwrap() + 1].parse().unwrap()
    }).collect();
    seeks.sort_by(f64::total_cmp);
    for (i, seek) in seeks.iter().enumerate() {
        assert!((seek - (5. + (i as f64 + 0.5) * 35. / 9.)).abs() <= 0.0005);
    }
    assert!(stderr.contains("tile=3x3:"));
    assert!(stderr.find(": plan · ").unwrap() < stderr.find(" -hide_banner").unwrap());
    let result = out.join("long_sheet.png");
    assert_eq!(png_dimensions(&result), (316, 184));
    let before = fs::read(&result).unwrap();
    let skipped = still_render(&config, &source, &out, &["--auto", "--verbose"]);
    assert!(!String::from_utf8_lossy(&skipped.stderr).contains(": plan · "));
    assert!(!String::from_utf8_lossy(&skipped.stderr).contains("+ "));
    assert_eq!(fs::read(&result).unwrap(), before);
    // Input expansion requires an existing path; skip still precedes probing.
    fs::rename(&source, root.path().join("saved.mp4")).unwrap();
    fs::write(&source, b"not media").unwrap();
    still_render(&config, &source, &out, &["--auto"]);
    fs::rename(root.path().join("saved.mp4"), &source).unwrap();
    let quiet = still_render(&config, &source, &out, &["--quiet", "--overwrite-existing"]);
    assert!(quiet.stderr.is_empty());
    assert_eq!(fs::read(&result).unwrap(), before);
    for flag in ["--cols", "--rows"] {
        fs::write(&config, "[sheet]\nauto=true\ncols=2\nrows=2\n").unwrap();
        let output = still_render(&config, &source, &out, &[flag, "2", "--overwrite-existing"]);
        assert!(!String::from_utf8_lossy(&output.stderr).contains(": plan · "));
        assert_eq!(png_dimensions(&result), (320, 184));
    }
    // Static 2x2 keeps the historical 9,18,27,36 second sampler.
    let output = still_render(&config, &source, &out, &["--cols", "2", "--verbose", "--overwrite-existing"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for ts in ["9.000", "18.000", "27.000", "36.000"] { assert!(stderr.contains(&format!("-ss {ts} "))); }
    fs::write(&config, "invalid TOML").unwrap();
    for flag in ["--cols", "--rows"] {
        let output = command(&config).args(["sheet", "missing.mp4", "--auto", flag, "3"])
            .assert().code(2).get_output().clone();
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("config parse failed"));
    }
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}

#[test]
fn still_auto_accepts_more_than_64_cells_and_continues_after_narrow_grid() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config.toml");
    fs::write(&config, "[layout]\nrules=[{cols=9,rows=8}]\n").unwrap();
    let short = root.path().join("short.mp4"); fixture(&short, 3);
    let out = root.path().join("out"); fs::create_dir(&out).unwrap();
    let output = still_render(&config, &short, &out, &["--auto", "--verbose"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("tile=9x8:"));
    assert_eq!(stderr.lines().filter(|l| l.starts_with("+ ") && l.contains("thumb_") && !l.contains(" -framerate ")).count(), 72);
    assert_eq!(png_dimensions(&out.join("short_sheet.png")), (310, 164));
    fs::write(&config, "[layout]\nrules=[{max_seconds=10,cols=2,rows=1},{cols=32,rows=1}]\n").unwrap();
    let long = root.path().join("long.mp4"); fixture(&long, 45);
    let output = command(&config).args(["sheet", "--auto", "--width", "320", "--gap", "8", "--no-header", "--no-timestamps", "--overwrite-existing", "-o"])
        .arg(&out).arg(&long).arg(&short).assert().code(1).get_output().clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("too narrow"));
    assert!(stderr.contains("1 done · 0 skipped · 1 failed"));
    assert!(!out.join("long_sheet.png").exists());
    assert_eq!(png_dimensions(&out.join("short_sheet.png")), (320, 98));
    assert!(fs::read_dir(&out).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".acsm-")));
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}

#[tokio::test]
async fn still_auto_rejects_malformed_plans_before_spawn_and_cancels_without_publication() {
    use acsm::{contact_sheet::{generate_auto, SheetOptions}, auto_layout::{resolve, LayoutTable}, jobs::{OutputPolicy, PipelineContext, ProgressReporter}};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let root = TempDir::new().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mp4");
    let tools = acsm::ffmpeg_test_hook_locate().unwrap();
    let info = acsm::ffmpeg_test_hook_probe(&tools, source.to_str().unwrap()).await.unwrap();
    let plan = resolve(&LayoutTable {trim_start_secs: 5, trim_end_secs: 5, rules: acsm::defaults::layout::RULES.to_vec()}, info.sampling_duration_secs(), 320, 4, None).unwrap();
    let opts = SheetOptions {cols: 32, rows: 32, width: 320, gap: 4, thumb_font_size: 14, header_font_size: 18,
        show_timestamps: false, show_header: false, format: acsm::output_path::OutputFormat::Png, jpeg_quality: 92, suffix: String::new(), theme: Default::default()};
    let out = root.path().join("sheet.png"); fs::write(&out, b"old destination").unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let emit = |_: u32, _: u32, _: &str| cancelled.store(true, Ordering::Relaxed);
    let reporter = ProgressReporter {emit: &emit};
    let mut ctx = PipelineContext {output_policy: OutputPolicy::OverwriteExisting, ffmpeg: Path::new("missing-ffmpeg"),
        cancelled: cancelled.clone(), reporter: &reporter, has_zscale: false};
    for case in 0..9 {
        let mut bad = plan.clone();
        match case {
            0 => bad.cols = 0, 1 => bad.rows = 33, 2 => bad.cells += 1,
            3 => { bad.centres.pop(); }, 4 => bad.centres[0] = f64::NAN,
            5 => bad.centres[0] = -1., 6 => bad.centres[0] = info.sampling_duration_secs(),
            7 => bad.duration_secs += 1., _ => bad.cols = 32,
        }
        let result = generate_auto(&source, &info, &out, &opts, &bad, Path::new("unused.ttf"), &ctx).await;
        assert!(matches!(result, Err(acsm::ffmpeg::RunError::Invalid(_))), "{result:?}");
    }
    let mut estimated = info.clone(); estimated.duration_source = acsm::video_info::DurationSource::FrameCount;
    let result = generate_auto(&source, &estimated, &out, &opts, &plan, Path::new("unused.ttf"), &ctx).await;
    assert!(matches!(result, Err(acsm::ffmpeg::RunError::Invalid(ref e)) if e.contains("no timestamps to seek by")));
    ctx.ffmpeg = &tools.ffmpeg;
    let result = generate_auto(&source, &info, &out, &opts, &plan, Path::new("unused.ttf"), &ctx).await;
    assert!(matches!(result, Err(acsm::ffmpeg::RunError::Killed)), "{result:?}");
    assert_eq!(fs::read(&out).unwrap(), b"old destination");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}

#[test]
fn invalid_setup_never_produces_outputs_and_warnings_are_live() {
    let root = TempDir::new().unwrap();
    let config = root.path().join("config.toml");
    let out = root.path().join("out"); fs::create_dir(&out).unwrap();
    fs::write(&config, "invalid TOML").unwrap();
    for flag in ["--cols", "--rows"] {
        let output = command(&config).args(["animated-sheet", "missing.mp4", "--auto", flag, "3", "-o"]).arg(&out)
            .assert().code(2).get_output().clone();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("cannot be used with"));
        assert!(!stderr.contains("config parse failed"));
    }
    for (body, message) in [
        ("rules=[]", "layout.rules must not be empty"),
        ("rules=[{max_seconds=30,cols=3,rows=2}]", "must contain exactly one rule without max_seconds"),
        ("rules=[{cols=3,rows=2},{cols=3,rows=3}]", "; found 2"),
        ("rules=[{cols=3,rows=2},{max_seconds=30,cols=3,rows=2},{max_seconds=30,cols=3,rows=3}]", "duplicate max_seconds = 30 (rules 2 and 3)"),
        ("rules=[{cols=1,rows=1}]", "must produce at least 2 cells (got 1 from 1x1)"),
        ("rules=[{cols=0,rows=2}]", ".cols = 0 out of range (1..=32)"),
        ("rules=[{cols=2,rows=0}]", ".rows = 0 out of range (1..=32)"),
        ("rules=[{cols=3,rows=2},{max_seconds=0,cols=3,rows=2}]", ".max_seconds = 0 out of range (1..)"),
        ("rules=[{cols=3,rows=2,max_second=30}]", "layout.rules[1] has unknown key 'max_second'"),
        ("rules=[{cols=3,rows=2},{max_seconds=30,cols=16,rows=8}]", "produces 128 cells (16x8), over the 64-cell animated auto-layout limit"),
    ] {
        fs::write(&config, format!("[layout]\n{body}\n")).unwrap();
        let output = command(&config).args(["animated-sheet", "--auto", "missing.mp4", "-o"]).arg(&out)
            .assert().code(2).get_output().clone();
        assert!(String::from_utf8_lossy(&output.stderr).contains(message), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    }
    fs::write(&config, "[sheet]\nbogus=99\n[unknown]\nx=1\n").unwrap();
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mp4");
    let output = command(&config).arg("probe").arg(sample).assert().success().get_output().clone();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("warning: unknown key 'sheet.bogus' in "));
    assert!(stderr.contains("warning: unknown section 'unknown' in "));
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    let path = root.path().to_path_buf(); root.close().unwrap(); assert!(!path.exists());
}
