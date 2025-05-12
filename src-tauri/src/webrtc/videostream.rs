use bytes::BytesMut;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::task;
use webrtc::api::APIBuilder;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::util::Unmarshal;

/// 启动 WebRTC 视频 RTP 推流
pub async fn start_webrtc_video_stream(port: u16) -> Result<(), webrtc::Error> {
    // 初始化 API 和 PeerConnection
    let api = APIBuilder::new().build();
    let pc = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await?);

    // 创建 H264 视频 Track
    let codec = RTCRtpCodecCapability {
        mime_type: "video/H264".to_string(),
        clock_rate: 90000,
        ..Default::default()
    };
    let video_track = Arc::new(TrackLocalStaticRTP::new(
        codec,
        "video".to_string(),
        "webrtc-rust".to_string(),
    ));

    pc.add_track(video_track.clone()).await?;

    // 监听 ICE 连接状态，连接成功后启动 UDP->RTP 推流
    let _pc_clone = pc.clone();
    pc.on_ice_connection_state_change(Box::new(move |state| {
        if state == webrtc::ice_transport::ice_connection_state::RTCIceConnectionState::Connected {
            let track = video_track.clone();
            let bind_port = port;
            // 异步推流任务
            task::spawn(async move {
                let addr = format!("0.0.0.0:{}", bind_port);
                let socket = UdpSocket::bind(&addr).await.unwrap();
                let mut buf = vec![0u8; 1500];
                loop {
                    if let Ok((size, _)) = socket.recv_from(&mut buf).await {
                        let mut packet_buf = BytesMut::from(&buf[..size]);
                        if let Ok(packet) = webrtc::rtp::packet::Packet::unmarshal(&mut packet_buf) {
                            let _ = track.write_rtp(&packet).await;
                        }
                    }
                }
            });
        }
        Box::pin(async {})
    }));

    Ok(())
}