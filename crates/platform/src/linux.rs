//! Linux 平台层（X11 / Wayland 通用）
//!
//! - 全局按键：读取 `/dev/input/event*`（evdev），不独占设备，因此**无法吞键**；
//!   与 macOS 一样，钩子只观察不拦截。需要 `input` 组或相应 udev 权限。
//! - 注入上屏：`/dev/uinput` 虚拟键盘发出按键；中文等任意 Unicode 走剪贴板
//!   （`wl-copy` / `xclip`）+ Ctrl+V。需要 `/dev/uinput` 可写。
//! - 光标位置、前台应用：Wayland 无全局查询 API，恒返回 None；候选面板落在默认位置。
//! - 中文实时上屏（边说边上屏）无法用 uinput 逐字打出，应用层在 Linux 下关闭该功能，
//!   改为结束时一次性粘贴。

use crate::{HookFn, KeyEvent, KeyKind, MicStatus, Rect, ALT, CTRL, META, SHIFT};
use evdev::{Device, EventType, InputEvent, KeyCode};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// uinput 虚拟设备名；扫描输入设备时按此跳过，避免自读自写死循环
const VIRT_NAME: &str = "voicekey-virtual";

// 平台原生键码（linux/input-event-codes.h），对上层与 macOS/Windows 键码一样不透明
const K_ESC: u32 = KeyCode::KEY_ESC.0 as u32;
const K_ENTER: u32 = KeyCode::KEY_ENTER.0 as u32;
const K_TAB: u32 = KeyCode::KEY_TAB.0 as u32;
const K_BACKSPACE: u32 = KeyCode::KEY_BACKSPACE.0 as u32;
const K_DELETE: u32 = KeyCode::KEY_DELETE.0 as u32;
const K_SPACE: u32 = KeyCode::KEY_SPACE.0 as u32;
const K_HOME: u32 = KeyCode::KEY_HOME.0 as u32;
const K_END: u32 = KeyCode::KEY_END.0 as u32;
const K_PAGEUP: u32 = KeyCode::KEY_PAGEUP.0 as u32;
const K_PAGEDOWN: u32 = KeyCode::KEY_PAGEDOWN.0 as u32;
const K_LEFT: u32 = KeyCode::KEY_LEFT.0 as u32;
const K_RIGHT: u32 = KeyCode::KEY_RIGHT.0 as u32;
const K_UP: u32 = KeyCode::KEY_UP.0 as u32;
const K_DOWN: u32 = KeyCode::KEY_DOWN.0 as u32;
const K_LEFTCTRL: u32 = KeyCode::KEY_LEFTCTRL.0 as u32;
const K_RIGHTCTRL: u32 = KeyCode::KEY_RIGHTCTRL.0 as u32;
const K_LEFTSHIFT: u32 = KeyCode::KEY_LEFTSHIFT.0 as u32;
const K_RIGHTSHIFT: u32 = KeyCode::KEY_RIGHTSHIFT.0 as u32;
const K_LEFTALT: u32 = KeyCode::KEY_LEFTALT.0 as u32;
const K_RIGHTALT: u32 = KeyCode::KEY_RIGHTALT.0 as u32;
const K_LEFTMETA: u32 = KeyCode::KEY_LEFTMETA.0 as u32;
const K_RIGHTMETA: u32 = KeyCode::KEY_RIGHTMETA.0 as u32;
/// 数字键 1–9 在 evdev 里是 2–10
const K_DIGIT_LO: u32 = 2;
const K_DIGIT_HI: u32 = 10;

static HOOK: Mutex<Option<HookFn>> = Mutex::new(None);
/// 当前按下的键（仅记录 < 256 的码，足够覆盖修饰键与常用键）
static DOWN: Mutex<[bool; 256]> = Mutex::new([false; 256]);

fn mods(down: &[bool; 256]) -> u8 {
    let mut m = 0;
    for (codes, b) in [([K_LEFTCTRL, K_RIGHTCTRL], CTRL), ([K_LEFTALT, K_RIGHTALT], ALT), ([K_LEFTSHIFT, K_RIGHTSHIFT], SHIFT), ([K_LEFTMETA, K_RIGHTMETA], META)] {
        if codes.iter().any(|&c| down[c as usize]) {
            m |= b;
        }
    }
    m
}

// MARK: - 全局按键（evdev）

/// 安装全局键盘钩子：后台线程扫描键盘设备，每个设备一个读取线程
pub fn start_hook(f: HookFn) {
    *HOOK.lock().unwrap() = Some(f);
    thread::spawn(|| {
        let opened: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        loop {
            if let Ok(entries) = std::fs::read_dir("/dev/input") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("event")) {
                        continue;
                    }
                    if opened.lock().unwrap().contains(&path) {
                        continue;
                    }
                    if let Some(dev) = open_keyboard(&path) {
                        opened.lock().unwrap().insert(path.clone());
                        // 读取线程退出（设备拔出）时把路径移出，允许后续重新打开
                        let op = opened.clone();
                        thread::spawn(move || {
                            read_loop(dev);
                            op.lock().unwrap().remove(&path);
                        });
                    }
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
}

/// 打开并确认是一个键盘（带字母/回车等按键）；虚拟设备跳过
fn open_keyboard(path: &PathBuf) -> Option<Device> {
    let dev = Device::open(path).ok()?;
    if dev.name() == Some(VIRT_NAME) {
        return None;
    }
    let is_keyboard = dev
        .supported_keys()
        .is_some_and(|k| k.contains(KeyCode::KEY_A) || k.contains(KeyCode::KEY_ESC) || k.contains(KeyCode::KEY_ENTER));
    is_keyboard.then_some(dev)
}

fn read_loop(mut dev: Device) {
    loop {
        match dev.fetch_events() {
            // fetch_events 默认阻塞，有事件才返回
            Ok(events) => {
                for ev in events {
                    if ev.event_type() == EventType::KEY {
                        handle_key(ev.code() as u32, ev.value());
                    }
                }
            }
            Err(_) => break,
        }
    }
}

fn handle_key(code: u32, value: i32) {
    let pressed = value != 0;
    let repeat = value == 2;
    let is_mod = keys::modifier(code).is_some();
    if (code as usize) < 256 {
        let mut d = DOWN.lock().unwrap();
        match value {
            0 => d[code as usize] = false,
            1 => d[code as usize] = true,
            _ => {}
        }
    }
    let m = mods(&DOWN.lock().unwrap());
    let kind = if is_mod { KeyKind::Modifier(pressed) } else if pressed { KeyKind::Down } else { KeyKind::Up };
    let chars = (kind == KeyKind::Down).then(|| char_for(code).map(String::from).unwrap_or_default());
    let ev = KeyEvent { kind, code, mods: m, repeat, chars };
    if let Some(f) = HOOK.lock().unwrap().as_mut() {
        f(&ev);
    }
}

/// 用于显示快捷键名：字母/数字/符号的可读字符
fn char_for(code: u32) -> Option<char> {
    Some(match code {
        2 => '1', 3 => '2', 4 => '3', 5 => '4', 6 => '5', 7 => '6', 8 => '7', 9 => '8', 10 => '9', 11 => '0',
        12 => '-', 13 => '=',
        16 => 'q', 17 => 'w', 18 => 'e', 19 => 'r', 20 => 't', 21 => 'y', 22 => 'u', 23 => 'i', 24 => 'o', 25 => 'p',
        26 => '[', 27 => ']',
        30 => 'a', 31 => 's', 32 => 'd', 33 => 'f', 34 => 'g', 35 => 'h', 36 => 'j', 37 => 'k', 38 => 'l',
        39 => ';', 40 => '\'', 41 => '`', 43 => '\\',
        44 => 'z', 45 => 'x', 46 => 'c', 47 => 'v', 48 => 'b', 49 => 'n', 50 => 'm',
        51 => ',', 52 => '.', 53 => '/', 57 => ' ',
        _ => return None,
    })
}

// MARK: - 注入（uinput）

/// 探一次注入设备可不可写，判定与文案见 [`crate::inject`]。
///
/// 只开一次文件：写入是上屏唯一的门槛，而打开 `/dev/uinput` 没有副作用——虚拟设备要等
/// `UI_DEV_SETUP` 才创建（[`build_virt`] 干的事），这里只是问一句"让不让我写"。
pub fn inject_device() -> crate::inject::Device {
    use crate::inject::Device;
    match std::fs::OpenOptions::new().write(true).open(crate::inject::DEVICE_PATH) {
        Ok(_) => Device::Writable,
        // 节点不在：uinput 没加载，跟权限不是一回事
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Device::Missing,
        Err(_) => Device::NotWritable,
    }
}

/// 虚拟键盘建好之后，合成器（KWin 里的 libinput）要过一小会儿才打开这个新出现的
/// `/dev/input/eventN`；在那之前发出去的按键**没有读者，内核直接丢掉**，而 `emit`
/// 照样返回 `Ok`——症状就是"第一次说话，字没上屏"。所以第一次发之前先把这段窗口等过去。
///
/// 实测（本机 KWin 6.8）：建好 0ms 就发 → 丢；80ms / 150ms / 250ms / 400ms 发 → 到。
/// 取 250ms 是给负载高的场合留余量；只在第一次发的时候等，设备之后一直活着。
const WARMUP: Duration = Duration::from_millis(250);

/// 虚拟键盘，外加它是什么时候建起来的（[`WARMUP`] 要用）
struct Virt {
    dev: Mutex<evdev::uinput::VirtualDevice>,
    born: Instant,
}

fn virt() -> Option<&'static Virt> {
    static V: OnceLock<Option<Virt>> = OnceLock::new();
    V.get_or_init(|| build_virt().ok().map(|dev| Virt { dev: Mutex::new(dev), born: Instant::now() })).as_ref()
}

fn build_virt() -> std::io::Result<evdev::uinput::VirtualDevice> {
    use evdev::AttributeSet;
    let mut keys = AttributeSet::<KeyCode>::new();
    for k in [
        KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT, KeyCode::KEY_LEFTALT, KeyCode::KEY_LEFTMETA,
        KeyCode::KEY_BACKSPACE, KeyCode::KEY_ENTER, KeyCode::KEY_ESC, KeyCode::KEY_TAB, KeyCode::KEY_SPACE,
        KeyCode::KEY_A, KeyCode::KEY_B, KeyCode::KEY_C, KeyCode::KEY_D, KeyCode::KEY_E, KeyCode::KEY_F, KeyCode::KEY_G,
        KeyCode::KEY_H, KeyCode::KEY_I, KeyCode::KEY_J, KeyCode::KEY_K, KeyCode::KEY_L, KeyCode::KEY_M, KeyCode::KEY_N,
        KeyCode::KEY_O, KeyCode::KEY_P, KeyCode::KEY_Q, KeyCode::KEY_R, KeyCode::KEY_S, KeyCode::KEY_T, KeyCode::KEY_U,
        KeyCode::KEY_V, KeyCode::KEY_W, KeyCode::KEY_X, KeyCode::KEY_Y, KeyCode::KEY_Z,
        KeyCode::KEY_0, KeyCode::KEY_1, KeyCode::KEY_2, KeyCode::KEY_3, KeyCode::KEY_4,
        KeyCode::KEY_5, KeyCode::KEY_6, KeyCode::KEY_7, KeyCode::KEY_8, KeyCode::KEY_9,
        KeyCode::KEY_MINUS, KeyCode::KEY_EQUAL, KeyCode::KEY_LEFTBRACE, KeyCode::KEY_RIGHTBRACE,
        KeyCode::KEY_SEMICOLON, KeyCode::KEY_APOSTROPHE, KeyCode::KEY_GRAVE, KeyCode::KEY_BACKSLASH,
        KeyCode::KEY_COMMA, KeyCode::KEY_DOT, KeyCode::KEY_SLASH,
    ] {
        keys.insert(k);
    }
    evdev::uinput::VirtualDevice::builder()?.name(VIRT_NAME).with_keys(&keys)?.build()
}

fn keyev(k: KeyCode, down: bool) -> InputEvent {
    InputEvent::new(EventType::KEY.0, k.0, down as i32)
}

fn emit(events: &[InputEvent]) {
    let Some(v) = virt() else { return };
    // 预热窗口：设备刚建好时合成器还没打开它，这一下发出去会被丢掉（见 WARMUP）
    if let Some(left) = WARMUP.checked_sub(v.born.elapsed()) {
        thread::sleep(left);
    }
    if let Ok(mut dev) = v.dev.lock() {
        let _ = dev.emit(events);
    }
}

/// 重放被吞掉的按键。Linux 不吞键（无独占设备），与 macOS 一样无需重放
pub fn inject_key(_code: u32, _down: bool) {}

pub(crate) fn backspace(n: usize) {
    let events: Vec<InputEvent> = (0..n)
        .flat_map(|_| [keyev(KeyCode::KEY_BACKSPACE, true), keyev(KeyCode::KEY_BACKSPACE, false)])
        .collect();
    emit(&events);
}

/// 仅 ASCII 能经 uinput 逐字打出；非 ASCII 跳过（应用层在 Linux 下已关闭实时上屏）
pub(crate) fn type_text(text: &str) {
    for ch in text.chars() {
        let Some((k, shift)) = ascii_key(ch) else { continue };
        if shift {
            emit(&[keyev(KeyCode::KEY_LEFTSHIFT, true), keyev(k, true), keyev(k, false), keyev(KeyCode::KEY_LEFTSHIFT, false)]);
        } else {
            emit(&[keyev(k, true), keyev(k, false)]);
        }
    }
}

fn ascii_key(ch: char) -> Option<(KeyCode, bool)> {
    const A: u16 = KeyCode::KEY_A.0;
    Some(match ch {
        'a'..='z' => (KeyCode::new(ch as u16 - b'a' as u16 + A), false),
        'A'..='Z' => (KeyCode::new(ch as u16 - b'A' as u16 + A), true),
        '0' => (KeyCode::KEY_0, false), '1' => (KeyCode::KEY_1, false), '2' => (KeyCode::KEY_2, false),
        '3' => (KeyCode::KEY_3, false), '4' => (KeyCode::KEY_4, false), '5' => (KeyCode::KEY_5, false),
        '6' => (KeyCode::KEY_6, false), '7' => (KeyCode::KEY_7, false), '8' => (KeyCode::KEY_8, false),
        '9' => (KeyCode::KEY_9, false),
        ' ' => (KeyCode::KEY_SPACE, false),
        '-' => (KeyCode::KEY_MINUS, false), '_' => (KeyCode::KEY_MINUS, true),
        '=' => (KeyCode::KEY_EQUAL, false), '+' => (KeyCode::KEY_EQUAL, true),
        '[' => (KeyCode::KEY_LEFTBRACE, false), '{' => (KeyCode::KEY_LEFTBRACE, true),
        ']' => (KeyCode::KEY_RIGHTBRACE, false), '}' => (KeyCode::KEY_RIGHTBRACE, true),
        ';' => (KeyCode::KEY_SEMICOLON, false), ':' => (KeyCode::KEY_SEMICOLON, true),
        '\'' => (KeyCode::KEY_APOSTROPHE, false), '"' => (KeyCode::KEY_APOSTROPHE, true),
        '`' => (KeyCode::KEY_GRAVE, false), '~' => (KeyCode::KEY_GRAVE, true),
        '\\' => (KeyCode::KEY_BACKSLASH, false), '|' => (KeyCode::KEY_BACKSLASH, true),
        ',' => (KeyCode::KEY_COMMA, false), '<' => (KeyCode::KEY_COMMA, true),
        '.' => (KeyCode::KEY_DOT, false), '>' => (KeyCode::KEY_DOT, true),
        '/' => (KeyCode::KEY_SLASH, false), '?' => (KeyCode::KEY_SLASH, true),
        _ => return None,
    })
}

/// 借剪贴板 + Ctrl+V 上屏（Wayland 无法逐字打中文）。复制失败时退回逐字打 ASCII。
///
/// 上屏完把原剪贴板文本放回去（mac / Windows 的 `paste` 同款语义）：借之前记下原内容，
/// 等 1.2 秒——上屏方从 `wl-copy` 取数据通常在毫秒级——剪贴板还是我们的文本（期间没人
/// 复制过别的）才写回。只能恢复文本：图片等多媒体读不回来，与旧版「不恢复」相比只差
/// 在有得救的时候救。Wayland 的剪贴板是所有权制，重新 `wl-copy` 一遍原内容即为恢复，
/// 原属主是谁、还活不活着都无所谓。
pub fn paste(text: &str) {
    let t = text.to_string();
    thread::spawn(move || {
        let saved = read_clipboard().filter(|s| s.as_str() != t);
        if !copy_to_clipboard(&t) {
            type_text(&t);
            return;
        }
        // 等 wl-copy 把内容提交到合成器后再发 Ctrl+V
        thread::sleep(Duration::from_millis(80));
        emit(&[
            keyev(KeyCode::KEY_LEFTCTRL, true),
            keyev(KeyCode::KEY_V, true),
            keyev(KeyCode::KEY_V, false),
            keyev(KeyCode::KEY_LEFTCTRL, false),
        ]);
        if let Some(saved) = saved {
            thread::sleep(Duration::from_millis(1200));
            if read_clipboard().as_deref() == Some(t.as_str()) {
                copy_to_clipboard(&saved);
            }
        }
    });
}

/// 与 [`paste`] 相同的上屏，但**不**恢复剪贴板：结果留在剪贴板里——AI 整理的定稿
/// 语义，整理出的文字用户往往还要贴去别处
pub fn paste_keep(text: &str) {
    let t = text.to_string();
    thread::spawn(move || {
        if !copy_to_clipboard(&t) {
            type_text(&t);
            return;
        }
        // 等 wl-copy 把内容提交到合成器后再发 Ctrl+V
        thread::sleep(Duration::from_millis(80));
        emit(&[
            keyev(KeyCode::KEY_LEFTCTRL, true),
            keyev(KeyCode::KEY_V, true),
            keyev(KeyCode::KEY_V, false),
            keyev(KeyCode::KEY_LEFTCTRL, false),
        ]);
    });
}

/// 读剪贴板文本（读不到或为空返回 None）
fn read_clipboard() -> Option<String> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let candidates: &[(&str, &[&str])] = if wayland {
        &[("wl-paste", &["-n"]), ("xclip", &["-o", "-selection", "clipboard"])]
    } else {
        &[("xclip", &["-o", "-selection", "clipboard"]), ("wl-paste", &["-n"])]
    };
    for (cmd, args) in candidates {
        if let Ok(out) = Command::new(cmd).args(*args).output() {
            if out.status.success() && !out.stdout.is_empty() {
                return Some(String::from_utf8_lossy(&out.stdout).into_owned());
            }
        }
    }
    None
}

/// 读 primary selection（中键粘贴的那个选区）。「选中文字」这个动作本身就会更新它：
/// 读它不发任何键、不碰剪贴板，Klipper 这类剪贴板历史里不会留下任何痕迹
fn read_primary() -> Option<String> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let candidates: &[(&str, &[&str])] = if wayland {
        &[("wl-paste", &["-n", "-p"]), ("xclip", &["-o", "-selection", "primary"])]
    } else {
        &[("xclip", &["-o", "-selection", "primary"]), ("wl-paste", &["-n", "-p"])]
    };
    for (cmd, args) in candidates {
        if let Ok(out) = Command::new(cmd).args(*args).output() {
            if out.status.success() && !out.stdout.is_empty() {
                return Some(String::from_utf8_lossy(&out.stdout).into_owned());
            }
        }
    }
    None
}

/// 取当前选中的文本：读 primary selection——选中即有，零按键、零剪贴板操作，剪贴板
/// 历史里不留痕迹。代价：primary 全局共享，当前窗口没有选中而别处选过时，取到的是
/// 那份旧内容（ Wayland 拿不到前台窗口信息，无法校验）。读不到 = 没有取到选中。
/// 目前三平台里只有 Linux 实现。
pub fn copy_selection() -> Option<String> {
    read_primary().filter(|t| !t.trim().is_empty())
}

fn copy_to_clipboard(text: &str) -> bool {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let candidates: &[(&str, &[&str])] = if wayland {
        &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
    } else {
        &[("xclip", &["-selection", "clipboard"]), ("wl-copy", &[])]
    };
    for (cmd, args) in candidates {
        let Ok(mut child) = Command::new(cmd).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
            continue;
        };
        if let Some(mut si) = child.stdin.take() {
            if si.write_all(text.as_bytes()).is_err() {
                let _ = child.wait();
                continue;
            }
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return true;
        }
    }
    false
}

// MARK: - 光标 / 前台应用

/// Wayland 无全局光标位置 API，恒 None（面板落在默认位置）
pub fn caret() -> Option<Rect> {
    None
}

#[derive(Clone, Copy, Debug)]
pub struct FrontApp;

pub fn front_app() -> Option<FrontApp> {
    None
}

impl FrontApp {
    pub fn activate(&self) {}
}

pub fn secure_input() -> bool {
    false
}

// MARK: - 系统输出静音

/// 「录音时静音系统」：把默认输出（扬声器）静音，原状态记进返回的 [`crate::OutputMute`]，
/// Drop 时恢复。识别音频本来只从麦克风来（cpal 只开输入设备），这条管的是别打扰人——
/// 说话这会儿放着的歌、会议声先停一停。
///
/// 走 pactl（PulseAudio 与 PipeWire 的 pipewire-pulse 都提供），没有再试 wpctl。
pub fn mute_output_start() -> crate::OutputMute {
    crate::OutputMute::from_restore(sink_muted().map(|orig| {
        set_output_muted(true);
        orig
    }))
}

/// 默认输出的当前静音状态；工具缺失或查询失败时 None——那样就不静音
fn sink_muted() -> Option<bool> {
    if let Some(out) = run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]) {
        if let Some(m) = pactl_mute(&out) {
            return Some(m);
        }
    }
    wpctl_muted(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?)
}

pub(crate) fn set_output_muted(mute: bool) {
    let v = if mute { "1" } else { "0" };
    if run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", v]).is_none() {
        run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", v]);
    }
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `pactl get-sink-mute` 的输出，如 `Mute: no`
fn pactl_mute(out: &str) -> Option<bool> {
    match out.rsplit(':').next()?.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}

/// `wpctl get-volume` 的输出，如 `Volume: 0.45` / `Volume: 0.45 [MUTED]`
fn wpctl_muted(out: &str) -> Option<bool> {
    Some(out.contains("[MUTED]"))
}

// MARK: - 权限（Linux 无需系统授权）

pub mod perm {
    use super::MicStatus;

    pub fn accessibility(_prompt: bool) -> bool {
        true
    }
    pub fn mic() -> MicStatus {
        MicStatus::Granted
    }
    pub fn request_mic() {}
    pub fn open_accessibility() {}
    pub fn open_mic() {}
}

#[cfg(test)]
mod mute_tests {
    use super::*;

    #[test]
    fn pactl_mute_parses_yes_no_and_rejects_garbage() {
        assert_eq!(pactl_mute("Mute: no\n"), Some(false));
        assert_eq!(pactl_mute("Mute: yes\n"), Some(true));
        assert_eq!(pactl_mute("没有输出设备\n"), None);
    }

    #[test]
    fn wpctl_muted_flags_bracketed_marker() {
        assert_eq!(wpctl_muted("Volume: 0.45\n"), Some(false));
        assert_eq!(wpctl_muted("Volume: 0.45 [MUTED]\n"), Some(true));
    }

    /// 本机真实验收：静音一瞬再还原（docs/linux.md「录音时静音系统」）。
    /// 需要声音服务在跑，`cargo test -p voicekey-platform -- --ignored` 显式跑。
    #[test]
    #[ignore = "需要本机声音服务"]
    fn mute_round_trip_mutes_then_restores_previous_state() {
        let before = sink_muted();
        let guard = mute_output_start();
        assert!(guard.active());
        assert_eq!(sink_muted(), Some(true));
        drop(guard);
        assert_eq!(sink_muted(), before);
    }
}

// MARK: - 键码映射

pub mod keys {
    use super::*;
    use crate::{HoldKey, Special};

    pub fn modifier(code: u32) -> Option<(u8, &'static str)> {
        Some(match code {
            K_LEFTSHIFT => (SHIFT, "左 Shift"),
            K_RIGHTSHIFT => (SHIFT, "右 Shift"),
            K_LEFTCTRL => (CTRL, "左 Ctrl"),
            K_RIGHTCTRL => (CTRL, "右 Ctrl"),
            K_LEFTALT => (ALT, "左 Alt"),
            K_RIGHTALT => (ALT, "右 Alt"),
            K_LEFTMETA => (META, "左 Super"),
            K_RIGHTMETA => (META, "右 Super"),
            _ => return None,
        })
    }

    pub fn special(code: u32) -> Option<Special> {
        Some(match code {
            K_ESC => Special::Escape,
            K_ENTER => Special::Enter,
            K_UP => Special::Up,
            K_DOWN => Special::Down,
            K_DIGIT_LO..=K_DIGIT_HI => Special::Digit((code - K_DIGIT_LO + 1) as u8),
            _ => return None,
        })
    }

    /// F1–F12（59–88）与 F13–F24（183–194）
    pub fn is_function_key(code: u32) -> bool {
        (59..=88).contains(&code) || (183..=194).contains(&code)
    }

    pub fn special_name(code: u32) -> Option<&'static str> {
        const F: [&str; 24] = [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
            "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
        ];
        if (59..=88).contains(&code) {
            return Some(F[(code - 59) as usize]);
        }
        if (183..=194).contains(&code) {
            return Some(F[(code - 183 + 12) as usize]);
        }
        Some(match code {
            K_SPACE => "Space",
            K_ENTER => "Enter",
            K_TAB => "Tab",
            K_BACKSPACE => "Backspace",
            K_DELETE => "Delete",
            K_ESC => "Esc",
            K_LEFT => "←",
            K_RIGHT => "→",
            K_UP => "↑",
            K_DOWN => "↓",
            K_HOME => "Home",
            K_END => "End",
            K_PAGEUP => "PgUp",
            K_PAGEDOWN => "PgDn",
            _ => return None,
        })
    }

    pub fn hold_keys() -> &'static [HoldKey] {
        &[
            HoldKey { id: "rightAlt", code: K_RIGHTALT, name: "右 Alt" },
            HoldKey { id: "rightCtrl", code: K_RIGHTCTRL, name: "右 Ctrl" },
            HoldKey { id: "rightShift", code: K_RIGHTSHIFT, name: "右 Shift" },
            HoldKey { id: "rightMeta", code: K_RIGHTMETA, name: "右 Super" },
        ]
    }
}
