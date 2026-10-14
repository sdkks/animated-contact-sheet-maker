use crate::ffmpeg::RunError;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::{TempDir, TempPath};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputPolicy {
    #[default]
    SkipExisting,
    OverwriteExisting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishOutcome { Written, Skipped }

impl OutputPolicy {
    /// Presence is sufficient for skipping; directories and special files are errors.
    pub fn should_skip(self, path: &Path) -> io::Result<bool> {
        let occupied = output_occupied(path)?;
        Ok(self == Self::SkipExisting && occupied)
    }
}

fn output_occupied(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() || m.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(io::Error::other(format!("output is not a file or symlink: {}", path.display()))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

struct Prepared {
    path: TempPath,
    // Also owns a possible extra hard link left by persist_noclobber.
    _dir: TempDir,
}

fn prepare(src: &Path, dst: &Path) -> io::Result<Prepared> {
    prepare_with(src, dst, |src, stage| {
        // Copy preserves source permission bits, unlike a contents-only copy
        // into a default-0600 temporary file. Neither branch touches dst.
        std::fs::rename(src, stage).or_else(|_| std::fs::copy(src, stage).map(|_| ()))
    })
}

fn prepare_with(src: &Path, dst: &Path, transfer: impl FnOnce(&Path, &Path) -> io::Result<()>) -> io::Result<Prepared> {
    let parent = dst.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let dir = tempfile::Builder::new().prefix(".acsm-").tempdir_in(parent)?;
    let stage = dir.path().join("prepared");
    transfer(src, &stage)?;
    Ok(Prepared { path: TempPath::try_from_path(stage)?, _dir: dir })
}

fn publish(stage: Prepared, dst: &Path, policy: OutputPolicy, cancelled: &AtomicBool) -> Result<PublishOutcome, RunError> {
    // Validate obstructions without following a final output symlink.
    output_occupied(dst)?;
    if cancelled.load(Ordering::Relaxed) { return Err(RunError::Killed); }
    let result = match policy {
        OutputPolicy::SkipExisting => stage.path.persist_noclobber(dst),
        OutputPolicy::OverwriteExisting => stage.path.persist(dst),
    };
    match result {
        Ok(()) => Ok(PublishOutcome::Written),
        Err(e) if policy == OutputPolicy::SkipExisting && e.error.kind() == io::ErrorKind::AlreadyExists => {
            if output_occupied(dst)? { Ok(PublishOutcome::Skipped) } else { Err(e.error.into()) }
        }
        Err(e) => Err(e.error.into()),
    }
}

/// Prepare privately on the destination filesystem, then commit one complete
/// file. The blocking worker never publishes, even if its async caller is dropped.
pub(crate) async fn move_into_place(src: &Path, dst: &Path, ctx: &PipelineContext<'_>) -> Result<PublishOutcome, RunError> {
    if ctx.cancelled.load(Ordering::Relaxed) { return Err(RunError::Killed); }
    let source = src.to_owned();
    let destination = dst.to_owned();
    let stage = tokio::task::spawn_blocking(move || prepare(&source, &destination))
        .await.map_err(io::Error::other)??;
    publish(stage, dst, ctx.output_policy, &ctx.cancelled)
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
    pub output_policy: OutputPolicy,
    /// Whether the ffmpeg binary supports zscale (libzimg) for HDR→SDR tonemapping.
    pub has_zscale: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn no_stages(dir: &Path) {
        assert!(fs::read_dir(dir).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".acsm-")));
    }

    #[test]
    fn fresh_overwrite_and_late_noclobber_conflict() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("render");
        let dst = dir.path().join("output");
        let cancel = AtomicBool::new(false);
        fs::write(&src, b"first").unwrap();
        let stage = prepare(&src, &dst).unwrap();
        assert!(!dst.exists());
        assert_eq!(publish(stage, &dst, OutputPolicy::SkipExisting, &cancel).unwrap(), PublishOutcome::Written);
        fs::write(&src, b"loser").unwrap();
        let stage = prepare(&src, &dst).unwrap();
        fs::write(&dst, b"race winner").unwrap();
        assert_eq!(publish(stage, &dst, OutputPolicy::SkipExisting, &cancel).unwrap(), PublishOutcome::Skipped);
        assert_eq!(fs::read(&dst).unwrap(), b"race winner");
        fs::write(&src, b"replacement").unwrap();
        assert_eq!(publish(prepare(&src, &dst).unwrap(), &dst, OutputPolicy::OverwriteExisting, &cancel).unwrap(), PublishOutcome::Written);
        assert_eq!(fs::read(&dst).unwrap(), b"replacement");
        no_stages(dir.path());
    }

    #[test]
    fn preparation_copy_failure_cancel_and_persist_failure_preserve_old_bytes() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("render");
        let dst = dir.path().join("output");
        fs::write(&dst, b"old").unwrap();
        assert!(prepare(&src, &dst).is_err());
        fs::write(&src, b"ready").unwrap();
        // Force the rename-fallback copy path and fail after a partial PRIVATE write.
        assert!(prepare_with(&src, &dst, |_, stage| {
            fs::write(stage, b"partial")?;
            Err(io::Error::other("injected stage-copy failure"))
        }).is_err());
        assert_eq!(fs::read(&dst).unwrap(), b"old");
        no_stages(dir.path());
        let stage = prepare(&src, &dst).unwrap();
        assert!(matches!(publish(stage, &dst, OutputPolicy::OverwriteExisting, &AtomicBool::new(true)), Err(RunError::Killed)));
        assert_eq!(fs::read(&dst).unwrap(), b"old");
        no_stages(dir.path());
        fs::write(&src, b"ready").unwrap();
        let stage = prepare(&src, &dst).unwrap();
        // Missing prepared file forces persist itself to fail, not the preflight.
        fs::remove_file(&stage.path).unwrap();
        assert!(publish(stage, &dst, OutputPolicy::OverwriteExisting, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&dst).unwrap(), b"old");
        no_stages(dir.path());
    }

    #[tokio::test]
    async fn cancellation_before_preparation_does_not_move_source() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("render");
        let dst = dir.path().join("output");
        fs::write(&src, b"ready").unwrap();
        fs::write(&dst, b"old").unwrap();
        let reporter = ProgressReporter { emit: &|_, _, _| {} };
        let ctx = PipelineContext { ffmpeg: Path::new("unused"), cancelled: Arc::new(AtomicBool::new(true)), reporter: &reporter, output_policy: OutputPolicy::OverwriteExisting, has_zscale: false };
        assert!(matches!(move_into_place(&src, &dst, &ctx).await, Err(RunError::Killed)));
        assert_eq!(fs::read(&src).unwrap(), b"ready");
        assert_eq!(fs::read(&dst).unwrap(), b"old");
        no_stages(dir.path());
    }

    #[test]
    fn obstruction_and_parent_errors_are_not_skips() {
        let dir = TempDir::new().unwrap();
        let dst = dir.path().join("obstruction");
        fs::create_dir(&dst).unwrap();
        for policy in [OutputPolicy::SkipExisting, OutputPolicy::OverwriteExisting] {
            assert!(policy.should_skip(&dst).is_err());
            let src = dir.path().join("render");
            fs::write(&src, b"ready").unwrap();
            assert!(publish(prepare(&src, &dst).unwrap(), &dst, policy, &AtomicBool::new(false)).is_err());
        }
        let file = dir.path().join("file");
        fs::write(&file, b"keep").unwrap();
        assert!(OutputPolicy::SkipExisting.should_skip(&file.join("child")).is_err());
        assert!(prepare(&file, &file.join("child")).is_err());
        assert_eq!(fs::read(file).unwrap(), b"keep");
        no_stages(dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped_or_replaced_not_followed_and_copy_keeps_permissions() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("render");
        let dst = dir.path().join("output");
        let target = dir.path().join("target");
        symlink(&target, &dst).unwrap();
        assert!(OutputPolicy::SkipExisting.should_skip(&dst).unwrap());
        fs::write(&target, b"untouched").unwrap();
        fs::write(&src, b"ready").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o640)).unwrap();
        let stage = prepare_with(&src, &dst, |s, d| fs::copy(s, d).map(|_| ())).unwrap();
        publish(stage, &dst, OutputPolicy::OverwriteExisting, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        assert_eq!(fs::read(&dst).unwrap(), b"ready");
        assert!(!fs::symlink_metadata(&dst).unwrap().file_type().is_symlink());
        assert_eq!(fs::metadata(&dst).unwrap().permissions().mode() & 0o777, 0o640);
        no_stages(dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn special_file_is_an_obstruction() {
        use std::os::unix::net::UnixListener;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("socket");
        let _socket = UnixListener::bind(&path).unwrap();
        assert!(OutputPolicy::SkipExisting.should_skip(&path).is_err());
        assert!(OutputPolicy::OverwriteExisting.should_skip(&path).is_err());
    }
}
