use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use actix_web::web;
use awc::{
    ws::{Frame, Message},
    Client,
};
use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Notify;

// 你的 payload 和消息结构
#[derive(Debug, Deserialize)]
struct ServerMessage {
    #[serde(rename = "type")]
    msg_type: String,
    from: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct PayloadWithCmd {
    cmd: String,
    #[serde(flatten)]
    data: Value,
}

// auth.rs 里有定义
use crate::{
    client_utils::{auth::AuthRequest, password::generate_connection_password},
    config::{update_uuid, CONFIG},
};
lazy_static! {
    pub static ref CLOSE_NOTIFY: Arc<Notify> = Arc::new(Notify::new());
}
pub async fn start_client(exit_flag: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    let server_ws = CONFIG.lock().unwrap().server_address.clone(); // ws:// 或 wss://
    generate_connection_password().await;
    println!("[CLIENT] Connecting to {}...", server_ws);

    let (response, mut connection) = Client::new().ws(&server_ws).connect().await?;
    //println!("[CLIENT] Connected, status: {:?}", response.status());
    println!("[CLIENT] Connected, status: {:?}", response.status());

    // split Sink (for sending) and Stream (for receiving)
    //let (sink, mut stream) = connection.split();
    // wrap the sink in an Arc<Mutex<>> so we can share it between tasks
    //let sink = Arc::new(Mutex::new(sink));

    let register_json = json!({
        "type": "register",
        "client_type": "desktop"
    });
    connection
        .send(Message::Text(register_json.to_string().into()))
        .await?;
    // --- 主动发送
    loop {
        tokio::select! {
                // 先检查退出
                _ = CLOSE_NOTIFY.notified() => {
                    println!("[CLIENT] Exit requested, sending close JSON");
                    let close_json = json!({ "type": "close" });
                    connection.send(Message::Text(close_json.to_string().into())).await?;
                    // 继续等服务器发协议层的 Close 帧，或者直接 break 结束
                    break;
                }
                Some(Ok(frame)) = connection.next()=> {
                    // if exit_flag.load(Ordering::Relaxed) {
                    //     println!(
                    //         "[CLIENT] Exit requested, sending close frame,{:?}",
                    //         exit_flag
                    //     );
                    //     let json = json! ({
                    //         "type":"close"
                    //     });
                    //     match connection
                    //         .send(Message::Text(json.to_string().into()))
                    //         .await{
                    //             Ok(ok)=>{
                    //                 println!("[CLOSE FRAME SEND]Success{:?}",ok);
                    //             },
                    //             Err(e)=>{
                    //                 println!("[CLOSE FRAME SEND]Failure{:?}",e);
                    //                 return Err(Box::new(e));
                    //             }
                    //         };
                    //     break;
                    // }
                match frame {
                    Frame::Text(txt_bytes) => {
                        let txt_str = String::from_utf8_lossy(&txt_bytes);
                        let v: Value = match serde_json::from_str(&txt_str) {
                            Ok(v) => v,
                            Err(e) => {
                                eprintln!("[CLIENT] 非法的 JSON: {} ({})", txt_str, e);
                                break;
                            }
                        };
                        println!("[CLIENT]收到JSON{:?}", v);
                        let msg_type = match v.get("type").and_then(Value::as_str) {
                            Some(t) => t,
                            None => {
                                eprintln!("[CLIENT] 找不到 type 字段: {}", txt_str);
                                break;
                            }
                        };

                        match msg_type {
                            "register_ack" => {
                                let uuid = v.get("uuid").and_then(Value::as_str);
                                match uuid {
                                    Some(id) => {
                                        update_uuid(id);
                                    }
                                    None => {
                                        eprintln!("[CLIENT] 找不到 uuid 字段: {}", txt_str);
                                        break;
                                    }
                                }
                            }
                            "register_reject" => {
                                let reason = v.get("reason").and_then(Value::as_str);
                                match reason {
                                    Some(re) => {
                                        println!("[CLIENT]服务器注册拒绝：{:?}", re);
                                        //服务器连接失败，需要修改前端连接状态为关闭
                                        todo!()
                                    }
                                    None => {
                                        eprintln!("[CLIENT] 找不到 reason 字段: {}", txt_str);
                                        break;
                                    }
                                }
                            }
                            "message" => {
                                let msg = serde_json::from_str::<ServerMessage>(&txt_str).unwrap();
                                if let Ok(p) = serde_json::from_value::<PayloadWithCmd>(msg.payload.clone())
                                {
                                    match p.cmd.as_str() {
                                        "auth" => {
                                            if let Ok(auth_req) =
                                                serde_json::from_value::<AuthRequest>(p.data.clone())
                                            {
                                                let result = crate::client_utils::auth::authenticate(
                                                    web::Json(auth_req),
                                                )
                                                .await;

                                                let reply = json!({
                                                    "type": "message",
                                                    "target_uuid": msg.from,
                                                    "from":"123",
                                                    "payload": result,
                                                });

                                                connection
                                                    .send(Message::Text(reply.to_string().into()))
                                                    .await?;
                                            }
                                        }
                                        _ => println!("[CLIENT] Unknown cmd: {}", p.cmd),
                                    }
                                }
                            }
                            other => println!("[CLIENT] Unknown message type: {}", other),
                        }
                    }
                    Frame::Ping(msg) => {
                        connection.send(Message::Pong(msg)).await?;
                    }
                    Frame::Close(reason) => {
                        println!("[CLIENT] Connection closed: {:?}", reason);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
