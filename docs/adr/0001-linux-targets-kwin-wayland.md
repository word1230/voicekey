# Linux 目标锁定 KWin Wayland、放弃 niri

Linux 支持最初在 Arch + niri（Wayland）上做出来并验证，但本机与目标环境是 KDE Plasma 6.8 / KWin 6.8（Wayland）。Wayland 下客户端**摆不了窗口、定不了焦点、去不掉装饰**，浮层（HUD / pick panel）的位置、置顶与不抢焦点**只能**由合成器一侧的窗口规则决定，而 KWin 与 niri 的规则机制互不相通（niri 用 `config.kdl` 的 `window-rule`，KWin 用 `kwinrulesrc`），同时维护两套无法互相验证的规则等于两套都不可信。因此 **Linux 浮层只面向 KWin Wayland**：niri 规则不再支持，`docs/linux.md` 的 niri 一节被本决策取代（改写见 #4）。

## Considered Options

- **继续以 niri 为准**：本机跑不了，规则在本机是死配置，无法验证。
- **同时支持两者**：需要引入合成器探测与两套规则生成，测试面与代码面翻倍，而其中一套没有运行环境。
- **客户端自己定位**（`set_position` 或 wlr-layer-shell）：`set_position` 在 Wayland 上是空操作；layer-shell 与「交给 KWin 窗口规则」是两条不同的路，选后者就不必让应用知道合成器是谁。

## Consequences

- 浮层的真实位置**只**由 KWin 规则决定，应用侧的 `set_position` 在 Wayland 上继续是空操作；因此「浮层该在哪」这份几何必须与应用共享同一份计算，否则两边会像旧的硬编码 `(620,936)` 那样漂移。
- 「保持置顶」要在规则里补上（应用侧的 `always_on_top` 在 Wayland 只是 hint），「不抢焦点」用 `acceptfocus=false`。
- 规则落在用户侧的 `kwinrulesrc`；怎么装、怎么免得重复、怎么卸载，见 #4。
- `docs/linux.md` 中 Linux 的验证记录与 niri 规则不再代表当前目标环境。
