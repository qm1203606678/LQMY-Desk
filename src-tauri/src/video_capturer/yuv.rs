use crate::video_capturer::assembly::MultiStreamManager;
use crate::video_capturer::assembly::QualityConfig;
use openh264::formats::YUVSource;
// YUV 数据结构
pub struct YuvData {
    pub width: usize,
    pub height: usize,
    /// Y 平面，长度 = width * height
    pub y: Vec<u8>,
    /// U 平面，长度 = (width/2) * (height/2)
    pub u: Vec<u8>,
    /// V 平面，长度 = (width/2) * (height/2)
    pub v: Vec<u8>,
}

impl YuvData {
    /// 构造函数，从 BGRA 或者其他源转换后填充 y,u,v
    pub fn new(width: usize, height: usize, y: Vec<u8>, u: Vec<u8>, v: Vec<u8>) -> Self {
        assert_eq!(y.len(), width * height);
        assert_eq!(u.len(), (width / 2) * (height / 2));
        assert_eq!(v.len(), (width / 2) * (height / 2));
        Self {
            width,
            height,
            y,
            u,
            v,
        }
    }
}

impl YUVSource for YuvData {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        // Y 平面每行 stride = width
        // U、V 平面每行 stride = width/2
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}
// 将 BGRA 转换为 YUV420
pub fn bgra_to_yuv420(bgra_data: &[u8], width: usize, height: usize) -> YuvData {
    let mut y_plane = Vec::with_capacity(width * height);
    let mut u_plane = Vec::with_capacity(width * height / 4);
    let mut v_plane = Vec::with_capacity(width * height / 4);

    // 转换 Y 分量（亮度）
    for chunk in bgra_data.chunks_exact(4) {
        let b = chunk[0] as f32;
        let g = chunk[1] as f32;
        let r = chunk[2] as f32;

        // ITU-R BT.601 标准
        let y = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
        y_plane.push(y);
    }

    // 对于 U 和 V 分量，进行 2x2 下采样
    for y_idx in (0..height).step_by(2) {
        for x_idx in (0..width).step_by(2) {
            let mut r_sum = 0.0;
            let mut g_sum = 0.0;
            let mut b_sum = 0.0;
            let mut count = 0;

            // 采样 2x2 块
            for dy in 0..2 {
                for dx in 0..2 {
                    let y = y_idx + dy;
                    let x = x_idx + dx;
                    if y < height && x < width {
                        let pixel_idx = (y * width + x) * 4;
                        if pixel_idx + 3 < bgra_data.len() {
                            b_sum += bgra_data[pixel_idx] as f32;
                            g_sum += bgra_data[pixel_idx + 1] as f32;
                            r_sum += bgra_data[pixel_idx + 2] as f32;
                            count += 1;
                        }
                    }
                }
            }

            if count > 0 {
                let r_avg = r_sum / count as f32;
                let g_avg = g_sum / count as f32;
                let b_avg = b_sum / count as f32;

                // ITU-R BT.601 标准
                let u = (-0.169 * r_avg - 0.331 * g_avg + 0.5 * b_avg + 128.0) as u8;
                let v = (0.5 * r_avg - 0.419 * g_avg - 0.081 * b_avg + 128.0) as u8;

                u_plane.push(u);
                v_plane.push(v);
            }
        }
    }

    YuvData {
        width: width,
        height: height,
        y: y_plane,
        u: u_plane,
        v: v_plane,
    }
}
// 使用示例
pub async fn example_usage() {
    // 创建多流管理器
    let manager = MultiStreamManager::new(50);

    // 启动桌面捕获
    manager.start_capture();

    // 定义不同的质量档次
    let qualities = vec![
        QualityConfig {
            name: "1080p".to_string(),
            width: 1920,
            height: 1080,
            bitrate: 2000000,
            fps: 30,
        },
        QualityConfig {
            name: "720p".to_string(),
            width: 1280,
            height: 720,
            bitrate: 1000000,
            fps: 30,
        },
        QualityConfig {
            name: "480p".to_string(),
            width: 854,
            height: 480,
            bitrate: 500000,
            fps: 24,
        },
    ];

    // 为每个质量档次创建编码流
    for quality in qualities {
        let _encoded_rx = manager.add_quality_stream(quality).await;
        // 现在可以将 encoded_rx 用于多个 WebRTC connections
    }

    // 当有新的 PeerConnection 时，为其创建对应质量的 track writer
    // let track = Arc::new(TrackLocalStaticSample::new(...));
    // manager.create_track_writer("720p", track).await;
}
