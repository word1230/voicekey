#!/bin/bash
# 端到端验收一轮：起沙箱应用 → 起靶子 → 注入按住右 Alt → 播 wav → 松开 → 看靶子日志。
#
# 用法：run_e2e.sh [渠道] [工作目录]
#   channel  要验的渠道，默认 wetype（写进沙箱 settings.json，见 docs/linux.md「端到端替身」）
#   workdir  放日志与 stop 文件的目录，默认 /tmp/vk-e2e
#
# 前置：/dev/uinput 可写（vk-inject 退出码 0）、用户属于 input 组、webview 有前端在渲染
#       （python3 -m http.server 1420 -d app/dist）、Python 有 gi 与 evdev、
#       已 cargo build --workspace（用 target/debug/vk-kwin 确认靶子焦点）。
# 每轮都重启应用：同一进程里同名 uinput 设备只被认领一次（见 docs/linux.md）。
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/../.." || exit 1

CH="${1:-wetype}"
WORK="${2:-/tmp/vk-e2e}"
SANDBOX="/tmp/vk-e2e-sandbox"
WAV="ios/Resources/sample.wav"

mkdir -p "$WORK" "$SANDBOX/VoiceKey"
# 单渠道沙箱：channel=all 会进 pick 模式（那是另一件事），清单要的是 hud
printf '{"channel":"%s","onboarded":true}' "$CH" > "$SANDBOX/VoiceKey/settings.json"

# 只停沙箱实例。用户真实的 VoiceKey 用 ~/.local/share/VoiceKey，误杀会打断正在用的人。
for pid in $(pgrep -x voicekey); do
    env_home=$(tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | sed -n 's/^XDG_DATA_HOME=//p')
    if [ "$env_home" = "$SANDBOX" ]; then
        kill "$pid" 2>/dev/null || true
    else
        echo "检测到非沙箱的 VoiceKey 正在运行（pid $pid）：先自己停掉它，或手动确认后再跑验收" >&2
        exit 1
    fi
done
sleep 1
rm -f "$WORK/target.log" "$WORK/inject.log" "$WORK/stop"

# 靶子必须真的拿到焦点：KWin 上新起的 GTK 窗口不会自动获得焦点，而 application 的
# 上屏走的是"当前焦点窗口"。没确认就注入，字会落到用户正在用的编辑器/终端里。
# 用 KWin 脚本激活标题以 vk-target 开头的窗口，再从 dump 确认；确认不了就拒绝注入。
ensure_target_focus() {
    local dump="$WORK/dump.txt"
    if ! ./target/debug/vk-kwin dump > "$dump" 2>/dev/null; then
        echo "读不到 KWin 窗口：先 cargo build --workspace 生成 target/debug/vk-kwin" >&2
        return 1
    fi
    awk '/vk-target/ && /active=true/ { found = 1 } END { exit !found }' "$dump" && return 0

    command -v qdbus6 >/dev/null || {
        echo "靶子没拿到焦点，且没有 qdbus6 可激活；拒绝注入（字会落到别的窗口）" >&2
        return 1
    }
    cat > "$WORK/activate.js" <<'EOF'
var list = workspace.windowList();
for (var i = 0; i < list.length; i++) {
    var w = list[i];
    if (w.caption && w.caption.indexOf("vk-target") === 0) {
        workspace.activeWindow = w;
    }
}
EOF
    local id
    id=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$WORK/activate.js" "vk-activate-$$") || return 1
    qdbus6 org.kde.KWin /Scripting/Script"$id" org.kde.kwin.Script.run >/dev/null || return 1
    sleep 1
    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "vk-activate-$$" >/dev/null 2>&1 || true
    ./target/debug/vk-kwin dump > "$dump" 2>/dev/null
    awk '/vk-target/ && /active=true/ { found = 1 } END { exit !found }' "$dump" || {
        echo "靶子仍没拿到焦点，拒绝注入（字会落到别的窗口）" >&2
        return 1
    }
}

# 靶子先起、应用后起（浮层不影响靶子焦点，但顺序固定更好复现）
python3 "$HERE/target.py" "$WORK/target.log" & echo $! > "$WORK/target.pid"
sleep 2
XDG_DATA_HOME="$SANDBOX" ./target/debug/voicekey > "$WORK/app.log" 2>&1 & echo $! > "$WORK/app.pid"
sleep 5   # 让钩子扫到 /dev/input 里的设备

pgrep -x voicekey >/dev/null || { echo "应用没起来：看 $WORK/app.log" >&2; exit 1; }
pgrep -F "$WORK/target.pid" >/dev/null || { echo "靶子没起来：看 $WORK/target.log" >&2; exit 1; }
ensure_target_focus || exit 1

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
