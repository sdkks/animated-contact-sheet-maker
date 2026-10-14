// src/run/screenshots.rs
use crate::cli::ScreenshotsArgs;
use crate::config::Config;
use crate::progress::Reporter;
use crate::run::{format::resolve_img_format, inputs};
use acsm::{
    defaults,
    ffmpeg::RunError,
    ffmpeg_test_hook_locate, ffmpeg_test_hook_probe,
    jobs::{OutputPolicy, PipelineContext, ProgressReporter},
    output_path::{screenshot_path, DEFAULT_SHOTS_SUFFIX},
    screenshots::{generate, ScreenshotsOptions},
};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

pub async fn run(args: ScreenshotsArgs, cfg: &Config) -> i32 {
    let tools = match ffmpeg_test_hook_locate() {
        Ok(t) => t,
        Err(e) => { eprintln!("{e}"); crate::hints::print_tool_install_hint(); return 2; }
    };
    let inputs = match inputs::expand(&args.shared.inputs, !args.shared.no_recursive, args.shared.null) {
        Ok(v) => v, Err(e) => { eprintln!("{e}"); return 2; }
    };
    if inputs.is_empty() { eprintln!("no input files"); return 2; }

    let caps = tools.detect_caps();
    let has_zscale = caps.has_zscale();
    let cancelled = Arc::new(AtomicBool::new(false));
    crate::signals::install(cancelled.clone());

    let total = inputs.len() as u64;
    let reporter = Reporter::new(total, args.shared.quiet);
    let emit = reporter.emit_fn();
    let pr = ProgressReporter { emit: &emit };

    let count   = args.count.or(cfg.screenshots.count).unwrap_or(defaults::screenshots::COUNT);
    let fmt     = resolve_img_format(&args.format, cfg.screenshots.format.as_deref(), defaults::screenshots::FORMAT);
    let quality = args.quality
        .or(cfg.screenshots.quality)
        .unwrap_or(defaults::screenshots::JPEG_QUALITY);
    let suffix_raw = args.suffix.clone()
        .or_else(|| cfg.screenshots.suffix.clone())
        .unwrap_or_else(|| DEFAULT_SHOTS_SUFFIX.to_string());
    let suffix = match crate::run::suffix::validate(&suffix_raw) {
        Ok(s) => s,
        Err(e) => { eprintln!("{e}"); return 2; }
    };

    let opts = ScreenshotsOptions {
        count, format: fmt, jpeg_quality: quality, suffix,
    };

    if let Err(e) = opts.validate() {
        eprintln!("{e}");
        return 2;
    }

    let mut done = 0u64;
    let mut failed = 0u64;
    let mut skipped = 0u64;
    let output_policy = if args.shared.overwrite_existing { OutputPolicy::OverwriteExisting } else { OutputPolicy::SkipExisting };
    for (i, src) in inputs.iter().enumerate() {
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) { break; }
        let idx = i as u64 + 1;
        reporter.start_file(idx, total, src);
        let out_dir = inputs::out_dir(&args.shared, src);
        let all_existing = (1..=count).try_fold(true, |all, idx| {
            let path = screenshot_path(src, &out_dir, fmt, &opts.suffix, idx, count, &|_| false);
            output_policy.should_skip(&path).map(|skip| all && skip)
        });
        match all_existing {
            Ok(true) => {
                skipped += 1;
                if !args.shared.quiet { reporter.message(&format!("{}: 0 images produced · {count} images skipped (already exist)", src.display())); }
                continue;
            }
            Ok(false) => {}
            Err(e) => { eprintln!("{}: {e}", src.display()); failed += 1; continue; }
        }
        let info = match ffmpeg_test_hook_probe(&tools, &src.to_string_lossy()).await {
            Ok(v) => v,
            Err(e) => { eprintln!("{}: {e}", src.display()); failed += 1; continue; }
        };
        let ctx = PipelineContext {
            output_policy,
            ffmpeg: &tools.ffmpeg,
            cancelled: cancelled.clone(),
            reporter: &pr,
            has_zscale,
        };
        let report = generate(src, &info, &out_dir, &opts, &ctx).await;
        for p in &report.written { println!("{}", p.display()); }
        if !args.shared.quiet {
            reporter.message(&format!("{}: {} images produced · {} images skipped (already exist)", src.display(), report.written.len(), report.skipped));
        }
        match report.result {
            Ok(()) if report.written.is_empty() => { skipped += 1; }
            Ok(()) => { done += 1; }
            Err(RunError::Killed) => { cancelled.store(true, std::sync::atomic::Ordering::Relaxed); }
            Err(e) => { eprintln!("{}: {e}", src.display()); failed += 1; }
        }
        if cancelled.load(std::sync::atomic::Ordering::Relaxed) { break; }
    }
    reporter.finish();
    if !args.shared.quiet {
        eprintln!("{done} done · {skipped} skipped · {failed} failed · {} cancelled",
                  if cancelled.load(std::sync::atomic::Ordering::Relaxed) { total - done - skipped - failed } else { 0 });
    }
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) { return 130; }
    if failed > 0 { 1 } else { 0 }
}
