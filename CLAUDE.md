## 仓库布局（命令落点）

- `app/` —— 桌面端（Tauri + Svelte）。`npm ci` / `npm test` / `npx tauri dev|build` 都在这里跑：仓库根**没有** `package.json`。装依赖前先看 `docs/linux.md` 的 npm gotcha。
- 仓库根 —— Cargo workspace。`cargo check --workspace --all-targets` / `cargo test --workspace` 在这里跑。Rust 侧在 `crates/`（core 识别、platform 平台层），应用层在 `app/src-tauri/`（浮层 hud/pick 与托盘都在 `ui.rs`）。Rust 侧合计约 3.7k 行：直接读文件比派搜索子代理便宜。
- 共享代码放 `crates/`：`app/src-tauri` 是纯 bin（没有 `[lib]`），别的 crate 依赖不了它。要对外暴露 CLI，照 `crates/core/src/bin/vk-test.rs` 的先例。
- `crates/platform/src/{mac,win,linux}.rs` 三平台接口同步，以 `mac.rs` 为准。
- Linux 平台层、本机验收手法（含「macOS / Windows 仍能编译」这类 AC 在本机能验到哪）：`docs/linux.md`。浮层目标已锁定 KWin Wayland（`docs/adr/0001-*`）。
- 上游同步（`sourcecode/` 是 J3n5en/voicekey 的克隆）：`docs/upstream-sync.md`——merge 不 rebase，冲突与版本策略都在里面。
- 提交与推送会跑 `.githooks/` 里的检查（提交：cargo check 无警告 + 前端 `npm run check` 类型检查与单测；推送：全量测试）。跳过用 `--no-verify`。

## Agent skills

### Issue tracker

Issues and specs live as GitHub issues on `word1230/voicekey`, read and written with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles, using the default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` at the repo root covering the desktop app and its platform layer, with ADRs in `docs/adr/`. See `docs/agents/domain.md`.
