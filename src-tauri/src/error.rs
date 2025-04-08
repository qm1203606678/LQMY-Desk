use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("WebRTC error: {0}")]
    WebRTCError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("I/O error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl actix_web::ResponseError for ServerError {
    fn error_response(&self) -> actix_web::HttpResponse {
        actix_web::HttpResponse::InternalServerError().body(self.to_string())
    }
}
