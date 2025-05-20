use actix_web::web;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use crate::config::{CANDIDATES, PEER_CONNECTION};
use crate::webrtc::videostream::start_webrtc_video_stream;

#[derive(Deserialize)]
pub struct OfferRequest {

    pub sdp: String,
    pub mode: String, // "low_latency", "balanced", "high_quality"
}

#[derive(Serialize)]
pub struct AnswerResponse {
    pub session_id: String,
    pub sdp: String,
}

#[derive(Deserialize)]
pub struct CandidateRequest {
    pub session_id: String,
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
}

#[derive(Serialize)]
pub struct CandidateResponse {
    pub candidates: Vec<RTCIceCandidateInit>,
}

#[derive(Deserialize)]
struct ControlCmd {
    cmd: String,
    mode: String,
}

// 初始 Offer/Answer，返回 AnswerResponse
pub async fn handle_webrtc_offer(offer: web::Json<OfferRequest>) -> AnswerResponse {
    // 1. 初始化 MediaEngine 并注册 codecs
    let mut m = MediaEngine::default();
    if let Err(e) = m.register_default_codecs() {
        let msg = format!("MediaEngine 注册失败: {:?}", e);
        return AnswerResponse {
            session_id: "0".into(),
            sdp: msg,
        };
    }
    let api = APIBuilder::new().with_media_engine(m).build();

    // 2. 创建 PeerConnection
    let pc = match api.new_peer_connection(RTCConfiguration::default()).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            let msg = format!("PeerConnection 创建失败: {:?}", e);
            return AnswerResponse {
                session_id: "0".into(),
                sdp: msg,
            };
        }
    };

    // 3. (可选) negotiationneeded 调试
    pc.on_negotiation_needed(Box::new(|| {
        println!("[WEBRTC] negotiationneeded");
        Box::pin(async {})
    }));

    // 4. 添加音轨（Opus）
    let audio_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: "audio/opus".into(),
            clock_rate: 48000,
            channels: 2,
            ..Default::default()
        },
        "audio".into(),
        "rust-audio".into(),
    ));
    let _ = pc.add_track(audio_track).await;

    // 5. 添加视频轨，初始模式决定 fmtp line
    let (mime, fmt) = match offer.mode.as_str() {
        "low_latency" => ("video/VP8", "max-fr=30;max-fs=360"),
        "high_quality" => ("video/H264", "profile-level-id=42e01f;level-asymmetry-allowed=1"),
        _ => ("video/VP8", "max-fr=24;max-fs=480"), // balanced
    };
    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: mime.into(),
            sdp_fmtp_line: fmt.into(),
            clock_rate: 90000,
            ..Default::default()
        },
        "video".into(),
        "rust-video".into(),
    ));
    let video_sender = pc.add_track(video_track.clone()).await.unwrap();

    // 6. DataChannel 信令与重协商
    let dc = {
        let init = RTCDataChannelInit { ordered: Some(true), ..Default::default() };
        pc.create_data_channel("control", Some(init)).await.unwrap()
    };
    let dc_re = dc.clone();
    let pc_for_dc = pc.clone();
    let video_sender_for_dc = video_sender.clone();
    dc.on_message(Box::new(move |msg| {
        let pc = pc_for_dc.clone();
        let video_sender = video_sender_for_dc.clone();
        let dc_inner = dc_re.clone();
        let text = String::from_utf8_lossy(&msg.data).to_string();
        if let Ok(cmd) = serde_json::from_str::<ControlCmd>(&text) {
            if cmd.cmd == "switch_mode" {
                let mode = cmd.mode.clone();
                tokio::spawn(async move {
                    let (mime, fmt) = match mode.as_str() {
                        "low_latency" => ("video/VP8", "max-fr=30;max-fs=360"),
                        "high_quality" => ("video/H264", "profile-level-id=42e01f;level-asymmetry-allowed=1"),
                        _ => ("video/VP8", "max-fr=24;max-fs=480"),
                    };
                    let new_track = Arc::new(TrackLocalStaticSample::new(
                        RTCRtpCodecCapability {
                            mime_type: mime.into(),
                            sdp_fmtp_line: fmt.into(),
                            clock_rate: 90000,
                            ..Default::default()
                        },
                        "video".into(),
                        "rust-video".into(),
                    ));
                    let _ = video_sender.replace_track(Some(new_track)).await;
                    let offer = pc.create_offer(None).await.unwrap();
                    pc.set_local_description(offer.clone()).await.unwrap();
                    let msg_json = json!({ "cmd": "renegotiate", "sdp": offer.sdp });
                    let _ = dc_inner.send_text(msg_json.to_string()).await;
                });
            }
        }
        Box::pin(async {})
    }));

    // 7. 收集本地 ICE 候选
    let session_id = Uuid::new_v4().to_string();
    CANDIDATES.lock().unwrap().insert(session_id.clone(), Vec::new());
    {
        let sid = session_id.clone();
        pc.on_ice_candidate(Box::new(move |opt| {
            if let Some(c) = opt {
                if let Ok(json) = c.to_json() {
                    let init = RTCIceCandidateInit {
                        candidate: json.candidate,
                        sdp_mid: json.sdp_mid,
                        sdp_mline_index: json.sdp_mline_index,
                        username_fragment: None,
                    };
                    CANDIDATES.lock().unwrap().get_mut(&sid).unwrap().push(init);
                }
            }
            Box::pin(async {})
        }));
    }

    // 8. 推流时机：ICE Connected
    {
        let pc2 = pc.clone();
        pc.on_ice_connection_state_change(Box::new(move |state| {
            if state == RTCIceConnectionState::Connected {
                tokio::spawn(async move {
                    let _ = start_webrtc_video_stream(5004).await;
                });
            }
            Box::pin(async {})
        }));
    }

    // 9. SDP Offer/Answer
    let remote = RTCSessionDescription::offer(offer.sdp.clone()).unwrap();
    pc.set_remote_description(remote).await.unwrap();
    let answer = pc.create_answer(None).await.unwrap();
    pc.set_local_description(answer.clone()).await.unwrap();

    // 10. 保存并返回
    PEER_CONNECTION.lock().unwrap().insert(session_id.clone(), pc.clone());
    AnswerResponse {
        session_id,
        sdp: answer.sdp,
    }
}

// 客户端上传远端 ICE 候选，直接返回结果字符串
pub async fn handle_ice_candidate(req: web::Json<CandidateRequest>) -> String {
    if let Some(pc) = PEER_CONNECTION.lock().unwrap().get(&req.session_id) {
        let init = RTCIceCandidateInit {
            candidate: req.candidate.clone(),
            sdp_mid: req.sdp_mid.clone(),
            sdp_mline_index: req.sdp_mline_index,
            username_fragment: None,
        };
        pc.add_ice_candidate(init).await.unwrap();
        "ICE 注入成功".into()
    } else {
        "无效 session_id".into()
    }
}

// 客户端拉取本地 ICE 候选，直接返回 CandidateResponse
pub async fn get_ice_candidates(info: web::Query<HashMap<String, String>>) -> CandidateResponse {
    let session_id = info.get("session_id").cloned().unwrap_or_default();
    let cands = CANDIDATES
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .unwrap_or_default();
    CandidateResponse { candidates: cands }
}
