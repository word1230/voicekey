<script lang="ts">
  import { onMount } from "svelte";
  import { app, save, refreshMics } from "../lib/store.svelte";
  import { call, listSounds, previewSound } from "../lib/api";
  import Meter from "../lib/Meter.svelte";

  const s = $derived(app.s!);
  const missing = $derived(s.mic && !app.mics.includes(s.mic));
  let sounds = $state<{ id: string; name: string }[]>([]);
  // 设置值和 UI 都直接表示“降低比例”；100% 只改变展示为“静音”，数值本身不做反转。
  const duckReductionLabel = $derived(s.lowerSystemVolumePercent === 100 ? "静音" : `−${s.lowerSystemVolumePercent}%`);

  onMount(() => {
    refreshMics();
    listSounds().then((v) => (sounds = v));
    // 电平表只在设置窗处于前台时占用麦克风，切到其他应用立即释放。
    const onFocus = () => {
      refreshMics();
      call("meter", { on: true });
    };
    const onBlur = () => {
      call("meter", { on: false });
    };
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    call("meter", { on: true });
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      call("meter", { on: false });
    };
  });

  function pick(v: string) {
    save({ mic: v });
  }

  // 面板行：标题按会话时刻命名，副标题只描述该时刻（不提发声与否）；
  // key 对应后端逐事件设置字段（存 macOS 系统音名或 none）。
  // 无「识别完成」行：识别很快，处理音即完成信号，独立完成音会与之短间隔连响
  const CHIME_ROWS: { title: string; desc: string; key: "soundStart" | "soundProcessing" | "soundError" }[] = [
    { title: "录音开始", desc: "开始录入语音时", key: "soundStart" },
    { title: "识别处理", desc: "录音结束，进入识别处理", key: "soundProcessing" },
    { title: "识别失败", desc: "出错、超时或没有内容时", key: "soundError" },
  ];
</script>

<h2>音频</h2>
<div class="sub" aria-hidden="true">&nbsp;</div>
<h3>输入设备</h3>
<div class="group">
  <div class="row">
    <div class="lbl">麦克风<small>用于识别的麦克风，设备断开时自动切回系统默认</small></div>
    <select class="sel" value={s.mic} onchange={(e) => pick(e.currentTarget.value)}>
      <option value="">系统默认</option>
      {#each app.mics as m}<option value={m}>{m}</option>{/each}
      {#if missing}<option value={s.mic}>已断开（暂用系统默认）</option>{/if}
    </select>
  </div>
  <div class="row">
    <div class="lbl">录音时静音系统<small>按下快捷键开始说话时静音系统输出，结束后恢复；识别始终只从上面的麦克风读取</small></div>
    <button class="sw" class:on={s.muteWhileRecording} onclick={() => save({ muteWhileRecording: !s.muteWhileRecording })} aria-label="录音时静音系统"></button>
  </div>
  <div class="row">
    <div class="lbl">输入电平<small>对着麦克风说话测试</small></div>
    <Meter />
  </div>
</div>
<h3>系统音频</h3>
<div class="group">
  <div class="row">
    <div class="lbl">录音时降低音量<small>减少背景声音的干扰，结束后自动恢复</small></div>
    {#if s.lowerSystemVolume}
      <!-- 录音中拖动比例会即时生效；worker 从当前实际音量平滑重定向到新目标，不等待当前过渡完成。 -->
      <input class="duck-range" type="range" min="20" max="100" step="5" value={s.lowerSystemVolumePercent} oninput={(e) => save({ lowerSystemVolumePercent: +e.currentTarget.value })} aria-label="录音时降低音量比例" aria-valuetext={s.lowerSystemVolumePercent === 100 ? "静音" : `降低 ${s.lowerSystemVolumePercent}%`} />
      <b class="duck-value">{duckReductionLabel}</b>
    {/if}
    <button class="sw" class:on={s.lowerSystemVolume} onclick={() => save({ lowerSystemVolume: !s.lowerSystemVolume })} aria-label="录音时降低音量"></button>
  </div>
</div>

<h3>提示音</h3>
<div class="group">
  <div class="row">
    <div class="lbl">提示音<small>{s.soundEnabled ? "录音各阶段播放提示音" : "已关闭，录音各阶段不播放提示音"}</small></div>
    <button class="sw" class:on={s.soundEnabled} onclick={() => save({ soundEnabled: !s.soundEnabled })} aria-label="提示音"></button>
  </div>
  <!-- 逐事件系统铃声选择：每行一个选择器（不发声 + macOS 系统铃声），试听即所得 -->
  {#each CHIME_ROWS as row (row.key)}
    <div class="row">
      <div class="lbl">{row.title}<small>{row.desc}</small></div>
      <select class="sel" value={s[row.key]} onchange={(e) => save({ [row.key]: e.currentTarget.value })} aria-label={row.title}>
        <option value="none">不发声</option>
        {#each sounds as snd}<option value={snd.id}>{snd.name}</option>{/each}
      </select>
      <!-- 「不发声」时试听置灰（不可点），保持行内布局稳定 -->
      <button class="btn" disabled={s[row.key] === "none"} onclick={() => previewSound(s[row.key])} aria-label={`试听${row.title}`}>试听</button>
    </div>
  {/each}
</div>

<style>
  .duck-range { width: 160px; accent-color: var(--accent); }
  .duck-value { width: 42px; text-align: right; font-variant-numeric: tabular-nums; }
  .btn:disabled { opacity: 0.4; cursor: default; }
</style>
