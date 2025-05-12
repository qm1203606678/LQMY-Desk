use actix_web::web;
use lazy_static::lazy_static;
use rand::{distr::Alphanumeric, Rng};
use std::sync::Mutex;
use std::{env, path::PathBuf};

use crate::server_utils::{
    auth::AuthRequest,
    user_manager::{UserInfo, UserType},
};

pub const NO_CONNECTION_INDENTIFIER: &str = "!@#$%^&*()";
// 存储全局信息的结构体
pub struct Config {
    pub server_address: String,      // 电脑开放的端口
    pub connection_password: String, // 生成的连接口令
}

lazy_static! {
    // 服务器信息 websocket IP/ 连接口令
    pub static ref CONFIG: Mutex<Config> = Mutex::new(Config {
        server_address: env::var("SERVER_ADDRESS").unwrap_or_else(|_| "127.0.0.1:9876".to_string()),
        connection_password: "Uninitia".to_string(),
    });
    // 当前连接用户信息
    pub static ref CURRENT_USER:Mutex<UserInfo>=Mutex::new(UserInfo{
        device_name:"".to_string(),
        device_id:NO_CONNECTION_INDENTIFIER.to_string(),
        user_type:UserType::Normal
    });
    pub static ref APPDATA_PATH:Mutex<PathBuf>=Mutex::new(load_storage_path());
    // JWT加密密钥，每次启动不一样
    pub static ref JWT_KEY:Mutex<String>=Mutex::new(generate_jwt_key());
    // 标识jwt是本次启动生成的
    pub static ref THIS_TIME:Mutex<String>=Mutex::new(generate_jwt_key());
    //已经移到user_manage.rs管理 pub static ref DEVICE_LIST: Mutex<HashMap<String, DeviceInfo>> = Mutex::new(HashMap::new());// 没有放到CONFIG，为了减少不必要的并发访问冲突
}

fn load_storage_path() -> PathBuf {
    "E:/WHU/SoftwareEngineering/GroupWork/LQMY-Desk".into()
}

pub fn get_userinfo_path() -> PathBuf {
    let path = APPDATA_PATH.lock().unwrap();
    path.join("user_data.json")
}

fn generate_jwt_key() -> String {
    let password: String = rand::rng()
        .sample_iter(&Alphanumeric)
        .take(8) // 这里修改口令长度
        .map(char::from)
        .collect();
    password
}

pub fn update_cur_user(info: &web::Json<AuthRequest>, usertype: UserType) {
    let mut cur_user = CURRENT_USER.lock().unwrap();

    cur_user.device_id = info.device_serial.clone();
    cur_user.device_name = info.device_name.clone();
    cur_user.user_type = usertype;
    println!(
        "[SERVER_INFO]连接用户信息更新：设备名：{:?}，设备序列号：{:?}，用户类型：{:?}",
        cur_user.device_name, cur_user.device_id, cur_user.user_type
    );
}

pub fn reset_cur_user() {
    let mut cur_user = CURRENT_USER.lock().unwrap();

    cur_user.device_id = NO_CONNECTION_INDENTIFIER.to_string();
    cur_user.device_name = "".to_string();
    cur_user.user_type = UserType::Normal;
    println!(
        "[SERVER_INFO]连接用户信息重置为：设备名：{:?}，设备序列号：{:?}，用户类型：{:?}",
        cur_user.device_name, cur_user.device_id, cur_user.user_type
    );
}
