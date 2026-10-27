package j3.voicekey

import android.view.KeyEvent.*

/** 长按说话的按键：音量键、耳机键、外接键盘键等任意组合 */
object Keys {
    val DEFAULT = listOf(KEYCODE_VOLUME_DOWN)
    private val shifts = setOf(KEYCODE_SHIFT_LEFT, KEYCODE_SHIFT_RIGHT)

    /** 外接键盘修饰键：不吞，保留系统组合键 */
    val modifiers = shifts + setOf(
        KEYCODE_CTRL_LEFT, KEYCODE_CTRL_RIGHT, KEYCODE_ALT_LEFT, KEYCODE_ALT_RIGHT,
        KEYCODE_META_LEFT, KEYCODE_META_RIGHT, KEYCODE_FUNCTION,
    )
    val volume = mapOf(KEYCODE_VOLUME_UP to 1, KEYCODE_VOLUME_DOWN to -1)

    /** 录制时不接收、留给系统的导航键 */
    val system = setOf(KEYCODE_BACK, KEYCODE_HOME, KEYCODE_APP_SWITCH, KEYCODE_POWER)

    /** 会打字的键须配 Shift 以外的修饰键，且最多一个，否则正常打字会被吞 */
    fun valid(codes: List<Int>, printing: (Int) -> Boolean): Boolean {
        if (codes.isEmpty() || codes.any { it in system }) return false
        val typed = codes.filter(printing)
        return typed.isEmpty() || typed.size == 1 && codes.any { it in modifiers && it !in shifts }
    }

    fun name(code: Int): String = when (code) {
        KEYCODE_VOLUME_DOWN -> "音量下键"
        KEYCODE_VOLUME_UP -> "音量上键"
        KEYCODE_HEADSETHOOK -> "耳机键"
        KEYCODE_CAMERA -> "相机键"
        KEYCODE_MEDIA_PLAY_PAUSE -> "播放/暂停"
        KEYCODE_SHIFT_LEFT -> "左 Shift"
        KEYCODE_SHIFT_RIGHT -> "右 Shift"
        KEYCODE_CTRL_LEFT -> "左 Ctrl"
        KEYCODE_CTRL_RIGHT -> "右 Ctrl"
        KEYCODE_ALT_LEFT -> "左 Alt"
        KEYCODE_ALT_RIGHT -> "右 Alt"
        KEYCODE_META_LEFT -> "左 Meta"
        KEYCODE_META_RIGHT -> "右 Meta"
        KEYCODE_FUNCTION -> "Fn"
        else -> keyCodeToString(code).removePrefix("KEYCODE_").lowercase().replaceFirstChar { it.uppercase() }
    }

    fun name(codes: List<Int>) = codes.joinToString(" + ") { name(it) }
}

/** 组合长按：全部按齐（无多余键）计时，超过阈值开始；松开任一组合键结束。修饰键不吞，其余组合键在可用时吞掉 */
class Chord(private val codes: List<Int>) {
    sealed interface Out
    data object Arm : Out
    data object Disarm : Out
    data object Release : Out
    /** 本轮未达长按：act 为完整按过且吞了键（可当作点按），replay 为需补回系统的音量键 */
    data class Short(val act: Boolean, val replay: List<Int>) : Out

    private val keys = codes.toSet()
    private val held = mutableSetOf<Int>()
    private val eaten = mutableListOf<Int>()
    private var armed = false
    private var long = false
    private var full = false
    private var round = false
    var gen = 0
        private set

    val engaged get() = round

    /** eligible 仅在组合键按下时求值；返回 true 吞掉该事件 */
    fun key(code: Int, down: Boolean, repeat: Boolean, eligible: () -> Boolean, out: (Out) -> Unit): Boolean {
        if (down && repeat) return code in eaten
        if (down) {
            held += code
            if (code !in keys) {
                if (armed && !long) disarm(out)
                full = false
                return false
            }
            if (!round && !eligible()) return false
            round = true
            if (!armed && held == keys) {
                armed = true
                full = true
                gen++
                out(Arm)
            }
            if (code in Keys.modifiers) return false
            eaten += code
            return true
        }
        held -= code
        val swallow = code in eaten
        if (code !in keys || !round) return swallow
        if (armed) {
            if (long) {
                armed = false
                out(Release)
            } else disarm(out)
        }
        if (held.none { it in keys }) {
            if (!long) out(Short(full && eaten.isNotEmpty(), eaten.filter { it in Keys.volume }))
            eaten.clear()
            long = false
            full = false
            round = false
        }
        return swallow
    }

    /** 计时到点；返回 true 表示开始长按 */
    fun fire(g: Int): Boolean {
        if (g != gen || !armed) return false
        long = true
        return true
    }

    private fun disarm(out: (Out) -> Unit) {
        armed = false
        gen++
        out(Disarm)
    }

    fun reset() {
        held.clear()
        eaten.clear()
        armed = false
        long = false
        full = false
        round = false
        gen++
    }
}
