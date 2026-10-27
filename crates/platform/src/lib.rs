//! 平台相关能力：全局键盘钩子、模拟输入、光标位置、权限

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as imp;

pub use imp::{
    caret, copy_selection, front_app, inject_device, inject_key, mute_output_start, paste, paste_keep,
    secure_input, start_hook, FrontApp,
};

/// 浮层几何：给定工作区与浮层尺寸算落点，应用与 KWin 规则生成共用
pub mod overlay;

/// 上屏前提：注入设备可不可写（判定与文案是纯函数；探测在各平台一份）
pub mod inject;

/// 上屏前提的结论：探一次注入设备，能上屏给 `None`，不能则给出原因
/// （给用户看的话由 [`inject::advice`] 出）。
///
/// 每次调用都真去开一次设备，不缓存：设备权限是会话级的，用户装上 udev 规则后重新登录
/// 就好，而已经跑着的应用下一次问就该答"能上屏"，不该举着一个开机时的旧结论。
pub fn inject_problem() -> Option<inject::Problem> {
    inject::problem(inject_device())
}

/// KWin 窗口规则：生成、安装、卸载（仅 Linux；macOS / Windows 由应用自己摆窗口）
#[cfg(target_os = "linux")]
pub mod kwin;

pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const SHIFT: u8 = 4;
pub const META: u8 = 8;
pub const FN: u8 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Down,
    Up,
    /// 修饰键状态变化，true 为按下
    Modifier(bool),
}

#[derive(Clone, Debug)]
pub struct KeyEvent {
    pub kind: KeyKind,
    /// 平台原生键码（macOS virtual keycode / Windows VK）
    pub code: u32,
    /// 事件发生时按住的修饰键（CTRL|ALT|SHIFT|META|FN）
    pub mods: u8,
    pub repeat: bool,
    /// 按键产生的字符（仅 Down 事件，用于显示快捷键名）
    pub chars: Option<String>,
}

/// 返回 true 表示吞掉该事件
pub type HookFn = Box<dyn FnMut(&KeyEvent) -> bool + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Special {
    Escape,
    Enter,
    Up,
    Down,
    Digit(u8),
}

pub use imp::keys::{hold_keys, is_function_key, modifier, special};

/// 长按说话可选的修饰键
pub struct HoldKey {
    pub id: &'static str,
    pub code: u32,
    pub name: &'static str,
}

pub fn key_name(e: &KeyEvent) -> String {
    if let Some((_, n)) = modifier(e.code) {
        return n.into();
    }
    imp::keys::special_name(e.code)
        .map(String::from)
        .or_else(|| e.chars.as_ref().map(|c| c.trim().to_uppercase()).filter(|c| !c.is_empty()))
        .unwrap_or_else(|| format!("#{}", e.code))
}

/// 修饰键前缀，如 ⌃⌥⇧⌘ / Ctrl+Alt+Shift+Win+
pub fn mods_prefix(m: u8) -> String {
    let mut s = String::new();
    let table: &[(u8, &str)] = if cfg!(target_os = "macos") {
        &[(CTRL, "⌃"), (ALT, "⌥"), (SHIFT, "⇧"), (META, "⌘")]
    } else {
        &[(CTRL, "Ctrl+"), (ALT, "Alt+"), (SHIFT, "Shift+"), (META, "Win+")]
    };
    for (f, n) in table {
        if m & f != 0 {
            s.push_str(n);
        }
    }
    s
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// true：物理像素（Windows）；false：逻辑点（macOS）
    pub physical: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicStatus {
    Granted,
    Denied,
    Undetermined,
}

/// 系统权限（Windows 无需授权，恒为已授权）
pub mod perm {
    pub use crate::imp::perm::*;
}

/// 「录音时静音系统」：开麦时把系统输出静音，收麦时恢复原来的静音状态。
/// 恢复挂在 [`Drop`] 上——打断、超时这些收尾路径不用各自记得恢复，guard 一丢就还原。
/// 平台未实现（Linux 上是 pactl / wpctl 都问不出原状态）时静音不生效：
/// 恢复不了的事不能做一半，guard 整个生命周期都是无操作。
pub struct OutputMute {
    /// Some(原静音状态)：静音生效中；None：未生效
    restore: Option<bool>,
}

impl OutputMute {
    pub(crate) fn from_restore(restore: Option<bool>) -> Self {
        Self { restore }
    }

    /// 静音是否真的生效了
    pub fn active(&self) -> bool {
        self.restore.is_some()
    }
}

impl Drop for OutputMute {
    fn drop(&mut self) {
        if let Some(m) = self.restore.take() {
            imp::set_output_muted(m);
        }
    }
}

/// 边说边上屏：与已打出的文本比对，退格删掉分歧部分再补打新内容（不经剪贴板）
#[derive(Default)]
pub struct Typer {
    typed: Vec<char>,
    done: bool,
}

impl Typer {
    pub fn update(&mut self, text: &str) {
        if self.done {
            return;
        }
        let next: Vec<char> = text.chars().collect();
        let common = self.typed.iter().zip(&next).take_while(|(a, b)| a == b).count();
        imp::backspace(self.typed.len() - common);
        let tail: String = next[common..].iter().collect();
        if !tail.is_empty() {
            imp::type_text(&tail);
        }
        self.typed = next;
    }

    pub fn finish(&mut self) {
        self.done = true;
    }
}
