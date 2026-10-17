# Linux 桌面版

`crates/platform` 的 Linux 实现：用内核原语（evdev / uinput）替代 CGEventTap 与 WH_KEYBOARD_LL，因而不依赖合成器 API，X11 与 Wayland 通用。目前的目标环境是 KDE Plasma 6.8 / KWin 6.8（Wayland，[ADR 0001](adr/0001-linux-targets-kwin-wayland.md)）；早先在 Arch + niri（Wayland）跑通并打包的记录见文末，已不代表当前环境。

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

## 浮层与 KWin 窗口规则

目标环境是 KDE Plasma 6 / KWin 6（Wayland）（[ADR 0001](adr/0001-linux-targets-kwin-wayland.md)）。
`hud` / `pick`（[术语](GLOSSARY.md)：HUD / Pick panel）是 `680×120` / `520×N` 的**透明顶层
窗口**，内容只占其中一小块（胶囊浮条、候选列表），其余全透明。macOS / Windows 下窗口无边框
无阴影，看不出窗口本体；但在 Wayland 下有三件事客户端无能为力，只有合成器能做：

1. **边框与阴影**：合成器会围绕**整个窗口矩形**画焦点环 / 阴影。窗口一大半是空白的，那圈
   环就把空白也描了出来，看上去是"一整块 tile"——不是 HUD 变大了，是窗口本体被描了边。
2. **定位**：Wayland 没有"客户端自己摆窗口"的协议，`set_position`（GTK 的 `gtk_window_move`）
   在 Wayland 上是空操作；KWin 会把浮窗**居中**，而不是贴底居中。
3. **置顶与焦点**：`always_on_top` 在 Wayland 只是 hint，别的窗口能盖住它；`focused(false)`
   只是"请别给我焦点"，合成器可以不理——HUD 一打开就把键盘焦点抢走，正在打的字当场就断了。

于是 Linux 上必须给 KWin 加窗口规则，规则由 `vk-kwin` 装（理由见
[ADR 0002](adr/0002-overlay-rules-over-client-positioning.md)）：

```bash
cargo run -p voicekey-platform --bin vk-kwin -- probe      # 看 KWin 里有哪些窗口、app_id 会取哪个
cargo run -p voicekey-platform --bin vk-kwin -- install    # 装（幂等）
cargo run -p voicekey-platform --bin vk-kwin -- uninstall  # 卸（只删自己装的组）
```

规则生成是 `crates/platform/src/kwin.rs` 里的纯函数，摆放几何与应用共用 `overlay.rs` 那一份，
两边不会漂移。

### 规则内容

`~/.config/kwinrulesrc` 里两个组，`voicekey-hud` 与 `voicekey-pick`：

| 规则项 | KWin 键 | 值 |
|---|---|---|
| 匹配应用 | `wmclass` + `wmclassmatch=1`（精确）+ `wmclasscomplete=false` | KWin 报的 `resourceClass` |
| 只匹配浮层 | `title` + `titlematch=3`（正则） | `^hud$` / `^pick$` |
| 不抢焦点 | `acceptfocus=false`、`acceptfocusrule=2`（Force） | 这一项 KWin 只收 Force 类策略，见下 |
| 关装饰（含阴影） | `decorationpolicy=none`、`decorationpolicyrule=3` | 键名是 `decorationpolicy`，**不是**老式的 `noborder` |
| 置顶 | `above=true`、`aboverule=3` | 键名是 `above`，**不是** `keepabove` |
| 位置 | `position=x,y`、`positionrule=3` | 只有 `voicekey-hud` 有，取自 `overlay.rs` 的贴底居中 |

每条还写一个 `Description`——KWin 规则管理器里那一行显示的就是它，不写就是个无名规则。

`*rule` 写的是 KWin `Rules` 那个匿名枚举的**数值**：`Unused=0, DontAffect=1, Force=2,
Apply=3, Remember=4, ApplyNow=5, ForceTemporarily=6`。规则管理器下拉里的 "Apply Initially"
是 **3**——窗口映射那一刻应用一次；**2 是 Force**（持续强制），别按字面把它当成 Apply。
`position` / `above` / `decorationpolicy` / `skiptaskbar` / `skipswitcher` 都是 `SetRule`，
3 与 2 各自都生效，我们取 3（映射时应用一次，之后窗口就是它自己的位置了）。

例外是不抢焦点：`acceptfocus` 在 KWin 里是 `forceRule`，策略只认 `DontAffect` / `Force` /
`ForceTemporarily`（1/2/6），所以那一项固定 `acceptfocusrule=2`——把"不抢焦点"钉死。

**`position` 只在窗口映射那一刻应用**，而候选面板的高度随候选行数变——写死一个 `y`，短列表
悬在半空、长列表顶出屏幕，所以 pick 那条不设位置。
`[General] Order` 只追加自己这两个组名，别人的规则（包括系统 `/etc/xdg/kwinrulesrc` 里的）
一概不动。

**键名要用 KWin 6 的**，这两处都踩过：

- 关装饰的老键名是 `noborder`，KWin 6 加载时把它迁移成 `decorationpolicy` 并**回写
  `kwinrulesrc`**——写老键名同样生效，但装完十几秒后文件自己变了一次，逐字节的幂等检查
  就成了假警报。
- 置顶没有别名：`keepabove` 不是 KWin 的键，写了也不报错，就是**静默不生效**（窗口的
  `keepAbove` 一直是 false），得用 `above`。

### app_id 从 KWin 查得，不猜

规则的 `wmclass` 必须**逐字等于 KWin 报的 `resourceClass`**，猜错（比如拿进程名当 `app_id`）
规则会**静默地不匹配**，看不出任何异常——旧的手写 `voicekey-overlay` 规则就是这么失效的，
安装器会把它替换掉。所以安装时不猜，而是跑一段 KWin 脚本把窗口列出来（`print` 进 journal，
探针解析 `VKWIN|` 行），按标题取：`hud` → `pick` → 设置窗口 `VoiceKey 设置`；一个都取不到就
退回上次装的 `app_id` 并**打印提示**（不默默换个猜的值）。`--app-id <id>` 可以手动指定。

同理，规则匹配的标题（`^hud$` / `^pick$`）就是 `ui.rs` 里浮层窗口的 label（`overlay()` 直接
拿 label 当标题），改名字要两边一起改——规则按 `wmclass` + `title` 两个条件匹配，任何一个
对不上都是"规则看起来装了，其实没生效"。

`probe` 是只读的，装之前先看它认出了什么：

```
$ vk-kwin probe
可用区域（KWin.PlacementArea，逻辑像素）：0,44 1920x1036
KWin 看到的窗口（app_id = resourceClass）：
  voicekey                         pid=204544   VoiceKey 设置
  ...
据此会用的 app_id：voicekey
```

### 装了之后

- 新映射的浮层**立刻**生效。重载那一刻**已经开着**的窗口也会被重新评估（实测 `above` /
  不抢焦点当场生效），但它的窗口尺寸要等自己 resize 才更新——位置与装饰以"装完之后新开的
  浮层"为准。
- 换分辨率 / 接外接屏 / 改工作区尺寸后位置会不对：重跑一次 `install`——`position` 是死坐标，
  KWin 不会替你重算（[ADR 0002](adr/0002-overlay-rules-over-client-positioning.md)）。
- 卸载只删 `voicekey-hud` / `voicekey-pick` 两个组和它们在 `[General] Order` 里的引用。

更彻底的路子是 wlr-layer-shell 表面（`tauri::WebviewWindow::gtk_window()` 拿到 GtkWindow，
配 gtk-layer-shell）：合成器根本不把它当普通窗口，也就无所谓规则。但那要另写一条窗口创建
路径，不在本轮范围内。

## 本机验收手法

本机（KWin Wayland）**没有** `xdotool` / `kdotool`，客户端也拿不到窗口几何；niri 那套
（`niri msg windows`、`grim`）在此一律无效。可用的是下面四件。

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

### 合成按键 — uinput 注入

要触发热键（`hud` 只有在按住热键时才映射）而手上没有真键盘可按时，自己造一把 uinput 键盘：
写 `/dev/uinput`，`UI_DEV_SETUP` 里名字**不能**叫 `voicekey-virtual`（那是应用自己的虚拟键盘，
它的钩子会跳过），`UI_SET_KEYBIT` 要把字母键一起声明上（钩子按「有没有 KEY_A / KEY_ESC /
KEY_ENTER」判断是不是键盘）。建设备后等 3 秒——应用的钩子每 2 秒扫一次 `/dev/input`。

**坑**：写完 `EV_KEY` 必须再写一个 `EV_SYN` / `SYN_REPORT`（type 0, code 0, value 0），否则内核
不把这一包交给任何 evdev 读者：应用收不到、KWin 也收不到，KWin 脚本里什么都看不到，而
python-evdev 那边同样静默。少了这一个 SYN，整件事看上去就像"钩子坏了"。

### mac / Windows 编译回归在本机能验到哪

每条 KDE ticket 都带一条「macOS / Windows 仍能编译」，本机（Linux）能给出的证据只有一半：

- **能**：`cargo check -p voicekey-platform --target x86_64-apple-darwin` 与 `--target
  x86_64-pc-windows-msvc` 都跑通（各 10–20s）。所以改 `crates/platform` 时，可以机器确认
  「没把 mac/win 编坏」。
- **不能**：整包 `cargo check -p voicekey --target …`。它挂在**依赖的 build script** 上，与改动
  无关——Windows 侧 `ring` 要 MSVC 的 `lib.exe`，macOS 侧 `objc2-exception-helper` 要 Apple SDK
  的 clang。改动落在 `ui.rs` 这类 app 侧时，能给的只有一条论证：改的是与旧 `cfg!` 同型、类型中立
  的分支。最终结论留给 `release.yml`——它的 matrix 才是真跑 mac/win 的地方。

缺的是原生 SDK，所以本机不必装交叉工具链，装也不解决 app 侧那个 build script。

### 判定时的五个坑

- **先看文本再截图**：窗口 dump、`GetLayout`、journal 能回答的先问它们；确认有东西可看
  才落截图，贴图很贵。
- **窗口刚打开就 dump / 截图会扑空**：先 `sleep 3` 或重采一次。
- **直接跑 `target/debug/voicekey` 时 webview 是空的**：dev 构建加载 `devUrl`
  （`http://localhost:1420`），没起前端服务就加载失败，HUD 里什么都没有——而且此时 KWin 报的
  尺寸**不是浮层真实尺寸**（实测 184 高，是 WebKit 空白页的 min-size 被 `resizable(false)`
  钉死；页面正常加载时才是 `680×120`）。量几何前先确认页面在渲染：`python3 -m http.server
  1420 -d app/dist` 或直接 `npx tauri dev`。
- **`settings.json` 的 mtime 不是「设置被改过」的证据**：任何一次保存都重写文件，哪怕值
  逐字节不变。要比内容，就 `sha256sum` / `diff`（成因见 #6）。
- **把 cargo 的输出管进 `tail` / `grep` 会吃掉退出码**：`cargo check … | tail` 失败时也返回 0，
  后台任务里就显示成 `exited with code 0`。认输出里的 `Finished` / `error` 字样，或用
  `set -o pipefail`。

## 渠道

| 渠道 | Linux |
|---|---|
| 云端（豆包 / 微信 / 千问 / 百度 / 搜狗 / 讯飞） | 可用 |
| 微信离线 | 可用 |
| 豆包离线 | 不可用（仅 macOS arm64） |

## 验证记录

### KDE Plasma 6.8 / KWin 6.8（当前环境）

- `cargo check --workspace` 无警告；`cargo test --workspace` 全绿（`crates/platform` 19 项：
  浮层几何、规则生成、探针解析、INI 读写、`Order` 合并）；mac / Windows 的编译回归：
  `cargo check -p voicekey-platform --target x86_64-apple-darwin` 与 `--target
  x86_64-pc-windows-msvc` 均通过
- `vk-kwin probe` 在真应用上认出了 `app_id=voicekey`（`resourceClass`，标题 `VoiceKey 设置`），
  与「进程名猜 `app_id`」是两回事
- `vk-kwin install` 幂等：连跑三次，`~/.config/kwinrulesrc` 逐字节相同（sha256 比对），
  之后静置 20 秒也没被谁改写。**这条是踩出来的**：规则里原写老式的 `noborder=true`，装完
  十几秒后 KWin 会把它迁移成 `decorationpolicy=none` 并回写文件，于是"同一个文件两次
  install 之间自己变了"。改成 KWin 6 的键名后文件才稳定（迁移与改写只发生在我们这边有
  旧键时）
- 旧的手写 `voicekey-overlay` 规则被安装器替换掉；`Order` 里别人的规则（本机是 `steam`）
  原样保留；`uninstall` 后只剩 `Order=steam` 与系统 `/etc/xdg/kwinrulesrc` 里的 `steam` 规则
- 策略值复验：`*rule` 由 `2` 改成 `3`（Apply Initially；`acceptfocusrule` 因 KWin 只收
  forceRule 保持 `2`）后重装，文件仍是逐字节幂等（sha256 `a80e94c0…`），`positionrule=3`
  下新映射的 HUD 依旧落在 `620,936`——改的是语义标签，行为不变
- **真应用 + 注入的按住右 Alt**（合成按键手法见上，且 webview 要有前端在渲染，见上表），
  KWin 脚本读回来的装上规则后的 HUD：
  `geom=620,936 680x120`（贴底居中，窗口就是内容那 `680×120`，没有装饰帧）、
  `active=false`、`wantsInput=false`、`keepAbove=true`、`noBorder=true`、`skipTaskbar=true`、
  `layer=3`，而前台窗口仍是 Ghostty（键盘焦点没被抢走）。同一窗口**未装规则**时是
  `geom=620,502`（屏幕正中——`overlay.rs` 算的 `620,936` 被 Wayland 忽略）、`active=true`、
  `wantsInput=true`、`keepAbove=false`、`noBorder=false`、`layer=2`
- 截图确认（不是只看 dump）：装上规则后按住热键，胶囊贴在屏幕底部居中、离底 24px，四周
  没有标题栏 / 边框 / 阴影，底下终端的状态行照常露着——这三件事确实是合成器做的
- **装饰**这一项对照得最清楚的是替身窗口（`app_id=voicekey`、标题 `hud`、请求 `680×120`
  的 GTK 窗口）：无规则时 KWin 给它套上装饰帧，实际 `688×158` 落在屏幕正中 `616,483`、
  `active=true`、`keepAbove=false`、`noBorder=false`、`layer=2`；装上规则后新映射一次，
  就是 `680×120`、`620,936`、`active=false`、`noBorder=true`、`layer=3`。真应用的 HUD
  本来就 `decorations(false)`，KWin 两种情况下都没给它加帧（都是 `680×120`），所以只拿它
  对照位置与焦点，装饰看替身
- pick 那条规则用一个 `app_id=voicekey title=pick` 的替身窗口复验：`geom=700,362 520x400`
  （无装饰、KWin 居中）、`active=false`、`layer=3`
- 装/卸顺序也各验了一次：`uninstall` 之后新映射的浮层立刻退回"居中 + 抢焦点"（真应用 HUD
  `620,502`、`active=true`），`install` 之后新映射的立刻拿到规则
- 未验证：**pick 面板本体**。它只在「全部渠道」模式下出现，而本轮不动用户设置
  （`settings.json`），所以只验到规则对同 `app_id` 同标题的窗口生效这一步。

### Arch + niri（历史，[ADR 0001](adr/0001-linux-targets-kwin-wayland.md) 之后不再代表当前环境）

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
