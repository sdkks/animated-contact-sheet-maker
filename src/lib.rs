pub mod video_info;
pub mod defaults;
pub mod input_scan;
pub mod drawtext;
mod layout;
pub mod mediainfo;
pub mod output_path;
mod header;
pub mod ffmpeg;
pub mod contact_sheet;
pub mod screenshots;
pub mod preview_reel;
pub mod animated_sheet;
pub mod jobs;
mod probe;

pub fn ffmpeg_test_hook_locate() -> Result<ffmpeg::Tools, ffmpeg::ToolsError> {
    ffmpeg::locate_tools()
}

pub async fn ffmpeg_test_hook_probe(tools: &ffmpeg::Tools, path: &str) -> Result<video_info::VideoInfo, String> {
    probe::probe(tools, path).await
}

pub fn video_info_test_hook_parse(json: &str) -> Result<video_info::VideoInfo, video_info::ProbeParseError> {
    video_info::parse(json)
}
