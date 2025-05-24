use bytes::BytesMut;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::task;
use webrtc::api::media_engine::MediaEngine;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp::packet::Packet;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::util::Unmarshal;
use webrtc::Error;

/// 给定一个已存在的 PeerConnection 和 UDP 端口，向它添加一个 H264 Track
/// 然后从 UDP socket 读取 RTP 包写入该 Track。
pub async fn start_webrtc_video_stream_on_pc(
    pc: Arc<RTCPeerConnection>,
    udp_port: u16,
) -> Result<(), Error> {
    // 1. 定义 H264 codec
    let codec = RTCRtpCodecCapability {
        mime_type: "video/H264".to_owned(),
        clock_rate: 90000,
        ..Default::default()
    };

    // 2. 创建一个 TrackLocalStaticRTP
    let video_track = Arc::new(TrackLocalStaticRTP::new(
        codec,
        "video".to_owned(),
        "webrtc-rust".to_owned(),
    ));

    // 3. 将 Track 添加到 PeerConnection
    pc.add_track(video_track.clone()).await?;

    // 4. 绑定 UDP Socket
    let bind_addr: SocketAddr = format!("127.0.0.1:{}", udp_port).parse().unwrap();
    let socket = UdpSocket::bind(bind_addr).await.unwrap();

    // 5. 启动后台任务，不断读包并写入 track
    task::spawn(async move {
        let mut buf = vec![0u8; 1500];
        loop {
            if let Ok((size, _peer)) = socket.recv_from(&mut buf).await {
                let mut packet_buf = BytesMut::from(&buf[..size]);
                if let Ok(pkt) = Packet::unmarshal(&mut packet_buf) {
                    let _ = video_track.write_rtp(&pkt).await;
                }
            }
        }
    });
    println!("[VIDEOTRACK]启动");
    Ok(())
}
