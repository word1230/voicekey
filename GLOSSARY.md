# VoiceKey

VoiceKey turns speech into text at the insertion point of whatever app is focused. It ships as a desktop app (Tauri shell over a Rust recognition core), an iOS keyboard, and an Android accessibility service. This glossary is the desktop app's language, and the language of its platform layer.

## Language

**Overlay**:
A borderless, transparent, always-on-top window the app shows without taking focus. The app has exactly two.
_Avoid_: popup, floating window

**HUD**:
The overlay shown during dictation — a capsule with a waveform and the live text.
_Avoid_: toast, banner, indicator

**Pick panel**:
The overlay shown after dictation when more than one channel was used, listing each channel's result so one can be chosen for insertion.
_Avoid_: candidate box, results window, chooser

**Channel** (渠道):
One configured speech-recognition provider — a cloud vendor or a local offline engine. Several may run in parallel and be compared.
_Avoid_: engine, provider, backend, model

**Caret**:
The text-insertion point inside the focused app. Knowing its position would let the pick panel sit beside it.
_Avoid_: cursor (reserve that for the mouse pointer)

**Streaming typer**:
The path that types characters into the focused app as they are recognised, before the final result is known.
_Avoid_: live typing, incremental insert

**Hold key** / **Tap key**:
The two configurable dictation gestures. The hold key records for as long as it is held; the tap key starts on a short press and stops on the next press or on silence.
_Avoid_: hotkey (too broad), shortcut

**Swallow**:
Whether the app can consume a key it observes, so the focused app never sees it. On Linux the app cannot, which is why tap keys must be a bare modifier.
_Avoid_: grab, capture, intercept

**Dictation session**:
One round of speech input, from the moment recording starts until recognised text is inserted.
_Avoid_: session (ambiguous with the desktop login session)

**Insert**:
To place recognised text into the focused app. On Linux this happens through the clipboard rather than by typing.
_Avoid_: paste (that is the mechanism, not the act)
