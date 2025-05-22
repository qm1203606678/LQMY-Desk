use std::{
    sync::{atomic::AtomicBool, Arc, Mutex},
    thread,
    time::Instant,
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
use tokio::{
    sync::Notify,
    time::{self, Duration},
};

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
    data: Value,
}

// auth.rs 里有定义
use crate::{
    client_utils::{
        auth::AuthRequest, dialog::show_iknow_dialog, password::generate_connection_password,
    },
    config::{update_uuid, CONFIG, UUID},
    webrtc::webrtc_connect::{CandidateRequest, OfferRequest},
};
lazy_static! {
    pub static ref CLOSE_NOTIFY: Arc<Notify> = Arc::new(Notify::new());
    pub static ref SEND_NOTIFY: Arc<Notify> = Arc::new(Notify::new());
    pub static ref PENDING: Mutex<Vec<Value>> = Mutex::new(vec![]);
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

    {
        let register_json = json!({
            "type": "register",
            "client_type": "desktop"
        });
        connection
            .send(Message::Text(register_json.to_string().into()))
            .await?;
    }
    // ping消息的计时器，pong的超时检测
    let mut interval = time::interval(Duration::from_secs(2));
    // 上次心跳时间，由register_ack初始化
    let mut last_heartbeat = Instant::now();
    // pong 超时时间
    const PONG_TIMEOUT: Duration = Duration::from_secs(6);
    // 心跳检测的开关
    let mut registered_flag = false;
    // 发送锁
    let send_lock = Mutex::new("lock".to_string());

    // --- 主动发送
    loop {
        tokio::select! {
                // 发送消息
                _=SEND_NOTIFY.notified()=>{
                    let mut pending=PENDING.lock().unwrap();
                    while let Some(json) = pending.pop(){
                        connection.send(Message::Text(json.to_string().into())).await?;
                        println!("[CLIENT]发送消息:{:?}",json);

                    }
                    println!{"[CLIENT]pending应该为空{:?}",pending};
                    drop(pending);


                }
                // 先检查退出
                _ = CLOSE_NOTIFY.notified() => {
                    println!("[CLIENT] Exit requested, sending close JSON");
                    let close_json = json!({ "type": "close" });
                    send_lock.lock();
                    connection.send(Message::Text(close_json.to_string().into())).await?;
                    drop(send_lock.lock().unwrap());
                    // 继续等服务器发协议层的 Close 帧，或者直接 break 结束
                    break;
                }
                // ping信息
                _=interval.tick()=>{
                    if registered_flag
                    {
                        println!("[CLIENT] Ping");
                        let uuid=UUID.lock().unwrap().clone();
                        let ping_json=json!({
                            "type":"ping",
                            "from":uuid
                        });
                        drop(uuid);
                        send_lock.lock();
                        connection.send(Message::Text(ping_json.to_string().into())).await?;
                        drop(send_lock.lock().unwrap());
                    }
                    // heartbeat check
                    let now = Instant::now();
                    if now.duration_since(last_heartbeat)>PONG_TIMEOUT{
                        show_iknow_dialog("服务器断开", "请检查本地网络，或更换服务器").await;
                        println!("[CLIENT]pong超时，last{:?},check time{:?}",last_heartbeat,now);
                        return Ok(());
                    }

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
                                        //更新heartbeat
                                        last_heartbeat = Instant::now();
                                        registered_flag=true;
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
                                if let Ok(p) = serde_json::from_str::<PayloadWithCmd>(msg.payload.clone().as_str().unwrap())
                                {
                                    println!("[message]payload {:?}",p);
                                    match p.cmd.as_str() {
                                        "auth" => {
                                            if let Ok(auth_req) =
                                                serde_json::from_str::<AuthRequest>(p.data.as_str().unwrap())
                                            {
                                                println!("[message]payload value {:?}",auth_req);
                                                tokio::spawn(async move{
                                                    let result =
                                                        // 在这里弹出阻塞的 GUI 对话框，比如 rfd、native-dialog 等
                                                        crate::client_utils::auth::authenticate(web::Json(auth_req))
                                                    .await;
                                                    let uuid=UUID.lock().unwrap().clone();
                                                    let reply = json!({
                                                        "type": "message",
                                                        "target_uuid": msg.from,
                                                        "from":uuid,
                                                        "payload": json!(result),
                                                    });
                                                    drop(uuid);
                                                    let mut pending=PENDING.lock().unwrap();
                                                    pending.push(reply.clone());
                                                    drop(pending);
                                                    SEND_NOTIFY.notify_one();
                                                    // send_lock.lock();
                                                    // connection
                                                    //     .send(Message::Text(reply.to_string().into()))
                                                    //     .await;
                                                    // drop(send_lock);
                                                    println!("[CLIENT]认证返回：{:?}",reply)
                                                });

                                            }
                                        }
                                        "RTCoffer"=>{
                                            if let Ok(offer_req)=
                                                serde_json::from_value::<OfferRequest>(p.data.clone())
                                            {
                                                let res=crate::webrtc::webrtc_connect::handle_webrtc_offer(web::Json(offer_req)).await;
                                                let uuid=UUID.lock().unwrap().clone();
                                                let payload=json!({"cmd":"RTCansear","value":res});
                                                let reply = json!({
                                                    "type": "message",
                                                    "target_uuid": msg.from,
                                                    "from":uuid,
                                                    "payload": payload,
                                                });
                                                drop(uuid);
                                                connection
                                                    .send(Message::Text(reply.to_string().into()))
                                                    .await?;
                                                println!("[CLIENT]RTC返回Answear：{:?}",reply)
                                            }
                                        }
                                        "candidate"=>{
                                            if let Ok(candidate_req)=
                                                serde_json::from_value::<CandidateRequest>(p.data.clone())
                                            {
                                                let res=crate::webrtc::webrtc_connect::handle_ice_candidate(web::Json(candidate_req)).await;
                                                let uuid=UUID.lock().unwrap().clone();
                                                let payload=json!({"cmd":"candiate","value":res});
                                                let reply = json!({
                                                    "type": "message",
                                                    "target_uuid": msg.from,
                                                    "from":uuid,
                                                    "payload": res,
                                                });
                                                drop(uuid);
                                                connection
                                                    .send(Message::Text(reply.to_string().into()))
                                                    .await?;
                                                println!("[CLIENT]RTC返回Answear：{:?}",reply)
                                            }
                                        }
                                        _ => println!("[CLIENT] Unknown cmd: {}", p.cmd),
                                    }
                                }
                            }
                            "pong"=>{
                                last_heartbeat=Instant::now();
                                println!("[CLIENT]pong!")
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
