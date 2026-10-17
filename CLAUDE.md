## 仓库布局（命令落点）

- `app/` —— 桌面端（Tauri + Svelte）。`npm ci` / `npm test` / `npx tauri dev|build` 都在这里跑：仓库根**没有** `package.json`。装依赖前先看 `docs/linux.md` 的 npm gotcha。
- 仓库根 —— Cargo workspace。`cargo check|test --workspace` 在这里跑。Rust 侧在 `crates/`（core 识别、platform 平台层），应用层在 `app/src-tauri/`。
- `crates/platform/src/{mac,win,linux}.rs` 三平台接口同步，以 `mac.rs` 为准。
- Linux 平台层与验收手法：`docs/linux.md`。浮层目标已锁定 KWin Wayland（`docs/adr/0001-*`）。
- 提交与推送会跑 `.githooks/` 里的检查（提交：cargo check 无警告 + 前端单测；推送：全量测试）。跳过用 `--no-verify`。

## Agent skills

### Issue tracker

Issues and specs live as GitHub issues on `word1230/voicekey`, read and written with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical triage roles, using the default label strings (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` at the repo root covering the desktop app and its platform layer, with ADRs in `docs/adr/`. See `docs/agents/domain.md`.
