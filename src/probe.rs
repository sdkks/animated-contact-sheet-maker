use crate::ffmpeg::{run_capture, Tools};
use crate::video_info::{parse_with_hints, DurationHints, ProbeParseError, VideoInfo};

/// Canonical probe pipeline: ffprobe for structured metadata, MediaInfo for
/// enrichment (HDR format, commercial audio name, bit depth, etc.). Both
/// tools are required at startup; a non-zero exit or malformed MediaInfo
/// output still degrades gracefully (`enrichment = None`) so a single bad
/// file doesn't block the whole batch. MediaInfo's duration, then a frame
/// count, stand in when ffprobe reports none (see `DurationSource`).
///
/// The two probes are independent I/O against the same file, so they run
/// concurrently to reduce per-file probe latency.
pub(crate) async fn probe(tools: &Tools, path: &str) -> Result<VideoInfo, String> {
    let ffprobe_args = [
        "-v", "error",
        "-show_entries", "format=filename,start_time,duration,size,bit_rate",
        "-show_entries", "stream=index,start_time,codec_name,codec_type,width,height,r_frame_rate,duration,nb_frames,sample_rate,channels,bit_rate,profile,color_transfer,sample_aspect_ratio",
        "-show_entries", "stream_side_data=side_data_type,dv_profile,rotation",
        "-of", "json",
        path,
    ];
    let mediainfo_args = ["--Output=JSON", path];
    let (ffprobe_res, mediainfo_res) = tokio::join!(
        run_capture(&tools.ffprobe, &ffprobe_args),
        run_capture(&tools.mediainfo, &mediainfo_args),
    );
    let json = ffprobe_res.map_err(|e| e.to_string())?;
    let mediainfo_json = mediainfo_res.ok();
    let mut hints = DurationHints {
        mediainfo_secs: mediainfo_json.as_deref().and_then(crate::mediainfo::parse_duration),
        packet_count: None,
    };
    let mut info = match parse_with_hints(&json, hints) {
        Err(ProbeParseError::MissingDuration) => {
            let index = crate::video_info::selected_video_index(&json).map_err(|e| e.to_string())?;
            hints.packet_count = count_video_packets(tools, path, index).await;
            parse_with_hints(&json, hints)
        }
        parsed => parsed,
    }
    .map_err(|e| e.to_string())?;
    info.enrichment = mediainfo_json.as_deref().and_then(crate::mediainfo::parse_enrichment);
    Ok(info)
}

/// Reads the whole file, so it only runs when nothing else gave a duration.
async fn count_video_packets(tools: &Tools, path: &str, index: usize) -> Option<u64> {
    let out = run_capture(&tools.ffprobe, &[
        "-v", "error",
        "-select_streams", &index.to_string(),
        "-count_packets",
        "-show_entries", "stream=nb_read_packets",
        "-of", "csv=p=0",
        path,
    ]).await.ok()?;
    out.trim().parse().ok()
}
