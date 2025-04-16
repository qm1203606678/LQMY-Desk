use webrtc::api::APIBuilder;
use webrtc::peer_connection::{configuration::RTCConfiguration, RTCPeerConnection};
use webrtc::error::Result; // 这里的 Result 是 webrtc::Result<T>

pub async fn create_webrtc_connection() -> Result<RTCPeerConnection> {
    // 创建 API 实例
    let api = APIBuilder::new().build();

    // 配置 WebRTC 连接参数
    let config = RTCConfiguration::default();

    // 创建 PeerConnection
    let peer_connection = api.new_peer_connection(config).await?;

    Ok(peer_connection)
}
