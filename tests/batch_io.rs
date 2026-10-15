//! End-to-end batch input, resume and exact-name replacement contracts.
use assert_cmd::Command;
use predicates::prelude::*;
use std::{fs, path::Path};
use tempfile::TempDir;

fn sample() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mp4")
}
fn bin(dir: &TempDir) -> Command {
    let config = dir.path().join("config.toml");
    if !config.exists() { fs::write(&config, "").unwrap(); }
    let mut cmd = Command::cargo_bin("acsm").unwrap();
    cmd.env("ACSM_CONFIG", config);
    cmd
}
fn cases() -> Vec<Vec<&'static str>> {
    vec![
        vec!["sheet", "--cols", "1", "--rows", "1", "--width", "320", "--no-header", "--no-timestamps", "--format", "png"],
        vec!["sheet", "--cols", "1", "--rows", "1", "--width", "320", "--no-header", "--no-timestamps", "--format", "jpeg"],
        vec!["screenshots", "--count", "1", "--format", "png"],
        vec!["screenshots", "--count", "1", "--format", "jpeg"],
        vec!["reel", "--count", "1", "--clip-length", "1", "--height", "120", "--fps", "2", "--format", "webp"],
        vec!["reel", "--count", "1", "--clip-length", "1", "--height", "120", "--fps", "2", "--format", "webm"],
        vec!["reel", "--count", "1", "--clip-length", "1", "--height", "120", "--fps", "2", "--format", "gif"],
        vec!["animated-sheet", "--cols", "2", "--rows", "1", "--width", "320", "--clip-length", "1", "--fps", "2", "--no-header", "--no-timestamps"],
    ]
}

#[test]
fn all_formats_skip_exact_names_and_overwrite_without_ordinals() {
    let original = fs::read(sample()).unwrap();
    for args in cases() {
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("out");
        let first = bin(&dir).args(&args).arg("--quiet").arg("-o").arg(&out).arg(sample())
            .assert().success().stderr("").get_output().stdout.clone();
        let path = std::path::PathBuf::from(String::from_utf8(first.clone()).unwrap().trim_end());
        assert!(fs::metadata(&path).unwrap().len() > 0);
        fs::write(&path, b"old sentinel").unwrap();
        bin(&dir).args(&args).arg("-o").arg(&out).arg(sample()).assert().success().stdout("")
            .stderr(predicate::str::contains("0 done · 1 skipped · 0 failed · 0 cancelled"));
        bin(&dir).args(&args).arg("--quiet").arg("-o").arg(&out).arg(sample()).assert().success().stdout("").stderr("");
        assert_eq!(fs::read(&path).unwrap(), b"old sentinel");
        bin(&dir).args(&args).args(["--quiet", "--overwrite-existing"]).arg("-o").arg(&out).arg(sample())
            .assert().success().stdout(first).stderr("");
        assert_ne!(fs::read(&path).unwrap(), b"old sentinel");
        assert_eq!(fs::read_dir(&out).unwrap().count(), 1, "no ordinals or stage leftovers for {args:?}");
    }
    assert_eq!(fs::read(sample()).unwrap(), original);
}

#[test]
fn screenshots_resume_original_indices_and_timestamps() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("out");
    let args = ["screenshots", "--count", "4", "--quiet"];
    let first = bin(&dir).args(args).arg("-o").arg(&out).arg(sample()).assert().success().get_output().stdout.clone();
    let paths: Vec<_> = String::from_utf8(first).unwrap().lines().map(std::path::PathBuf::from).collect();
    assert_eq!(paths.len(), 4);
    let second_bytes = fs::read(&paths[1]).unwrap();
    let fourth_bytes = fs::read(&paths[3]).unwrap();
    fs::write(&paths[0], b"first retained").unwrap();
    fs::write(&paths[2], b"third retained").unwrap();
    fs::remove_file(&paths[1]).unwrap();
    fs::remove_file(&paths[3]).unwrap();
    bin(&dir).args(["screenshots", "--count", "4"]).arg("-o").arg(&out).arg(sample()).assert().success()
        .stdout(format!("{}\n{}\n", paths[1].display(), paths[3].display()))
        .stderr(predicate::str::contains("2 images produced · 2 images skipped"))
        .stderr(predicate::str::contains("1 done · 0 skipped · 0 failed · 0 cancelled"));
    assert_eq!(fs::read(&paths[1]).unwrap(), second_bytes, "original timestamp for index 2");
    assert_eq!(fs::read(&paths[3]).unwrap(), fourth_bytes, "original timestamp for index 4");
    assert_eq!(fs::read(&paths[0]).unwrap(), b"first retained");
    assert_eq!(fs::read(&paths[2]).unwrap(), b"third retained");
    assert_eq!(fs::read_dir(&out).unwrap().count(), 4);
}

#[test]
fn stdin_crlf_batches_reach_all_four_renderers_in_order() {
    for args in cases().into_iter().filter(|a| a.last() != Some(&"jpeg") && a.last() != Some(&"webm") && a.last() != Some(&"gif")) {
        let dir = TempDir::new().unwrap();
        let before = dir.path().join("before.mp4");
        let middle = dir.path().join("literal '雪' space.mp4");
        let after = dir.path().join("after.mp4");
        for p in [&before, &middle, &after] { fs::copy(sample(), p).unwrap(); }
        let out = dir.path().join("out");
        let result = bin(&dir).args(&args).arg("--quiet").arg("-o").arg(&out).arg(&before).arg("-").arg(&after)
            .write_stdin(format!("\r\n{}\r\n", middle.display())).assert().success().stderr("").get_output().stdout.clone();
        let text = String::from_utf8(result).unwrap();
        let paths: Vec<_> = text.lines().collect();
        assert_eq!(paths.len(), 3, "{text}");
        assert!(paths[0].contains("before"));
        assert!(paths[1].contains("literal '雪' space"));
        assert!(paths[2].contains("after"));
        for path in paths { assert!(Path::new(path).is_file()); }
    }
}

#[cfg(unix)]
#[test]
fn nul_input_preserves_unusual_names_and_literal_dash_record() {
    let dir = TempDir::new().unwrap();
    let weird = dir.path().join("  '雪'\\\n\r.mp4");
    fs::copy(sample(), &weird).unwrap();
    fs::copy(sample(), dir.path().join("-")).unwrap();
    let out = dir.path().join("out");
    let result = bin(&dir).current_dir(dir.path()).args(["screenshots", "--count", "1", "--quiet", "-0", "-o"])
        .arg(&out).arg("-").write_stdin(format!("{}\0-", weird.display())).assert().success().stderr("")
        .get_output().stdout.clone();
    let expected = [out.join("  '雪'\\\n\r_screens_01.png"), out.join("-_screens_01.png")];
    assert_eq!(String::from_utf8(result).unwrap(), format!("{}\n{}\n", expected[0].display(), expected[1].display()));
    for path in expected { assert!(path.is_file()); }
}

#[test]
fn invalid_lists_are_input_errors_and_flags_do_not_change_probe() {
    let dir = TempDir::new().unwrap();
    for (args, data, message) in [
        (vec!["sheet", "-", "-"], Vec::new(), "only once"),
        (vec!["sheet", "--null", "missing.mp4"], Vec::new(), "requires '-'"),
        (vec!["sheet", "-"], vec![255], "UTF-8"),
        (vec!["sheet", "-"], b"a\0b".to_vec(), "use --null"),
        (vec!["sheet", "-"], b"\r\n\n".to_vec(), "no input files"),
    ] {
        bin(&dir).args(args).write_stdin(data).assert().code(2).stdout("").stderr(predicate::str::contains(message));
    }
    for flag in ["--null", "--overwrite-existing"] {
        bin(&dir).args(["probe", flag]).arg(sample()).assert().code(2);
    }
}

#[test]
fn exact_names_respect_config_suffix_format_type_and_output_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("config.toml"), "[sheet]\nsuffix = '_configured'\n").unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let configured = out.join("sample_configured.png");
    fs::write(&configured, b"configured sentinel").unwrap();
    let args = ["sheet", "--cols", "1", "--rows", "1", "--width", "320", "--no-header", "--no-timestamps", "--quiet"];
    bin(&dir).args(args).arg("-o").arg(&out).arg(sample()).assert().success().stdout("").stderr("");
    for (flags, name) in [
        (vec!["--format", "jpeg"], "sample_configured.jpg"),
        (vec!["--suffix", "_other"], "sample_other.png"),
        (vec!["--suffix", ""], "sample_sheet.png"),
    ] {
        bin(&dir).args(args).args(flags).arg("-o").arg(&out).arg(sample()).assert().success()
            .stdout(format!("{}\n", out.join(name).display())).stderr("");
        assert!(fs::metadata(out.join(name)).unwrap().len() > 0);
    }
    let elsewhere = dir.path().join("elsewhere");
    bin(&dir).args(args).arg("-o").arg(&elsewhere).arg(sample()).assert().success()
        .stdout(format!("{}\n", elsewhere.join("sample_configured.png").display()));
    // A WebP from one renderer is not evidence that another type is complete.
    fs::write(out.join("sample_reel.webp"), b"reel sentinel").unwrap();
    bin(&dir).args(cases().last().unwrap()).args(["--quiet", "-o"]).arg(&out).arg(sample())
        .assert().success().stdout(format!("{}\n", out.join("sample_animated_sheet.webp").display()));
    assert_eq!(fs::read(configured).unwrap(), b"configured sentinel");
    assert_eq!(fs::read(out.join("sample_reel.webp")).unwrap(), b"reel sentinel");
}

#[tokio::test]
async fn screenshot_late_obstruction_race_and_cancel_keep_accurate_report() {
    use acsm::jobs::{OutputPolicy, PipelineContext, ProgressReporter};
    use acsm::screenshots::{generate, ScreenshotsOptions};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let tools = acsm::ffmpeg_test_hook_locate().unwrap();
    let source = sample();
    let info = acsm::ffmpeg_test_hook_probe(&tools, source.to_str().unwrap()).await.unwrap();
    for scenario in ["obstruction", "race", "cancel"] {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("sample_screens_01.png");
        let second = dir.path().join("sample_screens_02.png");
        let cancelled = Arc::new(AtomicBool::new(false));
        let emit = |done, total, _: &str| {
            if done == total {
                match scenario {
                    "obstruction" => fs::create_dir(&second).unwrap(),
                    "race" => fs::write(&second, b"race winner").unwrap(),
                    _ => cancelled.store(true, Ordering::Relaxed),
                }
            }
        };
        let reporter = ProgressReporter { emit: &emit };
        let ctx = PipelineContext { ffmpeg: &tools.ffmpeg, cancelled: cancelled.clone(), reporter: &reporter,
            output_policy: OutputPolicy::SkipExisting, has_zscale: false };
        let result = generate(&source, &info, dir.path(), &ScreenshotsOptions {
            count: 2, format: acsm::output_path::OutputFormat::Png, jpeg_quality: 92, suffix: String::new(),
        }, &ctx).await;
        match scenario {
            "obstruction" => {
                assert!(matches!(result.result, Err(acsm::ffmpeg::RunError::Io(_))));
                assert_eq!(result.written, vec![first.clone()]);
                assert_eq!(result.skipped, 0);
                assert!(first.is_file());
                assert!(second.is_dir());
            }
            "race" => {
                result.result.unwrap();
                assert_eq!(result.written, vec![first.clone()]);
                assert_eq!(result.skipped, 1);
                assert!(first.is_file());
                assert_eq!(fs::read(&second).unwrap(), b"race winner");
            }
            _ => {
                assert!(matches!(result.result, Err(acsm::ffmpeg::RunError::Killed)));
                assert!(result.written.is_empty());
                assert_eq!(result.skipped, 0);
                assert!(!first.exists());
                assert!(!second.exists());
            }
        }
        assert!(fs::read_dir(dir.path()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".acsm-")));
    }
}

#[tokio::test]
async fn all_renderers_preserve_old_output_on_render_failure_and_cancellation() {
    use acsm::{animated_sheet, contact_sheet, preview_reel, screenshots};
    use acsm::jobs::{OutputPolicy, PipelineContext, ProgressReporter};
    use acsm::output_path::{OutputFormat, ReelFormat};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let tools = acsm::ffmpeg_test_hook_locate().unwrap();
    let source = sample();
    let info = acsm::ffmpeg_test_hook_probe(&tools, source.to_str().unwrap()).await.unwrap();
    for cancel in [false, true] {
        let dir = TempDir::new().unwrap();
        let missing_ffmpeg = dir.path().join("missing-ffmpeg");
        let cancelled = Arc::new(AtomicBool::new(false));
        let emit = |_: u32, _: u32, _: &str| { if cancel { cancelled.store(true, Ordering::Relaxed); } };
        let reporter = ProgressReporter { emit: &emit };
        let ctx = PipelineContext { ffmpeg: if cancel { &tools.ffmpeg } else { &missing_ffmpeg },
            cancelled: cancelled.clone(), reporter: &reporter, output_policy: OutputPolicy::OverwriteExisting, has_zscale: false };
        let font = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/DejaVuSans.ttf");
        for kind in ["sheet", "screenshots", "reel", "animated-sheet"] {
            cancelled.store(false, Ordering::Relaxed);
            let out = dir.path().join(if kind == "screenshots" { "sample_screens_01.png" } else { "output" });
            fs::write(&out, b"old sentinel").unwrap();
            let result = match kind {
                "sheet" => contact_sheet::generate(&source, &info, &out, &contact_sheet::SheetOptions {
                    cols: 1, rows: 1, width: 320, gap: 2, thumb_font_size: 14, header_font_size: 16,
                    show_timestamps: false, show_header: false, format: OutputFormat::Png, jpeg_quality: 92,
                    suffix: String::new(), theme: Default::default(),
                }, &font, &ctx).await.map(|_| ()),
                "screenshots" => {
                    let report = screenshots::generate(&source, &info, dir.path(), &screenshots::ScreenshotsOptions {
                        count: 1, format: OutputFormat::Png, jpeg_quality: 92, suffix: String::new(),
                    }, &ctx).await;
                    assert!(report.written.is_empty());
                    assert_eq!(report.skipped, 0);
                    report.result
                }
                "reel" => preview_reel::generate(&source, &info, &out, &preview_reel::PreviewOptions {
                    count: 1, clip_length_secs: 1, height: 120, fps: 2, quality: 60, suffix: String::new(), format: ReelFormat::Webp,
                }, &ctx).await.map(|_| ()),
                _ => animated_sheet::generate(&source, &info, &out, &animated_sheet::AnimatedSheetOptions {
                    cols: 2, rows: 1, width: 320, gap: 2, clip_length_secs: 1, fps: 2, quality: 60,
                    compression_effort: acsm::defaults::animated_sheet::COMPRESSION_EFFORT,
                    thumb_font_size: 14, header_font_size: 16, show_timestamps: false, show_header: false,
                    suffix: String::new(), theme: Default::default(),
                }, &font, &ctx).await.map(|_| ()),
            };
            if cancel { assert!(matches!(result, Err(acsm::ffmpeg::RunError::Killed)), "{kind}: {result:?}"); }
            else { assert!(matches!(result, Err(acsm::ffmpeg::RunError::Io(_))), "{kind}: {result:?}"); }
            assert_eq!(fs::read(&out).unwrap(), b"old sentinel", "{kind}");
            assert!(fs::read_dir(dir.path()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".acsm-")));
        }
    }
}

#[test]
fn failed_render_preserves_existing_output_and_reports_processing_error() {
    let dir = TempDir::new().unwrap();
    let src = dir.path().join("broken.mp4");
    fs::write(&src, b"not a video").unwrap();
    let dst = dir.path().join("broken_sheet.png");
    fs::write(&dst, b"old").unwrap();
    bin(&dir).args(["sheet", "--quiet", "--overwrite-existing"]).arg(&src).assert().code(1).stdout("");
    assert_eq!(fs::read(&dst).unwrap(), b"old");
    // Default skip occurs before per-source probing, but after global validation.
    bin(&dir).args(["sheet", "--quiet"]).arg(&src).assert().success().stdout("").stderr("");
}
