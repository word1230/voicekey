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

## 本机验收手法

本机（KWin Wayland）**没有** `xdotool` / `kdotool`，客户端也拿不到窗口几何；niri 那套
（`niri msg windows`、`grim`）在此一律无效。可用的是下面四件；另有一条前端缓存坑，先看它再判"界面怎么没变"。

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

沙箱里没有凭据文件，靠凭据的渠道会报 `invalid handle`（实测讯飞那行就是），但面板与候选行
照常出现——按住热键（`channel=all` 时 `begin()` 直接进 `Mode::Pick`）面板就出来了，可以点行、
点 ✕。两点提醒：单实例插件按 app_id 认领，**先停掉真应用**再起沙箱；沙箱实例照样在录音、
往能用的渠道发请求，别一边验一边说话。

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
  28 项：浮层几何、规则生成、探针解析、INI 读写、`Order` 合并、`check` 的判定逻辑——含装上
  规则 / 未装规则的 HUD 实测基线各一条）；mac / Windows 的编译回归：
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
