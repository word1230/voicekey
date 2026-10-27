# Linux 桌面版

`crates/platform` 的 Linux 实现：用内核原语（evdev / uinput）替代 CGEventTap 与 WH_KEYBOARD_LL，因而不依赖合成器 API，X11 与 Wayland 通用。目前的目标环境是 KDE Plasma 6.8 / KWin 6.8（Wayland，[ADR 0001](adr/0001-linux-targets-kwin-wayland.md)）；早先在 Arch + niri（Wayland）跑通并打包的记录见文末，已不代表当前环境。

## 跑起来

先备齐两项系统权限（见下节「[权限](#权限)」），再构建：

```bash
cd app
npm ci --allow-remote=all                 # 见下方 gotcha
npx tauri dev                             # 开发运行
npx tauri build --bundles appimage,deb    # 打包
```

完成判据：打包产物出现在 `target/release/bundle/{appimage,deb}/`。

**Gotcha — npm**：全局设了 `allow-remote = none` 时（本机如此），而 lockfile 指向 `registry.npmmirror.com`，`npm ci` 报 `EALLOWREMOTE`。用 `--allow-remote=all` 覆盖。

## 权限

两项，各管一件事，互相独立：

| 要做什么 | 需要什么 | 怎么给 |
|---|---|---|
| 读全局按键（长按 / 点按说话键） | 能读 `/dev/input/event*` | 用户加入 `input` 组（本机已如此，保持现状） |
| 上屏（把字写进前台应用） | `/dev/uinput` 可写 | 装仓库自带的 udev 规则，见下 |

**`input` 组不含 `/dev/uinput`**（它是 `misc` 子系统的 `root:root 0660`），所以加组只解决读按键、
不解决写入。`/dev/uinput` 不可写时上屏是**静默失败**：按住说话键、说话、松开，什么都不会发生，
也没有任何报错——两项权限缺哪一项，症状都是"说了话，字没上屏"。

### 上屏：/dev/uinput 与仓库自带的 udev 规则

```bash
sudo install -m 644 Scripts/udev/60-voicekey-uinput.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo modprobe -r uinput && sudo modprobe uinput
```

规则只有一行（文件里的注释写了理由）：

```
KERNEL=="uinput", SUBSYSTEM=="misc", TAG+="uaccess"
```

`TAG+="uaccess"` 让 systemd-logind 给**当前座席（seat）的登录用户**挂一条 ACL——`getfacl
/dev/uinput` 里那条 `user:你:rw-` 就是它，与用户属于哪个组无关。所以**不要**为了上屏把用户加进
`input` 组。装到 `/etc/udev/rules.d/` 而不是 `/usr/lib/udev/rules.d/`：前者是本地管理员的规则，
升级软件包不会覆盖。

**文件名开头的 `60` 不能改大。** udev 按文件名顺序处理规则，而真正把 `uaccess` 标签变成 ACL 的是
系统自带的 `73-seat-late.rules` 里那句 `RUN{builtin}+="uaccess"`。排到 73 之后（比如 `99-`）标签
加得太晚：`udevadm info --name=/dev/uinput` 里 `TAGS=:uaccess:` 明明有，ACL 却不会出现，上屏照样
不行。**这条是踩出来的**——第一版叫 `99-voicekey-uinput.rules`，装上后 `vk-inject` 一直报不可写。
Steam 那条能生效，也正是因为它是 `60-`。

**装完要让设备节点重新创建一次**（`modprobe -r` 再 `modprobe`）：logind 只在节点出现时算 ACL，
对**已经存在**的节点跑 `udevadm trigger --name-match=/dev/uinput` 不会让它补上（实测 ACL 原样）。
不想卸模块的话，重新登录也等价。

**为什么不用 Steam 那条**：本机此前能写 `/dev/uinput`，靠的是 Steam 装的
`/usr/lib/udev/rules.d/60-steam-input.rules`（内容是同一句 `TAG+="uaccess"`）。那跟 VoiceKey
毫无关系，卸掉 Steam 就没了。仓库自带这条之后，上屏不再依赖别的软件；两条都在也不冲突（同一个
tag 加两遍，文件名相同前缀，谁先谁后都生效）。

两种失败，修法不同：

| `/dev/uinput` | 说明什么 | 修法 |
|---|---|---|
| 可写 | —— | 无 |
| 不可写 | 权限没给到这个用户 | 装上面那条 udev 规则，然后让节点重建（`modprobe -r`/`modprobe`）或重新登录 |
| 不存在 | uinput 没加载 | `sudo modprobe uinput`；让它开机加载：`echo uinput \| sudo tee /etc/modules-load.d/uinput.conf` |

**怎么看现在是哪种**：`vk-inject`——退出码 0 就是能上屏，非 0 把原因与修法打出来（写进脚本、
写进验收清单都方便）。`vk-inject paste <文字>` 真走一次上屏链路（剪贴板 + uinput 的 Ctrl+V，
与说完话后那一下同一条路），所以**不必真说话**就能验上屏这一段。应用启动时也自己查一遍
（`perms.injectBlocked`）：不成立就把设置窗口开出来，顶部顶一条「现在无法上屏」+ 修法，侧栏那行从
「就绪 · 长按 X 说话」变成「现在无法上屏」——**特意查在启动时**，因为这种失败本身不给任何
提示，不主动说就等于没说。

`vk-inject paste` 打进去的字要落进**当前有键盘焦点的窗口**：先点一下目标输入框再跑。它和真应用
一样把文字先写进剪贴板再发 Ctrl+V——所以剪贴板里原来的东西会被覆盖（Linux 上不恢复，见
`linux.rs::paste`）。验证时用一个一次性的 GTK 窗口 + `Gtk.Entry` 当靶子，别拿自己正在用的编辑器。

### 虚拟键盘建好后要等一小会儿才发键

`uinput` 设备建起来到合成器打开它的事件节点之间有一小段：这段时间里发出去的按键**没有读者、
内核直接丢掉**，而 `evdev` 的 `emit` 照样返回 `Ok`。应用把设备建在第一次上屏的那一刻，所以
**第一次说话**就会踩上——症状仍是"字没上屏"，只是只发生在每个进程的头一次。

实测（本机 KWin 6.8，用剪贴板 + Ctrl+V 量，判据是靶子窗口里有没有出现那段文字）：建好
**0ms** 就发 → 丢；**80 / 150 / 250 / 400ms** 发 → 到。`linux.rs` 的 `emit` 因此在设备出生后
的 250ms 内先等一下再发（只在头一次发的时候等）。量的时候别用裸字母当探针：fcitx5 会把它们
吃进拼音预编辑，看着像"没到"（实测还顺手提交了一个"下"字）。

## 平台层

三平台各有 `crates/platform/src/{mac,win,linux}.rs`，`lib.rs` 按 `cfg(target_os)` 选 `imp`。增删接口时三者同步，接口以 `mac.rs` 为准。

Linux 侧的机制：

| 能力 | 手段 |
|---|---|
| 全局按键 | evdev 读 `/dev/input/event*`（多读不独占） |
| 上屏 | `/dev/uinput` 虚拟键盘 + 剪贴板（`wl-copy` / `xclip`）+ Ctrl+V |
| 光标位置、前台应用 | 恒 `None`（Wayland 无全局 API） |
| 系统输出静音 | `pactl`（PulseAudio 与 pipewire-pulse 都提供）/ `wpctl` 兜底 |

### 录音时静音系统

「音频」页的开关（`muteWhileRecording`）。开麦那一刻把默认输出静音，收麦恢复原状——
原状态是开麦时问出来的（`pactl get-sink-mute`，没有再试 `wpctl get-volume`），
**问不出原状态就不静音**：恢复不了的事不能做一半，开关表现为无效但不报错。
识别音频始终只从麦克风来（cpal 只开输入设备），这条管的是别打扰人。

实现落在 `platform::OutputMute`：恢复挂在 guard 的 `Drop` 上，打断、超时这些收尾
路径不用各自记得恢复，controller 在 `stop_recording()` / `disarm()` 把 guard 一丢即可。

验收：

- 开着音乐按住长按键 → 音乐立刻停；松手 → 音乐回来。
- 事先把系统静音再录音 → 松手后**仍是静音**（恢复的是原状，不是一律取消静音）。
- `pactl` / `wpctl` 都不在（或连不上声音服务）→ 开录音不报错、系统也不被静音。

## 吞键差异

Linux 不独占设备，因此**不吞键**：钩子只观察、不拦截。这一条决定了下面几处分支，改前先看理由：

| 站点 | 为什么 |
|---|---|
| `ui.rs` 上 HUD 不走 `set_ignore_cursor_events`，自己清输入区 | tao 那条要么 panic、要么设早了白设，且它设的是 1×1 不是空区域——见「[HUD 点击穿透](#hud-点击穿透)」 |
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
`hud` / `pick`（[术语](../GLOSSARY.md)：HUD / Pick panel）是 `680×120` / `520×N` 的**透明顶层
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
cargo run -p voicekey-platform --bin vk-kwin -- dump       # 看 KWin 把每扇窗口摆成了什么样
cargo run -p voicekey-platform --bin vk-kwin -- check      # 验收：文件里的规则 + KWin 现在的表现
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

- **新映射的浮层立刻生效**；重载那一刻**已经开着**的浮层只吃到一半：`acceptfocus` 是
  `forceRule`（持续强制），当场就不再有焦点，但 `above` / `decorationpolicy` / `skiptaskbar`
  / `position` 都**不会追认**，要等它下次映射。实测装完规则、`dump` 那扇装之前就开着的
  HUD：`active=false wantsInput=false`（这一项生效了）但 `keepAbove=false noBorder=false
  skipTaskbar=false`、位置仍是屏幕正中 `620,502`。所以装完之后**重启一下应用**（或按住
  热键让浮层重开一次）再验收，别拿旧窗口判规则没生效。
- 换分辨率 / 接外接屏 / 改工作区尺寸后位置会不对：重跑一次 `install`——`position` 是死坐标，
  KWin 不会替你重算（[ADR 0002](adr/0002-overlay-rules-over-client-positioning.md)）。
- 卸载只删 `voicekey-hud` / `voicekey-pick` 两个组和它们在 `[General] Order` 里的引用。

### 验收：`vk-kwin dump` / `check`

KWin 对两类毛病都是**静默**的：规则文件里键名写错 / 组没进 `Order`，和规则写对了但窗口没被
摆成那样。所以验收分两层，`check` 各给一条条结论：

- **静态层**（读 `kwinrulesrc`，随时可跑）：我们该有的两组 vs 文件里实际的组——组在不在、
  `Order` 里有没有、每个键的值对不对、有没有**多出来的键**（最后一个专抓 KWin 迁移老键名：
  它会把 `noborder` 换成 `decorationpolicy` 并回写文件）。
- **行为层**（跑 KWin 脚本读现在的窗口）：`app_id`、`active` / `wantsInput`、`noBorder`、
  `keepAbove`、`skipTaskbar`、尺寸（hud 连高度一起验 `680x120`；pick 高度随候选行数变，
  只验宽 `520`），hud 另验位置 `620,936`。尺寸不对时那条会提示多半是前端没在渲染。

**浮层此刻没映射时是「跳过」而不是通过**——没东西可看就别报绿。默认跳过不报错（`check`
在只有静态层可验时仍有意义），`--require-overlays` 把跳过也算失败，给"必须验到"的场合。
退出码：有 FAIL（或 `--require-overlays` 下的 skip）就非零。

```
$ vk-kwin check          # 未装规则、浮层正开着（节选）
  ok    hud：app_id 是 voicekey
  ok    hud：窗口就是内容那 680x120
  FAIL  voicekey-hud：组在文件里 —— 没装过，或者被删了：跑 vk-kwin install
  FAIL  hud：不抢键盘焦点 —— active=true wantsInput=true——任一为真，正在打的字就断了
  FAIL  hud：贴底居中（620,936） —— 实际 620,502
  skip  pick 浮层 —— 此刻没映射：等候选列表出来再跑（或加 --require-overlays 把这条当失败）
vk-kwin: 9 项没过（2 项过（1 项跳过未验））；上面带 FAIL 的就是
```

每条的 `why` 都写明现状或下一步（实际值、`跑 vk-kwin install`），不必再回去翻文件猜哪一项错了。

`dump` 是行为层看到的原始事实，一行一扇窗口，自己比对时用：

```
$ vk-kwin dump
plasmashell                                 0,0     1920x1080 active=false wantsInput=true keepAbove=false noBorder=false skipTaskbar=true layer=0
com.mitchellh.ghostty        ◑ #3 完成状态      0,44    1920x1036 active=true wantsInput=true keepAbove=false noBorder=false skipTaskbar=false layer=2
voicekey                     hud          620,936    680x120  active=false wantsInput=false keepAbove=true noBorder=true skipTaskbar=true layer=3
```

更彻底的路子是 wlr-layer-shell 表面（`tauri::WebviewWindow::gtk_window()` 拿到 GtkWindow，
配 gtk-layer-shell）：合成器根本不把它当普通窗口，也就无所谓规则。但那要另写一条窗口创建
路径，不在本轮范围内。

## HUD 点击穿透

`hud` 是 `680×120` 的透明矩形，透明的部分照样吃点击：不处理的话，胶囊两侧那两条空白会
把底下窗口的点击一起吞掉。做法只有一个——把这个顶层窗口的**输入区清空**（GDK 的
`input_shape_combine_region` → Wayland 的 `wl_surface.set_input_region`），窗口还在，
鼠标点到哪儿都不落它身上。pick 面板**不做**这件事：它贴合内容、死区很小，还要用鼠标点选
候选（见下面的验收）。

`ui.rs` 里是 `hud_click_through()`：在窗口的 `map` 信号上清一次（这一下照实测是白设，见下表
第二行，留着只是因为便宜无害），再在映射后 400ms / 800ms 各补一次。两个坑都是本机实测出来的，
改之前先看：

| 坑 | 现象 |
|---|---|
| 不能在映射之前设 | tao 是拿窗口的 gdk window 设的，没映射时它取不到、`unwrap` 当场 panic——`create_overlays` 里 Linux 当初就为这个整条绕过。所以清输入区只能挂在 `map`（或之后） |
| **设早了会白设** | HUD 的 WebKit 内容起来时（合成层 + 入场动画，第一次合成帧大约在映射后 200ms）顶层 surface 的输入区被冲回默认，映射前后那一下设的都不算数。映射时设、映射后立刻设、映射后 120ms 设 → 点击仍被吃掉；300ms / 400ms / 500ms 设 → 穿透 |

挂在 `map` 上而不是 `hud()` 里跟着 `show()`：`map` 才是"窗口真到屏幕上了"，而且 hide / show
重新映射时它每次都会再来一发，正好每次都能补上（实测两个映射轮次都穿透）。补的那两下先让出
一轮事件循环，等 WebKit 把第一帧合成上去；**补两遍而不是一遍**是给冷启动、首帧来得更晚的情况
兜底——清空是幂等的，多设一次没有副作用，而"点击又被吃掉"这个失败方式不给任何提示，宁可多设。

顺带一条：tao 的 `set_ignore_cursor_events(true)` 设的是**左上角 1×1**、不是空区域（见
`tao-*/src/platform_impl/linux/event_loop.rs` 的 `WindowRequest::CursorIgnoreEvents`）。
替身窗口验过：同尺寸同规则的窗口，`1×1` 时点窗口中心能穿透、点左上角那一像素仍被吃掉；
换成空区域后那一像素也穿透。macOS / Windows 上仍用 tao 这条（那边没有这两个坑）。

**退路不用走了**：issue 里留了"若这条路在 KWin Wayland 上不可靠，就把 HUD 窗口缩到内容尺寸"。
实测两个映射轮次都稳定穿透、之后也没再出现过吃掉点击（数据见验证记录），所以缩窗口那条没有实现
——HUD 窗口照旧是 `680×120` 的整块，不用跟着内容改尺寸，胶囊两侧的留白也保持不变。这条结论
只对本机这套 KWin 6.8 / Wayland 成立；换合成器或换 tao 版本，先按验证记录那套手法重测一遍。

## 端到端验收：说话 → 识别 → 落进输入框

这是本项目的完成标准本身：按住热键说一句中文，松开后文字落进**当前聚焦的输入框**。
下面按顺序做，每步都写了"该看到什么"；哪一步对不上就停在那一行，看「对不上时」那一列。

### 前置（一次）

| 项 | 怎么确认 |
|---|---|
| 能读按键 | 用户属于 `input` 组（见「[权限](#权限)」） |
| 能上屏 | `vk-inject` 退出码 0；非 0 时它把原因与修法打出来 |
| 浮层规则已装、应用在装完之后重启过 | `vk-kwin check` 全 ok。注意它只在**浮层已映射**时才验行为层；浮层没开时那几条记 skip，这时它只证明规则文件对了，**证明不了应用重启过**。映射之后再跑一次，或直接看 `vk-kwin dump`：浮层那行该有 `keepAbove=true noBorder=true skipTaskbar=true layer=3`，只有 `acceptfocus=false` 生效、其余还是默认值，就是没重启（见「[装了之后](#装了之后)」） |
| 前端在渲染 | `npx tauri dev`，或 `python3 -m http.server 1420 -d app/dist` 后跑 `target/debug/voicekey` |
| 靶子 | 一个**一次性的**输入框，别拿正在用的编辑器——上屏会覆盖剪贴板且不恢复（`linux.rs::paste`）。没有现成的就用「[端到端替身](#端到端替身没有麦克风时)」里那扇 GTK 窗口。真人对着麦克风跑时，那一节的虚拟声卡换成你的真麦克风（`mic` 留空即用系统默认输入），uinput 注入换成你的真键盘，**沙箱照旧**（见该节第 4 件） |
| 输入法 | fcitx5 照常开着即可，结论见「[fcitx5 与 Ctrl+V](#fcitx5-与-ctrlv)」 |

### 对照清单

| # | 做什么 | 该看到什么 | 对不上时 |
|---|---|---|---|
| 1 | 点一下靶子的输入框，插入点停在里面 | `vk-kwin dump` 里靶子那行 `active=true wantsInput=true` | 焦点不在靶子上：直接点输入框；别去 `activate` 浮层，那会把它抬到上面 |
| 2 | 按住热键（默认右 Alt）不放 | 约 **0.3 秒**后（长按阈值）胶囊出现在**屏幕底部居中**（1920×1080 → `620,936 680×120`），波形开始动；`dump` 里 `voicekey hud … active=false wantsInput=false`，靶子仍是 `active=true` | 胶囊没出来：`pkill -x voicekey` 重启应用再试（钩子每 2 秒扫一次 `/dev/input`，且同名设备一个进程只认领一次）；位置 / 装饰 / 置顶不对：重跑 `vk-kwin install` 再重启应用 |
| 3 | 按住不放，说一句中文 | 胶囊里逐字出现识别中的文字（「胶囊显示实时文字」开着时，默认开；4.6 秒的音频播到一半时，胶囊里已经能看到前几个字）；输入法**不弹候选框**——热键是修饰键，不会被当成拼音 | 一直没有文字：识别没通，看第 6 步 |
| 4 | 松开热键 | 胶囊先切到"识别中"，约 1.2 秒后消失；文字随后落进靶子的输入框——**松手到落字 1.4–1.8 秒**（微信渠道实测 1.46–1.77；别的渠道会不同，见验证记录里那张表） | 文字没进输入框：`wl-paste` 看剪贴板——**有字** = 识别成功、上屏失败（跑 `vk-inject` 查权限；输入法有预编辑时也会这样，见下节）；**没变** = 识别没回来（第 6 步） |
| 5 | 核对落点 | 文字在**靶子的输入框**里，不在胶囊里；`dump` 里靶子仍是活动窗口 | 字进了浮层：规则没生效，看 `vk-kwin check` 里 `acceptfocus` 那几条 |
| 6 | （单独验识别链路）`XDG_DATA_HOME=/tmp/vksb7 ./target/debug/vk-test wetype ios/Resources/sample.wav`（沙箱目录随便给，`vk-test` 也会在里面建渠道身份） | `FINAL: 今天天气不错，我们下午一起去公园散步吧。 (5.97s)`——括号里是含 4.6 秒实时喂音频在内的总耗时 | 报错就是网络 / 渠道问题，与应用无关；六个云端渠道本机都通，耗时见验证记录 |
| 7 | **由人**对着麦克风真读一句 | 同第 3–5 步，落进输入框的文字与所说一致 | —— |

第 7 步是 issue 里那句"最后一句由人来读"：1–6 步可以用录制音频替身跑（见「[端到端替身](#端到端替身没有麦克风时)」），
"真人对着麦克风说一句中文"只能由人完成，替身跑通不等于这一步做过。

第 1–5 步的靶子用**普通应用的输入框**最有说服力（#7 额外在 Kate 里验过一次，见验证记录）；
「端到端替身」那扇 GTK 窗口只是把"该看到什么"变成可 grep 的日志，两者不冲突。

### fcitx5 与 Ctrl+V

**结论：没有预编辑时互不干扰；有预编辑时那一下 Ctrl+V 会被 fcitx5 吃掉。**

| 输入法状态 | 上屏 | 判据 |
|---|---|---|
| 未激活（`fcitx5-remote -c`；或 `-t` 从中文态切过去，此时 `-n` 报 `keyboard-us`），`state=1` | 到 | 靶子 `Entry` 收到 `Ctrl+V`，文字落进去 |
| 激活、中文态但没有预编辑（`fcitx5-remote -o`，`state=2`） | 到 | 同上；上屏后输入法状态不变（仍是 `state=2`） |
| **有预编辑**（敲了拼音、还没选字，输入法的候选窗开着） | **不到** | 靶子只收到 `Ctrl` 的 key-press，`V` 根本没到（fcitx5 拦了）；剪贴板**已经**被换成识别结果，预编辑原样留着，输入框里一个字都不多 |

`-c` 是"停用输入法"、`-t` 是"在启用/停用之间切换"（`fcitx5-remote -h`），两者最后都停在 `state=1`，
上屏都正常。第三行才是分水岭；它拦的是 Ctrl+V 这个按键本身，与识别结果来自哪个渠道无关
（预编辑那一轮用的是沙箱里配置的微信渠道）。

有预编辑时怎么自救（按实测顺序）：

- 按 **Esc** 取消预编辑，再上屏 / 粘贴一次 → 到。
- 点一下 Shift（fcitx5 的 `AltTriggerKeys`）会切到英文并**提交第一个候选**，剩下的字母原样留在输入框里
  （实测留下 `你哈onihao`）——不算干净的解法。
- **Shift+Insert** 在预编辑期间照样能粘贴（fcitx5 不拦它），但应用走的是 Ctrl+V，这条只作诊断用。

热键本身**不会**清掉预编辑：右 Alt 是纯修饰键，fcitx5 不拿它当输入，按住说话、识别都照常，只有最后那一下
Ctrl+V 落空。所以这种"说了话没上屏"**不是识别坏了**——`wl-paste` 里有字就是证据。本轮没有改代码（issue 只要求
观察并记录）：修法（上屏前先发 Esc、或改用 Shift+Insert）会改变所有输入法下的行为，记在这里作为后续候选。

## 本机验收手法

本机（KWin Wayland）**没有** `xdotool` / `kdotool`，客户端也拿不到窗口几何；niri 那套
（`niri msg windows`、`grim`）在此一律无效。可用的是下面这些；另有一条前端缓存坑，先看它再判"界面怎么没变"。

### 界面改了没生效 — WebKit 缓存

`target/debug/voicekey` 里的 webview 从 `:1420` 取页面，而 WebKitGTK 有自己的磁盘缓存
（`~/.local/share/do.j3.voicekey/WebKitCache`）。改了 `app/src`、重新 `npm run build` 之后，
**已跑着的旧进程或新起的进程都可能还渲染旧 bundle**，症状是"改的界面没出现"，Rust 那半却明明
生效了（实测：启动时因 `perms.injectBlocked` 把设置窗口弹出来了，窗口里却没有那条提示）。

判据：在 `app/dist` 的 JS 里塞一个显眼标记，重启应用看它出不出现；不出现就是缓存。清法是把这个
应用自己的 `WebKitCache` 目录挪走（或删掉）再起——它与 `~/.local/share/VoiceKey/` 下的设置、
凭据无关，**别把两个目录搞混**（前者是 webview 的，后者是用户的）。

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

三个坑，都表现成"什么都没发生"：

1. **写完 `EV_KEY` 必须再补一个 `EV_SYN` / `SYN_REPORT`**（type 0, code 0, value 0），否则内核
   不把这一包交给任何 evdev 读者：应用收不到、KWin 也收不到，KWin 脚本里什么都看不到，
   python-evdev 那边同样静默。少了这一个 SYN，整件事看上去就像"钩子坏了"。
2. **同名设备在一个应用进程里只会被认领一次**：同一进程内第二次注入（哪怕销毁旧设备、重建
   同名设备）浮层不再出现——钩子扩进的是已经 stale 的 fd。**每次注入前先重启应用**。
3. **重启用 `pkill -x voicekey`，别用 `pkill -f <路径>`**：`-f` 拿整条命令行匹配，会连你自己
   那条 `bash -c '… pkill -f target/debug/voicekey …'` 一起杀掉，当场自杀、退出码 144。

所以一次完整的编排是：`pkill -x voicekey` → 起应用（webview 要有前端在渲染）→
`sleep 4` 让钩子扫到设备 → 注入并**长按住**（≥30s，留出截图与 `check` 的时间）→
`vk-kwin check` / `dump` / 截图。

### 端到端替身（没有麦克风时）

验「说话 → 识别 → 落进输入框」不必真开口：让应用**从虚拟麦克风**录一段现成的语音，
其余（热键、识别、上屏）全走真链路。仓库里有 `ios/Resources/sample.wav`（16kHz 单声道，
内容是"今天天气不错，我们下午一起去公园散步吧。"），拿它当"说的话"。

三件东西，缺一不可（沙箱是第四件，建议一起用）：

1. **虚拟麦克风**：null sink + remap source，把播进 sink 的声音变成一路输入源。
   ```bash
   pactl load-module module-null-sink sink_name=vk7_sink sink_properties=device.description=vk7_sink
   pactl load-module module-remap-source master=vk7_sink.monitor source_name=vk7_src
   pactl set-default-source vk7_src          # 应用 mic 留空时用系统默认输入
   paplay --device=vk7_sink ios/Resources/sample.wav
   ```
   验完 `pactl set-default-source <原来那个>`（`pactl get-default-source` 先记下来）。
   **别去改用户的 `settings.json` 里的 `mic`**——用默认输入切换就够了。
2. **靶子**：`Scripts/e2e/target.py`——一扇 GTK 窗口 + `Gtk.Entry`，把发生的事逐行写进日志。
   判据是日志里的 `CHANGED len=20 text='…'`，不是截图。`python3 Scripts/e2e/target.py <日志>`
3. **热键**：`Scripts/e2e/inject_hold.py`——上一节那把 uinput 键盘的成品，按住右 Alt 直到 stop
   文件出现。它自己会先 `sleep 3` 再按下（原因见下面第 4 条坑）。`python3
   Scripts/e2e/inject_hold.py <stop 文件>`
4. **沙箱（建议）**：`XDG_DATA_HOME=<沙箱>`，里面放一份**单渠道**的 settings：
   ```bash
   mkdir -p /tmp/vk-e2e-sandbox/VoiceKey
   printf '{"channel":"wetype","onboarded":true}' > /tmp/vk-e2e-sandbox/VoiceKey/settings.json
   ```
   `channel` 按你要验的渠道填（下面耗时表里的数字是 `wetype` 的）。**别照抄
   「[不碰用户设置](#不碰用户设置把-pick-面板弄出来)」那节的 `channel=all`**——那是为了把 pick 面板弄出来，
   而 `all` 模式按住热键出来的是 `pick` 浮层、松开后还要选一行才上屏，跟本清单第 2–5 步要的
   `hud` 不是一回事。不设沙箱的话应用读写的就是你真实的 `~/.local/share/VoiceKey/`
   （渠道身份、`settings.json` 全在里面），一次误操作就落到用户数据上了。用后删掉沙箱目录即可。

一轮的编排已经写成脚本：`Scripts/e2e/run_e2e.sh [渠道] [工作目录]`，做的是下面这串（**每轮重启
应用**，原因见「[合成按键](#合成按键--uinput-注入)」第 2 条坑）：

```
pkill -x voicekey → 起靶子 → 起应用（XDG_DATA_HOME=沙箱）→ sleep 5（让钩子扫到设备）
→ 起注入脚本（它自己 sleep 3 再按下右 Alt：等钩子认领新设备）
→ paplay 那段 wav → 等它播完 + 1 秒 → 让注入脚本松开右 Alt → sleep 10
→ 看靶子日志，算"按住多久 / 松手到落字"
```

那个 `sleep 3` 在**按下之前**，不在按下与播放之间——它等的是"钩子扫到刚建的 uinput 设备"
（见下面第 4 条坑）。所以实测的按住时长就是 `4.6s（wav）+ 1s ≈ 5.7s`，而不是 3+4.6+1。

四个坑，都是本轮踩过的（后三条与「[合成按键](#合成按键--uinput-注入)」那节是同一件事，
写在这里是因为端到端每轮都会碰到）：

- **靶子窗口必须拿到焦点**，否则上屏落到别处。点它，或用 KWin 脚本
  `workspace.activeWindow = <窗口>`；用 KWin 脚本激活后要确认 `dump` 里它 `active=true`。
- **每轮重启应用**：同一个进程里同名 uinput 设备只被认领一次；上一轮注入的设备还在、
  下一轮换了名字也不一定来得及被扫到——`sleep 3` 只是经验值，最稳的是重启。
- **`pkill -f 'e2e/target.py'` 会匹配到你自己那条 `bash -c`**，和 `pkill -f <应用路径>` 是同一个坑：
  模式里加个方括号（`'target[.]py'`）或改用 `pkill -x`。
- **注入设备建好后至少等 2 秒再发键**：应用的钩子每 2 秒扫一次 `/dev/input`，设备晚被扫到，
  按下去的右 Alt 就没人收（症状是"应用毫无反应"）。`sleep 3` 是留了余量的经验值。

时间戳怎么算：注入脚本里"松开右 Alt"的时刻是松手，靶子日志里 `CHANGED` 的时刻是"字落进输入框"，
两者之差就是**松手后的识别 + 上屏耗时**（应用在松手后才收尾上传、等定稿、再粘贴）。替身里松开发生在
音频播完之后 1 秒，所以这个差值不含录音时长；验证记录里那 1.46–1.77 秒就是这么来的。

### KWin 脚本里没有定时器

`timeout` / `setTimeout` / `setInterval` 在 KWin 的 JS 引擎里**都不存在**（试过，报
`timeout is not defined`），所以轮询式的观察脚本写不了。要带时间戳的事件流，就挂信号：
`workspace.windowAdded` / `windowRemoved` / `windowActivated`，每个回调里 `print("VKEV|" + Date.now() + …)`，
输出同样进合成器的 journal。拿 `journalctl --user -u plasma-kwin_wayland --since "-30s" | grep VKEV`
捞回来即可。这条用来量"胶囊什么时候映射 / 什么时候消失"最准。

顺带一条：`workspace.windowList()` 里同一个 GTK 应用会列出**好几扇窗口**（`target.py` 那扇实测出现
四行，几何各不相同，只有第一行是真窗口）。按标题找窗口时别用 `windowList()[0]` 这类下标，
要逐行比对标题 + `active`。

### 合成鼠标 — 判点击落在谁身上

验"这一下点击被谁吃了"要两样东西：一把 uinput 相对指针，和一个会自己举手的窗口。

指针：`EV_REL` 的 `REL_X` / `REL_Y` + `BTN_LEFT`，设备名的坑同上面那把键盘。**别指望一次
写到位**：libinput 有加速，`+100,-50` 落不到你以为的地方。按 KWin 脚本读回来的光标位置迭代
过去——`workspace.cursorPos` 是**可读**的（只有 `setPos` 不可用），每步 ≤25px，收敛到尾差几像素。

举手窗口：一扇 GTK 窗口，`button-press-event` 里把自己的标题改成 `vk-hit-N`，标题从
`vk-kwin dump` 读回来。"标题没变 = 被浮层吃了；变了 = 穿透到它了"。

**判据不能看活动窗口**：我们的规则给浮层 `acceptfocus=false`，点击被它吃掉时活动窗口**也不
会变**——跟穿透过去长得一模一样（第一轮就是这么误判的：HUD 明明吃掉点击，却看着像穿透）。
要拿活动窗口当判据，得先让替身自己 `acceptfocus=true`。

编排：替身先映射、待测窗口后映射（后映射的在上面，别去 `activate` 替身——激活会把它抬到
待测窗口上面）。点之前"HUD 矩形 ∩ 替身矩形"里挑点：避开压在上面那层（`layer=3` 的 plasma
面板 / Dock，它们的 `layer=0` 桌面窗不算，那是铺满屏幕的一块），也别贴待测窗口左上角
（tao 那条路径会留 1×1 的死角，≥30px 就稳）。截图对照只是辅证：测到的波形 / 流式文字一直在
动，像素差永远"有变化"，别拿它当判据。

### 不碰用户设置，把 pick 面板弄出来

pick 面板只在`全部渠道`（`channel=all`）模式下出现，而验证时不能改用户的
`~/.local/share/VoiceKey/settings.json`。做法是**另起一个沙箱实例**：

```bash
mkdir -p /tmp/vksb/VoiceKey && printf '{"channel":"all","onboarded":true}' > /tmp/vksb/VoiceKey/settings.json
XDG_DATA_HOME=/tmp/vksb ./target/debug/voicekey   # dirs::data_dir() 认这个环境变量
```

沙箱里**一开始**没有凭据文件，面板与候选行照常出现——按住热键（`channel=all` 时 `begin()` 直接进
`Mode::Pick`）面板就出来了，可以点行、点 ✕。两点提醒：单实例插件按 app_id 认领，**先停掉真应用**
再起沙箱；沙箱实例照样在录音、往能用的渠道发请求，别一边验一边说话。

**沙箱会自己建凭据文件**：豆包 / 微信 / 千问 / 百度 / 搜狗这五路在第一次请求时把设备号 / uin 之类
写进 `XDG_DATA_HOME/VoiceKey/*.json`（`data_file()` 跟着 `XDG_DATA_HOME` 走）；讯飞用的是
`ifly.rs` 里写死的 appid / key，不读文件。所以"沙箱里没有凭据文件"只影响第一次请求之前，
跑起来之后沙箱和真配置一样能用。#7 在一个全新沙箱里连跑六个云端渠道，**全部出结果**
（耗时见验证记录）。这也是沙箱顺带的好处：验渠道连通不必碰用户的凭据，用后把整个沙箱目录删掉即可。

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

- `cargo check --workspace --all-targets` 无警告；`cargo test --workspace` 全绿（`crates/platform`
  32 项：浮层几何、规则生成、探针解析、INI 读写、`Order` 合并、`check` 的判定逻辑——含装上
  规则 / 未装规则的 HUD 实测基线各一条；#6 又加了 4 项上屏前提的判定与文案）；mac / Windows 的编译回归：
  `cargo check -p voicekey-platform --target x86_64-apple-darwin` 与 `--target
  x86_64-pc-windows-msvc` 均通过
- 前端类型检查上线：`app` 加 `npm run check`（`svelte-check` 读 `tsconfig.json` 那个 `strict`），
  接进 `.githooks/pre-commit` 与 CI——之前 `strict: true` 谁都读不到，隐式 `any` 也能提交。
  首跑 101 个文件报 3 处同一个毛病：`Logo.svelte` 没有 `<script>` 块，svelte2tsx 就不为它生成
  `.d.ts`，import 它的三处全报「implicitly has an 'any' type」；补一个空的 `<script lang="ts">`
  后 0 错 0 警。为确认它**真会**报，故意塞了个类型错：`ERROR "src/lib/Logo.svelte" 4:9 "Type
  'string' is not assignable to type 'number'."`、退出码 1，撤掉后回到 0
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
- `vk-kwin check` 四种状态都在真应用上走过：① 浮层没映射 → 静态层 36 项 ok、行为层两条
  `skip`，退出码 0；同一条加 `--require-overlays` 退出码 1（跳过就得是跳过的样子，不能顺手
  报绿）。② 按住注入的热键 → 静态 36 + 行为 7 全 ok，pick 仍 `skip`（它要等候选列表出来）。
  ③ `uninstall` 后新映射的 HUD → 9 项 FAIL（静态 4：两个组不在 `Order` 里、文件里也没这两组；
  行为 5：焦点 / 装饰 / 置顶 / 任务栏 / 位置，位置那条报 `实际 620,502`），退出码 1。④ 重装
  规则但**不重启应用** → 静态 36 项全 ok，行为层只过了 `app_id` 与尺寸两条、仍在这 5 条上
  FAIL——`check` 报的是 KWin **现在的样子**，不是文件的样子，这正是行为层存在的理由
- `vk-kwin dump` 在真桌面上的输出（节选）：`plasmashell 0,0 1920x1080 layer=0`、
  `com.mitchellh.ghostty 0,44 1920x1036 active=true layer=2`，装上规则、按住热键时多一行
  `voicekey hud 620,936 680x120 active=false wantsInput=false keepAbove=true noBorder=true
  skipTaskbar=true layer=3`——前台仍是 Ghostty，键盘焦点没被抢走
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
  （`settings.json`），所以只验到规则对同 `app_id` 同标题的窗口生效这一步。#5 用沙箱实例
  补上了这一条（见下）
- **HUD 点击穿透（#5）**：先拿替身把机制验通，再落到真应用上。替身（`app_id=voicekey`、
  标题 `hud`，吃同一套规则）四种情况各点一次窗口中心（`layer=0` 的桌面窗与 `layer=3` 的
  plasmashell 面板都垫在下面，点之前从 dump 里确认过）：不设输入区 → 点击被替身吃掉；
  设空区域 → 穿透到下面；**设 tao 那套左上角 1×1 → 中心照样穿透**（1×1 只吃掉左上角那一
  像素）；带 WebKit 子窗口的替身（页面带 CSS 动画，逼出合成层）同样只在顶层设空区域就穿透，
  说明 WebKit 那层子窗口不是元凶
- 真应用上逐条试"什么时候设"（每轮重启应用 + 注入按住右 Alt，判据是垫在下面的举手窗口
  标题变没变）：映射信号里设 → 被吃；`show()` 之后立刻设（tao 那条）→ 被吃；映射后 40ms /
  120ms 设 → 被吃；**300ms / 400ms / 500ms 设 → 穿透**。设晚了以后只清顶层窗口就够，把 10 个
  子窗口一起递归清结果一样——"设早了"是唯一的变量
- 最终实现（`ui.rs::hud_click_through`）挂在 `map` 信号上、每次映射补两次输入区（400ms 与
  800ms，第二次是给冷启动 / 首帧晚来兜底）：**连续两个映射轮次（首次映射 → 松开热键隐藏 →
  再按住重新映射）都穿透**，两次都是垫在下面的举手窗口收到点击（`vk-idle → vk-hit-2 →
  vk-hit-4`）。复审后把补设从一遍改成两遍，改完用同一套用例重跑，结果相同
- 反向对照：pick 面板**不**做这件事，点面板里的行 → 面板自己处理了（选中高亮从第 1 行豆包
  移到被点的第 4 行百度，前后截图对比），垫在下面的举手窗口标题没变——"没穿透"在这里正是
  要的结果。沙箱实例里面板是 `700,326 520x472`（`layer=3`、`active=false`、`noBorder=true`，
  KWin 居中——Wayland 下 `place_pick()` 算的位置同样无效），6 行候选都渲染出来了。复审后的
  构建又跑了一遍：点第一行 → 整块面板像素都变了（选完自动收起），下面的举手窗口仍是
  `vk-idle`
- 截图确认 HUD 显示没受影响：按住热键时胶囊贴底居中、波形成形、底下终端的状态行照常露着
  （清输入区只改命中，不改渲染）
- 启动不 panic：真应用与沙箱实例的启动 + 首次映射日志里都没有 panic（tao 那条"未映射就
  `unwrap`"的路径没有走到）
- 全程 `sha256sum ~/.local/share/VoiceKey/settings.json` 未变（`d9c9e0ef…`）；沙箱用的
  `/tmp/vksb` 用后即删
- 判据本身踩过两个坑，记一笔免得再费一遍时间。①`vk-kwin dump` 第一列（app_id）是**空格
  补齐**的，行长这样：`voicekey` + 一堆空格 + `hud`，所以 `grep "^voicekey hud"` 永远匹配
  不上（我在 shell 里这么判了一轮，白等）；在脚本里按空白切列，或者 `grep voicekey`。
  ②应用已经跑过几轮之后遇到过"按住热键 → 确实开始录音（`pactl list source-outputs` 里有
  `[voicekey]` 在抓）→ 但 HUD 窗口没映射出来"（dump 里没有那一行，截图里也没有胶囊）；杀掉
  应用重开就好，之后连跑几轮都正常。原因没查到根上（"卡在录音态"这个猜测专门试过，可以
  排除），所以跑这套用例**每轮重启应用**最省事
- 收尾三件套：`cargo check --workspace --all-targets` 无警告、`cargo test --workspace` 全绿、
  `app` 里 `npm run check` 101 文件 0 错 0 警、`npm test` 3 项通过。`Cargo.lock` 只多一条
  `gtk` 依赖边（`voicekey` 包下），版本树没动——`gtk 0.18` 本来就在 tao / wry 的依赖里
- **上屏前提（#6）**：`crates/platform` 32 项单测里新增 4 项（`inject` 的谓词、两种原因的文案、
  规则路径常量）。实测按"先把它弄坏、再看它报得对不对"的顺序走：
  - 不可写的造法：Steam 的规则先改名挪开（`60-steam-input.rules` → `.vk-off`），再
    `sudo setfacl -b /dev/uinput` 把 Steam 此前挂上的 ACL 摘掉——这样 `/dev/uinput` 就是
    `root:root 0660`、**没有任何规则给它权限**，与"卸掉 Steam"等价。`vk-inject` 随即报
    `/dev/uinput：不可写`、退出码 1，并打出装规则的命令
  - 同一状态下起真应用：设置窗口**自己弹出来**（`perms.injectBlocked` 有值 → `show_settings`），
    顶部一条「现在无法上屏 /dev/uinput 不可写。装仓库自带的 udev 规则，然后让设备节点重建
    一次（或重新登录）：sudo install -m 644 …」+ 侧栏那行从「就绪 · 长按右 Alt 说话」变成
    「现在无法上屏」。截图对照过（上面那句文案之后又补上了重建节点那半条命令，补的是同一个
    修法，没再截图）
  - 说完话那条路也不再静默：`controller.rs` 的上屏统一走 `insert()`，设备不可写时把同一条
    文案顶到 HUD 上（`hud_error`）而不是什么都不发生。启动预检管"开机时"，这条管"每一次
    说话"。首次运行走的是 onboarding（Linux 步骤是「麦克风 → 试一句」），那里也接了同一个
    字段：不成立时不再写「松开即上屏」，换成同一条说明。**这两处只做了类型检查与代码审读，
    没有在真机上截图复验**（`vk-inject` / 设置窗口那两处是实测过的）
  - 装上 `60-voicekey-uinput.rules`、Steam 那条仍是 `.vk-off`、`modprobe -r`/`modprobe` 重建
    节点之后：`getfacl` 里 `user:cheems:rw-` 回来了，`udevadm test` 只读到我们这一条（加上
    `60-steam-vr.rules`，它与 uinput 无关）；`vk-inject` 退出码 0；**冷进程** `vk-inject paste
    不依赖 Steam 也能上屏` 落进了靶子窗口（GTK `Entry`）。恢复 Steam 那条后
    `sha256sum` 与动手前一致（`284930f8…`），我们那条留在 `/etc/udev/rules.d/`
  - 规则名踩了一脚：第一版是 `99-voicekey-uinput.rules`，`udevadm info` 里 `TAGS=:uaccess:`
    有、ACL 却没有——`73-seat-late.rules` 里的 `uaccess` builtin 排在 99 之前。改成 `60-`
    才生效（见权限小节）
  - 顺手量出一个既有的静默失败：`uinput` 设备建好后立刻发的按键会被丢掉（0ms 丢，80ms 起
    才到；判据是剪贴板 + Ctrl+V 打进靶子窗口的文字）。应用是第一次上屏时才建设备，所以每个
    进程的**头一次上屏**都会踩到——`linux.rs` 因此加了 250ms 预热，改完用冷进程复验，第一次
    就落进去了
  - 界面上第一次没看到那条提示：Rust 侧明明把窗口弹出来了，页面却是旧的——WebKitGTK 的磁盘
    缓存（`~/.local/share/do.j3.voicekey/WebKitCache`）顶着旧 bundle。挪走缓存后同一次构建就
    正常显示（验证手法见「本机验收手法」）
  - 全程 `sha256sum ~/.local/share/VoiceKey/settings.json` 未变（`d9c9e0ef…`）
  - 跨平台编译回归（改到了 `mac.rs` / `win.rs`）：
    `cargo check -p voicekey-platform --target x86_64-apple-darwin` 与
    `--target x86_64-pc-windows-msvc` 均通过（`--all-targets` 也算上，`vk-inject` 在非 Linux 上
    是那条报错入口，跟着编过）。app 侧的改动（`main.rs` 一行 `inject_problem()`）按
    「[mac / Windows 编译回归在本机能验到哪](#mac--windows-编译回归在本机能验到哪)」的说明，
    本机只能给论证：与旧 `cfg!` 同型、类型中立
- **端到端（#7）**：用「[端到端替身](#端到端替身没有麦克风时)」跑，热键、录音、识别、上屏全是真链路，
  只有"说话"是 `paplay` 播的 `ios/Resources/sample.wav`。靶子是一扇 GTK 窗口的 `Gtk.Entry`，
  判据是它的 `changed` 日志。**本轮没有改代码**——链路本来就是通的，这条票是验收。
  - **文字落进输入框**：靶子日志 `CHANGED len=20 text='今天天气不错，我们下午一起去公园散步吧。'`，
    与 `vk-test wetype` 的定稿逐字相同；KWin 事件流里同一时刻是
    `ADDED voicekey|hud|620,936 680x120|active=false|wantsInput=false|keepAbove=true|noBorder=true|layer=3`
    → 松开 → `REMOVED`，活动窗口自始至终是靶子（`active=vk7-target`；这条事件流脚本没打
    `skipTaskbar`，那一项由上一条的 `vk-kwin dump` / `check` 覆盖）。用同一套编排连跑四轮（三轮
    纯替身 + 一轮带截图和 `vk-kwin check`），都是同一句、同一落点。
  - **普通应用的输入框**：另起一轮，靶子换成 **Kate**（`kate --new`，用 KWin 脚本把它的窗口设为
    活动窗口，再用 uinput 注入 `Ctrl+N` 开一篇新的无标题文档）。同样的按住 → 播 wav → 松开之后，
    Kate 的文档里出现整句 `今天天气不错，我们下午一起去公园散步吧。`，标题栏同步变成
    `无标题(今天天气不错，我们下午一起去公园散步吧。) * — Kate`。这一轮证明上屏不依赖那个
    GTK 替身——它和用户日常用的输入框走的是同一条路。
  - **浮层不抢焦点**：HUD 映射那一刻靶子仍是 `active=true`；上屏那一下靶子的 `key-press-event` 里
    直接看到 `Ctrl` 和 `PASTE-CLIPBOARD`——按键落在靶子上，不在胶囊里。`vk-kwin check` 在
    HUD 开着时 `43 项全过（1 项跳过未验）`（pick 未映射记 skip），包括 `hud：不抢键盘焦点` 与 `贴底居中（620,936）`。
  - **耗时**（各轮都是同一段 4.6 秒音频、松开时间不同；时间戳取自注入的"松开"与靶子日志里的 `CHANGED`）：

    | 轮次 | 按住时长 | 松手 → 字进输入框 |
    |---|---|---|
    | 1 | 5.67s | **1.77s** |
    | 2 | 5.67s | **1.73s** |
    | 3 | 5.67s | **1.77s** |
    | 带截图的最终轮 | 5.67s | **1.46s**（同一次里 `vk-kwin check` 也在跑） |
    | 带预编辑的一轮（见下） | 5.69s | 识别正常；Ctrl+V 被输入法吃掉（靶子无变化） |

    拆开看：松手到落字 = 识别收尾 + 上屏。收尾是等云端最后一个回复（定稿，
    `wetype.rs` 里最多轮询 8×250ms）；上屏本身是 `wl-copy` 提交前固定等 80ms、虚拟键盘
    冷启动预热 250ms（每轮都重启应用，所以每轮都吃这 250ms）。胶囊在松手后约 1.2s 消失，
    再过 0.2–0.6 秒字进输入框（1.46s 那轮快些，1.77s 那几轮慢些）——即"胶囊消失"是上屏的
    前一瞬，不是失败信号（`controller.rs` 里 `hud_hide(0)` 紧跟着 `insert`）。
  - **识别链路连通与耗时**：外网经本机代理转发（fake-IP：`wetype.weixin.qq.com` 解析成
    `198.18.0.12`，路由走 `Meta` 接口；直接 `curl` 该域名 TLS 握手约 150ms）。六个云端渠道用
    `vk-test` 跑同一段 wav，**全部连通**——其中豆包 / 微信 / 千问 / 百度 / 搜狗五路在沙箱里
    本来**没有**凭据文件，是首次请求时自己建的身份（讯飞用写死的 key，见
    「[不碰用户设置](#不碰用户设置把-pick-面板弄出来)」）：

    | 渠道 | 定稿 | 耗时 |
    |---|---|---|
    | 豆包 | 今天天气不错，我们下午一起去公园散步吧。 | 5.45s |
    | 微信 | 同上 | 5.97s |
    | 千问 | 同上 | 5.26s |
    | 百度 | 今天天气不错我们下午一起去公园散步吧（无标点） | 5.07s |
    | 搜狗 | 今天天气不错我们下午一起去公园散步（无标点、少"吧"） | 5.06s |
    | 讯飞 | 今天天气不错，我们下午一起去公园散步吧。 | 5.18s |

    这 5–6 秒里包含 `file_frames(realtime=true)` 按实时节奏喂 4.6 秒音频，所以音频播完后的收尾
    约 0.5–1.4 秒（微信 5.97 − 4.6 ≈ 1.4s）。端到端那 1.46–1.77 秒 = 这段收尾 + 剪贴板等待 80ms +
    虚拟键盘预热 250ms，两边对得上（1.46 那一轮识别收尾短些，正好落在下限）。代理没有让任何一个渠道不可达。
  - **fcitx5 与 Ctrl+V**：见「[fcitx5 与 Ctrl+V](#fcitx5-与-ctrlv)」。本轮实测：没有预编辑时照常上屏，
    有预编辑时那一下 Ctrl+V 被吃掉，剪贴板已经换成识别结果、输入框里没有字；热键与识别都不受影响。
  - 用户数据：全程 `XDG_DATA_HOME=/tmp/vksb7` 跑沙箱，`~/.local/share/VoiceKey/` 六个文件的
    `sha256sum` 与动手前一致（`settings.json` `d9c9e0ef…`），mtime 也没变。虚拟声卡与沙箱目录
    用后即删 / 还原默认输入。
  - 收尾：`cargo check --workspace --all-targets` 无警告、`cargo test --workspace` 全绿、
    `app` 里 `npm run check` 101 文件 0 错 0 警、`npm test` 全过
  - **没做的**：真人对着麦克风说一句（对照清单第 7 步）——这一步只能由人完成；
    "全部渠道"模式的 pick 面板本轮没再复验（#5 已验过，与本票无关）。

- **录音时静音系统（2026-10-25）**：`mute_round_trip_mutes_then_restores_previous_state`（标
  `#[ignore]`，`cargo test -p voicekey-platform -- --ignored` 显式跑）在本机过：guard 生效期间
  `pactl get-sink-mute` 为 yes，Drop 后回到原状态；先手动把系统静音再跑一轮，结束仍是静音
  （恢复原状，不是一律取消静音），跑完手动还原。pactl 解析有单测覆盖（`yes`/`no`/垃圾输出）。

- **上游 v1.7.0 同步（2026-10-27）**：把 `J3n5en/voicekey` 的 10 个提交（v1.5.0–v1.7.0）合进
  fork。共同祖先 `ea0b425`；只有三处文本冲突（`Cargo.lock`、`ui.rs`、`Onboard.svelte`），
  加上「录音时静音系统」那个提交后多一处 `api.ts`。上游**没有碰** `crates/platform`，
  Linux 平台层原样保留。
  - 合并后 `cargo check --workspace --all-targets` 无警告、`cargo test --workspace` 全绿、
    `npm run check` 101 文件 0 错 0 警、`npm test` 3/3；`cargo check -p voicekey-platform
    --target x86_64-apple-darwin` 与 `--target x86_64-pc-windows-msvc` 均通过。
  - 单渠道 `wetype` 沙箱端到端：HUD `620,936 680x120`（`layer=3`、`keepAbove=true`、
    `wantsInput=false`），靶子日志 `CHANGED len=20 text='今天天气不错，我们下午一起去公园散步吧。'`，
    松手到落字 1.89s（与既有记录同量级）。
  - 自定义长按组合（`holdKey=custom`，左 Ctrl + 左 Alt，evdev 29/56）：HUD 正常弹出，
    同一段 wav 落字一致；托盘菜单显示「长按 Ctrl+Alt 或点按 右 Ctrl 说话」。
  - 「全部渠道」沙箱：pick 面板 `700,335 520x454`、`layer=3`、`active=false`，长结果未
    panic（上游 35b5bd7 的 `clamp_axis` / `cap_len` 与本 fork 的 `Area::units` 改动合在一起）。
  - 录音时静音：默认未静音 → 录音中 `Mute: yes` → 松手 `Mute: no`；先把 sink 静音再录一轮，
    松手仍是 `Mute: yes`（恢复原状）。默认输出临时指向 `module-null-sink`，真实输出没被动过。
  - 设置窗口截图确认 `v1.7.0`、Chatterfly 渠道在；`app/dist` 里含「录音时静音系统」与
    「再点一次结束」文案。`vk-kwin check --require-overlays` 在 HUD / pick 映射期间分别跑过：
    对应浮层的行为层全 ok，另一扇未映射的浮层按约定记 skip（`--require-overlays` 下算失败）。
  - 全程 `XDG_DATA_HOME=/tmp/vk-e2e-sandbox`；`~/.local/share/VoiceKey/` 无新文件、mtime 未变。
    虚拟声卡、默认输入/输出与模块用后还原并卸载。
  - 顺手修 `Scripts/e2e/run_e2e.sh` 两处跑不起来的地方：`target.py` / `inject_hold.py` 改按
    `$HERE` 定位（此前从仓库根跑必然找不到），`VK_SINK` 未设时不再被 `set -u` 打掉。
    靶子焦点仍要按上文用 KWin 脚本激活，脚本没有代劳。

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

未验证：真说话 → 识别 → 上屏 的完整链路，需麦克风输入。（这一条后来在 KDE 环境上由 #7 补上，
见上面的「端到端（#7）」——那边仍是用播出来的音频替身，真人开口那一步留给验收的人。）
