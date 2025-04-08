// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod config;
mod error;
mod server;
mod server_utils;
mod webrtc;
use std::sync::{Arc, Mutex};

use config::CONFIG;
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
    println!("[SERVER_INFO: Server stopped.");
}

#[tauri::command]
fn get_server_info() -> (String, String) {
    let config = CONFIG.lock().unwrap();
    println!(
        "[SERVER_INFO: Acquiring addr {:?} & password {:?}]",
        config.server_address, config.connection_password
    );
    (
        config.server_address.clone(),
        config.connection_password.clone(),
    )
}
fn main() {
    tauri::Builder::default()
        .manage(AppState {
            is_running: Arc::new(Mutex::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            start_server,
            stop_server,
            get_server_info
        ])
        .run(tauri::generate_context!())
        .expect("Failed to run Tauri application");
}
