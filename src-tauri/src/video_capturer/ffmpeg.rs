use std::process::Command;

// 你的 FFmpeg 启动函数
pub fn start_screen_capture() {
    println!("[FFMPEG]启动");
    let _ = Command::new("ffmpeg.exe")
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
            "rtp://127.0.0.1:5004",
        ])
        .spawn()
        .expect("启动 FFmpeg 失败");
}
