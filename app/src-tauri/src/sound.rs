//! 会话提示音：macOS 系统铃声（NSSound）+ 内置 CC0 WAV 兜底的即发即忘播放
//!
//! 播放不依赖任何前端窗口；WAV 输出流懒初始化并常驻（菜单栏应用生命周期内复用），
//! 每次播放新建 Sink 存入注册表并顺带清理已结束项。任何失败只打日志、静默跳过，
//! 绝不影响录音与识别主流程。

use crate::settings::Settings;
use crate::AppState;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use std::sync::OnceLock;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, Runtime};

/// macOS 系统铃声集合（/System/Library/Sounds 标准音，NSSound(named:) 运行时引用，不随应用分发）。
/// 设置值恒为该集合内的音名或 `none`（不发声，见 settings.rs 保留字）。
pub const SYSTEM_SOUNDS: &[&str] = &[
    "Basso", "Blow", "Bottle", "Frog", "Funk", "Glass", "Hero", "Morse", "Ping", "Pop", "Purr", "Sosumi", "Submarine", "Tink",
];

/// 各事件默认音效（macOS 系统音名）；失败默认不发声——与英文原版 cometix-dictation
/// 对齐（仅视觉提示），需要时可在面板手动选择失败音。完成不设独立事件：识别很快，
/// 处理音即完成信号（review #1 移除「识别完成」行）。
/// 开始音默认按平台取：macOS 沿用 Tink（cometix-dictation 对齐）；非 macOS 无系统音，
/// 默认 Glass——其兜底玻璃音的音色即用户选定音，名称与实际发声一致（用户反馈）
#[cfg(target_os = "macos")]
pub const DEFAULT_START: &str = "Tink";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_START: &str = "Glass";
pub const DEFAULT_PROCESSING: &str = "Pop";
pub const DEFAULT_ERROR: &str = "none";

/// 非 macOS 兜底：选中的系统音若有同名内置近似 WAV（resources/sounds/，CC0）则以其发声。
/// Glass 的兜底按用户试听选定 glass_005（玻璃轻碰）；原 Tink 近似音音色不符已移除，
/// 「选 Tink 在 Windows 静音」的已知限制随之消失——不可发声的项不再出现在列表里
fn wav_fallback_id(name: &str) -> Option<&'static str> {
    match name {
        "Glass" => Some("glass"),
        "Pop" => Some("pop"),
        "Basso" => Some("error"),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundEvent {
    Start,
    Processing,
    Error,
}

/// 事件对应的逐事件音效（macOS 系统音名或 `none`；不考虑总开关）
pub fn event_id<'a>(event: SoundEvent, s: &'a Settings) -> &'a str {
    match event {
        SoundEvent::Start => &s.sound_start,
        SoundEvent::Processing => &s.sound_processing,
        SoundEvent::Error => &s.sound_error,
    }
}

/// 事件→音效的取值门控：总开关关闭或该事件选「不发声」时返回 None；
/// 选中的音效名必须在系统铃声集合中（sanitize 保证，此处防御性再查）
pub fn gated_id<'a>(event: SoundEvent, s: &'a Settings) -> Option<&'a str> {
    if !s.sound_enabled {
        return None;
    }
    let id = event_id(event, s);
    if id == crate::settings::NONE {
        return None;
    }
    SYSTEM_SOUNDS.contains(&id).then_some(id)
}

/// 音效名是否合法（系统铃声集合或 `none`）
pub fn is_known(id: &str) -> bool {
    id == crate::settings::NONE || SYSTEM_SOUNDS.contains(&id)
}

/// 设置页可选择（当前平台能发声）的音效集合，不含 `none`（由调用方单独提供）。
/// macOS 提供全部系统铃声；非 macOS 仅有同名兜底 WAV 的项——其余选择不会发声，
/// 不应出现在下拉里（review #3）。sanitize 亦按此集合校验，历史配置自动回退默认。
pub fn selectable_sounds() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        SYSTEM_SOUNDS
    }
    #[cfg(not(target_os = "macos"))]
    {
        // 从全集按兜底映射过滤；集合恒定，leak 一次性固化
        SYSTEM_SOUNDS
            .iter()
            .copied()
            .filter(|id| wav_fallback_id(id).is_some())
            .collect::<Vec<_>>()
            .leak()
    }
}

/// 会话事件播放：遵循总开关与逐事件选择。macOS 用 NSSound(named:) 主线程直放；
/// Windows 无系统音，按同名内置近似 WAV 兜底（无同名近似音则静音，已知平台限制）。
pub fn play<R: Runtime>(app: &AppHandle<R>, event: SoundEvent) {
    let st = app.state::<AppState>();
    let s = st.settings.read().unwrap();
    if !s.sound_enabled {
        return;
    }
    let id = gated_id(event, &s).map(str::to_string);
    drop(s);
    let Some(id) = id else { return };
    #[cfg(target_os = "macos")]
    play_system_quietly(app, &id, wav_fallback_id(&id).map(str::to_string));
    #[cfg(not(target_os = "macos"))]
    if let Some(wav) = wav_fallback_id(&id) {
        play_quietly(app, wav);
    }
}

/// 试听播放：不受总开关约束；未知音名静默忽略（试听即所得，播选中的系统铃声）
pub fn play_id<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if !is_known(id) || id == crate::settings::NONE {
        return;
    }
    #[cfg(target_os = "macos")]
    play_system_quietly(app, id, wav_fallback_id(id).map(str::to_string));
    #[cfg(not(target_os = "macos"))]
    if let Some(wav) = wav_fallback_id(id) {
        play_quietly(app, wav);
    }
}

/// macOS 系统音：投递到主线程用 NSSound 播放（cometix-dictation 同策略）；系统音不可用时
/// 在主线程回退内置近似音。投递失败（应用退出中）直接走兜底，不影响主流程。
#[cfg(target_os = "macos")]
fn play_system_quietly<R: Runtime>(app: &AppHandle<R>, name: &str, fallback_id: Option<String>) {
    let inner_app = app.clone();
    let name = name.to_string();
    let inner_fallback = fallback_id.clone();
    let posted = app.run_on_main_thread(move || {
        if !system::play_named(&name) {
            if let Some(id) = inner_fallback {
                play_quietly(&inner_app, &id);
            }
        }
    });
    if posted.is_err() {
        if let Some(id) = fallback_id {
            play_quietly(app, &id);
        }
    }
}

#[cfg(target_os = "macos")]
mod system {
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSString;
    use std::cell::RefCell;
    use std::collections::HashMap;

    // NSSound 播放期间必须保持对象存活，否则会被提前析构、播放中途被切断；
    // 按音名缓存实例（仅主线程访问，thread_local 无 Send 约束）。重复播放=干净地从头重播。
    thread_local! {
        static PLAYING: RefCell<HashMap<String, objc2::rc::Retained<NSSound>>> = RefCell::new(HashMap::new());
    }

    /// 主线程调用：播放系统命名提示音；返回是否成功发声。
    /// 播放中重复触发时先 stop 再 play，强制从头重播（连点试听立即打断上一遍）
    pub fn play_named(name: &str) -> bool {
        let Some(sound) = NSSound::soundNamed(&NSString::from_str(name)) else {
            return false;
        };
        sound.stop();
        sound.play();
        PLAYING.with(|cache| cache.borrow_mut().insert(name.to_string(), sound));
        true
    }
}

/// 非 macOS 平台没有系统提示音路径：系统铃声不可用，WAV 兜底由调用方的
/// `fallback_id` 分支承担；空实现仅保证跨平台编译。
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn play_system_quietly<R: Runtime>(_: &AppHandle<R>, _: &str, _: Option<String>) {}

fn play_quietly<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let path = match sound_path(app, id) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("提示音播放失败（已跳过）：{e:#}");
            return;
        }
    };
    match audio_tx() {
        Some(tx) => {
            let _ = tx.send(path);
        }
        None => eprintln!("提示音播放失败（已跳过）：音频线程不可用"),
    }
}

fn sound_path<R: Runtime>(app: &AppHandle<R>, id: &str) -> anyhow::Result<PathBuf> {
    Ok(app.path().resolve(format!("resources/sounds/{id}.wav"), BaseDirectory::Resource)?)
}

/// 单个音效 id 的发声体时长（不含投递余量）：已知系统音用固定值，
/// 其余优先读同名兜底 WAV 头实测；无任何数据的系统音按 250ms 保守估计——
/// 系统音文件混响尾音远长于可听主体（如 Tink 全长 564ms、可听主体约 60ms），
/// 不能按文件全长延迟压音量，但也不能为 0（否则开始音未播完就被压掉）。
/// 「不发声」与非法 id 为 0。
fn estimate_duration(id: &str, fallback_wav: Option<Duration>) -> Duration {
    const UNKNOWN_SYSTEM_SOUND: Duration = Duration::from_millis(250);
    match id {
        _ if id == crate::settings::NONE => Duration::ZERO,
        "Tink" => Duration::from_millis(60),
        "Pop" => Duration::from_millis(75),
        _ => fallback_wav
            .or_else(|| SYSTEM_SOUNDS.contains(&id).then_some(UNKNOWN_SYSTEM_SOUND))
            .unwrap_or_default(),
    }
}

/// 事件提示音的大致时长：「先播音、后压音量」的延迟依据。
/// 不发声（总开关关/「不发声」/完成事件）为 0。附投递余量：主线程队列与音量生效各需几毫秒。
pub fn play_duration<R: Runtime>(app: &AppHandle<R>, event: SoundEvent, s: &Settings) -> Duration {
    const MARGIN: Duration = Duration::from_millis(40);
    if !s.sound_enabled {
        return Duration::ZERO;
    }
    let id = event_id(event, s);
    let body = estimate_duration(id, wav_fallback_id(id).and_then(|wid| pool_wav_duration(app, wid)));
    if body.is_zero() {
        Duration::ZERO
    } else {
        body + MARGIN
    }
}

/// 解析池内 WAV 的时长：标准 PCM 头 byte_rate@28..32、data 大小@40..44；解析失败返回 None
fn pool_wav_duration<R: Runtime>(app: &AppHandle<R>, id: &str) -> Option<Duration> {
    let path = sound_path(app, id).ok()?;
    let bytes = std::fs::read(path).ok()?;
    wav_duration_bytes(&bytes)
}

/// 解析 PCM WAV 时长：从偏移 12 遍历 RIFF 块链（fourcc + LE u32 size，偶对齐步进），
/// 取 `fmt ` 的 byte_rate 与 `data` 的大小；兼容 ffmpeg 写出的 LIST/INFO 元数据块。
/// 非 RIFF/WAVE、缺块或字节率为 0 时返回 None（调用方按 0 时长立即压音量）。
fn wav_duration_bytes(b: &[u8]) -> Option<Duration> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let mut off = 12usize;
    let mut byte_rate = None;
    let mut data_len = None;
    while off + 8 <= b.len() {
        let fourcc = &b[off..off + 4];
        let size = u32::from_le_bytes(b[off + 4..off + 8].try_into().ok()?) as usize;
        let body = off + 8;
        match fourcc {
            // fmt：wFormatTag(2) nChannels(2) nSamplesPerSec(4) nAvgBytesPerSec(4)…
            b"fmt " if body + 16 <= b.len() => {
                byte_rate = Some(u32::from_le_bytes(b[body + 8..body + 12].try_into().ok()?));
            }
            b"data" => {
                data_len = Some(size.min(b.len().saturating_sub(body)) as u64);
            }
            _ => {}
        }
        if byte_rate.is_some() && data_len.is_some() {
            break;
        }
        off = body.saturating_add(size + (size & 1));
    }
    let byte_rate = byte_rate? as u64;
    if byte_rate == 0 {
        return None;
    }
    Some(Duration::from_millis(data_len?.saturating_mul(1000) / byte_rate))
}

/// 音频工作线程的请求通道；线程首次播放请求时懒启动
static AUDIO_TX: OnceLock<Option<mpsc::Sender<PathBuf>>> = OnceLock::new();

fn audio_tx() -> Option<mpsc::Sender<PathBuf>> {
    if let Some(tx) = AUDIO_TX.get() {
        return tx.clone();
    }
    let (tx, rx) = mpsc::channel();
    let ok = std::thread::Builder::new()
        .name("voicekey-sound".into())
        .spawn(move || audio_loop(rx))
        .is_ok();
    let result = ok.then_some(tx);
    if AUDIO_TX.set(result.clone()).is_err() {
        // 并发首播：另一线程已登记通道；弃用本次新建（其发送端无人持有，线程会自行退出），
        // 改用已登记的通道，避免把请求发进即将断开的管道
        return AUDIO_TX.get().cloned().flatten();
    }
    result
}

/// 常驻音频线程：OutputStream/Sink 全部在本线程内创建与持有（OutputStream 不满足 Send，
/// 不能跨线程移动）。设备打开失败时按请求重试，稍后设备就绪即可恢复出声。
fn audio_loop(rx: mpsc::Receiver<PathBuf>) {
    struct Output {
        _stream: OutputStream,
        handle: OutputStreamHandle,
        sinks: Vec<Sink>,
    }
    fn open() -> anyhow::Result<Output> {
        let (stream, handle) = OutputStream::try_default().map_err(|_| anyhow::anyhow!("无法打开系统音频输出设备"))?;
        Ok(Output { _stream: stream, handle, sinks: Vec::new() })
    }
    fn play_once(out: &mut Output, path: &PathBuf) -> anyhow::Result<()> {
        let sink = Sink::try_new(&out.handle)?;
        sink.append(Decoder::new_wav(BufReader::new(std::fs::File::open(path)?))?);
        out.sinks.retain(|s| !s.empty());
        out.sinks.push(sink);
        Ok(())
    }

    let mut output: Option<Output> = None;
    while let Ok(path) = rx.recv() {
        if output.is_none() {
            output = open().map_err(|e| eprintln!("提示音播放失败（已跳过）：{e:#}")).ok();
        }
        if let Some(out) = output.as_mut() {
            if let Err(e) = play_once(out, &path) {
                eprintln!("提示音播放失败（已跳过）：{e:#}");
                // 设备可能已失效（拔出/切换）：置空以便下次请求时重新打开
                output = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_sound_list_is_standard_and_unique() {
        assert!(SYSTEM_SOUNDS.len() >= 14, "标准系统铃声应完整收录");
        let mut seen = std::collections::HashSet::new();
        for id in SYSTEM_SOUNDS {
            assert!(!id.is_empty() && *id != crate::settings::NONE, "非法音名：{id}");
            assert!(seen.insert(*id), "重复音名：{id}");
        }
        // 发声默认值都在集合中；失败默认「不发声」
        for id in [DEFAULT_START, DEFAULT_PROCESSING] {
            assert!(SYSTEM_SOUNDS.contains(&id), "默认音效 {id} 必须在系统铃声集合中");
        }
        assert_eq!(DEFAULT_ERROR, crate::settings::NONE, "失败默认不发声");
    }

    #[test]
    fn is_known_accepts_system_sounds_and_none_only() {
        assert!(is_known("Basso"));
        assert!(is_known("Tink"));
        assert!(is_known(crate::settings::NONE));
        assert!(!is_known(""));
        assert!(!is_known("nonexistent-sound"));
        assert!(!is_known("switch"), "历史池内 id 不再属于合法值");
    }

    #[test]
    fn wav_fallback_covers_default_sounding_events() {
        assert_eq!(wav_fallback_id("Glass"), Some("glass"));
        assert_eq!(wav_fallback_id("Pop"), Some("pop"));
        assert_eq!(wav_fallback_id("Basso"), Some("error"));
        assert_eq!(wav_fallback_id("Tink"), None, "Tink 无兜底近似音，非 macOS 不可选（名称须与实际音色一致）");
    }

    /// 可选集合必须保证「列出即可发声」：每一项都有发声路径（review #3）
    #[test]
    fn selectable_sounds_all_have_playback_path() {
        for id in selectable_sounds() {
            assert!(is_known(id), "可选集合必须是合法音名：{id}");
            #[cfg(not(target_os = "macos"))]
            assert!(wav_fallback_id(id).is_some(), "非 macOS 可选项必须有兜底 WAV：{id}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn selectable_sounds_cover_all_system_sounds_on_macos() {
        assert_eq!(selectable_sounds(), SYSTEM_SOUNDS);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn selectable_sounds_limited_to_fallbackable_on_non_macos() {
        assert_eq!(selectable_sounds().len(), 3, "非 macOS 只应有 Basso/Glass/Pop 三项");
        assert!(selectable_sounds().contains(&"Basso"));
        assert!(selectable_sounds().contains(&"Glass"));
        assert!(selectable_sounds().contains(&"Pop"));
        assert!(!selectable_sounds().contains(&"Tink"), "无兜底音不得出现在非 macOS 列表");
    }

    /// 压音量延迟依据：无兜底 WAV 的系统音不得估为 0，否则开始音未播完就被压掉
    #[test]
    fn duration_estimate_covers_all_system_sounds() {
        assert_eq!(estimate_duration("Tink", None), Duration::from_millis(60));
        assert_eq!(estimate_duration("Pop", None), Duration::from_millis(75));
        // 有同名兜底 WAV 时优先实测值
        assert_eq!(estimate_duration("Basso", Some(Duration::from_millis(120))), Duration::from_millis(120));
        // 无兜底 WAV 的系统音（如 Glass）按保守估计，不得为 0
        for id in SYSTEM_SOUNDS {
            let d = estimate_duration(id, None);
            assert!(d > Duration::ZERO, "系统音 {id} 的发声体估计不得为 0");
        }
        assert_eq!(estimate_duration(crate::settings::NONE, None), Duration::ZERO);
        assert_eq!(estimate_duration("nonexistent", None), Duration::ZERO);
    }

    #[test]
    fn master_switch_off_gates_all_events() {
        let mut s = Settings::default();
        s.sound_enabled = false;
        for event in [SoundEvent::Start, SoundEvent::Processing, SoundEvent::Error] {
            assert_eq!(gated_id(event, &s), None, "总开关关闭时 {event:?} 不应发声");
        }
    }

    #[test]
    fn master_switch_on_maps_each_event_to_its_own_setting() {
        let mut s = Settings::default();
        s.sound_enabled = true;
        // 改一个事件的选择只影响该事件
        let error_before = event_id(SoundEvent::Error, &s).to_string();
        s.sound_processing = error_before.clone();
        assert_eq!(event_id(SoundEvent::Processing, &s), error_before);
        s.sound_processing = String::new();
        assert_eq!(event_id(SoundEvent::Error, &s), error_before, "其他事件的选择不受影响");
    }

    #[test]
    fn defaults_follow_per_event_system_sounds() {
        let s = Settings::default();
        #[cfg(target_os = "macos")]
        assert_eq!(event_id(SoundEvent::Start, &s), "Tink");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(event_id(SoundEvent::Start, &s), "Glass", "非 macOS 默认开始音=Glass（兜底音色即其所指）");
        assert_eq!(event_id(SoundEvent::Processing, &s), "Pop");
        assert_eq!(event_id(SoundEvent::Error, &s), crate::settings::NONE, "失败默认不发声");
        let mut s = Settings::default();
        s.sound_enabled = true;
        #[cfg(target_os = "macos")]
        assert_eq!(gated_id(SoundEvent::Start, &s), Some("Tink"));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(gated_id(SoundEvent::Start, &s), Some("Glass"));
        assert_eq!(gated_id(SoundEvent::Processing, &s), Some("Pop"));
        assert_eq!(gated_id(SoundEvent::Error, &s), None, "失败默认不发声");
    }

    #[test]
    fn gating_never_touches_per_event_fields() {
        let mut s = Settings::default();
        s.sound_enabled = true;
        s.sound_start = "Glass".into();
        s.sound_processing = "Funk".into();
        s.sound_error = "Sosumi".into();
        let before = (s.sound_start.clone(), s.sound_processing.clone(), s.sound_error.clone());
        for event in [SoundEvent::Start, SoundEvent::Processing, SoundEvent::Error] {
            let _ = gated_id(event, &s);
        }
        assert_eq!(
            (s.sound_start.clone(), s.sound_processing.clone(), s.sound_error.clone()),
            before,
            "门控必须只读，不改写逐事件选择"
        );
    }

    /// RIFF 块链解析：标准头、ffmpeg 实际布局（fmt → LIST 元数据 → data）、坏头
    #[test]
    fn wav_duration_parses_riff_chunk_chain() {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(b"WAVE");
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0]);
        b.extend_from_slice(&44_100u32.to_le_bytes());
        b.extend_from_slice(&88_200u32.to_le_bytes());
        b.extend_from_slice(&[2, 0, 16, 0]);
        b.extend_from_slice(b"LIST");
        b.extend_from_slice(&26u32.to_le_bytes());
        b.extend_from_slice(&vec![0u8; 26]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&8_820u32.to_le_bytes());
        b.extend_from_slice(&vec![0u8; 8_820]);
        assert_eq!(wav_duration_bytes(&b), Some(Duration::from_millis(100)));
        assert_eq!(wav_duration_bytes(&[]), None);
        let mut notwav = b.clone();
        notwav[0..4].copy_from_slice(b"RIFX");
        assert_eq!(wav_duration_bytes(&notwav), None);
    }

    /// 回归：固定偏移解析曾对真实池内文件（含 LIST 元数据块）返回 0ms。
    /// 真实文件必须解析出合理时长，且不为 0。
    #[test]
    fn wav_duration_matches_real_pool_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/sounds");
        for (id, expected_ms) in [("glass", 44), ("pop", 72), ("error", 130)] {
            let bytes = std::fs::read(dir.join(format!("{id}.wav"))).expect("池内文件存在");
            let d = wav_duration_bytes(&bytes).expect("真实 WAV 应能解析出时长");
            let got = d.as_millis() as i64;
            assert!(got > 0, "{id} 时长不应为 0（固定偏移回归）");
            assert!(
                (got - expected_ms).abs() <= 10,
                "{id} 时长 {got}ms 与 ffprobe 基准 {expected_ms}ms 偏差过大"
            );
        }
    }
}
