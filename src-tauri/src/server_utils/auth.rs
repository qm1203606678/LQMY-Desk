// 用户认证
use actix_web::{web, HttpResponse, Responder};

use crate::config::{CONFIG, DEVICE_LIST};

#[derive(Debug, Clone, PartialEq)]
enum UserType {
    Blacklist,
    Normal,
    Trusted,
}

pub struct DeviceInfo {
    pub identifier: String,
    pub user_type: UserType,
}

pub fn check_device(identifier: &str, password: Option<&str>) -> Result<UserType, String> {
    let devices = DEVICE_LIST.lock().unwrap();
    match devices.get(identifier) {
        Some(device) => match device.user_type {
            UserType::Blacklist => Err("拒绝连接: 设备在黑名单中".to_string()),
            UserType::Trusted => Ok(UserType::Trusted),
            UserType::Normal => {
                let stored_password = CONFIG.lock().unwrap().connection_password.clone();
                if Some(stored_password.as_str()) == password {
                    Ok(UserType::Normal)
                } else {
                    Err("口令错误".to_string())
                }
            }
        },
        None => {
            let stored_password = CONFIG.lock().unwrap().connection_password.clone();
            if Some(stored_password.as_str()) == password {
                let mut devices = DEVICE_LIST.lock().unwrap();
                devices.insert(
                    identifier.to_string(),
                    DeviceInfo {
                        identifier: identifier.to_string(),
                        user_type: UserType::Normal,
                    },
                );
                Ok(UserType::Normal)
            } else {
                Err("口令错误".to_string())
            }
        }
    }
}

pub fn set_user_type(identifier: &str, user_type: UserType) {
    let mut devices = DEVICE_LIST.lock().unwrap();
    if let Some(device) = devices.get_mut(identifier) {
        device.user_type = user_type;
    }
}

/// 二级连接，处理连接口令
async fn handle_sec_level_connect(info: web::Json<(String, Option<String>)>) -> impl Responder {
    let (device_id, password) = info.into_inner();
    match check_device(&device_id, password.as_deref()) {
        Ok(UserType::Trusted) => HttpResponse::Ok().body("连接成功（信任设备）"),
        Ok(UserType::Normal) => HttpResponse::Ok().body("连接成功（普通设备）"),
        Ok(UserType::Blacklist) => HttpResponse::Forbidden().body("拒绝连接: 设备在黑名单中"), //正常情况不会match这个条目，这里写出来是为了编译通过
        Err(msg) => HttpResponse::Forbidden().body(msg),
    }
}
