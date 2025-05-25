use std::{
    process::{Child, Command},
    sync::Mutex,
};

use crate::config::APPDATA_PATH;
use lazy_static::lazy_static;
lazy_static! {
    pub static ref FFMPEG_CHILD: Mutex<Option<Child>> = Mutex::new(None);
}

// 你的 FFmpeg 启动函数
pub fn start_screen_capture(udp_port: u16) {
    println!("[FFMPEG]启动");
    let path = APPDATA_PATH.lock().unwrap().clone().join("ffmpeg.exe");
    let addr = format!("rtp://127.0.0.1:{}", udp_port);
    let ffmpeg_child_process = Command::new(path.clone())
        .args(&[
            "-f",
            "gdigrab",
            "-framerate",
            "30",
            "-i",
            "desktop",
            "-vf",
            "scale=640:360",
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-pix_fmt",
            "nv12",
            "-preset",
            "ultrafast",
            "-b:v",
            "2500k", // ⬅️ 平均码率限制
            "-maxrate",
            "3000k", // ⬅️ 最大码率限制
            "-bufsize",
            "5000k", // ⬅️ 码率控制缓冲区
            "-x264-params",
            "keyint=30:scenecut=0:repeat-headers=1",
            "-tune",
            "zerolatency",
            "-force_key_frames",
            "expr:gte(t,n_forced*1)",
            "-f",
            "rtp",
            &addr,
        ])
        .spawn()
        .expect(&format!("启动 FFmpeg 失败,请检查路径{:?}", path));

    let mut ffmpeg = FFMPEG_CHILD.lock().unwrap();
    *ffmpeg = Some(ffmpeg_child_process)
}
