use actix_web::{web, HttpResponse};
use crate::error::ServerError;

pub async fn handle_control_input(payload: web::Json<String>) -> Result<HttpResponse, ServerError> {
    let command = payload.into_inner();
    println!("Received control command: {}", command);

    // 解析并执行远程控制指令
    match command.as_str() {
        "mouse_move" => {
            println!("Moving mouse...");
            Ok(HttpResponse::Ok().body("Mouse moved"))
        }
        "mouse_click" => {
            println!("Clicking mouse...");
            Ok(HttpResponse::Ok().body("Mouse clicked"))
        }
        _ => Err(ServerError::Other("Unknown command".into())),
    }
}
