// src/video_streamer.rs

//! Capture with `rusty_duplication`, multi‐quality H.264 streaming over WebRTC.

use bytes::Bytes;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use rusty_duplication::{FrameInfoExt, Scanner, VecCapturer};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use webrtc::media::Sample;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

/// Raw BGRA frame
#[derive(Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
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

/// Spawn a task that captures frames via rusty_duplication and broadcasts them.
pub fn start_broadcast(buffer_size: usize) -> broadcast::Sender<RawFrame> {
    let (tx, _) = broadcast::channel(buffer_size);
    // 克隆一份给异步任务
    let tx_task = tx.clone();
    tokio::spawn(async move {
        // Initialize monitor scanner
        let mut scanner = Scanner::new().unwrap();
        let monitor = scanner.next().expect("no monitor found");
        let mut capturer: VecCapturer = monitor.try_into().unwrap();

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
                    };
                    // 使用克隆的 tx_task 发射
                    let _ = tx_task.send(frame);
                }
            }
        }
    });
    tx
}

/// Quality settings for each stream
pub struct QualityConfig {
    pub width: i32,
    pub height: i32,
    pub bitrate: i32,
    pub fps: i32,
}

/// Subscribe to raw frames, encode to H.264 with openh264, and write to WebRTC track.
pub fn spawn_encoder(
    mut rx: broadcast::Receiver<RawFrame>,
    track: Arc<TrackLocalStaticSample>,
    cfg: QualityConfig,
) {
    tokio::spawn(async move {
        // 1) 使用内置 API Loader
        let loader = DynamicAPI::from_source();

        // 2) 解包 enum，得到源或动态 loader
        let api = match loader {
            DynamicAPI::Source(src) => src,
            DynamicAPI::Libloading(dl) => dl,
        };

        // 3) 配置编码器
        let mut ecfg = EncoderConfig::new();
        ecfg.target_bitrate = BitRate::from_bps(cfg.bitrate_bps);
        ecfg.usage_type = UsageType::CameraVideoRealTime;
        ecfg.data_format = videoFormatI420;
        ecfg.max_frame_rate = FrameRate::from_hz(cfg.fps);
        ecfg.rate_control_mode = RateControlMode::Quality;
        ecfg.sps_pps_strategy = SpsPpsStrategy::ConstantId;
        ecfg.multiple_thread_idc = 4;
        ecfg.complexity = Complexity::Medium;
        ecfg.intra_frame_period = IntraFramePeriod::from_num_frames(cfg.gop);

        let mut encoder = Encoder::with_config(api, ecfg).expect("OpenH264 encoder init failed");

        // 4) 循环处理、编码、发送
        while let Ok(raw) = rx.recv().await {
            let yuv =
                crate::utils::bgra_to_yuv420(&raw.bgra, raw.width as usize, raw.height as usize);
            let mut buf = YUVBuffer::new(cfg.width, cfg.height);
            buf.copy_from_slice(&yuv);

            if let Ok(bitstream) = encoder.encode(&buf) {
                let avcc = annexb_to_avcc(bitstream.get_data());
                let sample = Sample {
                    data: Bytes::from(avcc),
                    duration: Duration::from_millis((1000.0 / cfg.fps) as u64),
                    ..Default::default()
                };
                if track.write_sample(&sample).await.is_err() {
                    break;
                }
            }
        }
    });
}
// ─── Example Usage ─────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // 1. Setup MediaEngine & API
    let mut m = webrtc::api::media_engine::MediaEngine::default();
    m.register_default_codecs().unwrap();
    let api = webrtc::api::APIBuilder::new().with_media_engine(m).build();

    // 2. Start capture broadcaster
    let tx = start_broadcast(4);

    // 3. Create two WebRTC tracks with H.264 capability
    let codec_cap = RTCRtpCodecCapability {
        mime_type: "video/H264".to_owned(),
        clock_rate: 90000,
        channels: 0,
        sdp_fmtp_line: "packetization-mode=1;profile-level-id=42e01f".to_owned(),
        rtcp_feedback: vec![],
    };
    let track1 = Arc::new(TrackLocalStaticSample::new(
        codec_cap.clone(),
        "video1".into(),
        "stream1".into(),
    ));
    let track2 = Arc::new(TrackLocalStaticSample::new(
        codec_cap,
        "video2".into(),
        "stream2".into(),
    ));

    // 4. Spawn encoders with different quality
    let rx1 = tx.subscribe();
    let rx2 = tx.subscribe();
    spawn_encoder(
        rx1,
        track1,
        QualityConfig {
            width: 1280,
            height: 720,
            bitrate: 1_000_000,
            fps: 30,
        },
    );
    spawn_encoder(
        rx2,
        track2,
        QualityConfig {
            width: 640,
            height: 360,
            bitrate: 300_000,
            fps: 15,
        },
    );

    // 5. Continue with signaling & PeerConnection, attaching tracks, etc.
}
