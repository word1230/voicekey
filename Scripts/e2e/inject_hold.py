#!/usr/bin/env python3
"""端到端验收的注入脚本：造一把 uinput 键盘，按住右 Alt，直到 stop 文件出现。

两个坑写在注释里，因为都踩过：
  1. 写完 EV_KEY 必须补一个 EV_SYN，否则内核不把这一包交给任何 evdev 读者（应用收不到、
     KWin 也收不到），症状是"钩子坏了"。
  2. 设备名不能叫 voicekey-virtual（那是应用自己的虚拟键盘，钩子会跳过），并且建好之后要等
     3 秒再按——应用的钩子每 2 秒扫一次 /dev/input，太早按下没人收。

用法：inject_hold.py <stop 文件> [设备名]
  stop 文件出现时松开并退出；设备名默认 vk-e2e-hold。
输出：DOWN <ts> / UP <ts>——与靶子日志的 CHANGED 相减就是"松手到落字"耗时。
"""
import os
import sys
import time

from evdev import UInput, ecodes as e


def main() -> None:
    stop = sys.argv[1] if len(sys.argv) > 1 else "/tmp/vk-stop"
    name = sys.argv[2] if len(sys.argv) > 2 else "vk-e2e-hold"
    try:
        os.unlink(stop)
    except FileNotFoundError:
        pass

    keys = [e.KEY_RIGHTALT] + [getattr(e, f"KEY_{c}") for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ"]
    ui = UInput({e.EV_KEY: keys}, name=name)
    # 让钩子先认领这个新设备：应用每 2 秒扫一次 /dev/input，这里留 3 秒余量
    time.sleep(3.0)

    ui.write(e.EV_KEY, e.KEY_RIGHTALT, 1)
    ui.syn()
    print(f"DOWN {time.time():.3f}", flush=True)

    while not os.path.exists(stop):
        time.sleep(0.05)

    ui.write(e.EV_KEY, e.KEY_RIGHTALT, 0)
    ui.syn()
    print(f"UP {time.time():.3f}", flush=True)
    ui.close()


if __name__ == "__main__":
    main()
