use std::{
    process::{Child, Command},
    sync::Mutex,
};

use crate::config::{APPDATA_PATH, PEER_CONNECTION};
use lazy_static::lazy_static;
use sha2::digest::consts::False;
use webrtc::peer_connection;
lazy_static! {
    pub static ref FFMPEG_CHILD: Mutex<Option<Child>> = Mutex::new(None);
}

// 你的 FFmpeg 启动函数
pub fn start_screen_capture(udp_port: u16) {
    let mut ffmpeg = FFMPEG_CHILD.lock().unwrap();
    if ffmpeg.is_some() {
        println!("ffmpeg 已启动");
        return;
    };
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

    *ffmpeg = Some(ffmpeg_child_process)
}

/// 关闭视频捕获，没有peerconnection就关
pub fn end_screen_capture(force: bool) {
    let pcs = PEER_CONNECTION.lock().unwrap();
    if pcs.is_empty() || force {
        let mut child_lock = FFMPEG_CHILD.lock().unwrap();
        if let Some(child) = child_lock.as_mut() {
            match child.kill() {
                Ok(_) => {
                    println!("FFmpeg 进程已成功终止");
                }
                Err(e) => {
                    eprintln!("终止 FFmpeg 进程失败: {}", e);
                }
            }
            // 等待子进程真正退出，回收资源
            let _ = child.wait();
        }
        *child_lock = None;
    }
}
