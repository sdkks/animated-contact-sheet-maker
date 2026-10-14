// Install hint text for tool-missing errors.

pub fn print_tool_install_hint() {
    eprintln!();
    eprintln!("ACSM needs ffmpeg, ffprobe, and mediainfo on your PATH.");
    eprintln!("Install:");
    eprintln!("  macOS:   brew install ffmpeg-full mediainfo");
    eprintln!("  Windows: winget install Gyan.FFmpeg MediaArea.MediaInfo");
    eprintln!("  Linux:   apt install ffmpeg mediainfo   (or your distro equivalent)");
}
