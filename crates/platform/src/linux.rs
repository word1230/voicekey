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
use std::time::Duration;

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

fn virt() -> Option<&'static Mutex<evdev::uinput::VirtualDevice>> {
    static V: OnceLock<Option<Mutex<evdev::uinput::VirtualDevice>>> = OnceLock::new();
    V.get_or_init(|| build_virt().ok().map(Mutex::new)).as_ref()
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
    if let Some(v) = virt() {
        if let Ok(mut v) = v.lock() {
            let _ = v.emit(events);
        }
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

/// 借剪贴板 + Ctrl+V 上屏（Wayland 无法逐字打中文）。
/// 复制失败时退回逐字打 ASCII。剪贴板不恢复（Wayland 剪贴板归属语义复杂）。
pub fn paste(text: &str) {
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
