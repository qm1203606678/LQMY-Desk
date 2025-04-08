use crate::{
    config::CONFIG,
    server_utils::{
        control::handle_control_input, password::generate_connection_password,
        signaling::handle_webrtc_signaling,
    },
};
use actix_web::{web, App, HttpResponse, HttpServer, Responder};

async fn health_check() -> impl Responder {
    HttpResponse::Ok().body("Server is running")
}

pub async fn start_server() -> std::io::Result<()> {
    let addr: String = CONFIG.lock().unwrap().server_address.clone();
    println!("[SERVER INFO]服务器启动中...");
    println!("[SERVER INFO]绑定地址: {}", addr);
    generate_connection_password().await;
    HttpServer::new(|| {
        App::new()
            .route("/health", web::get().to(health_check))
            .route("/webrtc", web::post().to(handle_webrtc_signaling))
            .route("/control", web::post().to(handle_control_input))
    })
    .bind(&addr)?
    .run()
    .await
}
