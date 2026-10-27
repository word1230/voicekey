//! AI 整理：选中文字 → Ctrl+C 取到剪贴板 → OpenAI 兼容接口 → 结果覆盖原选区
use crate::settings::{AiPolish, Settings};
use crate::{ui, AppState};
use anyhow::{Context, Result};
use std::time::Duration;
use tauri::Manager;
use voicekey_platform as pf;

/// 触发一次整理。调用方保证当前没有录音/候选（Idle）；
/// 全程在独立线程里跑，取选中文本与请求都可能阻塞
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let s = app.state::<AppState>().settings.read().unwrap().clone();
        run(&app, &s);
    });
}

fn run(app: &tauri::AppHandle, s: &Settings) {
    let ai = &s.ai;
    if !ai.enabled {
        ui::hud_error(app, "AI 整理未开启，在设置的「AI 整理」页打开");
        return;
    }
    if ai.base_url.is_empty() || ai.model.is_empty() {
        ui::hud_error(app, "AI 整理未配置：填写接口地址与模型名");
        return;
    }
    ui::hud(app, "wait", "整理选中的内容…", s.channel);
    let Some(text) = pf::copy_selection() else {
        ui::hud_error(app, "没有取到选中的内容：先选中文字再按");
        return;
    };
    match tidy(ai, &text) {
        // 选区还在时 Ctrl+V 即覆盖；等待期间焦点挪走则贴到新位置（无法恢复选区，平台限制）。
        // 结果按定稿语义留在剪贴板（paste_keep），不恢复
        Ok(out) if !out.trim().is_empty() => {
            pf::paste_keep(out.trim());
            ui::hud(app, "info", "已整理并替换", s.channel);
            ui::hud_hide(app, 1200);
        }
        Ok(_) => ui::hud_error(app, "AI 返回了空结果"),
        Err(e) => ui::hud_error(app, &format!("整理失败：{e:#}")),
    }
}

async fn chat(ai: &AiPolish, text: &str) -> Result<String> {
    let url = format!("{}/chat/completions", ai.base_url.trim_end_matches('/'));
    let mut req = reqwest::Client::new()
        .post(&url)
        .timeout(Duration::from_secs(60))
        .json(&serde_json::json!({
            "model": ai.model,
            "messages": [
                {"role": "system", "content": ai.prompt},
                {"role": "user", "content": text},
            ],
            "temperature": 0.2,
        }));
    if !ai.api_key.is_empty() {
        req = req.bearer_auth(&ai.api_key);
    }
    let resp = req.send().await?.error_for_status()?;
    let v: serde_json::Value = resp.json().await?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(String::from)
        .context("回复里没有 choices[0].message.content")
}

/// chat 是 async 而取选中文本要阻塞线程，统一到独立线程 + block_on
fn tidy(ai: &AiPolish, text: &str) -> Result<String> {
    tauri::async_runtime::block_on(chat(ai, text))
}
