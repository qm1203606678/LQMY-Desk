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

async fn start_webrtc_video_stream() -> Result<(), webrtc::Error> {
    let api = APIBuilder::new().build();
    let peer_connection = Arc::new(api.new_peer_connection(RTCConfiguration::default()).await?);

    let codec = RTCRtpCodecCapability {
        mime_type: "video/H264".to_owned(),
        ..Default::default()
    };

    let video_track = Arc::new(TrackLocalStaticRTP::new(
        codec,
        "video".to_owned(),
        "webrtc-rust".to_owned(),
    ));

    peer_connection.add_track(video_track.clone()).await?;

    task::spawn(async move {
        let socket = UdpSocket::bind("127.0.0.1:5004").await.unwrap();
        let mut buf = vec![0u8; 1500];

        loop {
            let (size, _) = socket.recv_from(&mut buf).await.unwrap();
            let mut packet_buf = BytesMut::from(&buf[..size]);
            let packet = webrtc::rtp::packet::Packet::unmarshal(&mut packet_buf).unwrap();

            let _ = video_track.write_rtp(&packet).await;
        }
    });

    Ok(())
}
