use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Moves a finished output from the job's temp dir to its final path. Every
/// pipeline renders into a temp dir first so a cancelled or failed run never
/// leaves a truncated file at the user-visible path.
pub(crate) fn move_into_place(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // rename fails across filesystems (temp dir vs. an external drive).
    std::fs::rename(src, dst).or_else(|_| std::fs::copy(src, dst).map(|_| ()))
}

/// Reports per-file progress (step, total_steps, label) to the caller.
pub struct ProgressReporter<'a> {
    pub emit: &'a (dyn Fn(u32, u32, &str) + Send + Sync),
}

/// Shared execution environment for all pipeline generate() functions.
pub struct PipelineContext<'a> {
    pub ffmpeg: &'a Path,
    pub cancelled: Arc<AtomicBool>,
    pub reporter: &'a ProgressReporter<'a>,
    /// Whether the ffmpeg binary supports zscale (libzimg) for HDR→SDR tonemapping.
    pub has_zscale: bool,
}
