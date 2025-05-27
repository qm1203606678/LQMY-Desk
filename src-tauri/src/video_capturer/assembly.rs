use bytes::Bytes;
use openh264::encoder::{Encoder, EncoderConfig, IntraFramePeriod, QpRange};

use super::yuv::bgra_to_yuv420;
use rayon::prelude::*;
use rusty_duplication::{FrameInfoExt, Scanner, VecCapturer};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use webrtc::media::Sample;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
/// Raw BGRA frame
#[derive(Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub timestamp: u64, // 添加时间戳用于同步
}

/// Quality settings for each stream
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct QualityConfig {
    pub width: i32,
    pub height: i32,
    pub bitrate: i32,
    pub fps: i32,
    pub name: String, // 用于标识不同的质量档次
}

/// 编码后的帧数据
#[derive(Clone)]
pub struct EncodedFrame {
    pub data: Bytes,
    pub duration: Duration,
    pub quality: String,
}

/// 多路流管理器
pub struct MultiStreamManager {
    raw_frame_tx: broadcast::Sender<RawFrame>,
    encoded_streams: Arc<RwLock<HashMap<String, broadcast::Sender<EncodedFrame>>>>,
    active_qualities: Arc<RwLock<HashMap<String, QualityConfig>>>,
}

impl MultiStreamManager {
    pub fn new(buffer_size: usize) -> Self {
        let (raw_frame_tx, _) = broadcast::channel(buffer_size);

        Self {
            raw_frame_tx,
            encoded_streams: Arc::new(RwLock::new(HashMap::new())),
            active_qualities: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 启动桌面捕获（只启动一次）
    pub fn start_capture(&self) {
        let tx = self.raw_frame_tx.clone();

        tokio::spawn(async move {
            // Initialize monitor scanner
            let mut scanner = Scanner::new().unwrap();
            let monitor = scanner.next().expect("no monitor found");
            let mut capturer: VecCapturer = monitor.try_into().unwrap();
            let mut frame_counter = 0u64;

            loop {
                // Sleep to throttle (~30fps)
                tokio::time::sleep(Duration::from_millis(33)).await;

                if let Ok(info) = capturer.capture() {
                    if info.desktop_updated() {
                        let desc = capturer.monitor().dxgi_outdupl_desc();
                        let (w, h) = (desc.ModeDesc.Width, desc.ModeDesc.Height);
                        let bgra = capturer.buffer.clone();

                        let frame = RawFrame {
                            width: w,
                            height: h,
                            bgra,
                            timestamp: frame_counter,
                        };

                        frame_counter += 1;

                        // 发送原始帧给所有编码器
                        let _ = tx.send(frame);
                    }
                }
            }
        });
    }

    /// 添加新的质量档次（如果不存在则创建编码器）
    pub async fn add_quality_stream(
        &self,
        quality: QualityConfig,
    ) -> broadcast::Receiver<EncodedFrame> {
        let quality_name = quality.name.clone();

        // 检查是否已存在该质量档次
        {
            let streams = self.encoded_streams.read().await;
            if let Some(tx) = streams.get(&quality_name) {
                return tx.subscribe();
            }
        }

        // 创建新的编码流
        let (encoded_tx, encoded_rx) = broadcast::channel(100);

        // 添加到管理器
        {
            let mut streams = self.encoded_streams.write().await;
            let mut qualities = self.active_qualities.write().await;

            streams.insert(quality_name.clone(), encoded_tx.clone());
            qualities.insert(quality_name.clone(), quality.clone());
        }

        // 启动该质量档次的编码器
        self.spawn_quality_encoder(quality, encoded_tx).await;

        encoded_rx
    }

    /// 为特定质量档次启动编码器
    async fn spawn_quality_encoder(
        &self,
        quality: QualityConfig,
        encoded_tx: broadcast::Sender<EncodedFrame>,
    ) {
        let raw_rx = self.raw_frame_tx.subscribe();

        tokio::spawn(async move {
            Self::encode_stream(raw_rx, encoded_tx, quality).await;
        });
    }

    /// 编码流处理
    async fn encode_stream(
        mut raw_rx: broadcast::Receiver<RawFrame>,
        encoded_tx: broadcast::Sender<EncodedFrame>,
        quality: QualityConfig,
    ) {
        // 初始化 OpenH264 编码器
        let encoder_result = Self::create_encoder(&quality);
        let mut encoder = match encoder_result {
            Ok(enc) => enc,
            Err(e) => {
                eprintln!("Failed to create encoder for {}: {}", quality.name, e);
                return;
            }
        };

        let mut last_frame_time = std::time::Instant::now();
        let frame_interval = Duration::from_millis(1000 / quality.fps as u64);

        while let Ok(raw_frame) = raw_rx.recv().await {
            // 帧率控制
            let now = std::time::Instant::now();
            if now.duration_since(last_frame_time) < frame_interval {
                continue;
            }
            last_frame_time = now;

            // 缩放和处理帧
            let processed_bgra = Self::process_frame(&raw_frame, &quality);

            let yuv_data = bgra_to_yuv420(
                &processed_bgra,
                quality.width as usize,
                quality.height as usize,
            );

            // 创建 YUV buffer
            // let yuv_buffer = YUVBuffer::with_rgb_to_yuv(
            //     &yuv_data.y,
            //     &yuv_data.u,
            //     &yuv_data.v,
            //     quality.width as usize,
            //     quality.height as usize,
            // );
            // 编码
            match encoder.encode(&yuv_data) {
                Ok(bitstream) => {
                    //let avcc_data = annexb_to_avcc(&bitstream.to_vec());
                    let encoded_frame = EncodedFrame {
                        data: Bytes::from(bitstream.to_vec()),
                        duration: frame_interval,
                        quality: quality.name.clone(),
                    };

                    // 发送编码后的帧
                    if encoded_tx.send(encoded_frame).is_err() {
                        break; // 没有接收者了，退出编码循环
                    }
                }
                Err(e) => {
                    eprintln!("Encoding error for {}: {}", quality.name, e);
                }
            }
        }
    }

    /// 创建编码器
    fn create_encoder(
        quality: &QualityConfig,
    ) -> Result<Encoder, Box<dyn std::error::Error + Send + Sync>> {
        let loader = openh264::OpenH264API::from_source();

        // 3) 配置编码器
        let ecfg = EncoderConfig::new();
        let _ = ecfg.skip_frames(false); //不跳过静止帧
        let _ = ecfg.bitrate(openh264::encoder::BitRate::from_bps(quality.bitrate as u32));
        let _ = ecfg.max_frame_rate(openh264::encoder::FrameRate::from_hz(quality.fps as f32));
        let _ = ecfg.num_threads(4);
        let _ = ecfg.usage_type(openh264::encoder::UsageType::CameraVideoRealTime);
        let _ = ecfg.profile(openh264::encoder::Profile::Baseline);
        let _ = ecfg.level(openh264::encoder::Level::Level_3_1);
        let _ = ecfg.complexity(openh264::encoder::Complexity::Medium);
        let _ = ecfg.qp(QpRange::new(24, 38));
        let _ = ecfg.intra_frame_period(IntraFramePeriod::from_num_frames(30));
        let encoder = Encoder::with_api_config(loader, ecfg).expect("OpenH264 encoder init failed");

        Ok(encoder)
    }

    /// 处理帧（缩放）
    fn process_frame(raw_frame: &RawFrame, quality: &QualityConfig) -> Vec<u8> {
        let src_width = raw_frame.width as usize;
        let src_height = raw_frame.height as usize;
        let dst_width = quality.width as usize;
        let dst_height = quality.height as usize;

        // 如果尺寸匹配，直接返回
        if src_width == dst_width && src_height == dst_height {
            return raw_frame.bgra.clone();
        }

        // 执行缩放
        scale_bgra(
            &raw_frame.bgra,
            src_width,
            src_height,
            dst_width,
            dst_height,
        )
    }

    /// 为 WebRTC track 创建样本写入器
    pub async fn create_track_writer(
        &self,
        quality_name: &str,
        track: Arc<TrackLocalStaticSample>,
    ) {
        let encoded_streams = self.encoded_streams.read().await;
        if let Some(tx) = encoded_streams.get(quality_name) {
            let mut rx = tx.subscribe();

            tokio::spawn(async move {
                while let Ok(encoded_frame) = rx.recv().await {
                    let sample = Sample {
                        data: encoded_frame.data,
                        duration: encoded_frame.duration,
                        ..Default::default()
                    };

                    if let Err(e) = track.write_sample(&sample).await {
                        println!("[WRITE SAMPLE]关闭因为，{:?}", e);
                        break; // WebRTC track 关闭了
                    }
                }
            });
        }
    }

    /// 获取活跃的质量档次列表
    pub async fn get_active_qualities(&self) -> Vec<QualityConfig> {
        let qualities = self.active_qualities.read().await;
        qualities.values().cloned().collect()
    }

    /// 移除不再使用的质量档次
    pub async fn remove_quality_stream(&self, quality_name: &str) {
        let mut streams = self.encoded_streams.write().await;
        let mut qualities = self.active_qualities.write().await;

        streams.remove(quality_name);
        qualities.remove(quality_name);
    }
}

/// Convert Annex-B (0x00000001) to AVCC length-prefix format
fn annexb_to_avcc(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i + 4 <= data.len() {
        if &data[i..i + 4] == [0, 0, 0, 1] {
            i += 4;
            let start = i;
            while i + 4 <= data.len() && &data[i..i + 4] != [0, 0, 0, 1] {
                i += 1;
            }
            let nal = &data[start..i];
            out.extend(&(nal.len() as u32).to_be_bytes());
            out.extend(nal);
        } else {
            i += 1;
        }
    }
    out
}

// 实现图像缩放函数
fn scale_bgra(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> Vec<u8> {
    // 1. 定点精度：16-bit fractional (0..65535)
    const FP_SHIFT: usize = 16;
    const FP_ONE: usize = 1 << FP_SHIFT;

    // 2. 第一阶段：横向插值，输出中间缓冲区 mid (u32 packed with BGRA channels expanded to u16)
    //    大小 = src_height * dst_width
    let mut mid = vec![0u32; src_height * dst_width];

    // 2.1 预计算 dst_x -> (src_x0, weight_x0, weight_x1)
    let x_map: Vec<(usize, u32, u32)> = (0..dst_width)
        .map(|x| {
            // 对应 src 的浮点位置
            let fx = x * src_width * FP_ONE / dst_width;
            let sx = fx >> FP_SHIFT; // integer part
            let wx1 = (fx & (FP_ONE - 1)) as u32; // frac part
            let wx0 = (FP_ONE as u32 - wx1); // 1 - frac
                                             // clamp 防止溢界取最后一列
            let sx = if sx + 1 < src_width {
                sx
            } else {
                src_width - 1
            };
            (sx, wx0, wx1)
        })
        .collect();

    // 并行处理每一源行
    mid.par_chunks_exact_mut(dst_width)
        .enumerate()
        .for_each(|(y, mid_row)| {
            let src_row = &src[y * src_width * 4..(y + 1) * src_width * 4];
            for (dx, &(sx, wx0, wx1)) in x_map.iter().enumerate() {
                // 读两个相邻像素，u32 载入
                unsafe {
                    let p0 = *(src_row.as_ptr().add(sx * 4) as *const u32);
                    let p1 = *(src_row.as_ptr().add((sx + 1) * 4) as *const u32);
                    // 拆通道到 u16，按权重累加，再右移回 8-bit
                    let b0 = (p0 & 0xFF) as u32;
                    let g0 = ((p0 >> 8) & 0xFF) as u32;
                    let r0 = ((p0 >> 16) & 0xFF) as u32;
                    let a0 = ((p0 >> 24) & 0xFF) as u32;
                    let b1 = (p1 & 0xFF) as u32;
                    let g1 = ((p1 >> 8) & 0xFF) as u32;
                    let r1 = ((p1 >> 16) & 0xFF) as u32;
                    let a1 = ((p1 >> 24) & 0xFF) as u32;

                    let b = ((b0 * wx0 + b1 * wx1) >> FP_SHIFT) as u32;
                    let g = ((g0 * wx0 + g1 * wx1) >> FP_SHIFT) as u32;
                    let r = ((r0 * wx0 + r1 * wx1) >> FP_SHIFT) as u32;
                    let a = ((a0 * wx0 + a1 * wx1) >> FP_SHIFT) as u32;

                    // 重新打包到 u32
                    mid_row[dx] = b | (g << 8) | (r << 16) | (a << 24);
                }
            }
        });

    // 3. 第二阶段：纵向插值，从 mid -> dst
    let mut dst = vec![0u8; dst_width * dst_height * 4];

    // 3.1 预计算 dst_y -> (src_y0, weight_y0, weight_y1)
    let y_map: Vec<(usize, u32, u32)> = (0..dst_height)
        .map(|y| {
            let fy = y * src_height * FP_ONE / dst_height;
            let sy = fy >> FP_SHIFT;
            let wy1 = (fy & (FP_ONE - 1)) as u32;
            let wy0 = (FP_ONE as u32 - wy1);
            let sy = if sy + 1 < src_height {
                sy
            } else {
                src_height - 1
            };
            (sy, wy0, wy1)
        })
        .collect();

    // 并行生成每一目标行
    dst.par_chunks_exact_mut(dst_width * 4)
        .enumerate()
        .for_each(|(dy, dst_row)| {
            let (sy, wy0, wy1) = y_map[dy];
            let row0 = &mid[sy * dst_width..(sy + 1) * dst_width];
            let row1 = &mid[(sy + 1) * dst_width..(sy + 2) * dst_width];

            for x in 0..dst_width {
                unsafe {
                    let p0 = row0[x];
                    let p1 = row1[x];
                    // 拆通道
                    let b0 = (p0 & 0xFF) as u32;
                    let g0 = ((p0 >> 8) & 0xFF) as u32;
                    let r0 = ((p0 >> 16) & 0xFF) as u32;
                    let a0 = ((p0 >> 24) & 0xFF) as u32;
                    let b1 = (p1 & 0xFF) as u32;
                    let g1 = ((p1 >> 8) & 0xFF) as u32;
                    let r1 = ((p1 >> 16) & 0xFF) as u32;
                    let a1 = ((p1 >> 24) & 0xFF) as u32;

                    // 按权重累加
                    let b = ((b0 * wy0 + b1 * wy1) >> FP_SHIFT) as u8;
                    let g = ((g0 * wy0 + g1 * wy1) >> FP_SHIFT) as u8;
                    let r = ((r0 * wy0 + r1 * wy1) >> FP_SHIFT) as u8;
                    let a = ((a0 * wy0 + a1 * wy1) >> FP_SHIFT) as u8;

                    let dst_px = dst_row.as_mut_ptr().add(x * 4);
                    *dst_px = b;
                    *dst_px.add(1) = g;
                    *dst_px.add(2) = r;
                    *dst_px.add(3) = a;
                }
            }
        });

    dst
}
