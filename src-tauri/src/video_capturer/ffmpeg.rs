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
            // 抓屏基本参数
            "-f",
            "gdigrab",
            "-framerate",
            "30",
            "-i",
            "desktop",
            "-vf",
            "scale=720:480,format=yuv420p",
            // 编码器参数：和 TrackLocalStaticSample 里的一致
            "-c:v",
            "libx264",
            "-profile:v",
            "baseline",
            "-level",
            "3.1", // 对应 profile-level-id=42e01f 中的 level 31
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "ultrafast",
            "-b:v",
            "2500k",
            "-maxrate",
            "3000k",
            "-bufsize",
            "5000k",
            "-x264-params",
            "keyint=30:scenecut=0:repeat-headers=1",
            "-tune",
            "zerolatency",
            "-force_key_frames",
            "expr:gte(t,n_forced*1)",
            // 强制动态载荷类型 96，保证 SDP 用 96 而不是其他
            "-payload_type",
            "96",
            // 输出 SDP 文件
            "-sdp_file",
            "../ffmpeg.sdp",
            // RTP 复用
            "-f",
            "rtp",
            &addr,
        ])
        .spawn()
        .expect(&format!("启动 FFmpeg 失败,请检查路径{:?}", path));

    let mut ffmpeg = FFMPEG_CHILD.lock().unwrap();
    *ffmpeg = Some(ffmpeg_child_process)
}
