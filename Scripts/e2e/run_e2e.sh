#!/bin/bash
# 端到端验收一轮：起沙箱应用 → 起靶子 → 注入按住右 Alt → 播 wav → 松开 → 看靶子日志。
#
# 用法：run_e2e.sh [渠道] [工作目录]
#   channel  要验的渠道，默认 wetype（写进沙箱 settings.json，见 docs/linux.md「端到端替身」）
#   workdir  放日志与 stop 文件的目录，默认 /tmp/vk-e2e
#
# 前置：/dev/uinput 可写（vk-inject 退出码 0）、用户属于 input 组、webview 有前端在渲染
#       （python3 -m http.server 1420 -d app/dist）、Python 有 gi 与 evdev。
# 每轮都重启应用：同一进程里同名 uinput 设备只被认领一次（见 docs/linux.md）。
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/../.."

CH="${1:-wetype}"
WORK="${2:-/tmp/vk-e2e}"
SANDBOX="/tmp/vk-e2e-sandbox"
WAV="ios/Resources/sample.wav"

mkdir -p "$WORK" "$SANDBOX/VoiceKey"
# 单渠道沙箱：channel=all 会进 pick 模式（那是另一件事），清单要的是 hud
printf '{"channel":"%s","onboarded":true}' "$CH" > "$SANDBOX/VoiceKey/settings.json"

pkill -x voicekey 2>/dev/null; sleep 1
rm -f "$WORK/target.log" "$WORK/inject.log" "$WORK/stop"

# 靶子先起、拿焦点；应用后起（浮层不影响靶子焦点，但顺序固定更好复现）
python3 "$HERE/target.py" "$WORK/target.log" & echo $! > "$WORK/target.pid"
sleep 2
XDG_DATA_HOME="$SANDBOX" ./target/debug/voicekey > "$WORK/app.log" 2>&1 & echo $! > "$WORK/app.pid"
sleep 5   # 让钩子扫到 /dev/input 里的设备

# 注入脚本自己 sleep 3 再按下右 Alt（等钩子认领新设备）
python3 "$HERE/inject_hold.py" "$WORK/stop" > "$WORK/inject.log" 2>&1 & echo $! > "$WORK/inject.pid"
# 与按下同时开始播：按下后要过 0.3s 长按阈值才开始录，wav 有 4.6s，足够
sleep 3.5
if [ -n "${VK_SINK:-}" ]; then
    paplay --device="$VK_SINK" "$WAV" 2>/dev/null || paplay "$WAV"
else
    paplay "$WAV"
fi

# 播完再等 1 秒（把最后一句留足），然后松开
touch "$WORK/stop"
sleep 10

echo "── 注入 ──"; cat "$WORK/inject.log"
echo "── 靶子 ──"; cat "$WORK/target.log"
echo "── 耗时 ──"
python3 - "$WORK" <<'PY'
import re, sys, pathlib
work = pathlib.Path(sys.argv[1])
target = work.joinpath("target.log")
inject = work.joinpath("inject.log")
if not (target.exists() and inject.exists()):
    sys.exit("缺日志")
up = down = None
for line in inject.read_text().splitlines():
    if line.startswith("UP "):
        up = float(line.split()[1])
    elif line.startswith("DOWN "):
        down = float(line.split()[1])
changed = [float(m.group(1)) for m in re.finditer(r"^([\d.]+) CHANGED", target.read_text(), re.M)]
if up and changed:
    print(f"按住 {changed[0] - down:.2f}s；松手到落字 {changed[0] - up:.2f}s")
else:
    print("没看到 CHANGED —— 字没上屏，看上面的靶子日志与 app.log")
PY
