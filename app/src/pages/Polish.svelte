<script lang="ts">
  import { app, save } from "../lib/store.svelte";

  const s = $derived(app.s!);
  const ai = $derived(s.ai);
  const setAi = (patch: Partial<typeof ai>) => save({ ai: { ...ai, ...patch } });
</script>

<h2>AI 整理</h2>
<div class="sub">选中文字后按整理快捷键（在「快捷键」页设置），交给 AI 整理后替换原内容。接口为 OpenAI 兼容格式，整理时文字会发送给该服务。</div>

<div class="group">
  <div class="row">
    <div class="lbl">启用 AI 整理<small>关闭后整理快捷键不起作用</small></div>
    <button class="sw" class:on={ai.enabled} onclick={() => setAi({ enabled: !ai.enabled })} aria-label="启用 AI 整理"></button>
  </div>
  <div class="row">
    <div class="lbl">接口地址<small>填到 /v1，请求发到它的 /chat/completions</small></div>
    <input class="txt" type="text" placeholder="https://api.example.com/v1" value={ai.baseUrl} oninput={(e) => setAi({ baseUrl: e.currentTarget.value.trim() })} />
  </div>
  <div class="row">
    <div class="lbl">API Key<small>服务不要求鉴权时留空（如本机 Ollama）</small></div>
    <input class="txt" type="password" placeholder="sk-…" value={ai.apiKey} oninput={(e) => setAi({ apiKey: e.currentTarget.value.trim() })} />
  </div>
  <div class="row">
    <div class="lbl">模型名</div>
    <input class="txt" type="text" placeholder="deepseek-chat / qwen2.5 …" value={ai.model} oninput={(e) => setAi({ model: e.currentTarget.value.trim() })} />
  </div>
</div>

<h3>整理提示词</h3>
<div class="group">
  <div class="pad">
    <textarea rows="5" value={ai.prompt} oninput={(e) => setAi({ prompt: e.currentTarget.value })}></textarea>
    <div class="note">发一段口述原文，模型只回整理后的文字。想换风格（如深度润色）直接改这里。</div>
  </div>
</div>

<style>
  .txt { width: 300px; border: 1px solid var(--line); border-radius: var(--rs); background: var(--card2); padding: 7px 10px; font-size: 12.5px; }
  .txt:focus { outline: none; border-color: var(--accent); }
  .pad { padding: 12px 16px; }
  textarea { width: 100%; box-sizing: border-box; resize: vertical; border: 1px solid var(--line); border-radius: var(--rs); background: var(--card2); padding: 8px 10px; font: inherit; line-height: 1.6; }
  textarea:focus { outline: none; border-color: var(--accent); }
</style>
