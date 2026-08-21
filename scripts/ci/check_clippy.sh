#!/usr/bin/env bash
# Clippy 检查：对应 .github/workflows/ci.yml `test` job 的「Clippy」步骤。
#
# `RUSTFLAGS=-D warnings` 在 CI 里是工作流级别的 env（对 test/licenses/
# coverage/docs-and-links 全部生效），本脚本在本地独立调用时自己兜底
# 同一个默认值——已经设置过（例如 CI 环境、或用户 shell 里手动 export
# 过）则尊重现有值，不覆盖。
#
# 平台盲区：CI 在 ubuntu-latest 与 windows-latest 两个 target 上各跑一次
# clippy；本脚本本地只能跑当前平台（本仓库开发机是 Windows）。clippy 的
# lint 结果可能因目标平台的 `cfg`/条件编译分支不同而不同——例如只在
# `cfg(unix)`/`cfg(windows)` 下编译的代码，本地 Windows 跑不到 Linux 分支
# 的 lint。这类平台专属问题仍然只能靠 CI 兜底，本脚本不能替代 CI 矩阵。
set -euo pipefail

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"

cargo clippy --workspace --all-targets -- -D warnings
