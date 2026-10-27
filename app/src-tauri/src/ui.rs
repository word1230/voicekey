//! 窗口与托盘：设置窗口、胶囊浮层（hud）、候选面板（pick）
use crate::settings::{Channel, Settings};
use crate::AppState;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, Monitor, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use voicekey_platform::overlay::{bottom_center, Area, HUD_BOTTOM_MARGIN, HUD_H, HUD_W, PICK_BOTTOM_MARGIN, PICK_W};

const TRAY: &[u8] = include_bytes!("../icons/tray.png");
const TRAY_ACTIVE: &[u8] = include_bytes!("../icons/tray-active.png");

static HUD_TOKEN: AtomicU64 = AtomicU64::new(0);
/// 候选面板锚点（统一坐标空间：macOS 逻辑点 / Windows 物理像素）
static PICK_ANCHOR: Mutex<Option<Anchor>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Anchor {
    caret: Option<(f64, f64, f64, f64)>,
    area: Area,
    height: f64,
}

fn work_area(m: &Monitor) -> Area {
    let wa = m.work_area();
    let s = m.scale_factor();
    let physical = !cfg!(target_os = "macos");
    let k = if physical { 1.0 } else { s };
    Area {
        x: wa.position.x as f64 / k,
        y: wa.position.y as f64 / k,
        w: wa.size.width as f64 / k,
        h: wa.size.height as f64 / k,
        scale: s,
        physical,
    }
}

fn set_pos<R: Runtime>(w: &WebviewWindow<R>, physical: bool, x: f64, y: f64) {
    let _ = if physical {
        w.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32))
    } else {
        w.set_position(tauri::LogicalPosition::new(x, y))
    };
}

/// 逻辑点直接给尺寸；物理像素向上取整，否则 120% 缩放下 WebView 客户区比内容小一截
fn set_client<R: Runtime>(w: &WebviewWindow<R>, a: &Area, logical_w: f64, logical_h: f64) {
    let _ = if a.physical {
        w.set_size(tauri::PhysicalSize::new(a.units(logical_w).ceil() as u32, a.units(logical_h).ceil() as u32))
    } else {
        w.set_size(LogicalSize::new(logical_w, logical_h))
    };
}

fn cursor_area<R: Runtime>(app: &AppHandle<R>) -> Option<Area> {
    let p = app.cursor_position().ok();
    let m = p
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())?;
    Some(work_area(&m))
}

fn overlay<R: Runtime>(app: &AppHandle<R>, label: &str, w: f64, h: f64) -> tauri::Result<WebviewWindow<R>> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!("window.__VK_VIEW = {label:?};"))
        .title(label)
        .transparent(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .focusable(false)
        .visible_on_all_workspaces(true)
        .inner_size(w, h)
        .build()
}

pub fn create_overlays<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let hud = overlay(app, "hud", HUD_W, HUD_H)?;
    // pick panel 不设：它尺寸贴合内容、死区很小，且要用鼠标点选候选
    #[cfg(not(target_os = "linux"))]
    hud.set_ignore_cursor_events(true)?; // HUD 是纯显示，不该吃鼠标
    #[cfg(target_os = "linux")]
    hud_click_through(&hud); // Linux 上换一条路，见函数上的说明
    let pick = overlay(app, "pick", PICK_W, 300.0)?;
    #[cfg(target_os = "macos")]
    panel::convert(&pick);
    #[cfg(not(target_os = "macos"))]
    let _ = pick;
    Ok(())
}

pub fn show_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .initialization_script("window.__VK_VIEW = \"settings\";")
        .title("VoiceKey 设置")
        .inner_size(860.0, 600.0)
        .resizable(false)
        .maximizable(false)
        .center();
    #[cfg(target_os = "macos")]
    let b = b.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    let built = b.build();
    if let Ok(w) = built {
        let _ = w.set_focus();
    }
}

fn current_channel<R: Runtime>(app: &AppHandle<R>) -> Channel {
    app.state::<AppState>().settings.read().unwrap().channel
}

/// 胶囊浮层：state = listen | wait | error | info
pub fn hud<R: Runtime>(app: &AppHandle<R>, state: &str, text: &str, ch: Channel) {
    HUD_TOKEN.fetch_add(1, Ordering::Relaxed);
    let _ = app.emit_to("hud", "hud", json!({ "state": state, "text": text, "channel": ch }));
    let Some(w) = app.get_webview_window("hud") else { return };
    if !w.is_visible().unwrap_or(false) {
        if let Some(a) = cursor_area(app) {
            let (x, y) = bottom_center(&a, HUD_W, HUD_H, HUD_BOTTOM_MARGIN);
            set_client(&w, &a, HUD_W, HUD_H);
            set_pos(&w, a.physical, x, y);
        }
        let _ = w.show();
    }
}

/// 让 HUD 不吃鼠标：窗口是 680×120 的透明矩形，内容只占中间一条胶囊，不设输入区的话
/// 它盖住的地方就点不下去了。
///
/// 做法是把顶层窗口的输入区清空（GDK `input_shape_combine_region` → Wayland 的
/// `wl_surface.set_input_region`）。非 Linux 走 tao 的 `set_ignore_cursor_events`，Linux 上
/// 那条路有两个坑，结论如下，逐轮实测数据与验收手法见 `docs/linux.md`「HUD 点击穿透」：
///
/// 1. **不能在窗口映射之前设**。tao 的实现是拿窗口的 gdk window 去设一个 1×1 的输入区
///    （不是空区域，所以那一个像素照旧吃点击），窗口还没映射时 gdk window 是空的，
///    tao 那边 `unwrap` 当场 panic——`create_overlays` 里 Linux 当初就是为这个整条跳过的。
/// 2. **设早了会白设**。HUD 的 WebKit 内容起来的时候（合成层 + 入场动画）顶层 surface 的
///    输入区会被冲回默认，映射那一刻和紧随其后设的都被冲掉，得等第一帧合成完再设才站得住。
///    所以映射时那一遍不必指望（留着只是因为便宜无害），真正管用的是后面补的。
///
/// 挂在窗口的 `map` 信号上而不是跟着 `hud()` 里的 `show()`：`map` 才是「窗口真到屏幕上了」，
/// 而且每次 hide / show 重新映射时它都会再来一发，正好每次都能把输入区补上。
#[cfg(target_os = "linux")]
fn hud_click_through<R: Runtime>(w: &WebviewWindow<R>) {
    /// 第一次补设的延迟，第二次是它的一倍。早于此设会被 WebKit 的第一帧冲掉（阈值见
    /// `docs/linux.md`）；补两遍是给冷启动、首帧来得更晚的情况兜底——清空幂等，多设一次
    /// 没有副作用，而「点击又被吃掉」是不给任何提示的失败方式。
    const REPLAY_MS: u64 = 400;
    let Ok(gw) = w.gtk_window() else { return };
    let w = w.clone();
    gtk::prelude::WidgetExt::connect_map(&gw, move |_| {
        clear_input_region(&w);
        // 补的那两下：先让出这一轮事件循环，等 WebKit 把第一帧合成上去
        let w = w.clone();
        std::thread::spawn(move || {
            for _ in 0..2 {
                std::thread::sleep(Duration::from_millis(REPLAY_MS));
                let w2 = w.clone();
                let _ = w.run_on_main_thread(move || clear_input_region(&w2));
            }
        });
    });
}

/// 把窗口的输入区设成空（0×0 的矩形区域）：窗口还在，但鼠标点到哪儿都不落它身上
#[cfg(target_os = "linux")]
fn clear_input_region<R: Runtime>(w: &WebviewWindow<R>) {
    if let Ok(gw) = w.gtk_window() {
        if let Some(win) = gtk::prelude::WidgetExt::window(&gw) {
            let empty = gtk::gdk::cairo::Region::create_rectangle(&gtk::gdk::cairo::RectangleInt::new(0, 0, 0, 0));
            win.input_shape_combine_region(&empty, 0, 0);
        }
    }
}

pub fn hud_error<R: Runtime>(app: &AppHandle<R>, msg: &str) {
    hud(app, "error", msg, current_channel(app));
    hud_hide(app, 2500);
}

pub fn hud_hide<R: Runtime>(app: &AppHandle<R>, delay_ms: u64) {
    let token = HUD_TOKEN.load(Ordering::Relaxed);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        if HUD_TOKEN.load(Ordering::Relaxed) != token {
            return;
        }
        let _ = app.emit_to("hud", "hud", json!({ "state": "hidden" }));
        tokio::time::sleep(Duration::from_millis(160)).await;
        if HUD_TOKEN.load(Ordering::Relaxed) == token {
            if let Some(w) = app.get_webview_window("hud") {
                let _ = w.hide();
            }
        }
    });
}

fn caret_in_space<R: Runtime>(app: &AppHandle<R>, r: voicekey_platform::Rect) -> Option<((f64, f64, f64, f64), Area)> {
    let mons = app.available_monitors().ok()?;
    mons.iter().find_map(|m| {
        let s = m.scale_factor();
        let (px, py) = (m.position().x as f64, m.position().y as f64);
        let (pw, ph) = (m.size().width as f64, m.size().height as f64);
        let (bx, by, bw, bh) = if r.physical { (px, py, pw, ph) } else { (px / s, py / s, pw / s, ph / s) };
        let inside = r.x >= bx && r.x < bx + bw && r.y >= by && r.y < by + bh;
        inside.then(|| ((r.x, r.y, r.w, r.h), work_area(m)))
    })
}

fn place_pick<R: Runtime>(w: &WebviewWindow<R>, a: &Anchor) {
    let ar = &a.area;
    let gap = ar.units(6.0);
    let pw = ar.units(PICK_W);
    // 高度先限制在工作区内。否则 ph 过大时 clamp 上界小于下界，Rust 直接 panic 闪退。
    let requested = if a.height.is_finite() { ar.units(a.height) } else { ar.units(160.0) };
    let ph = cap_len(requested, ar.h, gap);
    let logical_h = if cfg!(target_os = "macos") { ph } else { ph / ar.scale };
    let (x, y) = match a.caret {
        Some((cx, cy, _, chh)) => {
            let above = cy - ph - gap;
            (cx - ar.units(40.0), if above >= ar.y { above } else { cy + chh + gap })
        }
        // 插入点（caret）未知（Wayland）时与胶囊一样落底部居中，只是离底更远
        None => bottom_center(ar, PICK_W, a.height, PICK_BOTTOM_MARGIN),
    };
    let x = clamp_axis(x, ar.x + gap, ar.x + ar.w - pw - gap);
    let y = clamp_axis(y, ar.y + gap, ar.y + ar.h - ph - gap);
    set_client(w, ar, PICK_W, logical_h);
    set_pos(w, ar.physical, x, y);
}

/// `f64::clamp` 在 min > max 时 panic。面板比屏幕还宽/高时上界可能小于下界。
fn clamp_axis(v: f64, min: f64, max: f64) -> f64 {
    v.clamp(min, max.max(min))
}

fn cap_len(requested: f64, area: f64, gap: f64) -> f64 {
    requested.min((area - gap * 2.0).max(1.0)).max(1.0)
}

pub fn pick_show<R: Runtime>(app: &AppHandle<R>, caret: Option<voicekey_platform::Rect>, rows: usize, focus: bool) {
    let Some(w) = app.get_webview_window("pick") else { return };
    let (caret, area) = match caret.and_then(|r| caret_in_space(app, r)) {
        Some((c, a)) => (Some(c), a),
        None => match cursor_area(app) {
            Some(a) => (None, a),
            None => return,
        },
    };
    let a = Anchor { caret, area, height: 92.0 + rows as f64 * 48.0 };
    *PICK_ANCHOR.lock().unwrap() = Some(a);
    place_pick(&w, &a);
    #[cfg(target_os = "macos")]
    panel::KEYABLE.store(focus, std::sync::atomic::Ordering::Relaxed);
    let _ = w.show();
    #[cfg(not(target_os = "macos"))]
    if focus {
        let _ = w.set_focus();
    }
}

/// 前端按内容高度回报，保持底边贴着光标
pub fn pick_resize<R: Runtime>(app: &AppHandle<R>, height: f64) {
    if !height.is_finite() || height < 1.0 {
        return;
    }
    let Some(w) = app.get_webview_window("pick") else { return };
    let mut g = PICK_ANCHOR.lock().unwrap();
    if let Some(a) = g.as_mut() {
        a.height = height;
        place_pick(&w, a);
    }
}

pub fn pick_hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("pick") {
        let _ = w.hide();
    }
}

fn tray_menu<R: Runtime>(app: &AppHandle<R>, s: &Settings, paused: bool) -> tauri::Result<Menu<R>> {
    let head = if paused {
        "已暂停监听".to_string()
    } else {
        let tap = s.tap_shortcut.as_ref().map(|t| format!(" 或点按 {}", t.title())).unwrap_or_default();
        format!("长按 {}{} 说话", s.hold_name(), tap)
    };
    let mut items: Vec<Box<dyn IsMenuItem<R>>> = vec![
        Box::new(MenuItem::with_id(app, "head", head, false, None::<&str>)?),
        Box::new(PredefinedMenuItem::separator(app)?),
    ];
    for c in Channel::available() {
        let id = format!("ch:{}", serde_json::to_value(c)?.as_str().unwrap_or(""));
        items.push(Box::new(CheckMenuItem::with_id(app, id, c.title(), true, s.channel == c, None::<&str>)?));
    }
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(CheckMenuItem::with_id(app, "pause", "暂停监听", true, paused, None::<&str>)?));
    if let Some(v) = crate::update::available() {
        items.push(Box::new(MenuItem::with_id(app, "update", format!("更新到 v{v} 并重启"), true, None::<&str>)?));
    } else if crate::update::downloading() {
        items.push(Box::new(MenuItem::with_id(app, "update", "正在下载更新…", false, None::<&str>)?));
    }
    items.push(Box::new(MenuItem::with_id(app, "settings", "设置…", true, Some("CmdOrCtrl+,"))?));
    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(MenuItem::with_id(app, "quit", "退出 VoiceKey", true, Some("CmdOrCtrl+Q"))?));
    let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

fn tray_icon<R: Runtime>(app: &AppHandle<R>, active: bool) -> Option<Image<'static>> {
    if cfg!(target_os = "macos") {
        Image::from_bytes(if active { TRAY_ACTIVE } else { TRAY }).ok()
    } else {
        app.default_window_icon().map(|i| i.clone().to_owned())
    }
}

pub fn build_tray<R: Runtime>(app: &AppHandle<R>, s: &Settings) -> tauri::Result<()> {
    let mut b = TrayIconBuilder::with_id("main")
        .menu(&tray_menu(app, s, false)?)
        .show_menu_on_left_click(true)
        .icon_as_template(true)
        .tooltip("VoiceKey")
        .on_menu_event(|app, ev| crate::tray_event(app, ev.id().as_ref()));
    if let Some(i) = tray_icon(app, false) {
        b = b.icon(i);
    }
    b.build(app)?;
    Ok(())
}

pub fn refresh_tray<R: Runtime>(app: &AppHandle<R>, s: &Settings, paused: bool) {
    if let (Some(t), Ok(m)) = (app.tray_by_id("main"), tray_menu(app, s, paused)) {
        let _ = t.set_menu(Some(m));
    }
}

pub fn set_tray_active<R: Runtime>(app: &AppHandle<R>, active: bool) {
    if !cfg!(target_os = "macos") {
        return;
    }
    if let Some(t) = app.tray_by_id("main") {
        let _ = t.set_icon(tray_icon(app, active));
        let _ = t.set_icon_as_template(true);
    }
}

/// macOS：候选面板换成不激活应用的 NSPanel。需要接收按键时（安全输入）成为 key window，
/// 键盘事件直达面板而前台应用保持激活；其余时候不抢键盘。
#[cfg(target_os = "macos")]
mod panel {
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
    use objc2::{msg_send, sel};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::{Runtime, WebviewWindow};

    pub static KEYABLE: AtomicBool = AtomicBool::new(false);
    const NONACTIVATING: usize = 1 << 7;

    extern "C" {
        fn object_setClass(obj: *mut AnyObject, cls: *const AnyClass) -> *const AnyClass;
    }

    extern "C-unwind" fn can_key(_: &AnyObject, _: Sel) -> Bool {
        Bool::new(KEYABLE.load(Ordering::Relaxed))
    }

    extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
        Bool::NO
    }

    fn class() -> &'static AnyClass {
        if let Some(c) = AnyClass::get(c"VKPanel") {
            return c;
        }
        let mut b = ClassBuilder::new(c"VKPanel", AnyClass::get(c"NSPanel").unwrap()).unwrap();
        unsafe {
            b.add_method(sel!(canBecomeKeyWindow), can_key as extern "C-unwind" fn(_, _) -> _);
            b.add_method(sel!(canBecomeMainWindow), no as extern "C-unwind" fn(_, _) -> _);
        }
        b.register()
    }

    pub fn convert<R: Runtime>(w: &WebviewWindow<R>) {
        let Ok(ptr) = w.ns_window() else { return };
        let ptr = ptr as usize;
        let _ = w.run_on_main_thread(move || unsafe {
            let obj = ptr as *mut AnyObject;
            object_setClass(obj, class());
            let o = &*obj;
            let mask: usize = msg_send![o, styleMask];
            let _: () = msg_send![o, setStyleMask: mask | NONACTIVATING];
            let _: () = msg_send![o, setHidesOnDeactivate: Bool::NO];
            let _: () = msg_send![o, setHasShadow: Bool::NO];
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{cap_len, clamp_axis};

    #[test]
    fn clamp_axis_when_panel_exceeds_work_area() {
        // 旧代码上界只 max(ar.y)，下界是 ar.y + gap，面板超高时 min > max。
        let ar_y = 25.0;
        let gap = 6.0;
        let min_y = ar_y + gap;
        let max_y = ar_y;
        assert!(min_y > max_y);
        assert_eq!(clamp_axis(10.0, min_y, max_y), min_y);
    }

    #[test]
    fn cap_len_keeps_panel_inside_work_area() {
        let ar_y = 25.0;
        let area = 800.0;
        let gap = 6.0;
        let ph = cap_len(1400.0, area, gap);
        assert!(ar_y + area - ph - gap >= ar_y + gap);
        assert!(ph + gap * 2.0 <= area + f64::EPSILON);
    }
}
