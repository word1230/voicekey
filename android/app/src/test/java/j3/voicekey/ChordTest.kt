package j3.voicekey

import android.view.KeyEvent.*
import org.junit.Assert.*
import org.junit.Test

class ChordTest {
    private val outs = mutableListOf<Chord.Out>()

    private fun Chord.press(code: Int, down: Boolean, eligible: Boolean = true) = key(code, down, false, { eligible }) { outs += it }

    @Test
    fun volumeDownShortPressActsAndReplays() {
        val c = Chord(Keys.DEFAULT)
        assertTrue(c.press(KEYCODE_VOLUME_DOWN, true))
        assertTrue(c.press(KEYCODE_VOLUME_DOWN, false))
        assertEquals(listOf(Chord.Arm, Chord.Disarm, Chord.Short(true, listOf(KEYCODE_VOLUME_DOWN))), outs)
    }

    @Test
    fun notEligiblePassesThrough() {
        val c = Chord(Keys.DEFAULT)
        assertFalse(c.press(KEYCODE_VOLUME_DOWN, true, eligible = false))
        assertFalse(c.press(KEYCODE_VOLUME_DOWN, false))
        assertTrue(outs.isEmpty())
    }

    @Test
    fun twoKeyChordLongPressReleasesOnFirstUp() {
        val c = Chord(listOf(KEYCODE_VOLUME_DOWN, KEYCODE_VOLUME_UP))
        c.press(KEYCODE_VOLUME_UP, true)
        assertEquals(emptyList<Chord.Out>(), outs)
        c.press(KEYCODE_VOLUME_DOWN, true)
        assertEquals(listOf(Chord.Arm), outs)
        assertTrue(c.fire(c.gen))
        c.press(KEYCODE_VOLUME_UP, false)
        c.press(KEYCODE_VOLUME_DOWN, false)
        assertEquals(listOf(Chord.Arm, Chord.Release), outs)
    }

    @Test
    fun partialChordOnlyReplaysVolume() {
        val c = Chord(listOf(KEYCODE_VOLUME_DOWN, KEYCODE_VOLUME_UP))
        c.press(KEYCODE_VOLUME_UP, true)
        c.press(KEYCODE_VOLUME_UP, false)
        assertEquals(listOf(Chord.Short(false, listOf(KEYCODE_VOLUME_UP))), outs)
    }

    @Test
    fun modifierPassesThroughAndForeignKeyCancels() {
        val c = Chord(listOf(KEYCODE_CTRL_LEFT, KEYCODE_SPACE))
        assertFalse(c.press(KEYCODE_CTRL_LEFT, true))
        assertTrue(c.press(KEYCODE_SPACE, true))
        val g = c.gen
        assertFalse(c.press(KEYCODE_A, true))
        assertFalse(c.fire(g))
        c.press(KEYCODE_A, false)
        c.press(KEYCODE_SPACE, false)
        c.press(KEYCODE_CTRL_LEFT, false)
        assertEquals(listOf(Chord.Arm, Chord.Disarm, Chord.Short(false, emptyList())), outs)
    }

    @Test
    fun validRules() {
        val printing = { c: Int -> c == KEYCODE_A || c == KEYCODE_SPACE }
        assertTrue(Keys.valid(listOf(KEYCODE_VOLUME_UP, KEYCODE_VOLUME_DOWN), printing))
        assertTrue(Keys.valid(listOf(KEYCODE_CTRL_LEFT, KEYCODE_SPACE), printing))
        assertFalse(Keys.valid(listOf(KEYCODE_SHIFT_LEFT, KEYCODE_A), printing))
        assertFalse(Keys.valid(listOf(KEYCODE_A), printing))
        assertFalse(Keys.valid(listOf(KEYCODE_BACK), printing))
        assertFalse(Keys.valid(emptyList(), printing))
    }
}
