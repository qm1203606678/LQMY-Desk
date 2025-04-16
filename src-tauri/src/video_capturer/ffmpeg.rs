use std::process::Command;

// 采集屏幕并推送到 WebRTC
fn start_screen_capture() {
    let ffmpeg_cmd = "ffmpeg -f gdigrab -framerate 30 -i desktop -vcodec libx264 -preset ultrafast -f rtp rtp://127.0.0.1:1234";
    let output = Command::new("ffmpeg.exe")
        .args(&[
            "-f",
            "gdigrab",
            "-framerate",
            "30",
            "-i",
            "desktop",
            "-vcodec",
            "libx264",
            "-preset",
            "ultrafast",
            "-f",
            "rtp",
            "rtp://127.0.0.1:1234",
        ])
        .output()
        .expect("Failed to start FFmpeg");
}
