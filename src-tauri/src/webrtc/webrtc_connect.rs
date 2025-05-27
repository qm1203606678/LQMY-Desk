use crate::client::{PENDING, SEND_NOTIFY};
use crate::config::{GLOBAL_STREAM_MANAGER, PEER_CONNECTION, UUID};
use crate::video_capturer::assembly::QualityConfig;
use crate::video_capturer::ffmpeg::{end_screen_capture, start_screen_capture};

use actix_web::web;

use rustls::client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use tokio::net::UdpSocket;
use webrtc::data_channel::RTCDataChannel;

use webrtc::rtp_transceiver::RTCPFeedback;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;

use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;

use webrtc::ice_transport::ice_server::RTCIceServer;

use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;

#[derive(Debug, Deserialize)]
pub struct JWTOfferRequest {
    pub client_uuid: String,
    pub sdp: String,
    pub mode: String, // "low_latency", "balanced", "high_quality"
    pub jwt: String,
}

#[derive(Serialize)]
pub struct AnswerResponse {
    pub client_uuid: String,
    pub sdp: String,
}

#[derive(Deserialize)]
pub struct JWTCandidateRequest {
    pub client_uuid: String,
    pub candidate: String,
    pub sdp_mid: Option<String>,
    pub sdp_mline_index: Option<u16>,
    pub jwt: String,
}

#[derive(Serialize)]
pub struct CandidateResponse {
    pub candidates: RTCIceCandidateInit,
}

#[derive(Deserialize)]
struct ControlCmd {
    cmd: String,
    mode: String,
}

// 初始 Offer/Answer，返回 AnswerResponse
pub async fn handle_webrtc_offer(offer: &web::Json<JWTOfferRequest>) -> AnswerResponse {
    println!("[WEBRTC]准备启动");
    let client_uuid = &offer.client_uuid;

    // 1. 初始化 MediaEngine 并注册 codecs
    let mut m = MediaEngine::default();
    if let Err(e) = m.register_default_codecs() {
        let msg = format!("MediaEngine 注册失败: {:?}", e);
        return AnswerResponse {
            client_uuid: client_uuid.clone(),
            sdp: msg,
        };
    }
    let api = APIBuilder::new().with_media_engine(m).build();

    // 2. 创建 PeerConnection
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec![
                "stun:stun.l.google.com:19302".into(),
                "stun:stun.qq.com:3478".into(),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    //let pc = api.new_peer_connection(config).await?;
    let pc = match api.new_peer_connection(config).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            let msg = format!("PeerConnection 创建失败: {:?}", e);
            return AnswerResponse {
                client_uuid: client_uuid.clone(),
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
    // let audio_track = Arc::new(TrackLocalStaticSample::new(
    //     RTCRtpCodecCapability {
    //         mime_type: "audio/opus".into(),
    //         clock_rate: 48000,
    //         channels: 2,

    //         ..Default::default()
    //     },
    //     "audio".into(),
    //     "rust-audio".into(),
    // ));
    // let _ = pc.add_track(audio_track).await;

    // 5. 添加视频轨，初始模式决定 fmtp line

    let video_track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: "video/H264".into(),
            sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                .into(),
            clock_rate: 90000,
            rtcp_feedback: vec![
                RTCPFeedback {
                    typ: "nack".to_owned(),
                    parameter: "".to_owned(),
                },
                RTCPFeedback {
                    typ: "nack".to_owned(),
                    parameter: "pli".to_owned(), // picture loss indication
                },
                RTCPFeedback {
                    typ: "goog-remb".to_owned(), // optional, for bandwidth estimation
                    parameter: "".to_owned(),
                },
                RTCPFeedback {
                    typ: "ccm".to_owned(),
                    parameter: "fir".to_owned(),
                },
            ],
            ..Default::default()
        },
        "video".into(),      // track ID
        "rust-video".into(), // stream ID
    ));
    // let video_track = Arc::new(TrackLocalStaticRTP::new(
    //     RTCRtpCodecCapability {
    //         mime_type: "video/H264".into(),
    //         sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
    //             .into(),
    //         clock_rate: 90000,
    //         rtcp_feedback: vec![
    //             RTCPFeedback {
    //                 typ: "nack".to_owned(),
    //                 parameter: "".to_owned(),
    //             },
    //             RTCPFeedback {
    //                 typ: "nack".to_owned(),
    //                 parameter: "pli".to_owned(), // picture loss indication
    //             },
    //             RTCPFeedback {
    //                 typ: "goog-remb".to_owned(), // optional, for bandwidth estimation
    //                 parameter: "".to_owned(),
    //             },
    //             RTCPFeedback {
    //                 typ: "ccm".to_owned(),
    //                 parameter: "fir".to_owned(),
    //             },
    //         ],
    //         ..Default::default()
    //     },
    //     "video".into(),
    //     "rust-video".into(),
    // ));
    pc.add_track(video_track.clone()).await.unwrap();

    // // 6. DataChannel 信令与重协商
    // 设置监听：对方创建的 DataChannel 到来时触发
    pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        println!("[WEBRTC] 收到远端 DataChannel：label = {}", dc.label());

        // 设置消息接收处理逻辑
        dc.on_message(Box::new(move |msg| {
            let data = &msg.data;

            // 解析为字符串
            if let Ok(text) = std::str::from_utf8(data) {
                println!("[WEBRTC] 收到 DataChannel 消息文本: {}", text);

                // 尝试解析 JSON
                match serde_json::from_str::<Value>(text) {
                    Ok(json) => {
                        println!("[WEBRTC] JSON 内容：{}", json);

                        // 你可以根据字段内容进行进一步处理
                        if let Some(cmd) = json.get("cmd").and_then(|v| v.as_str()) {
                            match cmd {
                                "mouse_move" => {
                                    println!("🖱️ 收到鼠标移动命令: {:?}", json);
                                    // TODO: 处理 mouse_move
                                }
                                "keyboard_input" => {
                                    println!("⌨️ 收到键盘输入命令: {:?}", json);
                                    // TODO: 处理 keyboard_input
                                }
                                _ => {
                                    println!("⚠️ 未知命令: {}", cmd);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ JSON 解析失败: {}", e);
                    }
                }
            } else {
                eprintln!("❌ 非 UTF-8 文本，无法处理");
            }

            Box::pin(async {})
        }));

        Box::pin(async {})
    }));

    // 7. 收集本地 ICE 候选
    {
        let uuid = client_uuid.clone();
        pc.on_ice_candidate(Box::new(move |opt| {
            if let Some(c) = opt {
                if let Ok(json) = c.to_json() {
                    let init = RTCIceCandidateInit {
                        candidate: json.candidate,
                        sdp_mid: json.sdp_mid,
                        sdp_mline_index: json.sdp_mline_index,
                        username_fragment: None,
                    };
                    let my_uuid = UUID.lock().unwrap().clone();
                    let res = send_ice_candidate(init);
                    let payload = json!({"cmd":"candidate","value":res});
                    let reply = json!({
                        "type": "message",
                        "target_uuid": uuid,
                        "from":my_uuid,
                        "payload": json!(payload),
                    });
                    drop(my_uuid);

                    let mut pending = PENDING.lock().unwrap();
                    pending.push(reply.clone());
                    drop(pending);
                    SEND_NOTIFY.notify_one();
                    println!("[CLIENT]RTC返回ICE：{:?}", reply);
                }
            }
            Box::pin(async {})
        }));
    }

    // 8. ICE 连接成功后推流
    {
        //let pc2 = pc.clone();
        pc.on_ice_connection_state_change(Box::new(move |state| {
            println!("[WEBRTC]连接状态改变，ICEState：{:?}", state);
            //monitor_video_send_stats(pc2.clone());
            Box::pin(async {})
        }));
    }

    {
        let pc2 = pc.clone();
        let client_uuid2 = client_uuid.clone();
        pc.on_peer_connection_state_change(Box::new(move |state| {
            println!("[WEBRTC]连接状态改变，ConnectionState： {:?}", state);

            if state == RTCPeerConnectionState::Connected {
                println!("✅ DTLS 握手成功");
                let video_track2 = video_track.clone();
                tokio::task::spawn(async move {
                    // 5. 启动后台任务，不断读包并写入 RTP Track
                    GLOBAL_STREAM_MANAGER.start_capture();
                    let q = QualityConfig {
                        name: "480p".to_string(),
                        width: 854,
                        height: 480,
                        bitrate: 500000,
                        fps: 30,
                    };
                    GLOBAL_STREAM_MANAGER.add_quality_stream(q).await;
                    GLOBAL_STREAM_MANAGER
                        .create_track_writer("480p", video_track2)
                        .await;
                });
                // tokio::task::spawn(async move {
                //     let mut buf = vec![0u8; 1500];
                //     let bind_addr: SocketAddr = format!("127.0.0.1:{}", 8765).parse().unwrap();
                //     let socket = UdpSocket::bind(bind_addr).await.unwrap();
                //     loop {
                //         if let Ok((size, _peer)) = socket.recv_from(&mut buf).await {
                //             //println!("[RTP]{:?}", buf[..12].to_vec());
                //             let mut inner = &buf[..size];
                //             if let Err(err) = video_track2.write(&inner).await {
                //                 eprintln!("[H264Sample] write err: {}", err);
                //             }
                //         } else {
                //             println!("[VIDEOSTREAM]packet_buf{:?}", buf)
                //         }
                //     }
                // });
                // // if let None = *FFMPEG_CHILD.lock().unwrap() {

                // // } else {
                // //     println!("[FFMPEG]已启动")
                // // }
                // start_screen_capture(8765);
                // println!("[VIDEOTRACK] 启动 UDP→WebRTC 推流 @{}", 8765);
            } else if state == RTCPeerConnectionState::Closed {
                let pc3 = pc2.clone();
                let client_uuid3 = client_uuid2.clone();
                tokio::task::spawn(async move {
                    if let Err(e) = pc3.close().await {
                        println!("[RTC]关闭peerconnection失败{:?}", e)
                    } else {
                        println!("[RTC]被动关闭{:?}的连接", client_uuid3)
                    }
                    end_screen_capture(false);
                });
            } else if state == RTCPeerConnectionState::Disconnected {
                let pc3 = pc2.clone();
                let client_uuid3 = client_uuid2.clone();
                tokio::task::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if pc3.connection_state() != RTCPeerConnectionState::Disconnected {
                        return;
                    }

                    if let Err(e) = pc3.close().await {
                        println!("[RTC]关闭peerconnection失败{:?}", e)
                    } else {
                        println!("[RTC]被动关闭{:?}的连接", client_uuid3)
                    };
                    end_screen_capture(false);
                });
            }
            Box::pin(async {})
        }));
    }
    // 9. SDP Offer/Answer
    let remote = RTCSessionDescription::offer(offer.sdp.clone()).unwrap();
    pc.set_remote_description(remote).await.unwrap();
    let answer = pc.create_answer(None).await.unwrap();
    if let Err(e) = pc.set_local_description(answer.clone()).await {
        eprint!("[LOCAL DES]{:?}", e)
    };

    // 10. 保存并返回
    PEER_CONNECTION
        .lock()
        .unwrap()
        .insert(client_uuid.clone(), pc.clone());
    AnswerResponse {
        client_uuid: client_uuid.clone(),
        sdp: answer.sdp,
    }
}

// 客户端上传远端 ICE 候选，直接返回结果字符串
pub async fn handle_ice_candidate(req: &web::Json<JWTCandidateRequest>) -> String {
    if let Some(pc) = PEER_CONNECTION.lock().unwrap().get(&req.client_uuid) {
        let init = RTCIceCandidateInit {
            candidate: req.candidate.clone(),
            sdp_mid: req.sdp_mid.clone(),
            sdp_mline_index: req.sdp_mline_index,
            username_fragment: None,
        };
        pc.add_ice_candidate(init).await.unwrap();
        "ICE 注入成功".into()
    } else {
        "无效 client_uuid".into()
    }
}

// 客户端拉取本地 ICE 候选，直接返回 CandidateResponse
// pub fn send_ice_candidate(uuid: &str) -> CandidateResponse {
//     //let client_uuid = info.get("client_uuid").cloned().unwrap_or_default();
//     // let res = CANDIDATES
//     //     .lock()
//     //     .unwrap()
//     //     .get(uuid)
//     //     .cloned()
//     //     .unwrap_or_default();
//     let mut lock = CANDIDATES.lock().unwrap();
//     let cands = lock.remove(uuid).unwrap_or_default();
//     CandidateResponse { candidates: cands }
// }
#[inline]
pub fn send_ice_candidate(candi: RTCIceCandidateInit) -> CandidateResponse {
    CandidateResponse { candidates: candi }
}

use tokio::time::{interval, Duration};
use webrtc::peer_connection::RTCPeerConnection;

pub fn monitor_video_send_stats(pc: Arc<RTCPeerConnection>) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;

            // 直接获取统计报告
            let report = pc.get_stats().await;
            println!("{:?}", report.reports);
            for (_id, stat) in report.reports {
                if let Ok(json) = serde_json::to_value(&stat) {
                    // 筛选出视频的发送统计
                    if json.get("type") == Some(&Value::String("outbound-rtp".into()))
                        && json.get("mediaType") == Some(&Value::String("video".into()))
                    {
                        let bytes_sent =
                            json.get("bytesSent").and_then(|v| v.as_u64()).unwrap_or(0);
                        let packets_sent = json
                            .get("packetsSent")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let frames_encoded = json
                            .get("framesEncoded")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        println!(
                            "[STATS] bytes_sent = {}, packets_sent = {}, frames_encoded = {}",
                            bytes_sent, packets_sent, frames_encoded
                        );
                    }
                }
            }
        }
    });
}

/// 关闭指定peerconnection，以uuid为索引
pub async fn close_peerconnection(client_uuid: &str) {
    // First, check if the connection exists and get a clone of the Arc
    let pc_option = {
        let pcs = PEER_CONNECTION.lock().unwrap();
        if pcs.is_empty() {
            println!("[CLOSE PC]指定用户的RTC连接不存在{:?}", client_uuid);
            return;
        }
        pcs.get(client_uuid).cloned() // Clone the Arc, not the connection itself
    }; // MutexGuard is dropped here

    // Now work with the cloned Arc outside the mutex
    if let Some(pc) = pc_option {
        if let Err(e) = pc.close().await {
            println!("[CLOSE PC]指定用户的RTC关闭失败，{:?},{:?}", e, client_uuid);
            return;
        }

        // Remove from the HashMap after successful close
        {
            let mut pcs = PEER_CONNECTION.lock().unwrap();
            pcs.remove(client_uuid);
        } // MutexGuard is dropped here

        println!("[CLOSE PC]指定用户的RTC关闭成功，{:?}", client_uuid);
        end_screen_capture(false);
    } else {
        println!("[CLOSE PC]指定用户的RTC连接不存在{:?}", client_uuid);
    }
}
