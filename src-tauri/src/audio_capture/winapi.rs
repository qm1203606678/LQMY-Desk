use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};
use webrtc::media::Sample;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use windows::core::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

// 音频配置
const SAMPLE_RATE: u32 = 48000;
const CHANNELS: u16 = 2;
const BITS_PER_SAMPLE: u16 = 16;
const BUFFER_SIZE: usize = 1920; // 40ms at 48kHz

#[derive(Clone)]
pub struct AudioSample {
    pub data: Vec<i16>,
    pub timestamp: u64,
}

pub struct AudioMixer {
    microphone_volume: f32,
    system_volume: f32,
}

impl AudioMixer {
    pub fn new() -> Self {
        Self {
            microphone_volume: 1.0,
            system_volume: 1.0,
        }
    }

    pub fn mix_audio(&self, mic_data: &[i16], system_data: &[i16]) -> Vec<i16> {
        let len = mic_data.len().max(system_data.len());
        let mut mixed = Vec::with_capacity(len);

        for i in 0..len {
            let mic_sample = if i < mic_data.len() {
                (mic_data[i] as f32 * self.microphone_volume) as i32
            } else {
                0
            };

            let system_sample = if i < system_data.len() {
                (system_data[i] as f32 * self.system_volume) as i32
            } else {
                0
            };

            // 混音并防止溢出
            let mixed_sample = (mic_sample + system_sample).clamp(-32768, 32767);
            mixed.push(mixed_sample as i16);
        }

        mixed
    }

    pub fn set_microphone_volume(&mut self, volume: f32) {
        self.microphone_volume = volume.clamp(0.0, 2.0);
    }

    pub fn set_system_volume(&mut self, volume: f32) {
        self.system_volume = volume.clamp(0.0, 2.0);
    }
}

pub struct WindowsAudioCapture {
    mic_client: Option<IAudioClient>,
    system_client: Option<IAudioClient>,
    mic_render_client: Option<IAudioRenderClient>,
    system_render_client: Option<IAudioRenderClient>,
}

impl WindowsAudioCapture {
    pub fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)?;
        }

        Ok(Self {
            mic_client: None,
            system_client: None,
            mic_render_client: None,
            system_render_client: None,
        })
    }

    pub fn initialize_microphone(&mut self) -> Result<()> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;
            let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let mut wave_format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: CHANNELS,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * (CHANNELS as u32) * (BITS_PER_SAMPLE as u32) / 8,
                nBlockAlign: (CHANNELS * BITS_PER_SAMPLE) / 8,
                wBitsPerSample: BITS_PER_SAMPLE,
                cbSize: 0,
            };

            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                10000000, // 1 second buffer
                0,
                &wave_format,
                None,
            )?;

            let render_client: IAudioRenderClient = audio_client.GetService()?;

            self.mic_client = Some(audio_client);
            self.mic_render_client = Some(render_client);
        }

        Ok(())
    }

    pub fn initialize_system_audio(&mut self) -> Result<()> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

            let mut wave_format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: CHANNELS,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * (CHANNELS as u32) * (BITS_PER_SAMPLE as u32) / 8,
                nBlockAlign: (CHANNELS * BITS_PER_SAMPLE) / 8,
                wBitsPerSample: BITS_PER_SAMPLE,
                cbSize: 0,
            };

            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                10000000,
                0,
                &wave_format,
                None,
            )?;

            let render_client: IAudioRenderClient = audio_client.GetService()?;

            self.system_client = Some(audio_client);
            self.system_render_client = Some(render_client);
        }

        Ok(())
    }

    pub fn capture_microphone_audio(&self) -> Result<Vec<i16>> {
        // 实现麦克风音频捕获逻辑
        // 这里需要调用Windows Audio API获取麦克风数据
        Ok(vec![0; BUFFER_SIZE])
    }

    pub fn capture_system_audio(&self) -> Result<Vec<i16>> {
        // 实现系统音频捕获逻辑（扬声器输出）
        // 使用WASAPI loopback模式
        Ok(vec![0; BUFFER_SIZE])
    }
}

pub struct PeerAudioController {
    volume: Arc<Mutex<f32>>,
    audio_sender: mpsc::UnboundedSender<AudioSample>,
}

impl PeerAudioController {
    pub fn new(audio_sender: mpsc::UnboundedSender<AudioSample>) -> Self {
        Self {
            volume: Arc::new(Mutex::new(1.0)),
            audio_sender,
        }
    }

    pub fn set_volume(&self, volume: f32) {
        if let Ok(mut vol) = self.volume.lock() {
            *vol = volume.clamp(0.0, 2.0);
        }
    }

    pub fn get_volume(&self) -> f32 {
        self.volume
            .lock()
            .unwrap_or_else(|_| std::sync::MutexGuard::new(1.0.into()))
    }

    pub fn process_received_audio(&self, mut sample: AudioSample) {
        let volume = self.get_volume();
        for sample_data in &mut sample.data {
            *sample_data = (*sample_data as f32 * volume).clamp(-32768.0, 32767.0) as i16;
        }
        let _ = self.audio_sender.send(sample);
    }
}

pub struct WebRTCAudioSystem {
    mixer: Arc<Mutex<AudioMixer>>,
    audio_capture: Arc<Mutex<WindowsAudioCapture>>,
    peer_controllers: Arc<Mutex<HashMap<String, PeerAudioController>>>,
    mixed_audio_sender: broadcast::Sender<AudioSample>,
    rtc_tracks: Arc<Mutex<Vec<Arc<TrackLocalStaticSample>>>>,
}

impl WebRTCAudioSystem {
    pub fn new() -> Result<Self> {
        let audio_capture = WindowsAudioCapture::new()?;
        let (mixed_audio_sender, _) = broadcast::channel(100);

        Ok(Self {
            mixer: Arc::new(Mutex::new(AudioMixer::new())),
            audio_capture: Arc::new(Mutex::new(audio_capture)),
            peer_controllers: Arc::new(Mutex::new(HashMap::new())),
            mixed_audio_sender,
            rtc_tracks: Arc::new(Mutex::new(Vec::new())),
        })
    }

    pub async fn initialize(&self) -> Result<()> {
        let mut capture = self.audio_capture.lock().unwrap();
        capture.initialize_microphone()?;
        capture.initialize_system_audio()?;
        Ok(())
    }

    pub fn add_rtc_track(&self, track: Arc<TrackLocalStaticSample>) {
        if let Ok(mut tracks) = self.rtc_tracks.lock() {
            tracks.push(track);
        }
    }

    pub fn add_peer(&self, peer_id: String) -> mpsc::UnboundedReceiver<AudioSample> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let controller = PeerAudioController::new(sender);

        if let Ok(mut peers) = self.peer_controllers.lock() {
            peers.insert(peer_id, controller);
        }

        receiver
    }

    pub fn set_peer_volume(&self, peer_id: &str, volume: f32) {
        if let Ok(peers) = self.peer_controllers.lock() {
            if let Some(controller) = peers.get(peer_id) {
                controller.set_volume(volume);
            }
        }
    }

    pub fn set_microphone_volume(&self, volume: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.set_microphone_volume(volume);
        }
    }

    pub fn set_system_audio_volume(&self, volume: f32) {
        if let Ok(mut mixer) = self.mixer.lock() {
            mixer.set_system_volume(volume);
        }
    }

    // 音频捕获和处理的主循环，运行在独立线程
    pub async fn start_audio_thread(&self) {
        let mixer = Arc::clone(&self.mixer);
        let audio_capture = Arc::clone(&self.audio_capture);
        let mixed_audio_sender = self.mixed_audio_sender.clone();
        let rtc_tracks = Arc::clone(&self.rtc_tracks);

        tokio::spawn(async move {
            let mut timestamp = 0u64;

            loop {
                // 捕获麦克风音频
                let mic_data = if let Ok(capture) = audio_capture.lock() {
                    capture
                        .capture_microphone_audio()
                        .unwrap_or_else(|_| vec![0; BUFFER_SIZE])
                } else {
                    vec![0; BUFFER_SIZE]
                };

                // 捕获系统音频
                let system_data = if let Ok(capture) = audio_capture.lock() {
                    capture
                        .capture_system_audio()
                        .unwrap_or_else(|_| vec![0; BUFFER_SIZE])
                } else {
                    vec![0; BUFFER_SIZE]
                };

                // 混音
                let mixed_data = if let Ok(mixer_guard) = mixer.lock() {
                    mixer_guard.mix_audio(&mic_data, &system_data)
                } else {
                    mic_data
                };

                let audio_sample = AudioSample {
                    data: mixed_data.clone(),
                    timestamp,
                };

                // 发送到所有RTC连接
                if let Ok(tracks) = rtc_tracks.lock() {
                    for track in tracks.iter() {
                        let sample = Sample {
                            data: mixed_data.iter().flat_map(|&x| x.to_le_bytes()).collect(),
                            duration: std::time::Duration::from_millis(40), // 40ms per buffer
                            ..Default::default()
                        };

                        let _ = track.write_sample(&sample).await;
                    }
                }

                // 广播混音后的音频
                let _ = mixed_audio_sender.send(audio_sample);

                timestamp += 40; // 40ms increment
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
        });
    }

    // 处理接收到的对方音频
    pub fn handle_received_audio(&self, peer_id: &str, audio_data: Vec<i16>, timestamp: u64) {
        if let Ok(peers) = self.peer_controllers.lock() {
            if let Some(controller) = peers.get(peer_id) {
                let sample = AudioSample {
                    data: audio_data,
                    timestamp,
                };
                controller.process_received_audio(sample);
            }
        }
    }

    // 获取混音后的音频流（用于本地播放或其他用途）
    pub fn subscribe_mixed_audio(&self) -> broadcast::Receiver<AudioSample> {
        self.mixed_audio_sender.subscribe()
    }
}

// 使用示例
pub async fn start_microphone_audio_stream(audio_track: Arc<TrackLocalStaticSample>) -> Result<()> {
    let audio_system = WebRTCAudioSystem::new()?;

    // 初始化音频系统
    audio_system.initialize().await?;

    // 添加RTC轨道
    audio_system.add_rtc_track(audio_track);

    // 添加对方连接
    let peer_audio_receiver = audio_system.add_peer("peer_1".to_string());

    // 设置音量
    audio_system.set_microphone_volume(1.0);
    audio_system.set_system_audio_volume(0.8);
    audio_system.set_peer_volume("peer_1", 1.2);

    // 启动音频处理线程
    audio_system.start_audio_thread().await;

    // 处理接收到的对方音频
    tokio::spawn(async move {
        let mut receiver = peer_audio_receiver;
        while let Some(audio_sample) = receiver.recv().await {
            // 这里可以播放接收到的音频
            println!(
                "Received audio sample with {} samples",
                audio_sample.data.len()
            );
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_mixer() {
        let mut mixer = AudioMixer::new();
        let mic_data = vec![1000, 2000, 3000];
        let system_data = vec![500, 1000, 1500];

        let mixed = mixer.mix_audio(&mic_data, &system_data);
        assert_eq!(mixed, vec![1500, 3000, 4500]);

        mixer.set_microphone_volume(0.5);
        mixer.set_system_volume(2.0);
        let mixed = mixer.mix_audio(&mic_data, &system_data);
        assert_eq!(mixed, vec![1500, 3000, 4500]);
    }
}
