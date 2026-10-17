# 浮层的定位 / 置顶 / 不抢焦点交给 KWin 规则，而不是客户端 positioning

Linux 浮层（HUD / pick panel）的三件事——**贴底居中、始终置顶、不抢键盘焦点**——全部由
KWin 窗口规则（`~/.config/kwinrulesrc`）决定；应用侧不改（`set_position` 在 Wayland 上是
空操作，`always_on_top` 只是 hint，`focused(false)` 只是请求，装饰更是客户端根本去不掉）。
规则由 `vk-kwin install|uninstall` 装/卸（[`crates/platform/src/bin/vk-kwin.rs`](../../crates/platform/src/bin/vk-kwin.rs)），
规则内容由 [`crates/platform/src/kwin.rs`](../../crates/platform/src/kwin.rs) 的纯函数生成，
与应用共用 `overlay.rs` 那份几何（见 #3），`app_id` 运行时**从 KWin 查得**而非写死。

## Considered Options

- **客户端自己摆**：`set_position` 在 Wayland 上是空操作；wlr-layer-shell 能让合成器不把浮层
  当普通窗口，但那要重写窗口创建路径，且与「规则」是两条互不相通的路（[0001](0001-linux-targets-kwin-wayland.md)
  已选规则这条）。
- **一条规则匹配 `^(hud|pick)$`**：KWin 的 `position` 只在窗口**映射那一刻**生效（Apply
  Initially），而候选面板的高度随候选行数变——写死一个 `y`，短列表会悬在半空，长列表会被
  顶出屏幕或盖住输入行。所以拆成两条：`voicekey-hud` 带 `position`，`voicekey-pick` 不带
  （由 KWin 居中），两者的其余四项完全相同。
- **把规则写在文档里让用户自己粘**：旧的手写 `voicekey-overlay` 规则就是这么来的，`wmclass`
  猜成了进程名，规则**静默地不匹配**——没人知道它没生效。安装器把这条旧规则当失败案例替换掉。
- **规则里写死 `app_id`**：同上，打包方式或进程名一变就静默失效。改为运行时探针取：KWin 报的
  `resourceClass` 才是规则匹配的那个字符串。

## Consequences

- **换分辨率 / 接外接屏 / 改工作区尺寸后要重跑一次 install**：`position` 是死的坐标（本机
  1920×1080 → `620,936`），KWin 不会替你重算。这是选「静态规则」的代价，不是 bug。多显示器
  且工作区不同时更明显：一条静态规则只对**探针取到的那块屏幕**（`workspace.activeScreen`）
  正确，换到另一块屏幕上显示会偏——彻底的做法要等 wlr-layer-shell 那条路。
- **装之前要有一扇 VoiceKey 窗口在 KWin 里出现过**（按住热键让 HUD 映射，或打开设置窗口），
  否则取不到 `app_id`；取不到时退回上次装的 `app_id` 并**打印提示**，而不是默默换个猜的值。
- **规则键名跟着 KWin 版本走**：置顶的键是 `above` / `aboverule`（**不是** `keepabove` /
  `keepaboverule`）；关装饰的键是 `decorationpolicy=none`（**不是**老式的 `noborder`——KWin 6
  加载时会把它迁移成前者并回写 `kwinrulesrc`，于是文件在我们装完之后自己变一次，逐字节的
  幂等检查成了假警报）。KWin 升级后要复验这几项是否仍生效。
- **策略值最容易看错**：`*rule` 写的是 KWin 的数值枚举（`DontAffect=1, Force=2, Apply=3,
  Remember=4, ApplyNow=5, ForceTemporarily=6`），`2` 是 **Force**，`Apply Initially` 是 `3`；
  而 `acceptfocus` 在 KWin 里是 `forceRule`，只收 1/2/6——同一个文件里两种策略数值并存是
  正常的，改的时候别"统一"成一个值。
- **幂等**：两条规则各占一个固定组名（`voicekey-hud` / `voicekey-pick`），安装时先删同名组
  再写，`[General] Order` 只追加、不重复；别人的规则（如系统 `/etc/xdg/kwinrulesrc` 里的）
  一概不动。重复执行后 `kwinrulesrc` 逐字节相同。
- **只影响 Linux**：`kwin` 模块是 `cfg(target_os = "linux")`，macOS / Windows 的浮层仍由应用
  自己摆（那两家的窗口管理 API 客户端就能用）。
