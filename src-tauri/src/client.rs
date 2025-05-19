use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use actix_web::web;
use actix_web::web::Bytes;
use awc::{
    ws::{Frame, Message},
    Client,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

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
    client_utils::auth::AuthRequest,
    config::{update_uuid, CONFIG},
};

pub async fn start_client(exit_flag: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    //退出逻辑，需要共享变量，后面stop_server函数触发
    let server_ws = CONFIG.lock().unwrap().server_address.clone(); // ws:// 或 wss://

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
    while let Some(Ok(frame)) = connection.next().await {
        if exit_flag.load(Ordering::Relaxed) {
            println!("[CLIENT] Exit requested, sending close frame");
            connection.send(Message::Close(None)).await?;
            break;
        }
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
                break;
            }
            _ => {}
        }
    }

    Ok(())
}
