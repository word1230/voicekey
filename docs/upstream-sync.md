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

重点看上游有没有碰 `crates/platform/`。截至 v1.7.0 **没碰过**，所以 Linux 平台层原样保留；
如果哪天碰了，按 `CLAUDE.md` 的约定以 `mac.rs` 为准把三平台接口对齐。

## 2. 试合并（不动工作区）

```bash
git merge-tree --write-tree --name-only --no-messages HEAD upstream/main
```

退出码 1 + 文件名列表就是会冲突的文件。v1.7.0 那次的预期是：

| 文件 | 解法 |
|---|---|
| `Cargo.lock` | `git checkout --theirs Cargo.lock`，再 `cargo check --workspace --all-targets` 把 fork 的 evdev/uinput 依赖补回来 |
| `app/src-tauri/src/ui.rs` | 保留 fork 的 `Area::units` / `set_pos(physical)` 重构，叠加上游的 `cap_len` / `clamp_axis` / `logical_h` 防 panic 修复 |
| `app/src/pages/Onboard.svelte` | 保留 fork 的 `injectBlocked` 告警分支，内层文案用上游的 `silenceStop` |
| `app/src/lib/api.ts` | 两边的 `Settings` 字段都保留（`muteWhileRecording` 与 `holdCombo` / `silenceStop` / `pickCounts`） |

`main.rs`、`settings.rs`、`controller.rs` 通常能自动合并，但要读一遍语义，上游改过：

- `hold_code()` → `hold_codes()` + `c.set_hold(...)`（自定义长按组合）
- `Settings` 新增 `hold_combo` / `silence_stop` / `pick_counts`，`Channel` 新增 `Chatterfly`
- `Segments::partial` 返回 `Option<bool>`，识别文字变化也算“在说话”

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
