package j3.voicekey

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo

/** 一次说话：录音 → 一个或多个渠道识别 → 写入输入框。状态只在主线程读写 */
class Session(private val svc: VoiceService) {
    enum class State { Listen, Wait, Final, Error }

    class Row(val ch: Channel) {
        var text = ""
        var state = State.Listen
        var ms: Long? = null
        var handle = 0L
    }

    private val main = Handler(Looper.getMainLooper())
    private var gen = 0
    private var recorder: Recorder? = null
    private var target: AccessibilityNodeInfo? = null
    private var typer: Typer? = null
    private var typed = false
    private var autoStop = false
    private var heard = false
    private var lastVoice = 0L
    private var releasedAt = 0L
    private var userPicked = false
    private var choosePending = false
    private var chooseSend = false

    var rows: List<Row> = emptyList()
        private set
    var sel = 0
        private set
    var picking = false
        private set
    val active get() = rows.isNotEmpty()
    val recording get() = recorder != null

    private fun channels(): List<Channel> = App.prefs.channel?.let { listOf(it) } ?: App.prefs.multi

    fun prewarm() {
        if (!active) channels().filter { it.ready }.forEach { Native.prewarm(it.id) }
    }

    /** 点按：没在说就开始（静音自动结束），在录就结束，候选框开着就取消 */
    fun tap() = when {
        recording -> end()
        active -> abort(null)
        else -> begin(true)
    }

    fun holdStart() {
        if (!active) begin(false)
    }

    fun holdEnd() {
        if (!autoStop) end()
    }

    private fun begin(auto: Boolean) {
        val chs = channels()
        val ready = chs.filter { it.ready }
        if (ready.isEmpty()) return svc.hud.error("${chs.first().title}模型未下载，请在 VoiceKey 里下载")
        MicService.start(svc)
        val g = ++gen
        target = svc.focusedInput()
        rows = (if (ready.size > 1) ready.sortedByDescending { App.prefs.pickCount(it) } else ready).map { Row(it) }
        picking = rows.size > 1
        userPicked = false
        choosePending = false
        chooseSend = false
        sel = 0
        if (picking) {
            rows.indexOfFirst { it.ch == App.prefs.lastPick }.takeIf { it >= 0 }?.let { sel = it; userPicked = true }
        }
        for (r in rows) {
            r.handle = Native.start(r.ch.id, Recorder.RATE) { kind, text -> main.post { event(g, r, kind, text) } }
            if (r.handle == 0L) {
                r.state = State.Error
                r.text = "渠道不可用"
            }
        }
        typer = if (!picking && App.prefs.streaming) target?.let(::Typer) else null
        typed = false
        val handles = rows.map { it.handle }.filter { it != 0L }.toLongArray()
        recorder = try {
            Recorder(
                onPcm = { buf, n -> for (h in handles) Native.push(h, buf, n) },
                onLevel = { v -> main.post { if (g == gen) level(v) } },
                onSilenced = { main.post { if (g == gen) abort("系统不允许后台录音，请打开一次 VoiceKey 再试") } },
            )
        } catch (e: Exception) {
            return abort(e.message ?: "麦克风不可用")
        }
        autoStop = auto
        heard = false
        lastVoice = SystemClock.elapsedRealtime()
        svc.onSessionChanged()
    }

    private fun level(v: Float) {
        svc.onLevel(v)
        if (!autoStop || !recording || !App.prefs.silenceStop) return
        val now = SystemClock.elapsedRealtime()
        if (v > 0.3f) {
            heard = true
            lastVoice = now
        } else if (now - lastVoice > if (heard) (App.prefs.silence * 1000).toLong() else 8000L) {
            end()
        }
    }

    fun end() {
        val r = recorder ?: return
        recorder = null
        r.stop()
        for (row in rows) {
            if (row.handle != 0L) Native.finish(row.handle)
            if (row.state == State.Listen) row.state = State.Wait
        }
        releasedAt = SystemClock.elapsedRealtime()
        val g = gen
        main.postDelayed({ if (g == gen && rows.any { it.state == State.Wait }) abort("识别超时") }, 15_000)
        svc.onSessionChanged()
    }

    private fun event(g: Int, row: Row, kind: Int, text: String) {
        if (g != gen || row.state == State.Final || row.state == State.Error) return
        when (kind) {
            Native.RECONNECT -> return svc.hud.info(text)
            Native.PARTIAL -> {
                // 识别文字仍在变化即视为在说话，避免声音小时被静音判定中途截断
                if (recording && text != row.text) {
                    heard = true
                    lastVoice = SystemClock.elapsedRealtime()
                }
                row.text = text
                if (typer?.update(text) == true) typed = true
            }
            Native.FINAL -> {
                if (text.isNotEmpty()) row.text = text
                if (row.text.isEmpty()) {
                    row.text = "没有识别到内容"
                    row.state = State.Error
                } else {
                    row.state = State.Final
                }
                row.ms = if (recording) 0 else SystemClock.elapsedRealtime() - releasedAt
            }
            else -> {
                if (row.text.isEmpty()) {
                    row.text = text
                    row.state = State.Error
                } else {
                    row.state = State.Final
                    row.ms = if (recording) 0 else SystemClock.elapsedRealtime() - releasedAt
                }
            }
        }
        if (kind == Native.PARTIAL) return svc.onSessionChanged()
        if (!picking) return single(row)
        val cur = rows[sel].state
        if (row.state == State.Final && cur != State.Final && (!userPicked || cur == State.Error)) sel = rows.indexOf(row)
        if (choosePending && rows[sel].state == State.Error) rows.indexOfFirst { it.state == State.Final }.takeIf { it >= 0 }?.let { sel = it }
        if (choosePending && rows[sel].state == State.Final) return choose(sel, chooseSend)
        if (rows.none { it.state == State.Listen || it.state == State.Wait }) choosePending = false
        svc.onSessionChanged()
    }

    private fun single(row: Row) {
        val t = typer
        val node = target
        close()
        when {
            row.state == State.Final -> if (t?.update(row.text) != true) Inject.insert(svc, node, row.text)
            typed -> {}
            else -> svc.hud.error(if (row.text == "没有识别到内容") row.text else "识别失败：${row.text}")
        }
    }

    /** 选一行上屏，send 时随后点发送；还在识别的行先结束录音，定稿后自动上屏 */
    fun choose(i: Int, send: Boolean = false) {
        val r = rows.getOrNull(i) ?: return
        sel = i
        userPicked = true
        when (r.state) {
            State.Listen, State.Wait -> {
                choosePending = true
                chooseSend = send
                end()
                svc.onSessionChanged()
            }
            State.Final -> {
                App.prefs.lastPick = r.ch
                if (picking) App.prefs.countPick(r.ch)
                val node = target
                close()
                if (Inject.insert(svc, node, r.text) && send) svc.send(node)
            }
            State.Error -> svc.onSessionChanged()
        }
    }

    /** 候选框里换到下一条可上屏的行 */
    fun next() {
        if (!picking) return
        val n = rows.size
        (1..n).map { (sel + it) % n }.firstOrNull { rows[it].state != State.Error }?.let {
            sel = it
            userPicked = true
            svc.onSessionChanged()
        }
    }

    fun abort(message: String?) {
        val row = if (picking) rows.getOrNull(sel) else rows.firstOrNull()
        val keep = if (message != null && row != null && row.state != State.Error) row.text.takeIf { it.isNotEmpty() } else null
        val t = typer
        val node = target
        val already = typed
        close()
        if (keep != null) {
            if (t?.update(keep) != true && !already) Inject.insert(svc, node, keep)
            svc.hud.info("$message，已保留已识别的文字")
        } else {
            message?.let(svc.hud::error)
        }
    }

    private fun close() {
        gen++
        recorder?.stop()
        recorder = null
        for (r in rows) if (r.handle != 0L) Native.free(r.handle)
        rows = emptyList()
        picking = false
        typer = null
        target = null
        svc.onSessionChanged()
    }
}
