use actix_web::{web, HttpResponse};
use crate::error::ServerError;

pub async fn handle_webrtc_signaling(payload: web::Json<String>) -> Result<HttpResponse, ServerError> {
    let sdp = payload.into_inner();
    println!("Received SDP: {}", sdp);

    // 处理 SDP 信令
    if sdp.is_empty() {
        return Err(ServerError::WebRTCError("Empty SDP received".into()));
    }

    Ok(HttpResponse::Ok().body("SDP received"))
}
