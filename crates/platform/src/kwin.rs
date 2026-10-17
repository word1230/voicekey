//! KWin 窗口规则（Linux / KWin Wayland）
//!
//! Wayland 下客户端摆不了窗口、定不了焦点、去不掉装饰（见 `docs/adr/0002`），浮层的
//! 位置/置顶/不抢焦点只能由合成器一侧的窗口规则决定。这里生成 `kwinrulesrc` 的规则
//! 内容（纯函数，可单测），并提供幂等的安装/卸载。
//!
//! 规则的 `app_id` 在安装时**运行时从 KWin 查得**（见 [`probe`]），不假设等于进程名：
//! 断言错误的 app_id 会让规则静默不生效，症状是"浮层看起来没装规则"。

use crate::overlay::{bottom_center, Area, HUD_BOTTOM_MARGIN, HUD_H, HUD_W};

/// 规则的生效策略写进 `kwinrulesrc` 的是 KWin `Rules` 那个匿名枚举的**数值**：
/// `Unused=0, DontAffect=1, Force=2, Apply=3, Remember=4, ApplyNow=5, ForceTemporarily=6`。
/// 规则管理器（KCM）下拉里叫 "Apply Initially" 的是 **3**——窗口映射那一刻应用一次。
///
/// 别按字面猜成 2：2 是 **Force**（持续强制）。`position` / `above` / `decorationpolicy` /
/// `skiptaskbar` / `skipswitcher` 在 KWin 里都是 `SetRule`，两种都收，但语义不同。
pub const APPLY_INITIALLY: &str = "3";

/// `acceptfocus` 是 KWin 里少数的 `forceRule`（`convertForceRule` 只认 `DontAffect` / `Force` /
/// `ForceTemporarily`），所以这一项**不能**用 `Apply`，得用 `Force`——把"不抢焦点"钉死。
const FORCE: &str = "2";

/// 组名匹配：`wmclassmatch=1` 是精确匹配
const MATCH_EXACT: &str = "1";
/// 标题匹配：`titlematch=3` 是正则
const MATCH_REGEXP: &str = "3";

/// 两条规则各占的组名：KWin 规则管理器里各是一行，卸载与幂等也按这个名字来
const HUD_GROUP: &str = "voicekey-hud";
const PICK_GROUP: &str = "voicekey-pick";

/// 这份规则要装的组；安装时先删同名组再写，重复执行因此幂等
pub const MANAGED_GROUPS: &[&str] = &[HUD_GROUP, PICK_GROUP];

/// 此前那条失败的手写规则：`app_id` 与位置都写死、没有置顶。安装时替换掉（不参考）。
pub const STALE_GROUPS: &[&str] = &["voicekey-overlay"];

/// 一条 KWin 窗口规则，即 `kwinrulesrc` 里的一个组
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// `kwinrulesrc` 的组名
    pub group: &'static str,
    /// 组里的键值对，第一项固定是 `Description`
    entries: Vec<(&'static str, String)>,
}

impl Rule {
    /// 该写进 `kwinrulesrc` 的键值对，顺序稳定（同内容 → 同文件）。
    /// 第一项是 `Description`（KWin 规则管理器里那一行显示的名字），安装时也一并写进去。
    pub fn entries(&self) -> &[(&'static str, String)] {
        &self.entries
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    /// 排成 `kwinrulesrc` 的一段文本（`vk-kwin install` 用来展示装了些什么）
    pub fn ini(&self) -> String {
        let mut s = format!("[{}]\n", self.group);
        for (k, v) in &self.entries {
            s.push_str(k);
            s.push('=');
            s.push_str(v);
            s.push('\n');
        }
        s
    }
}

/// HUD 的规则：贴底居中 + 置顶 + 关装饰 + 不抢焦点。
///
/// `area` 是 KWin 给出的可用区域（`KWin.PlacementArea`），坐标是**逻辑像素**——
/// KWin 的位置规则也在逻辑像素空间，`physical: false` 让 [`bottom_center`] 不做缩放换算。
pub fn hud_rule(app_id: &str, area: &Area) -> Rule {
    let (x, y) = bottom_center(area, HUD_W, HUD_H, HUD_BOTTOM_MARGIN);
    let mut r = overlay_rule(HUD_GROUP, app_id, &title_regex(HUD_TITLE), "voicekey HUD：贴底居中、置顶、不抢焦点");
    r.entries.push(("position", format!("{},{}", x.round() as i64, y.round() as i64)));
    r.entries.push(("positionrule", APPLY_INITIALLY.into()));
    r
}

/// pick panel 的规则：置顶 + 关装饰 + 不抢焦点，**没有位置**。
///
/// 位置规则在窗口映射时应用一次，而 pick 的高度随候选行数变化（`ui.rs` 里按行数算），
/// 静态位置没法既贴底又不上移长列表——写死的 `y` 要么压住输入行、要么把整个面板推出屏幕。
/// Wayland 上应用侧 `set_position` 是空操作，所以位置交给 KWin 的默认放置。
pub fn pick_rule(app_id: &str) -> Rule {
    overlay_rule(PICK_GROUP, app_id, &title_regex(PICK_TITLE), "voicekey 候选面板：置顶、不抢焦点")
}

// MARK: - 从 KWin 查现状（app_id 与可用区域）

/// KWin 探针报回来的一个窗口
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// KWin 的 `resourceClass`，即规则里 `wmclass` 要匹配的 app_id
    pub class: String,
    pub pid: i64,
    pub title: String,
}

/// KWin 现状：映射中的窗口 + 浮层可用区域
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub windows: Vec<Window>,
    pub area: Option<Area>,
}

const WINDOW_TAG: &str = "VKWIN|";
const AREA_TAG: &str = "VKAREA|";

/// 解析探针输出。探针的 `print()` 经 journald 中转，每行前面有 journal 自己的前缀，
/// 所以按标记找而不是按行首；标题排在最后一段（可能含 `|`）。坏行跳过而不是猜。
pub fn parse_probe(out: &str) -> Probe {
    let mut p = Probe::default();
    for line in out.lines() {
        if let Some((_, rest)) = line.split_once(WINDOW_TAG) {
            let mut f = rest.splitn(3, '|');
            if let (Some(class), Some(pid), Some(title)) = (f.next(), f.next(), f.next()) {
                if let Ok(pid) = pid.trim().parse() {
                    p.windows.push(Window { class: class.into(), pid, title: title.into() });
                }
            }
        } else if let Some((_, rest)) = line.split_once(AREA_TAG) {
            let n: Vec<f64> = rest.trim().split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if let [x, y, w, h] = n[..] {
                // KWin 的可用区域是逻辑像素，与位置规则同一空间
                p.area = Some(Area { x, y, w, h, scale: 1.0, physical: false });
            }
        }
    }
    p
}

/// `[General] Order` 的新值：删掉 `drop` 里已有的，再把 `keep` 补到末尾。
///
/// 其余条目（含系统级规则如 `steam`）原样保留、顺序不动；重复执行结果不变。
/// 结果为空时调用方应删掉 `Order` 键而不是写空串。
pub fn merged_order(existing: &str, keep: &[&str], drop: &[&str]) -> String {
    let mut out: Vec<&str> = existing.split(',').map(str::trim).filter(|g| !g.is_empty()).filter(|g| !drop.contains(g) && !keep.contains(g)).collect();
    out.extend(keep.iter().copied());
    out.join(",")
}

/// `kwinrulesrc` 里的一个组
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IniGroup {
    pub name: String,
    entries: Vec<(String, String)>,
}

impl IniGroup {
    pub fn value(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// 这个组里已有的键（卸载时按它逐个删，连带删掉上一版留下的键）
    pub fn keys(&self) -> Vec<&str> {
        self.entries.iter().map(|(k, _)| k.as_str()).collect()
    }
}

/// 解析 `kwinrulesrc` 原文。只认 `[组]` 与 `键=值` 两种行，按第一个 `=` 切
/// （`wmclass=steamwebhelper steam` 这种带空格的值不许被截断）。
pub fn parse_ini(text: &str) -> Vec<IniGroup> {
    let mut groups: Vec<IniGroup> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            groups.push(IniGroup { name: name.into(), entries: Vec::new() });
        } else if let Some((k, v)) = line.split_once('=') {
            if let Some(g) = groups.last_mut() {
                g.entries.push((k.trim().into(), v.trim().into()));
            }
        }
    }
    groups
}

/// 浮层窗口的标题（`ui.rs` 的 `overlay()` 拿 label 当标题）。规则里的 `title` 正则由它拼出，
/// 所以 `ui.rs` 那边改了标题这里的正则就跟着失效——两处是一件事，一起改
const HUD_TITLE: &str = "hud";
const PICK_TITLE: &str = "pick";
const OVERLAY_TITLES: [&str; 2] = [HUD_TITLE, PICK_TITLE];
/// 设置窗口的标题（同上，`ui.rs` 的 `show_settings`）
const SETTINGS_TITLE: &str = "VoiceKey 设置";

/// 规则的标题匹配用锚定正则（`titlematch=3` 是正则），免得 `hud2` 之类也中
fn title_regex(title: &str) -> String {
    format!("^{title}$")
}

/// 该给规则用哪个 app_id：**从 KWin 报回来的浮层窗口取**，不假设等于进程名。
///
/// 浮层没映射时退到设置窗口——app_id 是应用级的，三个窗口共用一个。都没有就返回
/// `None`，由调用方报错并给出下一步，绝不猜一个名字写进规则（猜错 = 规则静默失效）。
pub fn choose_app_id(windows: &[Window]) -> Option<&str> {
    let titled = |title: &str| windows.iter().find(|w| w.title == title && !w.class.is_empty());
    for title in OVERLAY_TITLES {
        if let Some(w) = titled(title) {
            return Some(&w.class);
        }
    }
    titled(SETTINGS_TITLE).map(|w| w.class.as_str())
}

// MARK: - 与 KWin / KDE 工具说话
//
// 这一节全是薄壳：判断都在上面的纯函数里。外部工具只用 Qt6 那一代的名字（本机没有
// qt5 的 `qdbus` / `kwriteconfig5`），只用 KWin 自己的 D-Bus 接口，不引入新依赖。

use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// KWin 规则文件（用户侧那份 `~/.config/kwinrulesrc`）
const RULES_FILE: &str = "kwinrulesrc";

/// 探针脚本：把 KWin 看到的窗口和浮层可用区域打到 journal。KWin 的脚本
/// `print()` 进合成器的 journal，没有别的取回通道（取法见 `docs/linux.md`）。
///
/// 窗口类排在前面、标题排在最后：标题里可能带 `|`。`resourceClass` 就是规则里
/// `wmclass` 要比的那个字符串。
const PROBE_JS: &str = r#"
var list = workspace.windowList();
for (var i = 0; i < list.length; i++) {
    var w = list[i];
    print("VKWIN|" + w.resourceClass + "|" + w.pid + "|" + w.caption);
}
var a = workspace.clientArea(KWin.PlacementArea, workspace.activeScreen, workspace.currentDesktop);
if (a) {
    print("VKAREA|" + a.x + "," + a.y + "," + a.width + "," + a.height);
}
"#;

/// 问 KWin 现在有什么：映射中的窗口（app_id 从这儿取）+ 浮层的可用区域（位置从这儿算）
pub fn probe() -> Result<Probe, String> {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("系统时间不对：{e}"))?
        .as_secs()
        .saturating_sub(1);
    let path = std::env::temp_dir().join(format!("vk-kwin-probe-{}.js", std::process::id()));
    std::fs::write(&path, PROBE_JS).map_err(|e| format!("写探针脚本 {} 失败：{e}", path.display()))?;
    let name = format!("vk-kwin-probe-{}", std::process::id());
    let ran = run_script(&path, &name);
    // 探针不留残留：成败都卸，也不写 kwinrc
    let _ = run("qdbus6", &["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", &name]);
    let _ = std::fs::remove_file(&path);
    ran?;
    // journald 落地有一点延迟，轮询到有输出为止
    for _ in 0..20 {
        let log = journal(since);
        if log.contains(WINDOW_TAG) || log.contains(AREA_TAG) {
            return Ok(parse_probe(&log));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("KWin 没在 2 秒内把探针结果写进 journal：确认本机是 KWin Wayland 会话，\
         且 `journalctl --user -u plasma-kwin_wayland` 有输出"
        .into())
}

/// 加载并运行一段 KWin 脚本。收尾（卸载脚本 + 删临时文件）留给调用方，好让失败路径也走到
fn run_script(path: &std::path::Path, name: &str) -> Result<(), String> {
    let id = run("qdbus6", &["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", &path.to_string_lossy(), name])?;
    run("qdbus6", &["org.kde.KWin", &format!("/Scripting/Script{}", id.trim()), "org.kde.kwin.Script.run"]).map(|_| ())
}

/// 安装：写规则 → 替换掉旧的失败规则 → 让 KWin 重载。重复执行结果一致。
///
/// `app_id` 由调用方从 [`probe`] 的结果里取（见 [`choose_app_id`]）。
pub fn install(app_id: &str, area: &Area) -> Result<Vec<Rule>, String> {
    if app_id.is_empty() {
        return Err("app_id 是空的：空的 `wmclass=` 会匹配到所有窗口".into());
    }
    let rules = vec![hud_rule(app_id, area), pick_rule(app_id)];
    let groups = parse_ini(&read_rules()?);
    // 先删后写：删自己的组（旧版本可能多写过键）与那条失败的手写规则
    for name in MANAGED_GROUPS.iter().chain(STALE_GROUPS) {
        delete_group(&groups, name)?;
    }
    for r in &rules {
        for (k, v) in r.entries() {
            run("kwriteconfig6", &["--file", RULES_FILE, "--group", r.group, "--key", k, v])?;
        }
    }
    let mut drop: Vec<&str> = MANAGED_GROUPS.to_vec();
    drop.extend_from_slice(STALE_GROUPS);
    write_order(&merged_order(&order(&groups), MANAGED_GROUPS, &drop))?;
    reconfigure()?;
    Ok(rules)
}

/// 上次装的那份规则里的 `app_id`：探针看不到 VoiceKey 窗口时的退路（重装应当照旧，
/// 而不是因为一时没有窗口可查就报错）。
///
/// 读自己刚写的组，不另起一个 `kreadconfig` 进程——`kwinrulesrc` 的读写都在这一个模块里。
pub fn installed_app_id() -> Option<String> {
    let groups = parse_ini(&read_rules().ok()?);
    MANAGED_GROUPS.iter().find_map(|g| wmclass_of(&groups, g))
}

/// 某个组里写的 `wmclass`（没有这个组、没这个键、或值是空串都算 None）
fn wmclass_of(groups: &[IniGroup], group: &str) -> Option<String> {
    groups
        .iter()
        .find(|g| g.name == group)
        .and_then(|g| g.value("wmclass"))
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// 卸载：只删自己装的组与 `Order` 里的自己，别人的规则（含系统级的）一个不动
pub fn uninstall() -> Result<(), String> {
    let groups = parse_ini(&read_rules()?);
    for name in MANAGED_GROUPS {
        delete_group(&groups, name)?;
    }
    write_order(&merged_order(&order(&groups), &[], MANAGED_GROUPS))?;
    reconfigure()
}

/// 用户侧 `kwinrulesrc` 的 `[General] Order`（没有就是空串）
fn order(groups: &[IniGroup]) -> String {
    groups.iter().find(|g| g.name == "General").and_then(|g| g.value("Order")).unwrap_or_default().to_string()
}

fn delete_group(groups: &[IniGroup], name: &str) -> Result<(), String> {
    let keys = groups.iter().find(|g| g.name == name).map(IniGroup::keys).unwrap_or_default();
    for k in keys {
        run("kwriteconfig6", &["--file", RULES_FILE, "--group", name, "--key", k, "--delete"])?;
    }
    Ok(())
}

/// `Order` 空了就删键，别留一个空串（`Order=` 与"没写过"不是一回事）
fn write_order(order: &str) -> Result<(), String> {
    if order.is_empty() {
        run("kwriteconfig6", &["--file", RULES_FILE, "--group", "General", "--key", "Order", "--delete"])?;
    } else {
        run("kwriteconfig6", &["--file", RULES_FILE, "--group", "General", "--key", "Order", order])?;
    }
    Ok(())
}

fn reconfigure() -> Result<(), String> {
    run("qdbus6", &["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"]).map(|_| ())
}

/// 取 journal 里 `since`（epoch 秒）之后的 KWin 输出
fn journal(since: u64) -> String {
    let since = format!("@{since}");
    let unit = run_ok("journalctl", &["--user", "-u", "plasma-kwin_wayland", "--since", &since, "--no-pager", "-q"]);
    if unit.contains(WINDOW_TAG) || unit.contains(AREA_TAG) {
        unit
    } else {
        // 单元名在不同发行版可能不一样，退回整份用户 journal
        run_ok("journalctl", &["--user", "--since", &since, "--no-pager", "-q"])
    }
}

fn run(prog: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(prog).args(args).output().map_err(|e| format!("跑不起来 {prog}：{e}"))?;
    if !out.status.success() {
        return Err(format!("{prog} 失败（{}）：{}", out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 失败当空输出（journalctl 在某些会话下会报错，不值得让探针整个失败）
fn run_ok(prog: &str, args: &[&str]) -> String {
    run(prog, args).unwrap_or_default()
}

fn rules_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("既没有 XDG_CONFIG_HOME 也没有 HOME，找不到 kwinrulesrc")?;
    Ok(home.join(RULES_FILE))
}

fn read_rules() -> Result<String, String> {
    let path = rules_path()?;
    match std::fs::read_to_string(&path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("读 {} 失败：{e}", path.display())),
    }
}

/// 浮层共有的四项之外的公共部分：匹配（app_id + 标题）+ 不上任务栏/切换器
fn overlay_rule(group: &'static str, app_id: &str, title: &str, description: &str) -> Rule {
    Rule {
        group,
        entries: vec![
            // KWin 规则管理器里显示的名字：`Description` 不写的话那一行是空的
            ("Description", description.into()),
            ("wmclass", app_id.into()),
            ("wmclassmatch", MATCH_EXACT.into()),
            ("wmclasscomplete", "false".into()),
            ("title", title.into()),
            ("titlematch", MATCH_REGEXP.into()),
            // 不抢键盘焦点：拿到焦点后注入的上屏按键会落进浮层自己，症状是"说了话字没上屏"
            ("acceptfocus", "false".into()),
            ("acceptfocusrule", FORCE.into()),
            // 关装饰：KWin 会围绕整个窗口矩形画标题栏/边框/阴影，而浮层的窗口大半是透明的。
            // 写 `decorationpolicy` 而不是老式的 `noborder`：KWin 6 加载规则时会把后者迁移成
            // 前者并回写 kwinrulesrc，于是同一个文件两次 install 之间会自己变（幂等验不了）
            ("decorationpolicy", "none".into()),
            ("decorationpolicyrule", APPLY_INITIALLY.into()),
            // 置顶：应用侧 `always_on_top` 在 Wayland 只是 hint，KWin 不认
            ("above", "true".into()),
            ("aboverule", APPLY_INITIALLY.into()),
            ("skiptaskbar", "true".into()),
            ("skiptaskbarrule", APPLY_INITIALLY.into()),
            ("skipswitcher", "true".into()),
            ("skipswitcherrule", APPLY_INITIALLY.into()),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// KWin `KWin.PlacementArea`（逻辑像素）：本机 1920×1080，顶部 44px 面板
    fn work_area(x: f64, y: f64, w: f64, h: f64) -> Area {
        Area { x, y, w, h, scale: 1.0, physical: false }
    }

    /// 四条硬要求：不抢焦点 / 关装饰 / 置顶 / 位置。位置取共享几何算出的贴底居中。
    #[test]
    fn hud_rule_says_no_focus_no_decoration_keep_above_and_position() {
        let r = hud_rule("voicekey", &work_area(0.0, 44.0, 1920.0, 1036.0));
        // 标题锚定匹配：规则只该认这一个浮层，不认 `hud2`
        assert_eq!(r.value("title"), Some("^hud$"));
        assert_eq!(r.value("acceptfocus"), Some("false"));
        // 策略值：3 = Apply Initially（窗口映射时应用一次），2 = Force。
        // `acceptfocus` 在 KWin 里是 forceRule，只认 1/2/6——那一项必须是 2
        assert_eq!(r.value("acceptfocusrule"), Some("2"));
        assert_eq!(r.value("decorationpolicy"), Some("none"));
        assert_eq!(r.value("decorationpolicyrule"), Some("3"));
        assert_eq!(r.value("above"), Some("true"));
        assert_eq!(r.value("aboverule"), Some("3"));
        assert_eq!(r.value("position"), Some("620,936"));
        assert_eq!(r.value("positionrule"), Some("3"));
    }

    /// `Description` 也要写进文件：KWin 规则管理器里那一行就是它，缺了规则没有名字
    #[test]
    fn rule_carries_a_description_for_the_kwin_ui() {
        let r = hud_rule("voicekey", &work_area(0.0, 44.0, 1920.0, 1036.0));
        assert_eq!(r.value("Description"), Some("voicekey HUD：贴底居中、置顶、不抢焦点"));
        assert!(r.ini().starts_with("[voicekey-hud]\nDescription="));
        assert_eq!(pick_rule("voicekey").value("Description"), Some("voicekey 候选面板：置顶、不抢焦点"));
    }

    /// app_id 是传入的：换个值，内容就得跟着变（写死 app_id 时这条会红）
    #[test]
    fn hud_rule_takes_the_app_id_from_its_caller() {
        let a = hud_rule("voicekey", &work_area(0.0, 0.0, 1920.0, 1080.0));
        let b = hud_rule("do.j3.voicekey", &work_area(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(a.value("wmclass"), Some("voicekey"));
        assert_eq!(b.value("wmclass"), Some("do.j3.voicekey"));
        assert_ne!(a.ini(), b.ini());
    }

    /// 位置跟着可用区域走：可用区域不从 (0,0) 开始时贴的是工作区的底，不是屏幕的底
    #[test]
    fn hud_rule_positions_against_the_work_area_not_the_screen() {
        // 底部一条 40px 面板（1920×1080 → 工作区高 1040）
        let r = hud_rule("voicekey", &work_area(0.0, 0.0, 1920.0, 1040.0));
        assert_eq!(r.value("position"), Some("620,896"));
    }

    /// pick panel 同样不许抢焦点、不许有装饰、要置顶，但**不写位置**：它的高度随候选行数
    /// 变化，而 KWin 的位置规则只在窗口映射时应用一次，静态位置要么压住输入行、要么把
    /// 长列表推出屏幕。位置交给 KWin 的默认放置。
    #[test]
    fn pick_rule_keeps_the_three_attributes_but_has_no_position() {
        let r = pick_rule("voicekey");
        assert_eq!(r.value("title"), Some("^pick$"));
        assert_eq!(r.value("acceptfocus"), Some("false"));
        assert_eq!(r.value("decorationpolicy"), Some("none"));
        assert_eq!(r.value("above"), Some("true"));
        // 策略值跟 HUD 那条一样（3 = Apply Initially；`acceptfocus` 是 forceRule 只能取 2）
        assert_eq!(r.value("aboverule"), Some("3"));
        assert_eq!(r.value("decorationpolicyrule"), Some("3"));
        assert_eq!(r.value("position"), None);
        assert_eq!(r.value("positionrule"), None);
    }

    /// 两条规则组名不同：KWin 的规则管理器里各占一行，卸载时按组名删
    #[test]
    fn hud_and_pick_are_separate_groups() {
        let h = hud_rule("voicekey", &work_area(0.0, 0.0, 1920.0, 1080.0));
        let p = pick_rule("voicekey");
        assert_ne!(h.group, p.group);
        assert_eq!(MANAGED_GROUPS, &[h.group, p.group]);
    }

    /// 探针输出经 journald 中转，每行前面是 journal 自己的时间戳/单元名前缀；
    /// 标题可能含 `|`（浏览器标题就有），所以标题排在行的最后一段。
    #[test]
    fn parses_windows_out_of_a_journal_dump() {
        let dump = "\
10月 17 14:11:37 archlinux kwin_wayland[1012]: VKWIN|com.mitchellh.ghostty|104144|◐ Claude Code
10月 17 14:11:37 archlinux kwin_wayland[1012]: VKWIN|voicekey|192221|hud
10月 17 14:11:37 archlinux kwin_wayland[1012]: 别的行
10月 17 14:11:38 archlinux kwin_wayland[1012]: VKAREA|0,44,1920,1036
";
        let p = parse_probe(dump);
        assert_eq!(
            p.windows,
            vec![
                Window { class: "com.mitchellh.ghostty".into(), pid: 104144, title: "◐ Claude Code".into() },
                Window { class: "voicekey".into(), pid: 192221, title: "hud".into() },
            ]
        );
        assert_eq!(p.area, Some(work_area(0.0, 44.0, 1920.0, 1036.0)));
    }

    /// 窗口类的 app_id、标题里的 `|`、非数字的 pid：都不许解析成假数据或 panic
    #[test]
    fn tolerates_odd_probe_lines() {
        let dump = "\
VKWIN|do.j3.voicekey|1|标题 | 带竖线
VKWIN|voicekey|?
VKAREA|garbage
VKWIN|voicekey|7|pick
";
        let p = parse_probe(dump);
        assert_eq!(p.area, None);
        assert_eq!(
            p.windows,
            vec![
                Window { class: "do.j3.voicekey".into(), pid: 1, title: "标题 | 带竖线".into() },
                Window { class: "voicekey".into(), pid: 7, title: "pick".into() },
            ]
        );
    }

    /// app_id 从浮层窗口本身取——规则要匹配的就是它们。HUD 优先，其次 pick。
    #[test]
    fn takes_the_app_id_from_the_overlay_windows() {
        let w = |class: &str, title: &str| Window { class: class.into(), pid: 1, title: title.into() };
        let other = w("com.mitchellh.ghostty", "◐ Claude Code");
        assert_eq!(choose_app_id(&[other.clone(), w("voicekey", "hud"), w("voicekey", "pick")]), Some("voicekey"));
        assert_eq!(choose_app_id(&[other.clone(), w("do.j3.voicekey", "pick")]), Some("do.j3.voicekey"));
    }

    /// 浮层此刻没映射（HUD 没弹、pick 没开）时退到设置窗口：app_id 是应用级的，
    /// 三个窗口共用一个。都没有就如实返回 None，由调用方报错，别瞎猜。
    #[test]
    fn falls_back_to_the_settings_window_then_gives_up() {
        let settings = Window { class: "voicekey".into(), pid: 1, title: "VoiceKey 设置".into() };
        let other = Window { class: "magpie".into(), pid: 2, title: "magpie".into() };
        assert_eq!(choose_app_id(&[other.clone(), settings]), Some("voicekey"));
        assert_eq!(choose_app_id(&[other]), None);
        assert_eq!(choose_app_id(&[]), None);
    }

    /// 类为空的窗口不能选：`wmclass=` 空值会匹配到**所有**窗口，比不装规则还糟。
    /// KWin 的脚本 API 对拿不到 app_id 的窗口就给空类（KCM 里会提示"窗口类不可用"）。
    #[test]
    fn never_takes_an_empty_app_id() {
        let blank = Window { class: "".into(), pid: 1, title: "hud".into() };
        assert_eq!(choose_app_id(&[blank]), None);
    }

    /// 安装器要读用户侧那份 `kwinrulesrc`：`Order` 的原值、以及自己那几个组里已有的键
    /// （先删后写，重复执行才不会留下上一版规则的残键）。原文就长这样。
    #[test]
    fn reads_groups_keys_and_order_out_of_kwinrulesrc() {
        let text = "\
[General]
Order=steam,voicekey-overlay

[voicekey-overlay]
Description=voicekey 浮层：不抢焦点、无边框、贴底居中
acceptfocus=false
position=620,936

[steam]
wmclass=steamwebhelper steam
wmclassmatch=1
";
        let groups = parse_ini(text);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].name, "General");
        assert_eq!(groups[0].value("Order"), Some("steam,voicekey-overlay"));
        assert_eq!(groups[1].name, "voicekey-overlay");
        assert_eq!(groups[1].keys(), ["Description", "acceptfocus", "position"]);
        // 值里可以有空格，且只按第一个 `=` 切
        assert_eq!(groups[2].keys(), ["wmclass", "wmclassmatch"]);
        assert_eq!(groups[2].value("wmclass"), Some("steamwebhelper steam"));
    }

    /// 装第二遍时若 KWin 里看不到 VoiceKey 窗口，退回上次装的 `app_id`——从自己写的组里读
    #[test]
    fn falls_back_to_the_wmclass_we_wrote_last_time() {
        let text = hud_rule("voicekey", &work_area(0.0, 0.0, 1920.0, 1080.0)).ini();
        assert_eq!(wmclass_of(&parse_ini(&text), HUD_GROUP), Some("voicekey".into()));

        // 组不在、键不在、值是空串，都当作"没装过"
        assert_eq!(wmclass_of(&parse_ini(&text), PICK_GROUP), None);
        assert_eq!(wmclass_of(&parse_ini("[General]\nOrder=\n"), HUD_GROUP), None);
        assert_eq!(wmclass_of(&parse_ini("[voicekey-hud]\nwmclass=\n"), HUD_GROUP), None);

        // hud 组被手删了，还有 pick 组兜着（两条规则的 wmclass 是同一个）
        let id = |text: &str| MANAGED_GROUPS.iter().find_map(|g| wmclass_of(&parse_ini(text), g));
        assert_eq!(id(&pick_rule("voicekey").ini()), Some("voicekey".into()));
        assert_eq!(id(""), None);
    }

    /// `[General] Order` 是 kwinrulesrc 的规则清单顺序。装的时候：别的规则一个不动
    /// （系统级的 steam 规则就是这样在 `/etc/xdg` 里的）、旧的失败规则除名、自己的排最后；
    /// 重复执行不许长出重复项（幂等）。
    #[test]
    fn order_keeps_other_rules_drops_the_stale_one_and_is_idempotent() {
        let once = merged_order("steam,voicekey-overlay", MANAGED_GROUPS, STALE_GROUPS);
        assert_eq!(once, "steam,voicekey-hud,voicekey-pick");
        assert_eq!(merged_order(&once, MANAGED_GROUPS, STALE_GROUPS), once);
        assert_eq!(merged_order("", MANAGED_GROUPS, STALE_GROUPS), "voicekey-hud,voicekey-pick");
        assert_eq!(
            merged_order("steam,my-rule", MANAGED_GROUPS, STALE_GROUPS),
            "steam,my-rule,voicekey-hud,voicekey-pick"
        );
        // 卸载：只除自己的名，旧规则/别人的规则都不动（旧规则该由安装器替换，不是卸载器删）
        assert_eq!(merged_order(&once, &[], MANAGED_GROUPS), "steam");
    }
}
