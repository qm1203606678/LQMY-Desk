use crate::server_utils::auth::DeviceInfo;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::env;
use std::sync::Mutex;
// 存储全局信息的结构体
pub struct Config {
    pub server_address: String,      // 电脑开放的端口
    pub connection_password: String, // 生成的连接口令
}

lazy_static! {
    pub static ref CONFIG: Mutex<Config> = Mutex::new(Config {
        server_address: env::var("SERVER_ADDRESS").unwrap_or_else(|_| "127.0.0.1:9876".to_string()),
        connection_password: "Uninitia".to_string(),
    });
    pub static ref DEVICE_LIST: Mutex<HashMap<String, DeviceInfo>> = Mutex::new(HashMap::new());// 没有放到CONFIG，为了减少不必要的并发访问冲突
}
