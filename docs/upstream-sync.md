# 同步上游（J3n5en/voicekey）

这个仓库是 [J3n5en/voicekey](https://github.com/J3n5en/voicekey) 的 fork，加了 Linux 平台层
（`crates/platform/src/linux.rs`）与 KWin 浮层。上游发版后按本文同步；日常开发不涉及。

## 原则

- **merge，不 rebase**。fork 的 `main` 已经推到 `origin`（word1230/voicekey），rebase 会改写
  已发布的 14 个提交，需要 force push；而且冲突要在每个提交上重解一遍。merge 只解一次。
- **版本号跟随上游**。`Cargo.toml` 的 `workspace.package.version` 与
  `app/src-tauri/tauri.conf.json` 的 `version` 都取上游的值。
- **上游的 iOS / Android 改动原样带入**。它们与 Linux 无关，但合并成本为零，跳过反而让
  下次同步更难。

## 0. 准备

先把 fork 的工作区弄干净：未提交的改动会挡住合并，先提交或 `git stash`。

上游来源是本机克隆 `sourcecode/voicekey`（已在 `.gitignore` 里）。刷新它：

```bash
git -C sourcecode/voicekey fetch origin
git -C sourcecode/voicekey checkout main
git -C sourcecode/voicekey pull --ff-only
```

也可以配成远端，之后直接用 `upstream/main`：

```bash
git remote add upstream https://github.com/J3n5en/voicekey.git
git fetch upstream
```

## 1. 看清分叉

```bash
# 把克隆的上游拿进 fork 的对象库（也顺带让 upstream/main 可用）
git fetch "$PWD/sourcecode/voicekey" main:refs/remotes/upstream/main

BASE=$(git merge-base HEAD upstream/main)
git log --oneline "$BASE"..upstream/main          # 上游新增
git log --oneline upstream/main..HEAD             # fork 独有
git rev-list --left-right --count HEAD...upstream/main
git diff --name-only "$BASE"..upstream/main
```

重点看上游有没有碰 `crates/platform/`。截至 v1.11.0 仍**没碰过**（只加了 `volume.rs`，
靠 `crate::imp::output_volume_*` 回调各平台），所以 Linux 平台层主体原样保留——但要在
`linux.rs` 补三个 `output_volume_*` 空桩（返回 `None`），否则上游的 ducking 线程在 Linux
编译不过；如果哪天碰了，按 `CLAUDE.md` 的约定以 `mac.rs` 为准把三平台接口对齐。

## 2. 试合并（不动工作区）

```bash
git merge-tree --write-tree --name-only --no-messages HEAD upstream/main
```

退出码 1 + 文件名列表就是会冲突的文件。v1.11.0 那次（fork 基线 v1.7.0，上游 +41 提交）是
8 个文件，预期与解法：

| 文件 | 解法 |
|---|---|
| `Cargo.lock` | `git checkout --theirs Cargo.lock`，再 `cargo check --workspace --all-targets` 把 fork 的 evdev/uinput 依赖补回来 |
| `app/src-tauri/Cargo.toml` | fork 的 `[target.linux] gtk` 与上游的 `objc2-app-kit`/`objc2-foundation` 都保留 |
| `app/src-tauri/src/controller.rs` | fork 的 `mute_output`/`set_tray_active`/`record_started`/`insert` 预检 与上游的 `chime_gen` 门闩/`play_after_restore`/Jev 评选 语义交织，逐处合并 |
| `app/src-tauri/src/settings.rs` | fork 字段（`mute_while_recording`/`polish_shortcut`/`ai`）与上游字段（`sound_*`/`lower_system_volume*`/`jev`）都留；`multi` 默认过滤用上游版（`login().is_none()`） |
| `app/src-tauri/src/ui.rs` | 保留 fork 的 `TRAY_ACTIVE`/`set_tray_active`（上游 v1.10 删了录音态高亮，决策是保留）；`create_overlays` 里的 pick 创建删掉，交给上游的 `sync_pick` 按需创建/销毁 |
| `app/src/lib/api.ts` | 两边 `Settings` 字段并集：fork 的 `muteWhileRecording`/`polishShortcut`/`ai` + 上游的 `sound*`/`lowerSystemVolume*`/`jev` |
| `crates/platform/src/lib.rs` | fork 主体（`inject`/`overlay`/`kwin`/`OutputMute`）+ 追加上游的 `mod volume`/`OutputVolumeDucker` |
| `crates/platform/src/mac.rs` | 注释二选一，取上游的（多了 autorelease pool 说明） |

v1.7.0 那次的历史预期（遗留参考）：`ui.rs` 保 fork 的 `Area::units`/`set_pos(physical)`、
`Onboard.svelte` 保 fork 的 `injectBlocked` 分支。

`main.rs` 自动合并，但读一遍：上游加了 `mod sound`/`mod login`、`ui::sync_pick` 调用
（pick 面板按 `s.channel == Channel::All` 按需创建），fork 的 `mod polish`/`inject_problem`
预检都还在。

## 3. 合并

```bash
git merge upstream/main
# 解冲突、cargo check、git add、提交
```

合并提交保留两个父提交，不要 squash。

## 4. 验证

```bash
Scripts/check.sh --full
```

再按 `docs/linux.md` 做一遍本机验收；v1.7.0 那次至少覆盖了：

- HUD 几何（`620,936 680x120`、`layer=3`、`wantsInput=false`）
- 单渠道说话 → 识别 → 落进输入框
- 自定义长按组合（左 Ctrl + 左 Alt）
- pick 面板（`channel=all`）
- 「录音时静音系统」的恢复原状

## 5. 收尾

```bash
git rev-list --left-right --count HEAD...upstream/main   # 右边应为 0
```

## 坑

- **tag 会跟着 fetch 进来**：`git fetch <clone> main:...` 默认 auto-follow 指向该历史的 tag。
  不想要就加 `--no-tags`；留着也无副作用。
- **updater 指向上游**：`app/src-tauri/tauri.conf.json` 的 updater endpoint 是
  `J3n5en/voicekey` 的 `latest.json`。自己发布 fork 构建前改掉或关掉，否则应用内更新会拉上游的包。
- **上游有未发布提交时**：`sourcecode/voicekey` 跟的是 `origin/main`，不一定等于最新的 release tag；
  要按 tag 同步就 `git checkout v1.7.0` 再 fetch。
