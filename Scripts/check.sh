#!/bin/bash
# 仓库检查的唯一入口：本地钩子与 CI 都调这里，别在两边各写一份。
#
# 用法：Scripts/check.sh [--fast|--full]
#   --fast（默认）：cargo check 无警告 + 前端类型检查与单测 + 脚本静态检查
#   --full：--fast + cargo test --workspace（pre-push 与 CI 用）
#
# 急着提交/推送时用 --no-verify 跳过钩子那层；CI 不受影响。
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

MODE="${1:---fast}"
case "$MODE" in
    --fast|--full) ;;
    *)
        echo "用法：$0 [--fast|--full]" >&2
        exit 2
        ;;
esac

# docs/linux.md 记的标准是「cargo check --workspace --all-targets 无警告」。
# 先把输出收进变量再判：`cargo ... | tail` 会把 cargo 的退出码换成 tail 的
# （docs/linux.md「判定时的五个坑」第 5 条）。
out=$(cargo check --workspace --all-targets 2>&1) || {
    printf '%s\n' "$out"
    exit 1
}
if printf '%s\n' "$out" | grep -q '^warning'; then
    printf '%s\n' "$out" | grep -A2 '^warning' | head -40
    echo "── 有编译警告：docs/linux.md 的标准是 cargo check --workspace --all-targets 无警告" >&2
    exit 1
fi

if [ -d app/node_modules ]; then
    (cd app && npm run check && npm test --silent)
else
    echo "跳过前端类型检查与单测：app/node_modules 不存在（先 cd app && npm ci --allow-remote=all）" >&2
fi

# 脚本静态检查。run_e2e.sh 这类验收脚本此前没有任何检查，两个必挂的 bug 直接进了仓库
# （路径写死、set -u 下引用未设的 VK_SINK），所以在这里兜住。
bash -n Scripts/*.sh Scripts/e2e/*.sh
for f in Scripts/e2e/*.py; do
    # 不用 py_compile：它会在 Scripts/e2e 里留下 __pycache__
    python3 -c 'import sys; compile(open(sys.argv[1]).read(), sys.argv[1], "exec")' "$f"
done
if command -v shellcheck >/dev/null; then
    shellcheck Scripts/*.sh Scripts/e2e/*.sh
else
    echo "跳过 shellcheck：未安装（Arch: pacman -S shellcheck；CI 里会跑）" >&2
fi

if [ "$MODE" = --full ]; then
    cargo test --workspace
fi
