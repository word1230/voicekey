# Linux 桌面版

`crates/platform` 的 Linux 实现：用内核原语（evdev / uinput）替代 CGEventTap 与 WH_KEYBOARD_LL，因而不依赖合成器 API，X11 与 Wayland 通用。已在 Arch + niri（Wayland）跑通并打包。

## 跑起来

权限（一次性，两者缺一不可）：

- 读全局按键：能读 `/dev/input/event*` —— 用户加入 `input` 组
- 注入上屏：`/dev/uinput` 可写 —— 自建 udev 规则把用户加入可写名单（`input` 组不含它）

构建：

```bash
cd app
npm ci --allow-remote=all                 # 见下方 gotcha
npx tauri dev                             # 开发运行
npx tauri build --bundles appimage,deb    # 打包
```

完成判据：打包产物出现在 `target/release/bundle/{appimage,deb}/`。

**Gotcha — npm**：全局设了 `allow-remote = none` 时（本机如此），而 lockfile 指向 `registry.npmmirror.com`，`npm ci` 报 `EALLOWREMOTE`。用 `--allow-remote=all` 覆盖。

## 平台层

三平台各有 `crates/platform/src/{mac,win,linux}.rs`，`lib.rs` 按 `cfg(target_os)` 选 `imp`。增删接口时三者同步，接口以 `mac.rs` 为准。

Linux 侧的机制：

| 能力 | 手段 |
|---|---|
| 全局按键 | evdev 读 `/dev/input/event*`（多读不独占） |
| 上屏 | `/dev/uinput` 虚拟键盘 + 剪贴板（`wl-copy` / `xclip`）+ Ctrl+V |
| 光标位置、前台应用 | 恒 `None`（Wayland 无全局 API） |

## 吞键差异

Linux 不独占设备，因此**不吞键**：钩子只观察、不拦截。这一条决定了下面几处分支，改前先看理由：

| 站点 | 为什么 |
|---|---|
| `ui.rs` 跳过 `set_ignore_cursor_events` | Wayland 下窗口未映射时 gdk window 为空，tao 会 panic |
| `controller.rs` 关闭流式 Typer | uinput 无法逐字打中文 → 结束时一次性粘贴 |
| `settings.rs` 点按默认键取 evdev `97` | Windows 的 VK `0xA3` 在 evdev 里是别的键 |
| `main.rs`：`xdg-open`、`platform = "linux"` | 打开链接与前端平台标识 |

不吞键带来的后果与应对：

| 后果 | 应对 |
|---|---|
| 组合键点按快捷键会同时传给前台应用 | 点按用单修饰键 |
| 候选面板打开时数字 / 方向键漏给前台应用 | 用单渠道模式 |
| 候选框无法贴光标（Wayland 无 API） | 浮层交给合成器贴底居中，见下节 |

## 浮层与合成器窗口规则

> **已失效**：Linux 浮层目标已改为 KWin Wayland（见 [`docs/adr/0001`](adr/0001-linux-targets-kwin-wayland.md)），
> 本节下面的 niri 规则**在本机无效**，KWin 规则与安装方式见 #4。

`hud` / `pick` 是 `680×120` / `520×N` 的**透明顶层窗口**，内容只占其中一小块（胶囊浮
条、候选列表），其余全透明。macOS / Windows 下窗口无边框无阴影，看不出窗口本体；但
Wayland 下有两件事只有合成器能做，客户端无能为力：

1. **边框与阴影**：合成器会围绕**整个窗口矩形**画焦点环 / 阴影。窗口一大半是空白的，
   那圈环就把空白也描了出来，看上去是"一整块 tile"——不是 HUD 变大了，是窗口本体被
   描了边。niri 默认还会在窗口打开时把焦点给它，焦点环用高亮色（本机 `#8a5023`，
   3px），尤其显眼。
2. **定位**：Wayland 没有"客户端自己摆窗口"的协议，`set_position`（GTK 的
   `gtk_window_move`）在 Wayland 上是空操作。niri 会把浮窗**居中**，而不是贴底居中。

于是 Linux 上必须给合成器加一条窗口规则。niri（`~/.config/niri/config.kdl`）：

```kdl
window-rule {
    match app-id="voicekey" title=r"^(hud|pick)$"
    open-floating true       // 别进平铺布局
    open-focused false       // 打开时不抢焦点
    border { off }
    focus-ring { off }       // 不描整块窗口的内环
    shadow { off }           // 不投阴影
    default-floating-position x=0 y=24 relative-to="bottom"  // 贴底居中，离底 24px
}
```

`app-id` 是**进程名**（release 与 `tauri dev` 都是 `voicekey`），不是 bundle id —— tauri
默认没开 `enableGTKAppId`。`title=r"^(hud|pick)$"` 把浮层和设置窗口（`VoiceKey 设置`）区分开。

更彻底的做法是把这两个窗口做成 wlr-layer-shell 表面（`tauri::WebviewWindow::gtk_window()`
能拿到 GtkWindow，配合 gtk-layer-shell），那样合成器根本不当作普通窗口，也就无所谓规则；
但目前只在 niri 上验证过规则这条路径。

## 本机验收手法

本机（KWin Wayland）**没有** `xdotool` / `kdotool`，客户端也拿不到窗口几何。本文档里记的
niri 手法（`niri msg windows`、`grim`）在此一律无效，可用的是下面三件。

### 窗口信息 — KWin 脚本

KWin 内建 JS 引擎，脚本跑在合成器里，比任何外部工具都权威：

```bash
cat > /tmp/vkprobe/dump.js <<'EOF'
var list = workspace.windowList();
for (var i = 0; i < list.length; i++) {
    var w = list[i];
    print("VKWIN|" + w.resourceClass + "|" + w.caption + "|" +
        w.frameGeometry.x + "," + w.frameGeometry.y + " " +
        w.frameGeometry.width + "x" + w.frameGeometry.height +
        "|active=" + w.active + "|keepAbove=" + w.keepAbove +
        "|skipTaskbar=" + w.skipTaskbar);
}
EOF
ID=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript /tmp/vkprobe/dump.js vk-dump)
qdbus6 org.kde.KWin /Scripting/Script$ID org.kde.kwin.Script.run
journalctl --user -u plasma-kwin_wayland -n 20 --no-pager | grep VKWIN
```

`print()` 的输出进合成器的 journal，不在 stdout。收尾 `qdbus6 org.kde.KWin /Scripting
org.kde.kwin.Scripting.unloadScript vk-dump`（`... isScriptLoaded vk-dump` 返回 `false`
即卸净；探针**不写** `kwinrc`，不留残留）。

### 托盘菜单 — dbusmenu

托盘走 StatusNotifierItem，菜单是 `com.canonical.dbusmenu`，可以直接发点击事件，不必真去点屏幕：

```bash
BUS=$(busctl --user list | awk '$2==<pid> && $1 ~ /^:1\./ {print $1}')
gdbus call -e -d $BUS -o /org/ayatana/NotificationItem/tray_icon_tray_app_main/Menu \
  -m com.canonical.dbusmenu.GetLayout -- 0 -1 "[]"
gdbus call -e -d $BUS -o /org/ayatana/NotificationItem/tray_icon_tray_app_main/Menu \
  -m com.canonical.dbusmenu.Event <id> "clicked" "<''>" 0
```

两个坑：`<id>` 会随 `refresh_tray` 重建菜单而变，**每次点击前重新 `GetLayout` 取**
（`grep -o "[0-9]*, {'label': <'退出 VoiceKey'>}"`）；variant 参数必须写 `"<''>"`，
`"<@v ''>"` 报错，末尾那个时间戳参数不能省。

### 截图 — spectacle

`grim` 在 KWin 上不出图，用 `spectacle -b -n -f -o /tmp/x.png`，再 `magick` 裁剪 / 缩放。

### 判定时的三个坑

- **先看文本再截图**：窗口 dump、`GetLayout`、journal 能回答的先问它们；确认有东西可看
  才落截图，贴图很贵。
- **窗口刚打开就 dump / 截图会扑空**：先 `sleep 3` 或重采一次。
- **`settings.json` 的 mtime 不是「设置被改过」的证据**：任何一次保存都重写文件，哪怕值
  逐字节不变。要比内容，就 `sha256sum` / `diff`（成因见 #6）。

## 渠道

| 渠道 | Linux |
|---|---|
| 云端（豆包 / 微信 / 千问 / 百度 / 搜狗 / 讯飞） | 可用 |
| 微信离线 | 可用 |
| 豆包离线 | 不可用（仅 macOS arm64） |

## 验证记录

Arch + niri 实跑通过：

- `cargo check --workspace` 无警告
- evdev 钩子 + uinput 注入闭环：注入的按键被钩子收到，修饰键状态正确
- 按住右 Alt → 录音启动、HUD 弹出（胶囊 + 实时文字）
- `niri msg windows`：`hud` 窗口 `680×120`、`is_floating=true`；未加窗口规则时位于
  `(620,480)`，即屏幕正中——`set_position` 请求的 `(620,936)` 被合成器忽略
- 加窗口规则后用一个 `app-id=voicekey title=hud` 的替身窗口复验：落在 `(620,936)`
  （贴底居中、离底 24px）、`is_focused=false`；`grim` 截图比对，聚焦状态下窗口四周
  无 `#8a5023` 焦点环像素（未加规则的对照窗口有 3px 环）
- AppImage 实跑无崩溃，热键同样触发 HUD
- `deb`（5.7 MB）与 `AppImage`（102 MB）均产出

未验证：真说话 → 识别 → 上屏 的完整链路，需麦克风输入。
