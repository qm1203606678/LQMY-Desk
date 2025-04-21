// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod config;
mod error;
mod server;
mod server_utils;
mod webrtc;
use std::sync::{Arc, Mutex};

use config::{reset_cur_user, CONFIG, CURRENT_USER};
use server_utils::user_manager::{
    delete_user, transfer_userinfo_to_vue, update_user_category, UserInfoString,
};
use tauri::Manager;
//use actix_web::{web, App, HttpServer, HttpResponse};
//use tauri::Manager;

struct AppState {
    is_running: Arc<Mutex<bool>>,
}

#[tauri::command]
fn start_server(state: tauri::State<AppState>) {
    let is_running = state.is_running.clone();

    std::thread::spawn(move || {
        let sys = actix_rt::System::new();
        *is_running.lock().unwrap() = true;

        let _ = sys.block_on(async { server::start_server().await });

        *is_running.lock().unwrap() = false;
    });
}

#[tauri::command]
fn stop_server(state: tauri::State<AppState>) {
    *state.is_running.lock().unwrap() = false;
    // 重置连接状况，将连接者信息清楚
    reset_cur_user();
    println!("[SERVER_INFO: Server stopped.");
}

#[tauri::command]
fn get_server_info() -> (String, String, String, String, String) {
    let config = CONFIG.lock().unwrap();
    println!(
        "[SERVER_INFO: Acquiring addr {:?} & password {:?}]",
        config.server_address, config.connection_password
    );
    let cur_user = CURRENT_USER.lock().unwrap();
    (
        config.server_address.clone(),
        config.connection_password.clone(),
        cur_user.device_name.clone(),
        cur_user.device_id.clone(),
        format!("{:?}", cur_user.user_type),
    )
}

#[tauri::command]
async fn get_user_info() -> Vec<UserInfoString> {
    let vec = transfer_userinfo_to_vue().await;
    println!("[USER LIST]传到VUE的用户信息为{:?}", vec);
    vec
}
#[tauri::command]
async fn update_user_type(serial: String, usertype: String) {
    update_user_category(serial, usertype).await;
}
#[tauri::command]
async fn delete_userinfo(serial: String) {
    delete_user(serial).await
}
fn main() {
    tauri::Builder::default()
        .manage(AppState {
            is_running: Arc::new(Mutex::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            start_server,
            stop_server,
            get_server_info,
            get_user_info,
            update_user_type,
            delete_userinfo
        ])
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}
