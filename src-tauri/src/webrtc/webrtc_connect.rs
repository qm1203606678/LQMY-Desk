use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use crate::webrtc::videostream::start_webrtc_video_stream;

#[derive(Deserialize)]
pub struct OfferRequest {
    pub sdp: String,
    pub mode: String,
}

#[derive(Serialize)]
pub struct AnswerResponse {
    pub sdp: String,
}

pub async fn handle_webrtc_offer(offer: web::Json<OfferRequest>) -> impl Responder {
    // 初始化并注册 codecs
    let mut m = MediaEngine::default();
    if let Err(e) = m.register_default_codecs() {
        eprintln!("MediaEngine 注册失败: {:?}", e);
        return HttpResponse::InternalServerError().body("初始化媒体失败");
    }
    let api = APIBuilder::new().with_media_engine(m).build();
    let pc = match api.new_peer_connection(RTCConfiguration::default()).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            eprintln!("PeerConnection 创建失败: {:?}", e);
            return HttpResponse::InternalServerError().body("连接初始化失败");
        }
    };

    // 音频 Track
    let audio = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: "audio/opus".to_string(),
            clock_rate: 48000,
            channels: 2,
            ..Default::default()
        },
        "audio".to_string(),
        "rust-audio".to_string(),
    ));
    let _ = pc.add_track(audio.clone()).await;

    // 视频 Track 根据模式
    let (mime, fmt) = match offer.mode.as_str() {
        "low_latency" => ("video/VP8", "max-fr=30;max-fs=360"),
        "high_quality" => ("video/H264", "profile-level-id=42e01f;level-asymmetry-allowed=1"),
        _ => ("video/VP8", "max-fr=24;max-fs=480"),
    };
    let video = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: mime.to_string(),
            sdp_fmtp_line: fmt.to_string(),
            clock_rate: 90000,
            ..Default::default()
        },
        "video".to_string(),
        "rust-video".to_string(),
    ));
    let _ = pc.add_track(video.clone()).await;

    // DataChannel
    let dc_init = RTCDataChannelInit { ordered: Some(true), ..Default::default() };
    if let Ok(dc) = pc.create_data_channel("control", Some(dc_init)).await {
        dc.on_message(Box::new(|msg| {
            println!("控制命令: {:?}", msg.data);
            // TODO: 解析执行命令
            Box::pin(async {})
        }));
    }

    // ICE 状态变更
    let pc2 = pc.clone();
    pc.on_ice_connection_state_change(Box::new(move |state| {
        println!("ICE 状态: {:?}", state);
        if state == RTCIceConnectionState::Connected {
            // 启动视频推流
            let pc_stream = pc2.clone();
            tokio::spawn(async move {
                let _ = start_webrtc_video_stream(5004).await;
            });
        }
        Box::pin(async {})
    }));

    // 处理 Offer
    let remote = match RTCSessionDescription::offer(offer.sdp.clone()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Offer 解析失败: {:?}", e);
            return HttpResponse::BadRequest().body("Offer 解析失败");
        }
    };
    let _ = pc.set_remote_description(remote).await;
    let answer = match pc.create_answer(None).await {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Answer 创建失败: {:?}", e);
            return HttpResponse::InternalServerError().body("Answer 创建失败");
        }
    };
    let _ = pc.set_local_description(answer.clone()).await;

    HttpResponse::Ok().json(AnswerResponse { sdp: answer.sdp })
}