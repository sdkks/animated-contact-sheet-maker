use crate::drawtext::{font_for_ffmpeg, format_hms_escaped, header_overlay, timestamp_overlay};
use crate::ffmpeg::{run_batch_cancellable, run_cancellable, Need, RunError};
use crate::header::build_header_lines;
use crate::jobs::{move_into_place, PipelineContext, PublishOutcome};
use crate::layout::{checked_sheet_layout, compute_sheet_layout, header_height, line_height, require_nonzero, sample_clip_timestamps, thumb_height, xstack_layout, SheetLayout};
use crate::output_path::SheetTheme;
use crate::video_info::VideoInfo;
use std::path::{Path, PathBuf};

/// xstack requires at least 2 inputs and accepts far more than 32.
/// This static upper bound limits stitch memory (one decoder per cell),
/// rather than reflecting an upstream filter limit.
const MIN_CELLS: u32 = 2;
const MAX_CELLS: u32 = 32;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnimatedSheetOptions {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub gap: u32,
    pub clip_length_secs: u32,
    pub fps: u32,
    pub quality: u32,
    /// WebP encoding effort (0..=6), independent of quality; zero is lowest effort.
    #[serde(default = "default_compression_effort")]
    pub compression_effort: u32,
    pub thumb_font_size: u32,
    pub header_font_size: u32,
    pub show_timestamps: bool,
    pub show_header: bool,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub theme: SheetTheme,
}

fn default_compression_effort() -> u32 {
    crate::defaults::animated_sheet::COMPRESSION_EFFORT
}

impl AnimatedSheetOptions {
    pub fn validate(&self) -> Result<(), String> {
        let layout = checked_sheet_layout(self.cols, self.rows, self.width, self.gap)?;
        if layout.total < MIN_CELLS || layout.total > MAX_CELLS {
            return Err(format!(
                "animated contact sheet requires {}..={} cells; requested {} (cols={}, rows={})",
                MIN_CELLS, MAX_CELLS, layout.total, self.cols, self.rows
            ));
        }
        self.validate_common()
    }

    /// Non-grid checks also apply when the grid comes from an auto plan.
    pub fn validate_common(&self) -> Result<(), String> {
        require_nonzero("clip length", self.clip_length_secs)?;
        require_nonzero("fps", self.fps)?;
        if self.compression_effort > 6 {
            return Err(format!("compression effort {} out of range (0..=6)", self.compression_effort));
        }
        Ok(())
    }

    pub fn needs(&self) -> Vec<Need> {
        let mut needs = vec![Need::Libx264, Need::Libwebp];
        if self.show_timestamps || self.show_header { needs.push(Need::Drawtext); }
        needs
    }
}

pub(crate) struct HeaderParams {
    pub(crate) textfiles: Vec<PathBuf>,
    pub(crate) height: u32,
    pub(crate) line_h: u32,
    pub(crate) font_size: u32,
    pub(crate) font_ffmpeg: String,
}

#[allow(clippy::too_many_arguments)]
pub fn build_extract_args(
    source: &Path,
    video: &crate::video_info::VideoStream,
    timestamp: f64,
    label_timestamp: f64,
    thumb_w: u32,
    thumb_h: u32,
    gap: u32,
    fps: u32,
    clip_length_secs: f64,
    show_timestamps: bool,
    thumb_font_size: u32,
    theme: SheetTheme,
    font: &Path,
    output: &Path,
    hdr_filter: Option<&str>,
) -> Vec<String> {
    let scale = format!("scale={}:{}", thumb_w, thumb_h);
    let ts_filter = show_timestamps.then(|| timestamp_overlay(
        &format_hms_escaped(label_timestamp),
        &font_for_ffmpeg(font),
        thumb_font_size,
        theme.fontcolor(),
        theme.shadowcolor(),
    ));
    let pad = format!(
        "pad={}:{}:{}:{}:{}",
        thumb_w + gap, thumb_h + gap, gap / 2, gap / 2, theme.bg()
    );
    // Sub-frame auto cells still need one retained frame at the requested rate.
    // Static clips are whole seconds, so their presentation timestamps stay unchanged.
    let reset_pts = (clip_length_secs > 0.0 && clip_length_secs < 1.0 / f64::from(fps))
        .then_some("setpts=PTS-STARTPTS");
    let vf = crate::ffmpeg::vf_chain(&[hdr_filter, Some(&scale), ts_filter.as_deref(), Some(&pad), reset_pts]);

    let mut args = crate::ffmpeg::base_args();
    args.extend(crate::ffmpeg::seek_input_args_clip(source, timestamp, video));
    args.extend([
        "-t".into(), crate::ffmpeg::format_seconds(clip_length_secs),
        "-vf".into(), vf,
        "-r".into(), format!("{}", fps),
    ]);
    args.extend(crate::ffmpeg::h264_clip_encoder());
    args.push(output.to_string_lossy().into_owned());
    args
}

/// Build the single `-filter_complex` graph that:
///   1. xstacks N pre-padded cells into a grid,
///   2. pads the grid by gap/2 on all sides so outer margin matches the still
///      contact sheet (total width = `layout.grid_w`),
///   3. optionally composites a header panel (color source + per-line drawtext) on
///      top via vstack.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_stitch_args(
    clips: &[PathBuf],
    layout: &SheetLayout,
    thumb_h: u32,
    gap: u32,
    theme: SheetTheme,
    header: Option<&HeaderParams>,
    clip_length_secs: f64,
    fps: u32,
    quality: u32,
    compression_effort: u32,
    output: &Path,
) -> Vec<String> {
    let n = clips.len() as u32;
    let step_w = layout.thumb_w + gap;
    let step_h = thumb_h + gap;
    let grid_w = layout.grid_w;
    let grid_h = layout.rows * step_h + gap;
    let bg = theme.bg();

    let mut args = crate::ffmpeg::base_args();
    for clip in clips {
        // Bound decoder buffers when many cached cells are opened together.
        args.extend(["-threads:v".into(), "1".into(), "-i".into(), clip.to_string_lossy().into_owned()]);
    }

    let inputs_tag: String = (0..n).map(|i| format!("[{}:v]", i)).collect();
    let layout_expr = xstack_layout(layout.cols, layout.rows, step_w, step_h);

    let mut graph = format!(
        "{}xstack=inputs={}:layout={}[xs];[xs]pad={}:{}:{}:{}:{}[grid]",
        inputs_tag, n, layout_expr, grid_w, grid_h, gap / 2, gap / 2, bg
    );

    let final_label = if let Some(h) = header {
        let hdr_draw = header_overlay(&h.textfiles, &h.font_ffmpeg, h.font_size, theme.fontcolor(), gap, h.line_h);
        graph.push_str(&format!(
            ";color=c={}:s={}x{}:d={}:r={},{}[hdr];[hdr][grid]vstack[out]",
            bg, grid_w, h.height, crate::ffmpeg::format_seconds(clip_length_secs), fps, hdr_draw
        ));
        "[out]"
    } else {
        "[grid]"
    };

    args.extend([
        "-filter_complex".into(), graph,
        "-map".into(), final_label.into(),
        "-c:v".into(), "libwebp".into(),
        "-loop".into(), "0".into(),
        "-quality".into(), format!("{}", quality),
        "-compression_level".into(), compression_effort.to_string(),
        output.to_string_lossy().into_owned(),
    ]);
    args
}

pub async fn generate(
    source: &Path,
    info: &VideoInfo,
    out: &Path,
    opts: &AnimatedSheetOptions,
    font: &Path,
    ctx: &PipelineContext<'_>,
) -> Result<PublishOutcome, RunError> {
    opts.validate().map_err(RunError::Invalid)?;
    info.require_seekable().map_err(RunError::Invalid)?;
    let layout = compute_sheet_layout(opts.cols, opts.rows, opts.width, opts.gap);

    let timestamps = sample_clip_timestamps(info.sampling_duration_secs(), layout.total, opts.clip_length_secs as f64);
    if timestamps.is_empty() {
        return Err(RunError::Invalid(format!(
            "source duration ({:.2}s) is too short for {}s clips",
            info.sampling_duration_secs(), opts.clip_length_secs
        )));
    }

    let clips = Clips { layout, labels: timestamps.clone(), timestamps, length: f64::from(opts.clip_length_secs) };
    render(source, info, out, opts, font, ctx, clips).await
}

struct Clips {
    layout: SheetLayout,
    timestamps: Vec<f64>,
    labels: Vec<f64>,
    length: f64,
}

pub async fn generate_auto(
    source: &Path,
    info: &VideoInfo,
    out: &Path,
    opts: &AnimatedSheetOptions,
    plan: &crate::auto_layout::AutoPlan,
    font: &Path,
    ctx: &PipelineContext<'_>,
) -> Result<PublishOutcome, RunError> {
    opts.validate_common().map_err(RunError::Invalid)?;
    info.require_seekable().map_err(RunError::Invalid)?;
    let clips = auto_clips(info, opts, plan).map_err(RunError::Invalid)?;
    render(source, info, out, opts, font, ctx, clips).await
}

fn auto_clips(info: &VideoInfo, opts: &AnimatedSheetOptions, plan: &crate::auto_layout::AutoPlan) -> Result<Clips, String> {
    use crate::auto_layout::{MIN_AUTO_CELLS, MAX_AUTO_CELLS};
    if plan.cells > MAX_AUTO_CELLS {
        return Err(format!("auto plan has {} cells ({}x{}), over the {MAX_AUTO_CELLS}-cell animated auto-layout limit", plan.cells, plan.cols, plan.rows));
    }
    if plan.cells < MIN_AUTO_CELLS || !(1..=32).contains(&plan.cols) || !(1..=32).contains(&plan.rows)
        || plan.cols.checked_mul(plan.rows) != Some(plan.cells)
        || plan.clip_starts.len() != plan.cells as usize || plan.centres.len() != plan.cells as usize {
        return Err("auto plan grid and cell counts are inconsistent".into());
    }
    let length = plan.cell_length_secs.filter(|l| l.is_finite() && *l > 0.0)
        .ok_or("auto plan cell length must be positive and finite")?;
    let duration = info.sampling_duration_secs();
    if !plan.duration_secs.is_finite() || plan.duration_secs <= 0.0 || plan.duration_secs != duration {
        return Err("auto plan duration must be positive, finite and match the selected video".into());
    }
    let epsilon = (32.0 * f64::EPSILON * duration).max(1e-9);
    let usable = duration - plan.trim_start_secs - plan.trim_end_secs;
    if [plan.trim_start_secs, plan.trim_end_secs].iter().any(|t| !t.is_finite() || *t < 0.0 || *t > duration / 4.0)
        || !plan.usable_secs.is_finite() || (plan.usable_secs - usable).abs() > epsilon
        || !plan.segment_secs.is_finite() || plan.segment_secs <= 0.0
        || (plan.segment_secs - usable / f64::from(plan.cells)).abs() > epsilon
        || (length - plan.segment_secs.min(f64::from(opts.clip_length_secs))).abs() > epsilon
        || plan.decoded_secs.is_none_or(|d| !d.is_finite() || (d - length * f64::from(plan.cells)).abs() > epsilon)
        || plan.clip_starts.iter().enumerate().any(|(i, start)| {
            let expected = plan.trim_start_secs + i as f64 * plan.segment_secs + (plan.segment_secs - length) / 2.0;
            (start - expected).abs() > epsilon
        }) {
        return Err("auto plan segment geometry is inconsistent".into());
    }
    if length > f64::from(opts.clip_length_secs) || plan.clip_starts.iter().zip(&plan.centres).any(|(&start, &centre)| {
        !start.is_finite() || !centre.is_finite() || start < 0.0 || start + length > duration + epsilon
            || (start + length / 2.0 - centre).abs() > epsilon
    }) || plan.clip_starts.windows(2).any(|pair| pair[0] + length > pair[1] + epsilon) {
        return Err("auto plan cell spans are inconsistent or outside the selected video".into());
    }
    let layout = checked_sheet_layout(plan.cols, plan.rows, opts.width, opts.gap)?;
    Ok(Clips { layout, timestamps: plan.clip_starts.clone(), labels: plan.centres.clone(), length })
}

async fn render(
    source: &Path,
    info: &VideoInfo,
    out: &Path,
    opts: &AnimatedSheetOptions,
    font: &Path,
    ctx: &PipelineContext<'_>,
    cells: Clips,
) -> Result<PublishOutcome, RunError> {
    let layout = cells.layout;
    let thumb_h = thumb_height(layout.thumb_w, info.video.width, info.video.height);
    let timestamps = &cells.timestamps;
    let total_steps = layout.total + 1;
    let tmp = tempfile::TempDir::new()?;

    let hdr_filter = crate::ffmpeg::tonemap_filter(ctx.has_zscale, info.video.color_transfer.as_deref(), info.video.dv_profile);
    let mut clips: Vec<PathBuf> = Vec::with_capacity(timestamps.len());
    let mut batch = Vec::with_capacity(timestamps.len());
    for (i, ts) in timestamps.iter().enumerate() {
        let idx = (i as u32) + 1;
        let cell = tmp.path().join(format!("cell_{:03}.mp4", idx));
        let args = build_extract_args(
            source, &info.video, *ts, cells.labels[i], layout.thumb_w, thumb_h, layout.gap, opts.fps,
            cells.length, opts.show_timestamps, opts.thumb_font_size,
            opts.theme, font, &cell, hdr_filter.as_deref(),
        );
        batch.push(args);
        clips.push(cell);
    }

    let mut done = 0u32;
    run_batch_cancellable(ctx.ffmpeg, batch, ctx.cancelled.clone(), |_| {
        done += 1;
        (ctx.reporter.emit)(done, total_steps, &format!("Cell {}/{}", done, layout.total));
    }).await?;

    (ctx.reporter.emit)(total_steps, total_steps, "Stitching sheet");

    let header_params = if opts.show_header {
        let display = source.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let lines = build_header_lines(info, &display);
        let line_count = lines.len() as u32;
        // These files remain in the job TempDir until the stitch process returns.
        let textfiles = lines.iter().enumerate().map(|(i, line)| {
            let path = tmp.path().join(format!("header_{i}.txt"));
            std::fs::write(&path, line)?;
            Ok(path)
        }).collect::<std::io::Result<Vec<_>>>()?;
        Some(HeaderParams {
            textfiles,
            height: header_height(opts.header_font_size, layout.gap, line_count),
            line_h: line_height(opts.header_font_size),
            font_size: opts.header_font_size,
            font_ffmpeg: font_for_ffmpeg(font),
        })
    } else {
        None
    };

    let staged = tmp.path().join("sheet.webp");
    let args = build_stitch_args(
        &clips, &layout, thumb_h, layout.gap, opts.theme, header_params.as_ref(),
        cells.length, opts.fps, opts.quality, opts.compression_effort, &staged,
    );
    run_cancellable(ctx.ffmpeg, &args, ctx.cancelled.clone()).await?;
    move_into_place(&staged, out, ctx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn sample_info(duration: f64, w: u32, h: u32) -> VideoInfo {
        VideoInfo {
            filename: String::new(),
            duration_secs: duration,
            duration_source: Default::default(),
            size_bytes: None,
            bit_rate: None,
            video: crate::video_info::VideoStream {
                index: 0, duration_secs: None, start_secs: 0.0, seek_offset_secs: 0.0,
                codec: String::new(),
                profile: None,
                width: w,
                height: h,
                fps: 30.0,
                bit_rate: None,
                is_hdr: false,
                color_transfer: None,
                dv_profile: None,
                rotation: None,
                sar: None,
            },
            audio: None,
            enrichment: None,
        }
    }

    #[test]
    fn compression_effort_validates_and_defaults_for_deserialized_options() {
        let legacy = serde_json::json!({
            "cols": 2, "rows": 1, "width": 320, "gap": 8,
            "clip_length_secs": 1, "fps": 4, "quality": 75,
            "thumb_font_size": 14, "header_font_size": 18,
            "show_timestamps": false, "show_header": false
        });
        let mut opts: AnimatedSheetOptions = serde_json::from_value(legacy).unwrap();
        assert_eq!(opts.compression_effort, 4);
        for effort in 0..=6 {
            opts.compression_effort = effort;
            assert!(opts.validate().is_ok());
        }
        opts.compression_effort = 7;
        assert_eq!(opts.validate().unwrap_err(), "compression effort 7 out of range (0..=6)");
    }

    #[test]
    fn stitch_args_pass_each_compression_effort_without_other_changes() {
        let layout = compute_sheet_layout(2, 1, 320, 8);
        let clips = vec![PathBuf::from("a.mp4"), PathBuf::from("b.mp4")];
        let mut reference = None;
        for effort in 0..=6 {
            let mut args = build_stitch_args(
                &clips, &layout, 80, 8, SheetTheme::Dark, None, 1.0, 4, 75, effort,
                Path::new("sheet.webp"),
            );
            let pos = args.iter().position(|arg| arg == "-compression_level").unwrap();
            assert_eq!(args[pos + 1], effort.to_string());
            assert_eq!(args.iter().filter(|arg| *arg == "-compression_level").count(), 1);
            assert_eq!(pos + 2, args.len() - 1, "effort must be an output option");
            args.drain(pos..pos + 2);
            if let Some(ref expected) = reference { assert_eq!(&args, expected); }
            else { reference = Some(args); }
        }
    }

    #[test]
    fn stitch_args_limit_each_input_decoder_without_changing_output_options() {
        for (cols, rows) in [(2, 1), (6, 3), (8, 4)] {
            let layout = compute_sheet_layout(cols, rows, 1280, 8);
            let clips: Vec<PathBuf> = (0..layout.total)
                .map(|i| PathBuf::from(format!("cached cells/cell_{i}.mp4"))).collect();
            for effort in [4, 1] {
                let args = build_stitch_args(
                    &clips, &layout, 80, 8, SheetTheme::Dark, None, 2.0, 12, 75, effort,
                    Path::new("sheet.webp"),
                );
                let base = crate::ffmpeg::base_args();
                assert_eq!(&args[..base.len()], &base);
                let fc = args.iter().position(|arg| arg == "-filter_complex").unwrap();
                let inputs = &args[base.len()..fc];
                assert_eq!(inputs.len(), clips.len() * 4);
                for (input, clip) in inputs.as_chunks::<4>().0.iter().zip(&clips) {
                    assert_eq!(input, &["-threads:v", "1", "-i", clip.to_str().unwrap()]);
                }
                let effort_arg = effort.to_string();
                assert_eq!(&args[fc + 2..], [
                    "-map", "[grid]", "-c:v", "libwebp", "-loop", "0",
                    "-quality", "75", "-compression_level", &effort_arg, "sheet.webp",
                ]);
            }
        }
    }

    #[test]
    fn extract_args_shape() {
        let args = build_extract_args(
            Path::new("/v/movie.mkv"),
            &sample_info(60.0, 1920, 1080).video,
            12.5, 12.5,
            320, 180, 10, 12, 2.0,
            true, 18, SheetTheme::Dark,
            Path::new("/f/font.ttf"),
            Path::new("/tmp/cell.mp4"),
            None,
        );
        assert_eq!(args[0], "-hide_banner");
        assert!(args.iter().any(|a| a == "-an"));
        // Clip seeking: single -ss (no -copyts) for compatibility with TS containers
        assert!(!args.iter().any(|a| a == "-copyts"));
        assert!(args.windows(2).any(|w| w[0] == "-ss" && w[1] == "12.500"));
        assert!(args.windows(2).any(|w| w[0] == "-i" && w[1] == "/v/movie.mkv"));
        assert!(args.windows(2).any(|w| w[0] == "-t" && w[1] == "2"));
        assert!(args.windows(2).any(|w| w[0] == "-r" && w[1] == "12"));
        assert!(args.windows(2).any(|w| w[0] == "-c:v" && w[1] == "libx264"));
        assert!(args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "yuv420p"));
        let vf_pos = args.iter().position(|a| a == "-vf").unwrap();
        // Stitch decoder limits must not alter source decoding or clip encoding.
        assert_eq!(&args[..vf_pos], [
            "-hide_banner", "-loglevel", "error", "-y", "-ss", "12.500",
            "-i", "/v/movie.mkv", "-map", "0:0", "-an", "-t", "2",
        ]);
        assert_eq!(&args[vf_pos + 2..], [
            "-r", "12", "-c:v", "libx264", "-preset", "veryfast", "-crf", "23",
            "-pix_fmt", "yuv420p", "/tmp/cell.mp4",
        ]);
        let vf = &args[vf_pos + 1];
        assert!(vf.contains("scale=320:180"));
        assert!(vf.contains("drawtext="));
        assert!(vf.contains("pad=330:190:5:5:0x000000"));
        assert_eq!(args.last().unwrap(), "/tmp/cell.mp4");
    }

    #[test]
    fn extract_args_light_theme_uses_white_bg_and_black_text() {
        let args = build_extract_args(
            Path::new("/v/movie.mkv"),
            &sample_info(60.0, 1920, 1080).video,
            5.0, 5.0,
            320, 180, 10, 12, 2.0,
            true, 18, SheetTheme::Light,
            Path::new("/f/font.ttf"),
            Path::new("/tmp/cell.mp4"),
            None,
        );
        let vf = args.iter().position(|a| a == "-vf").map(|i| &args[i + 1]).unwrap();
        assert!(vf.contains("pad=330:190:5:5:0xFFFFFF"));
        assert!(vf.contains("fontcolor=black"));
        assert!(vf.contains("shadowcolor=white"));
    }

    #[test]
    fn extract_args_omits_drawtext_when_timestamps_off() {
        let args = build_extract_args(
            Path::new("/v/movie.mkv"),
            &sample_info(60.0, 1920, 1080).video,
            5.0, 5.0,
            320, 180, 10, 12, 2.0,
            false, 18, SheetTheme::Dark,
            Path::new("/f/font.ttf"),
            Path::new("/tmp/cell.mp4"),
            None,
        );
        let vf = args.iter().position(|a| a == "-vf").map(|i| &args[i + 1]).unwrap();
        assert!(!vf.contains("drawtext"));
        assert!(vf.contains("scale=320:180"));
        assert!(vf.contains("pad=330:190:5:5:0x000000"));
    }

    #[test]
    fn stitch_args_no_header_maps_grid() {
        let layout = compute_sheet_layout(2, 2, 800, 10);
        let clips: Vec<PathBuf> = (0..4).map(|i| PathBuf::from(format!("/tmp/c{}.mp4", i))).collect();
        let args = build_stitch_args(
            &clips, &layout, 200, 10, SheetTheme::Dark, None, 3.0, 12, 75, 4,
            Path::new("/out/sheet.webp"),
        );
        // 4 inputs
        assert_eq!(args.iter().filter(|a| *a == "-i").count(), 4);
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        let graph = &args[fc + 1];
        assert_eq!(graph, "[0:v][1:v][2:v][3:v]xstack=inputs=4:layout=0_0|394_0|0_210|394_210[xs];[xs]pad=798:430:5:5:0x000000[grid]");
        let map_pos = args.iter().position(|a| a == "-map").unwrap();
        assert_eq!(args[map_pos + 1], "[grid]");
        assert!(args.windows(2).any(|w| w[0] == "-c:v" && w[1] == "libwebp"));
        assert!(args.windows(2).any(|w| w[0] == "-loop" && w[1] == "0"));
        assert!(args.windows(2).any(|w| w[0] == "-quality" && w[1] == "75"));
        assert_eq!(args.last().unwrap(), "/out/sheet.webp");
    }

    #[test]
    fn stitch_args_with_header_maps_out_and_has_vstack() {
        let layout = compute_sheet_layout(2, 2, 800, 10);
        let clips: Vec<PathBuf> = (0..4).map(|i| PathBuf::from(format!("/tmp/c{}.mp4", i))).collect();
        let header = HeaderParams {
            textfiles: vec!["/tmp/header_0.txt".into(), "/tmp/header_1.txt".into()],
            height: 72, line_h: 26, font_size: 20,
            font_ffmpeg: "/f/font.ttf".into(),
        };
        let args = build_stitch_args(
            &clips, &layout, 200, 10, SheetTheme::Dark, Some(&header), 3.0, 12, 75, 4,
            Path::new("/out/sheet.webp"),
        );
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        let graph = &args[fc + 1];
        assert!(graph.contains("color=c=0x000000"));
        assert!(graph.contains("drawtext=textfile=/tmp/header_0.txt:expansion=none"));
        assert!(graph.contains("vstack"));
        assert!(graph.ends_with("[out]"));
        let map_pos = args.iter().position(|a| a == "-map").unwrap();
        assert_eq!(args[map_pos + 1], "[out]");
    }

    #[test]
    fn stitch_args_light_theme_uses_white_bg() {
        let layout = compute_sheet_layout(2, 2, 800, 10);
        let clips: Vec<PathBuf> = (0..4).map(|i| PathBuf::from(format!("/tmp/c{}.mp4", i))).collect();
        let header = HeaderParams {
            textfiles: vec!["/tmp/header_0.txt".into(), "/tmp/header_1.txt".into()],
            height: 50, line_h: 20, font_size: 16,
            font_ffmpeg: "/f.ttf".into(),
        };
        let args = build_stitch_args(
            &clips, &layout, 200, 10, SheetTheme::Light, Some(&header), 3.0, 12, 75, 4,
            Path::new("/out/sheet.webp"),
        );
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        let graph = &args[fc + 1];
        assert!(graph.contains("color=c=0xFFFFFF"));
        assert!(graph.contains("pad=798:430:5:5:0xFFFFFF"));
        assert!(graph.contains("fontcolor=black"));
    }

    #[test]
    fn stitch_args_includes_all_cells_as_xstack_inputs() {
        let layout = compute_sheet_layout(3, 2, 900, 10);
        let clips: Vec<PathBuf> = (0..6).map(|i| PathBuf::from(format!("/tmp/c{}.mp4", i))).collect();
        let args = build_stitch_args(
            &clips, &layout, 150, 10, SheetTheme::Dark, None, 2.0, 10, 75, 4,
            Path::new("/out/sheet.webp"),
        );
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        let graph = &args[fc + 1];
        assert!(graph.starts_with("[0:v][1:v][2:v][3:v][4:v][5:v]xstack=inputs=6:"));
    }

    #[test]
    fn fractional_extract_keeps_selected_stream_seek_and_separate_label() {
        let mut info = sample_info(45.0, 1920, 1080);
        info.video.index = 2;
        info.video.start_secs = 100.0;
        info.video.seek_offset_secs = 7.0;
        let args = build_extract_args(Path::new("source.mp4"), &info.video, 12.5, 13.0,
            320, 180, 8, 12, 35.0 / 9.0, true, 14, SheetTheme::Dark,
            Path::new("font.ttf"), Path::new("cell.mp4"), None);
        assert!(args.windows(2).any(|w| w == ["-ss", "19.500"]));
        assert!(args.windows(2).any(|w| w == ["-map", "0:2"]));
        assert!(args.windows(2).any(|w| w == ["-t", "3.888889"]));
        let vf = &args[args.iter().position(|a| a == "-vf").unwrap() + 1];
        assert!(vf.contains(&format_hms_escaped(13.0)));
        let layout = compute_sheet_layout(3, 3, 1280, 8);
        let header = HeaderParams { textfiles: vec!["header.txt".into()], height: 40, line_h: 20, font_size: 18, font_ffmpeg: "font.ttf".into() };
        let args = build_stitch_args(&["cell.mp4".into()], &layout, 180, 8, SheetTheme::Dark,
            Some(&header), 35.0 / 9.0, 12, 75, 4, Path::new("sheet.webp"));
        assert!(args.iter().any(|a| a.contains(":d=3.888889:r=12")));
    }

    #[test]
    fn subframe_extract_resets_pts_only_below_one_output_frame() {
        let mut info = sample_info(45.0, 1920, 1080);
        info.video.index = 2;
        info.video.start_secs = 100.0;
        info.video.seek_offset_secs = 7.0;
        for fps in [1, 12, 24] {
            let frame = 1.0 / f64::from(fps);
            let args_for = |length| build_extract_args(Path::new("source.mp4"), &info.video,
                12.5, 13.0, 320, 180, 8, fps, length, true, 14, SheetTheme::Dark,
                Path::new("font.ttf"), Path::new("cell.mp4"), Some("format=yuv420p"));
            let normal = args_for(frame);
            let vf_pos = normal.iter().position(|a| a == "-vf").unwrap() + 1;
            assert!(!normal[vf_pos].contains("setpts"), "boundary uses normal path");
            for length in [frame / 2.0, frame / 4.0, frame, frame * 2.0, 2.0] {
                let mut args = args_for(length);
                assert!(args.windows(2).any(|w| w == ["-ss", "19.500"]));
                assert!(args.windows(2).any(|w| w == ["-map", "0:2"]));
                assert!(args[vf_pos].contains(&format_hms_escaped(13.0)));
                if length < frame {
                    assert_eq!(args[vf_pos], format!("{},setpts=PTS-STARTPTS", normal[vf_pos]));
                    args[vf_pos] = normal[vf_pos].clone();
                } else {
                    assert_eq!(args[vf_pos], normal[vf_pos]);
                }
                // Only the nominal duration and the conditional filter may differ.
                let t = args.iter().position(|a| a == "-t").unwrap() + 1;
                assert_eq!(args[t], crate::ffmpeg::format_seconds(length));
                args[t] = normal[t].clone();
                assert_eq!(args, normal);
            }
        }
    }

    #[test]
    fn auto_consumption_validates_plan_not_ignored_options_grid() {
        let mut info = sample_info(100.0, 1920, 1080);
        info.video.duration_secs = Some(45.0);
        let opts = AnimatedSheetOptions {
            cols: 32, rows: 32, width: 1280, gap: 8,
            clip_length_secs: 5, fps: 12, quality: 75, compression_effort: 4,
            thumb_font_size: 14, header_font_size: 18, show_timestamps: true,
            show_header: true, suffix: String::new(), theme: SheetTheme::Dark,
        };
        let table = crate::auto_layout::LayoutTable { trim_start_secs: 5, trim_end_secs: 5, rules: crate::defaults::layout::RULES.to_vec() };
        let plan = crate::auto_layout::resolve(&table, info.sampling_duration_secs(), 1280, 8, Some(5.0)).unwrap();
        let clips = auto_clips(&info, &opts, &plan).unwrap();
        assert_eq!((clips.layout.cols, clips.layout.rows), (3, 3));
        for (cols, rows) in [(4, 10), (8, 8)] {
            let table = crate::auto_layout::LayoutTable { rules: vec![crate::auto_layout::LayoutRule {max_seconds: None, cols, rows}], ..table.clone() };
            let p = crate::auto_layout::resolve(&table, 45.0, 1280, 8, Some(5.0)).unwrap();
            assert_eq!(auto_clips(&info, &opts, &p).unwrap().layout.total, cols * rows);
        }
        let mut invalid = Vec::new();
        let mut p = plan.clone(); p.cells = 128; p.cols = 16; p.rows = 8;
        assert_eq!(auto_clips(&info, &opts, &p).err().unwrap(), "auto plan has 128 cells (16x8), over the 64-cell animated auto-layout limit");
        let mut p = plan.clone(); p.cells = 8; invalid.push(p);
        let mut p = plan.clone(); p.cols = 0; invalid.push(p);
        let mut p = plan.clone(); p.centres.pop(); invalid.push(p);
        let mut p = plan.clone(); p.clip_starts.pop(); invalid.push(p);
        let mut p = plan.clone(); p.duration_secs = 100.0; invalid.push(p);
        let mut p = plan.clone(); p.duration_secs = f64::NAN; invalid.push(p);
        let mut p = plan.clone(); p.clip_starts[0] = -1.0; invalid.push(p);
        let mut p = plan.clone(); p.centres[0] = f64::NAN; invalid.push(p);
        let mut p = plan.clone(); p.segment_secs = 0.0; invalid.push(p);
        let mut p = plan.clone(); p.decoded_secs = None; invalid.push(p);
        for l in [None, Some(0.0), Some(f64::NAN), Some(f64::INFINITY)] {
            let mut p = plan.clone(); p.cell_length_secs = l; invalid.push(p);
        }
        for p in invalid { assert!(auto_clips(&info, &opts, &p).is_err(), "accepted {p:?}"); }
    }

    fn invoke_generate_with_grid(cols: u32, rows: u32) -> Result<PublishOutcome, RunError> {
        let info = sample_info(60.0, 1920, 1080);
        let opts = AnimatedSheetOptions {
            cols, rows, width: 1280, gap: 4,
            clip_length_secs: 1, fps: 8, quality: 75,
            compression_effort: crate::defaults::animated_sheet::COMPRESSION_EFFORT,
            thumb_font_size: 14, header_font_size: 18,
            show_timestamps: false, show_header: false,
            suffix: String::new(),
            theme: SheetTheme::Dark,
        };
        let out = PathBuf::from("/tmp/_never_written.webp");
        let font = Path::new("/bin/false");
        let reporter = crate::jobs::ProgressReporter { emit: &|_, _, _| {} };
        let ctx = PipelineContext {
            output_policy: Default::default(),
            ffmpeg: Path::new("/bin/false"),
            cancelled: Arc::new(AtomicBool::new(false)),
            reporter: &reporter,
            has_zscale: false,
        };
        let fut = generate(Path::new("/x.mp4"), &info, &out, &opts, font, &ctx);
        tokio::runtime::Builder::new_current_thread()
            .build().unwrap().block_on(fut)
    }

    #[test]
    fn generate_rejects_over_32_cells() {
        match invoke_generate_with_grid(6, 6) {
            Err(RunError::Invalid(stderr)) => {
                assert!(stderr.contains("requires 2..=32"), "stderr: {}", stderr);
                assert!(stderr.contains("requested 36"), "stderr: {}", stderr);
            }
            other => panic!("expected guard error, got {:?}", other),
        }
    }

    #[test]
    fn generate_rejects_single_cell() {
        match invoke_generate_with_grid(1, 1) {
            Err(RunError::Invalid(stderr)) => {
                assert!(stderr.contains("requires 2..=32"), "stderr: {}", stderr);
                assert!(stderr.contains("requested 1"), "stderr: {}", stderr);
            }
            other => panic!("expected guard error, got {:?}", other),
        }
    }
}
