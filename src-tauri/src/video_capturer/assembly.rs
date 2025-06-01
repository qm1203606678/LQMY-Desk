use bytes::Bytes;
use openh264::encoder::{Encoder, EncoderConfig, IntraFramePeriod, QpRange};
use openh264::formats::YUVSource;
use rusty_duplication::{FrameInfoExt, Scanner, VecCapturer};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use webrtc::media::Sample;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

// ==================== 核心数据结构 ====================

/// 原始帧数据 - 使用引用计数避免拷贝
#[derive(Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub data: Arc<Vec<u8>>, // BGRA数据
    pub timestamp: u64,
    pub frame_id: u64,
}

/// 质量配置
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct QualityConfig {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bitrate: u32,
    pub fps: u32,
    pub max_keyframe_interval: u32, // 最大关键帧间隔
}

impl QualityConfig {
    pub fn new(name: &str, width: u32, height: u32, bitrate: u32, fps: u32) -> Self {
        Self {
            name: name.to_string(),
            width,
            height,
            bitrate,
            fps,
            max_keyframe_interval: fps * 2, // 默认2秒一个关键帧
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.width == 0 || self.height == 0 || self.width % 2 != 0 || self.height % 2 != 0 {
            return Err("dimensions must be positive and even");
        }
        if self.fps == 0 || self.fps > 120 {
            return Err("fps must be between 1 and 120");
        }
        if self.bitrate == 0 {
            return Err("bitrate must be positive");
        }
        Ok(())
    }
}

/// 编码后的帧
#[derive(Clone)]
pub struct EncodedFrame {
    pub data: Bytes,
    pub timestamp: u64,
    pub frame_id: u64,
    pub is_keyframe: bool,
    pub quality: String,
}

/// YUV数据结构 - 优化内存布局
pub struct YuvBuffer {
    pub width: usize,
    pub height: usize,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl YuvBuffer {
    pub fn new(width: usize, height: usize) -> Self {
        let y_size = width * height;
        let uv_size = y_size / 4;

        Self {
            width,
            height,
            y: vec![0; y_size],
            u: vec![0; uv_size],
            v: vec![0; uv_size],
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if self.width != width || self.height != height {
            let y_size = width * height;
            let uv_size = y_size / 4;

            self.width = width;
            self.height = height;
            self.y.resize(y_size, 0);
            self.u.resize(uv_size, 0);
            self.v.resize(uv_size, 0);
        }
    }
}

impl YUVSource for YuvBuffer {
    fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn strides(&self) -> (usize, usize, usize) {
        (self.width, self.width / 2, self.width / 2)
    }

    fn y(&self) -> &[u8] {
        &self.y
    }

    fn u(&self) -> &[u8] {
        &self.u
    }

    fn v(&self) -> &[u8] {
        &self.v
    }
}

// ==================== 高效图像处理 ====================

/// 快速BGRA到YUV420转换 - 优化算法
pub fn convert_bgra_to_yuv420(
    bgra: &[u8],
    src_width: usize,
    src_height: usize,
    yuv: &mut YuvBuffer,
) {
    yuv.resize(src_width, src_height);

    // 使用查找表优化的转换系数
    const Y_R: i32 = 77; // 0.299 * 256
    const Y_G: i32 = 150; // 0.587 * 256
    const Y_B: i32 = 29; // 0.114 * 256
    const U_R: i32 = -43; // -0.169 * 256
    const U_G: i32 = -85; // -0.331 * 256
    const U_B: i32 = 128; // 0.5 * 256
    const V_R: i32 = 128; // 0.5 * 256
    const V_G: i32 = -107; // -0.419 * 256
    const V_B: i32 = -21; // -0.081 * 256

    // Y平面处理 - 逐行处理提高缓存效率
    for y in 0..src_height {
        let y_offset = y * src_width;
        let bgra_offset = y * src_width * 4;

        for x in 0..src_width {
            let pixel_idx = bgra_offset + x * 4;
            let b = bgra[pixel_idx] as i32;
            let g = bgra[pixel_idx + 1] as i32;
            let r = bgra[pixel_idx + 2] as i32;

            let y_val = ((Y_R * r + Y_G * g + Y_B * b) >> 8) + 16;
            yuv.y[y_offset + x] = y_val.clamp(16, 235) as u8;
        }
    }

    // UV平面处理 - 2x2子采样
    let uv_width = src_width / 2;
    let uv_height = src_height / 2;

    for uv_y in 0..uv_height {
        let uv_row_offset = uv_y * uv_width;

        for uv_x in 0..uv_width {
            let src_x = uv_x * 2;
            let src_y = uv_y * 2;

            // 采样2x2区域
            let mut sum_u = 0i32;
            let mut sum_v = 0i32;

            for dy in 0..2 {
                for dx in 0..2 {
                    let pixel_idx = ((src_y + dy) * src_width + (src_x + dx)) * 4;
                    let b = bgra[pixel_idx] as i32;
                    let g = bgra[pixel_idx + 1] as i32;
                    let r = bgra[pixel_idx + 2] as i32;

                    sum_u += (U_R * r + U_G * g + U_B * b) >> 8;
                    sum_v += (V_R * r + V_G * g + V_B * b) >> 8;
                }
            }

            // 平均值并添加偏移
            let uv_idx = uv_row_offset + uv_x;
            yuv.u[uv_idx] = ((sum_u >> 2) + 128).clamp(16, 240) as u8;
            yuv.v[uv_idx] = ((sum_v >> 2) + 128).clamp(16, 240) as u8;
        }
    }
}

/// 高效图像缩放 - 双线性插值
pub fn resize_bgra(
    src: &[u8],
    src_width: usize,
    src_height: usize,
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
) {
    let x_scale = src_width as f32 / dst_width as f32;
    let y_scale = src_height as f32 / dst_height as f32;

    for dst_y in 0..dst_height {
        let src_y_f = dst_y as f32 * y_scale;
        let src_y = src_y_f as usize;
        let y_frac = src_y_f - src_y as f32;
        let src_y1 = (src_y + 1).min(src_height - 1);

        for dst_x in 0..dst_width {
            let src_x_f = dst_x as f32 * x_scale;
            let src_x = src_x_f as usize;
            let x_frac = src_x_f - src_x as f32;
            let src_x1 = (src_x + 1).min(src_width - 1);

            // 获取4个相邻像素
            let p00_idx = (src_y * src_width + src_x) * 4;
            let p01_idx = (src_y * src_width + src_x1) * 4;
            let p10_idx = (src_y1 * src_width + src_x) * 4;
            let p11_idx = (src_y1 * src_width + src_x1) * 4;

            let dst_idx = (dst_y * dst_width + dst_x) * 4;

            // 对每个颜色通道进行双线性插值
            for c in 0..4 {
                let p00 = src[p00_idx + c] as f32;
                let p01 = src[p01_idx + c] as f32;
                let p10 = src[p10_idx + c] as f32;
                let p11 = src[p11_idx + c] as f32;

                let top = p00 * (1.0 - x_frac) + p01 * x_frac;
                let bottom = p10 * (1.0 - x_frac) + p11 * x_frac;
                let result = top * (1.0 - y_frac) + bottom * y_frac;

                dst[dst_idx + c] = result.clamp(0.0, 255.0) as u8;
            }
        }
    }
}

// ==================== 编码器管理 ====================

/// 单个质量流的编码器
struct QualityEncoder {
    encoder: Encoder,
    config: QualityConfig,
    frame_interval: Duration,
    last_encode_time: Instant,
    frame_count: u64,
    yuv_buffer: YuvBuffer,
    resize_buffer: Vec<u8>,
}

impl QualityEncoder {
    fn new(config: QualityConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let loader = openh264::OpenH264API::from_source();

        let enc_config = EncoderConfig::new();
        let _ = enc_config.skip_frames(false);
        let _ = enc_config.bitrate(openh264::encoder::BitRate::from_bps(config.bitrate));
        let _ = enc_config.max_frame_rate(openh264::encoder::FrameRate::from_hz(config.fps as f32));
        let _ = enc_config.usage_type(openh264::encoder::UsageType::ScreenContentRealTime);
        let _ = enc_config.profile(openh264::encoder::Profile::Baseline);
        let _ = enc_config.level(openh264::encoder::Level::Level_3_1);
        let _ = enc_config.complexity(openh264::encoder::Complexity::Low);
        let _ = enc_config.qp(QpRange::new(20, 35));
        let _ = enc_config.intra_frame_period(IntraFramePeriod::from_num_frames(
            config.max_keyframe_interval,
        ));

        let encoder = Encoder::with_api_config(loader, enc_config)
            .map_err(|e| format!("failed to create encoder: {}", e))?;

        let frame_interval = Duration::from_nanos(1_000_000_000 / config.fps as u64);
        let yuv_buffer = YuvBuffer::new(config.width as usize, config.height as usize);
        let resize_buffer = vec![0u8; (config.width * config.height * 4) as usize];

        Ok(Self {
            encoder,
            config,
            frame_interval,
            last_encode_time: Instant::now(),
            frame_count: 0,
            yuv_buffer,
            resize_buffer,
        })
    }

    fn should_encode(&self) -> bool {
        self.last_encode_time.elapsed() >= self.frame_interval
    }

    fn encode(
        &mut self,
        raw_frame: &RawFrame,
    ) -> Result<Option<EncodedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.should_encode() {
            return Ok(None);
        }

        self.last_encode_time = Instant::now();

        // 缩放处理
        let source_data =
            if raw_frame.width == self.config.width && raw_frame.height == self.config.height {
                &raw_frame.data[..]
            } else {
                resize_bgra(
                    &raw_frame.data,
                    raw_frame.width as usize,
                    raw_frame.height as usize,
                    &mut self.resize_buffer,
                    self.config.width as usize,
                    self.config.height as usize,
                );
                &self.resize_buffer[..]
            };

        // YUV转换
        convert_bgra_to_yuv420(
            source_data,
            self.config.width as usize,
            self.config.height as usize,
            &mut self.yuv_buffer,
        );

        // H.264编码
        match self.encoder.encode(&self.yuv_buffer) {
            Ok(bitstream) => {
                let is_keyframe = self.frame_count % self.config.max_keyframe_interval as u64 == 0;

                let encoded_frame = EncodedFrame {
                    data: Bytes::from(bitstream.to_vec()),
                    timestamp: raw_frame.timestamp,
                    frame_id: raw_frame.frame_id,
                    is_keyframe,
                    quality: self.config.name.clone(),
                };

                self.frame_count += 1;
                Ok(Some(encoded_frame))
            }
            Err(e) => Err(format!("encoding failed: {}", e).into()),
        }
    }
}

// ==================== 主管理器 ====================

/// 优化后的多流管理器
pub struct MultiStreamManager {
    // 原始帧广播
    raw_frame_tx: broadcast::Sender<RawFrame>,

    // 编码器管理
    encoders: Arc<Mutex<HashMap<String, QualityEncoder>>>,

    // 编码后的帧分发
    encoded_streams: Arc<RwLock<HashMap<String, broadcast::Sender<EncodedFrame>>>>,

    // WebRTC轨道管理
    track_writers: Arc<RwLock<HashMap<String, Vec<Arc<TrackLocalStaticSample>>>>>,

    // Track写关闭信号
    track_shutdown_tx: Arc<tokio::sync::Mutex<HashMap<String, Arc<AtomicBool>>>>,

    // Encoder关闭信号

    // 控制信号
    shutdown_signal: Arc<AtomicBool>,
    frame_counter: Arc<AtomicU64>,

    // 任务句柄 - 使用独占线程
    capture_handle: Option<thread::JoinHandle<()>>,
    encoding_handle: Option<thread::JoinHandle<()>>,

    // 用于与独占线程通信
    capture_shutdown_tx: Option<mpsc::UnboundedSender<()>>,
    encoding_shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}

impl MultiStreamManager {
    /// 创建一个新的 MultiStreamManager
    pub fn new() -> Self {
        let (raw_frame_tx, _) = broadcast::channel(16); // 增加缓冲区以应对突发流量

        Self {
            raw_frame_tx,
            encoders: Arc::new(Mutex::new(HashMap::new())),
            encoded_streams: Arc::new(RwLock::new(HashMap::new())),
            track_writers: Arc::new(RwLock::new(HashMap::new())),
            shutdown_signal: Arc::new(AtomicBool::new(false)),
            frame_counter: Arc::new(AtomicU64::new(0)),
            capture_handle: None,
            encoding_handle: None,
            capture_shutdown_tx: None,
            encoding_shutdown_tx: None,
            track_shutdown_tx: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 启动桌面捕获（如果尚未启动）；内部会 spawn 一个任务
    pub async fn start_capture(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.capture_handle.is_some() {
            return Ok(());
        }

        let tx = self.raw_frame_tx.clone();
        let frame_counter = self.frame_counter.clone();
        let shutdown_signal = self.shutdown_signal.clone();
        let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();

        // 创建独占线程进行屏幕捕获
        let handle = thread::Builder::new()
            .name("screen-capture".to_string())
            .spawn(move || {
                // 设置线程优先级（如果系统支持）
                #[cfg(target_os = "windows")]
                unsafe {
                    use winapi::um::processthreadsapi::{GetCurrentThread, SetThreadPriority};
                    use winapi::um::winbase::THREAD_PRIORITY_ABOVE_NORMAL;
                    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL as i32);
                }

                let mut scanner = Scanner::new().expect("failed to create scanner");
                let monitor = scanner.next().expect("no monitor found");
                let mut capturer: VecCapturer =
                    monitor.try_into().expect("failed to create capturer");

                let mut last_capture = Instant::now();
                let capture_interval = Duration::from_nanos(16_666_667); // 精确60fps

                // 预分配缓冲区以减少内存分配
                let mut frame_buffer = Vec::new();

                loop {
                    // 检查关闭信号
                    if shutdown_signal.load(Ordering::Relaxed) || shutdown_rx.try_recv().is_ok() {
                        break;
                    }

                    let now = Instant::now();
                    if now.duration_since(last_capture) < capture_interval {
                        // 精确的睡眠时间
                        let sleep_time = capture_interval - now.duration_since(last_capture);
                        if sleep_time > Duration::from_micros(100) {
                            thread::sleep(sleep_time - Duration::from_micros(50));
                        } else {
                            thread::yield_now();
                        }
                        continue;
                    }

                    match capturer.capture() {
                        Ok(info) if info.desktop_updated() => {
                            let desc = capturer.monitor().dxgi_outdupl_desc();
                            let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed);

                            // 复用缓冲区
                            frame_buffer.clear();
                            frame_buffer.extend_from_slice(&capturer.buffer);

                            let raw_frame = RawFrame {
                                width: desc.ModeDesc.Width,
                                height: desc.ModeDesc.Height,
                                data: Arc::new(frame_buffer.clone()),
                                timestamp: frame_id,
                                frame_id,
                            };

                            last_capture = now;

                            if tx.send(raw_frame).is_err() {
                                break; // 所有接收者都已关闭
                            }
                        }
                        Ok(_) => {
                            // 桌面未更新，短暂休眠
                            thread::sleep(Duration::from_millis(1));
                        }
                        Err(_) => {
                            // 捕获错误，短暂休眠后重试
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
            })?;

        self.capture_handle = Some(handle);
        self.capture_shutdown_tx = Some(shutdown_tx);

        // 启动编码工作线程
        self.start_encoding_worker().await;

        Ok(())
    }

    /// 启动编码工作线程（如果尚未启动）；内部会 spawn 一个任务
    async fn start_encoding_worker(&mut self) {
        if self.encoding_handle.is_some() {
            return;
        }

        let raw_rx = self.raw_frame_tx.subscribe();
        let encoders = self.encoders.clone();
        let encoded_streams = self.encoded_streams.clone();
        let track_writers = self.track_writers.clone();
        let shutdown_signal = self.shutdown_signal.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();

        let handle = thread::Builder::new()
            .name("video-encoder".to_string())
            .spawn(move || {
                Self::encoding_worker_thread(
                    raw_rx,
                    encoders,
                    encoded_streams,
                    track_writers,
                    shutdown_signal,
                    shutdown_rx,
                );
            })
            .expect("Failed to spawn encoding thread");

        self.encoding_handle = Some(handle);
        self.encoding_shutdown_tx = Some(shutdown_tx);
    }

    /// 独占线程的编码工作函数
    fn encoding_worker_thread(
        mut raw_rx: broadcast::Receiver<RawFrame>,
        encoders: Arc<Mutex<HashMap<String, QualityEncoder>>>,
        encoded_streams: Arc<RwLock<HashMap<String, broadcast::Sender<EncodedFrame>>>>,
        track_writers: Arc<RwLock<HashMap<String, Vec<Arc<TrackLocalStaticSample>>>>>,
        shutdown_signal: Arc<AtomicBool>,
        mut shutdown_rx: mpsc::UnboundedReceiver<()>,
    ) {
        // 设置线程优先级
        #[cfg(target_os = "windows")]
        unsafe {
            use winapi::um::processthreadsapi::{GetCurrentThread, SetThreadPriority};
            use winapi::um::winbase::THREAD_PRIORITY_ABOVE_NORMAL;
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL as i32);
        }

        // 创建一个轻量级的运行时用于锁操作
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        loop {
            // 检查关闭信号
            if shutdown_signal.load(Ordering::Relaxed) || shutdown_rx.try_recv().is_ok() {
                break;
            }

            match raw_rx.try_recv() {
                Ok(raw_frame) => {
                    rt.block_on(async {
                        // 快速获取编码器快照
                        let mut encoder_guard = encoders.lock().await;
                        let mut encoding_tasks = Vec::new();

                        // 对每个质量进行编码
                        for (quality_name, encoder) in encoder_guard.iter_mut() {
                            match encoder.encode(&raw_frame) {
                                Ok(Some(encoded_frame)) => {
                                    encoding_tasks.push((quality_name.clone(), encoded_frame));
                                }
                                Ok(None) => {
                                    // 跳帧，正常情况
                                }
                                Err(e) => {
                                    eprintln!("Encoding error for {}: {}", quality_name, e);
                                }
                            }
                        }

                        drop(encoder_guard);

                        // 只分发到广播通道，WebRTC轨道由独立的tokio任务处理
                        if !encoding_tasks.is_empty() {
                            let streams = encoded_streams.read().await;

                            for (quality_name, encoded_frame) in encoding_tasks {
                                // 发送到广播通道，WebRTC任务会从这里接收
                                if let Some(tx) = streams.get(&quality_name) {
                                    let _ = tx.send(encoded_frame);
                                }
                            }
                        }
                    });
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    // 没有新帧，短暂休眠
                    thread::sleep(Duration::from_millis(1));
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // 处理滞后，跳过旧帧
                    continue;
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    break; // 发送者关闭
                }
            }
        }
    }

    /// 添加一个新的质量流，返回可订阅的 EncodedFrame 接收器
    pub async fn add_quality_stream(
        &self,
        config: QualityConfig,
    ) -> Result<broadcast::Receiver<EncodedFrame>, Box<dyn std::error::Error + Send + Sync>> {
        config.validate()?;

        let quality_name = config.name.clone();

        // 检查是否已存在
        {
            let streams = self.encoded_streams.read().await;
            if let Some(tx) = streams.get(&quality_name) {
                return Ok(tx.subscribe());
            }
        }

        // 创建编码器
        let encoder = QualityEncoder::new(config)?;

        // 创建广播通道
        let (tx, rx) = broadcast::channel(16);

        // 添加到管理器
        {
            let mut encoders = self.encoders.lock().await;
            let mut streams = self.encoded_streams.write().await;

            encoders.insert(quality_name.clone(), encoder);
            streams.insert(quality_name.clone(), tx);
        }

        // 初始化轨道列表（为了兼容性保留）
        {
            let mut tracks = self.track_writers.write().await;
            tracks.insert(quality_name, Vec::new());
        }

        Ok(rx)
    }

    /// 为指定质量流添加一个 WebRTC 轨道，该轨道会消费对应质量的 EncodedFrame
    pub async fn add_webrtc_track(
        &self,
        quality_name: &str,
        track: Arc<TrackLocalStaticSample>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 获取编码流
        let mut encoded_rx = {
            let streams = self.encoded_streams.read().await;
            let tx = streams
                .get(quality_name)
                .ok_or("quality stream not found")?
                .clone();
            tx.subscribe()
        };

        // 添加到轨道管理器（为了统计和管理目的）
        {
            let mut track_writers = self.track_writers.write().await;
            track_writers
                .entry(quality_name.to_string())
                .or_insert_with(Vec::new)
                .push(track.clone());
        }

        // 启动WebRTC写入任务
        let shutdown_signal = self.shutdown_signal.clone();
        let quality_name = quality_name.to_string();
        let this_shutdown_signal = Arc::new(AtomicBool::new(false));
        let mut hash_gaurd = self.track_shutdown_tx.lock().await;
        if hash_gaurd.contains_key(&quality_name) {
            return Ok(());
        }
        hash_gaurd.insert(quality_name.clone(), this_shutdown_signal.clone());
        drop(hash_gaurd);
        tokio::spawn(async move {
            while !(shutdown_signal.load(Ordering::Relaxed)
                || this_shutdown_signal.load(Ordering::Relaxed))
            {
                match tokio::time::timeout(Duration::from_millis(100), encoded_rx.recv()).await {
                    Ok(Ok(encoded_frame)) => {
                        let sample = Sample {
                            data: encoded_frame.data,
                            duration: Duration::from_millis(33), // 根据实际帧率调整
                            ..Default::default()
                        };

                        if let Err(e) = track.write_sample(&sample).await {
                            eprintln!("Failed to write sample for {}: {}", quality_name, e);
                            break;
                        }
                    }
                    Ok(Err(_)) => {
                        // 发送者关闭
                        break;
                    }
                    Err(_) => {
                        // 超时，继续等待
                        continue;
                    }
                }
            }
        });

        Ok(())
    }

    /// 关闭指定的Track写入
    pub async fn close_track_write(&mut self, quality_name: &str) {
        let mut hash_gaurd = self.track_shutdown_tx.lock().await;
        if let Some(track_shutdown) = hash_gaurd.remove(quality_name) {
            track_shutdown.store(true, Ordering::Relaxed);
            self.remove_quality_stream(quality_name).await;
            println!("[ASSEMBLY]成功关闭写入 {:?}", quality_name)
        } else {
            println!("[ASSEMBLY]失败关闭写入 {:?}", quality_name)
        };
    }
    /// 移除指定的质量流及其相关资源
    pub async fn remove_quality_stream(&self, quality_name: &str) {
        let mut encoders = self.encoders.lock().await;
        let mut streams = self.encoded_streams.write().await;
        let mut track_writers = self.track_writers.write().await;

        encoders.remove(quality_name);
        streams.remove(quality_name);
        track_writers.remove(quality_name);

        // 注意：相关的WebRTC写入任务会在接收到关闭的广播通道时自动结束
    }
    /// 获取活跃的质量配置
    pub async fn get_active_qualities(&self) -> Vec<String> {
        let encoders = self.encoders.lock().await;
        encoders.keys().cloned().collect()
    }

    /// 关闭管理器
    pub async fn shutdown(&mut self) {
        self.shutdown_signal.store(true, Ordering::Relaxed);

        // 关闭捕获线程
        if let Some(tx) = self.capture_shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.capture_handle.take() {
            let _ = handle.join();
        }

        // 关闭编码线程
        if let Some(tx) = self.encoding_shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.encoding_handle.take() {
            let _ = handle.join();
        }

        // 清理资源
        {
            let mut encoders = self.encoders.lock().await;
            encoders.clear();
        }
        {
            let mut streams = self.encoded_streams.write().await;
            streams.clear();
        }
        {
            let mut tracks = self.track_writers.write().await;
            tracks.clear();
        }
    }
}

impl Drop for MultiStreamManager {
    fn drop(&mut self) {
        self.shutdown_signal.store(true, Ordering::Relaxed);
    }
}

// ==================== 使用示例 ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_manager_lifecycle() {
        let mut manager = MultiStreamManager::new();

        // 启动捕获
        manager.start_capture().await.unwrap();

        // 添加质量流
        let config = QualityConfig::new("720p", 1280, 720, 2_000_000, 30);
        let _rx = manager.add_quality_stream(config).await.unwrap();

        // 等待一段时间
        tokio::time::sleep(Duration::from_secs(1)).await;

        // 关闭
        manager.shutdown().await;
    }
}
