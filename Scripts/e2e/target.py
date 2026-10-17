#!/usr/bin/env python3
"""端到端验收的靶子：一扇 GTK 窗口 + Gtk.Entry，把发生的事逐行写进日志。

判据是日志，不是截图——`CHANGED len=N text='…'` 那一行就是"字落进输入框了"。
用法：target.py <日志文件>，窗口标题会同步显示当前长度（截图时一眼能看出进了几个字）。

日志行（每行前缀是 time.time() 的秒级小数，与注入脚本的 DOWN/UP 时间戳可比）：
    <ts> START                       进程起来
    <ts> CHANGED len=N text='…'      Entry 内容变了 —— 上屏成功的判据
    <ts> KEY <keyval> state=<n>      按键落到这个窗口（state 是 GdkModifierType 位掩码）
    <ts> PASTE-CLIPBOARD              有人往 Entry 里粘贴（Ctrl+V 真的到了）
"""
import sys
import time

import gi

gi.require_version("Gtk", "3.0")
from gi.repository import Gtk  # noqa: E402


def main() -> None:
    log_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/vk-target.log"
    log = open(log_path, "a", buffering=1)

    def note(msg: str) -> None:
        log.write(f"{time.time():.3f} {msg}\n")

    win = Gtk.Window(title="vk-target")
    win.set_default_size(560, 120)
    entry = Gtk.Entry()
    win.add(entry)

    def on_changed(e: Gtk.Entry) -> None:
        text = e.get_text()
        note(f"CHANGED len={len(text)} text={text!r}")
        win.set_title(f"vk-target len={len(text)}")

    def on_key(_w, event) -> bool:
        note(f"KEY {event.keyval} state={event.state}")
        return False

    def on_paste(_w) -> None:
        note("PASTE-CLIPBOARD")

    entry.connect("changed", on_changed)
    entry.connect("key-press-event", on_key)
    entry.connect("paste-clipboard", on_paste)
    win.connect("destroy", Gtk.main_quit)

    note("START")
    win.show_all()
    Gtk.main()


if __name__ == "__main__":
    main()
