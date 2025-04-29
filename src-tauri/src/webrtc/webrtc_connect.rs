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

#[derive(Deserialize)]
pub struct OfferRequest {
    pub sdp: String,
    pub mode: String, // "low_latency", "high_quality", "balanced"
}

#[derive(Serialize)]
pub struct AnswerResponse {
    pub sdp: String,
}

pub async fn handle_webrtc_offer(offer: web::Json<OfferRequest>) -> impl Responder {
    // 创建 MediaEngine
    let mut m = MediaEngine::default();
    match m.register_default_codecs() {
        Ok(()) => println!("[WEBRTC]MediaEngine注册成功"),
        Err(err) => {
            println!("[WEBRTC]MediaEngine注册失败,{:?}", err);
            return HttpResponse::InternalServerError().body("对方初始化媒体失败");
        }
    };

    let api = APIBuilder::new().with_media_engine(m).build();

    // 创建 PeerConnection
    let config = RTCConfiguration::default();

    let peer_connection = match api.new_peer_connection(config).await {
        Ok(ok) => Arc::new(ok),
        Err(err) => {
            println!("[WEBRTC]PeerConnect注册失败,{:?}", err);
            return HttpResponse::InternalServerError().body("对方初始化连接失败");
        }
    };

    // === 添加音频 Track ===
    let audio_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: "audio/opus".to_owned(),
            clock_rate: 48000,
            channels: 2,
            sdp_fmtp_line: "".to_owned(),
            rtcp_feedback: vec![],
        },
        "audio".to_owned(),
        "rust".to_owned(),
    ));

    peer_connection.add_track(audio_track.clone()).await;

    // === 添加视频 Track，根据模式切换参数 ===
    let (mime_type, sdp_fmtp_line) = match offer.mode.as_str() {
        "low_latency" => ("video/VP8", "max-fr=30;max-fs=360"),
        "high_quality" => (
            "video/H264",
            "profile-level-id=42e01f;level-asymmetry-allowed=1",
        ),
        "balanced" | _ => ("video/VP8", "max-fr=24;max-fs=480"),
    };

    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: mime_type.to_owned(),
            clock_rate: 90000,
            channels: 0,
            sdp_fmtp_line: sdp_fmtp_line.to_owned(),
            rtcp_feedback: vec![],
        },
        "video".to_owned(),
        "rust".to_owned(),
    ));

    peer_connection.add_track(video_track.clone()).await;

    // === 创建 DataChannel 收键鼠命令 ===
    let data_channel_init = RTCDataChannelInit {
        ordered: Some(true),
        ..Default::default()
    };
    let dc = match peer_connection
        .create_data_channel("control", Some(data_channel_init))
        .await
    {
        Ok(ok) => ok,
        Err(err) => {
            println!("[WEBRTC]数据通道建立失败，{:?}", err);
            return HttpResponse::InternalServerError().body("操纵命令传输通道建立失败");
        }
    };

    //let dc_clone = dc.clone();
    dc.on_open(Box::new(move || {
        println!("DataChannel 已连接！");
        Box::pin(async {})
    }));

    dc.on_message(Box::new(move |msg| {
        println!("收到控制指令: {:?}", msg.data);
        // 这里你可以解析msg.data，根据需要做键盘/鼠标控制
        Box::pin(async {})
    }));

    // === 处理连接状态变化 ===
    let pc_clone = peer_connection.clone();
    peer_connection.on_ice_connection_state_change(Box::new(move |state| {
        println!("ICE 状态变化: {:?}", state);
        if state == RTCIceConnectionState::Connected {
            println!("[WEBRTC]WebRTC连接成功，可以推送音视频！");
            // 可以在这里启动推流任务，比如从显卡采集画面
        }
        Box::pin(async {})
    }));

    // === 处理 Offer 和 创建 Answer ===
    let offer_sdp;
    match RTCSessionDescription::offer(offer.sdp.clone()) {
        Ok(ok) => offer_sdp = ok,
        Err(err) => {
            println!("[WEBRTC]Offer解析失败，{:?}", err);
            return HttpResponse::ExpectationFailed().body("建立连接失败");
        }
    }
    peer_connection.set_remote_description(offer_sdp).await;

    let answer;
    match peer_connection.create_answer(None).await {
        Ok(ok) => answer = ok,
        Err(err) => {
            println!("[WEBRTC]Answear创建失败，{:?}", err);
            return HttpResponse::ExpectationFailed().body("建立连接失败");
        }
    };
    peer_connection.set_local_description(answer.clone()).await;

    // 返回 answer 给Flutter端
    HttpResponse::Ok().json(AnswerResponse { sdp: answer.sdp })
}
