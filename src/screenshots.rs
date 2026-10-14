use crate::ffmpeg::{run_batch_cancellable, RunError};
use crate::jobs::{move_into_place, PipelineContext, PublishOutcome};
use crate::layout::{require_nonzero, sample_timestamps};
use crate::output_path::{jpeg_qv, screenshot_path, OutputFormat};
use crate::video_info::VideoInfo;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScreenshotsOptions {
    pub count: u32,
    pub format: OutputFormat,
    pub jpeg_quality: u32,
    #[serde(default)]
    pub suffix: String,
}

impl ScreenshotsOptions {
    pub fn validate(&self) -> Result<(), String> {
        require_nonzero("screenshot count", self.count)
    }
}

/// Retains committed paths even when a later image fails or is cancelled.
pub struct ScreenshotsOutcome {
    pub written: Vec<std::path::PathBuf>,
    pub skipped: u32,
    pub result: Result<(), RunError>,
}

pub async fn generate(
    source: &Path,
    info: &VideoInfo,
    out_dir: &Path,
    opts: &ScreenshotsOptions,
    ctx: &PipelineContext<'_>,
) -> ScreenshotsOutcome {
    let mut report = ScreenshotsOutcome { written: Vec::new(), skipped: 0, result: Ok(()) };
    report.result = generate_into(source, info, out_dir, opts, ctx, &mut report).await;
    report
}

async fn generate_into(
    source: &Path,
    info: &VideoInfo,
    out_dir: &Path,
    opts: &ScreenshotsOptions,
    ctx: &PipelineContext<'_>,
    report: &mut ScreenshotsOutcome,
) -> Result<(), RunError> {
    opts.validate().map_err(RunError::Invalid)?;
    info.require_seekable().map_err(RunError::Invalid)?;
    let tmp = tempfile::TempDir::new()?;
    let timestamps = sample_timestamps(info.sampling_duration_secs(), opts.count);

    let tonemap = crate::ffmpeg::tonemap_filter(ctx.has_zscale, info.video.color_transfer.as_deref(), info.video.dv_profile);
    // Anamorphic sources (non-square SAR) need an explicit resize to the
    // displayed dims because PNG and JPEG carry no pixel-aspect metadata
    // most viewers honour. Rotation alone is handled by ffmpeg's autorotate
    // (decoded frame is already upright), so we gate on `sar` being Some.
    let sar_scale: Option<String> = info.video.sar
        .map(|_| format!("scale={}:{}", info.video.width, info.video.height));

    let mut batch = Vec::with_capacity(timestamps.len());
    let mut outputs = Vec::with_capacity(timestamps.len());
    let mut rendered = Vec::with_capacity(timestamps.len());
    for (i, ts) in timestamps.iter().enumerate() {
        let idx = (i as u32) + 1;
        let out = screenshot_path(source, out_dir, opts.format, &opts.suffix, idx, opts.count, &|_| false);
        if ctx.output_policy.should_skip(&out)? { report.skipped += 1; continue; }
        let mut args = crate::ffmpeg::base_args();
        args.extend(crate::ffmpeg::seek_input_args(source, *ts, &info.video));
        args.extend(["-frames:v".into(), "1".into()]);
        let vf: Vec<&str> = [tonemap.as_deref(), sar_scale.as_deref()]
            .into_iter().flatten().collect();
        if !vf.is_empty() {
            args.extend(["-vf".into(), vf.join(",")]);
        }
        if matches!(opts.format, OutputFormat::Jpeg) {
            args.extend(["-q:v".into(), format!("{}", jpeg_qv(opts.jpeg_quality))]);
        }
        let staged = tmp.path().join(out.file_name().unwrap_or_default());
        args.push(staged.to_string_lossy().into_owned());
        batch.push(args);
        rendered.push(staged);
        outputs.push(out);
    }

    let total = batch.len() as u32;
    let mut done = 0u32;
    run_batch_cancellable(ctx.ffmpeg, batch, ctx.cancelled.clone(), |_| {
        done += 1;
        (ctx.reporter.emit)(done, total, &format!("Shot {}/{}", done, total));
    }).await?;

    for (staged, out) in rendered.iter().zip(&outputs) {
        publish_image(staged, out, ctx, report).await?;
    }
    Ok(())
}

async fn publish_image(staged: &Path, out: &Path, ctx: &PipelineContext<'_>, report: &mut ScreenshotsOutcome) -> Result<(), RunError> {
    match move_into_place(staged, out, ctx).await? {
        PublishOutcome::Written => report.written.push(out.to_owned()),
        PublishOutcome::Skipped => report.skipped += 1,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{OutputPolicy, ProgressReporter};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    #[tokio::test]
    async fn partial_publication_retains_paths_on_later_failure_and_cancellation() {
        for cancel in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let source = dir.path().join("render");
            let first = dir.path().join("first.png");
            let second = dir.path().join("second.png");
            let reporter = ProgressReporter { emit: &|_, _, _| {} };
            let ctx = PipelineContext { ffmpeg: Path::new("unused"), cancelled: Arc::new(AtomicBool::new(false)), reporter: &reporter, output_policy: OutputPolicy::OverwriteExisting, has_zscale: false };
            let mut report = ScreenshotsOutcome { written: Vec::new(), skipped: 0, result: Ok(()) };
            std::fs::write(&source, b"first complete").unwrap();
            publish_image(&source, &first, &ctx, &mut report).await.unwrap();
            std::fs::write(&second, b"old second").unwrap();
            // A missing render models a preparation error; cancellation instead
            // leaves a completed second render unpublished.
            if cancel {
                std::fs::write(&source, b"second complete").unwrap();
                ctx.cancelled.store(true, Ordering::Relaxed);
            }
            report.result = publish_image(&source, &second, &ctx, &mut report).await;
            if cancel {
                assert!(matches!(report.result, Err(RunError::Killed)));
            } else {
                assert!(matches!(report.result, Err(RunError::Io(_))));
            }
            assert_eq!(report.skipped, 0);
            assert_eq!(report.written, vec![first.clone()]);
            assert_eq!(std::fs::read(first).unwrap(), b"first complete");
            assert_eq!(std::fs::read(second).unwrap(), b"old second");
        }
    }
}
