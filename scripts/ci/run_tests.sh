#!/usr/bin/env bash
# 测试：对应 .github/workflows/ci.yml `test` job 的「测试」步骤。
#
# `RUSTFLAGS=-D warnings` 同 check_clippy.sh 的说明：CI 是工作流级 env，
# 本脚本本地独立调用时自己兜底同一个默认值，不覆盖已设置的值。
#
# 平台盲区（本次三条路径校验测试翻车的真实教训）：CI 在 ubuntu-latest 与
# windows-latest 两个 target 上各跑一次 `cargo test --workspace`；本脚本
# 本地只能跑当前平台（本仓库开发机是 Windows）。`std::path::Component`
# 对反斜杠/盘符/UNC 前缀的解析依平台而异，这类问题只在 Linux target 上
# 会暴露——2026-08 那次 `asset_vfs` 路径穿越校验测试就是活例子：本地
# Windows 全绿，CI 的 ubuntu-latest 三条测试全红，而本地弱化过的验证流程
# 从未跑过 Linux target，所以没人发现。本脚本无法替代 CI 矩阵，只能覆盖
# 当前平台这一半；跨平台确定性（规格 §14.4 双 target 世界哈希逐位相同）
# 与任何依赖 `Component`/`cfg(windows)`/`cfg(unix)` 分支的行为，最终仍要
# 靠 CI 的 Linux target 兜底。
set -euo pipefail

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"

cargo test --workspace
