use crate::{
    config::CONFIG,
    server_utils::{
        auth::authenticate, control::handle_control_input, password::generate_connection_password,
        signaling::handle_webrtc_signaling,
    },
};
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use rustls::{Certificate, PrivateKey, ServerConfig};
use std::{fs::File, io::BufReader, sync::Arc};

async fn health_check() -> impl Responder {
    HttpResponse::Ok().body("Server is running")
}

fn load_rustls_config() -> ServerConfig {
    let cert_file = &mut BufReader::new(File::open("cert.pem").unwrap());
    let key_file = &mut BufReader::new(File::open("key.pem").unwrap());

    let cert_chain = rustls_pemfile::certs(cert_file)
        .unwrap()
        .into_iter()
        .map(Certificate)
        .collect();

    let mut keys = rustls_pemfile::pkcs8_private_keys(key_file).unwrap();
    let key = PrivateKey(keys.remove(0));

    ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .unwrap()
}

pub async fn start_server() -> std::io::Result<()> {
    let addr: String = CONFIG.lock().unwrap().server_address.clone();
    println!("[SERVER INFO]服务器启动中...");
    println!("[SERVER INFO]绑定地址: {}", addr);

    let config = load_rustls_config();

    generate_connection_password().await;
    HttpServer::new(|| {
        App::new()
            .route("/auth", web::post().to(authenticate)) //认证用户，返回jwt
            .route("/health", web::get().to(health_check))
            .route("/webrtc", web::post().to(handle_webrtc_signaling))
            .route("/control", web::post().to(handle_control_input))
    })
    .bind_rustls_021(&addr, config)?
    .run()
    .await
}
