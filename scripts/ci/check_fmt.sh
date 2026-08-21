#!/usr/bin/env bash
# 格式检查：对应 .github/workflows/ci.yml `test` job 的「格式检查」步骤。
#
# 与 CI 的关系：CI 在 ubuntu-latest 与 windows-latest 两个 target 上各跑
# 一次本脚本对应的命令；本脚本在本地只能跑当前平台（本仓库开发机是
# Windows）。`cargo fmt` 本身不编译代码、不产出平台相关差异，两个 target
# 上的格式规则相同，因此这道检查没有已知的平台盲区——但如果未来
# rustfmt 配置引入了任何依平台生效的规则，仍然只有 CI 的 Linux target
# 能发现。
set -euo pipefail

cargo fmt --all --check
