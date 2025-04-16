use crate::config::{update_cur_user, CONFIG, CURRENT_USER, NO_CONNECTION_INDENTIFIER, THIS_TIME};
use crate::server_utils::user_manager::{add_device, get_user_by_serial};
use actix_web::{web, HttpResponse, Responder};
use chrono;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};

use super::dialog::show_confirmation_dialog;
use super::user_manager::UserType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    device_serial: String,
    this_time: String,
    exp: usize, // 过期时间
}

#[derive(Debug, Deserialize)]
pub struct AuthRequest {
    pub device_name: String,
    pub device_serial: String,
    pub password: String,
}

/// websocket连接,处理逻辑: 1.判断服务端是否空闲
///                        2.根据用户类别处理
///                             （1）黑名单：直接拒绝
///                             （2）信任：返回jwt，不验证口令，更新CURRENT——USER
///                             （3）普通：口令正确，并且ui确认，则返回jwt，更新CURRENT——USER
///                             （4）新用户：口令正确，并且ui确认，则返回jwt，更新CURRENT——USER，添加新用户信息
pub async fn authenticate(info: web::Json<AuthRequest>) -> impl Responder {
    {
        let cur_user = CURRENT_USER.lock().unwrap();
        if cur_user.device_id != NO_CONNECTION_INDENTIFIER {
            println!(
                "[SERVER_INFO]当前已连接设备,来自{:?}的连接请求直接拒绝",
                cur_user.device_name
            );
            return HttpResponse::Forbidden().body("已有设备连接，连接被拒绝");
        }
    }
    //let users = USER_LIST.lock().unwrap();
    let this_user = get_user_by_serial(&info.device_serial).await;

    match this_user {
        // 黑名单用户直接拒绝
        Some(user) if user.user_type == UserType::Blacklist => {
            HttpResponse::Forbidden().body("连接被拒绝")
        }
        // 信任用户直接返回jwt
        Some(user) if user.user_type == UserType::Trusted => {
            update_cur_user(&info, UserType::Trusted);
            let token = generate_jwt(&info.device_serial);
            HttpResponse::Ok().json(token)
        }
        //普通用户，验证成功则返回jwt
        Some(user) if user.user_type == UserType::Normal => {
            let stored_pw = CONFIG.lock().unwrap().connection_password.clone();
            if stored_pw == info.password {
                let msg = format!(
                    "是否允许来自{:?}({:?})的连接？",
                    info.device_name.clone(),
                    info.device_serial.clone()
                );
                if show_confirmation_dialog("连接请求", &msg) {
                    update_cur_user(&info, UserType::Normal);
                    let token = generate_jwt(&info.device_serial);
                    HttpResponse::Ok().json(token)
                } else {
                    HttpResponse::Unauthorized().body("连接被拒绝")
                }
            } else {
                HttpResponse::Unauthorized().body("连接口令错误")
            }
        }
        //新用户，验证成功则记录信息并返回jwt
        _ => {
            let stored_pw = CONFIG.lock().unwrap().connection_password.clone();
            if stored_pw == info.password {
                let msg = format!(
                    "是否允许来自{:?}({:?})的连接？",
                    info.device_name.clone(),
                    info.device_serial.clone()
                );
                if show_confirmation_dialog("连接请求", &msg) {
                    update_cur_user(&info, UserType::Normal);
                    let token = generate_jwt(&info.device_serial);
                    add_device(&info.device_name, &info.device_serial).await;
                    println!("[AUTH_INFO]生成jwt{:?}", token);
                    HttpResponse::Ok().json(token)
                } else {
                    HttpResponse::Unauthorized().body("连接被拒绝")
                }
            } else {
                HttpResponse::Unauthorized().body("连接口令错误")
            }
        }
    }
}

fn generate_jwt(device_serial: &str) -> String {
    let claims = Claims {
        device_serial: device_serial.to_string(),
        this_time: THIS_TIME.lock().unwrap().clone(),
        exp: chrono::Utc::now().timestamp() as usize + 3600, // 1 小时有效
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret("my_secret_key".as_ref()),
    )
    .unwrap()
}

fn validate_jwt(token: &str) -> bool {
    match decode::<Claims>(
        token,
        &DecodingKey::from_secret("my_secret_key".as_ref()),
        &Validation::default(),
    ) {
        Ok(decoded) => {
            let this_time = THIS_TIME.lock().unwrap().to_string();
            decoded.claims.this_time == this_time
        }
        Err(_) => false,
    }
}
