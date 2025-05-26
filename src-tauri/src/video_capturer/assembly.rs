use bytes::Bytes;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, QpRange};
use openh264::formats::{BgraSliceU8, RGBSource, YUVBuffer, YUVSource};
use openh264::OpenH264API::{self, *};
use rusty_duplication::{FrameInfoExt, Scanner, VecCapturer};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use webrtc::media::Sample;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use super::yuv::bgra_to_yuv420;
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
        let _ = ecfg.num_threads(2);
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
    let mut dst = vec![0u8; dst_width * dst_height * 4];

    // 简单的最近邻插值缩放
    let x_ratio = src_width as f32 / dst_width as f32;
    let y_ratio = src_height as f32 / dst_height as f32;

    for y in 0..dst_height {
        for x in 0..dst_width {
            let src_x = (x as f32 * x_ratio) as usize;
            let src_y = (y as f32 * y_ratio) as usize;

            let src_idx = (src_y * src_width + src_x) * 4;
            let dst_idx = (y * dst_width + x) * 4;

            if src_idx + 3 < src.len() && dst_idx + 3 < dst.len() {
                dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
            }
        }
    }

    dst
}
