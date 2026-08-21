#!/usr/bin/env bash
# 本地一条命令跑完 CI 的全部检查（覆盖率门禁默认跳过，见下）。
#
# 为什么只有这一份清单
#
# 2026-08 那次事故的根因不是"漏了某条检查"，是"本地心里那份验证清单
# 和 CI 那份不一致"（本地跑 cargo doc 时忘了带 --document-private-items，
# 于是一批断链在本地永远是绿的，只有 CI 会红）。维护两份独立写出来的
# 清单——一份在 CI 配置里、一份在本地脚本/文档里——只是把同一个错误换
# 一种形式重犯：两份文本迟早会在某次改动里只更新一份，然后再次分叉。
# 所以这里不重新写一遍等价的命令，而是把 CI 每个 job 的真实命令抽成
# scripts/ci/ 下的独立脚本，.github/workflows/ci.yml 的每个 job 现在只是
# "调用某个脚本"；本地跑同一批脚本，就是唯一、单一来源地跑了一遍 CI
# 实际会跑的东西。修改检查内容只需要改脚本这一处，CI 和本地永远同步。
#
# 平台盲区：本机（本仓库开发机是 Windows）只能覆盖 windows-latest 这一
# 半 target。CI 的 `test` job 在 ubuntu-latest 与 windows-latest 两个
# target 上各跑一次 fmt/clippy/test；`licenses`/`no-manual-euclidean-
# distance`/`coverage`/`no-hardcoded-i18n-strings`/`docs-and-links` 五个
# job 全部只在 ubuntu-latest 上跑，本地完全没有对应环境。任何依平台生效
# 的差异——`std::path::Component` 解析、`cfg(unix)`/`cfg(windows)` 条件
# 编译分支、规格 §14.4 要求的双 target 世界哈希逐位相同——本脚本永远
# 发现不了，例子见 2026-08 那次 `asset_vfs` 路径校验测试事故（本地
# Windows 全绿，CI 的 ubuntu-latest 三条测试全红）。本脚本只是本地这一
# 半的单一入口，不能替代真正推送后跑的 CI 矩阵，PR 合并前仍然必须等
# CI 跑绿。
#
# 覆盖率门禁默认不跑
#
# `scripts/ci/check_coverage.sh` 需要先装 `cargo-llvm-cov` +
# `llvm-tools-preview`，且要把整个 workspace 完整测一遍再各口径汇总一次
# （CI 上实测跑了几分钟），比这里其余检查加起来还慢很多。日常改动多数
# 不触及覆盖率门槛，默认跳过能让 `run_all.sh` 保持"改完代码随手跑一遍、
# 几十秒内出结果"的可用性；真正要提交前，或怀疑这次改动可能拉低覆盖率
# 时，用 `RUN_COVERAGE=1 bash scripts/ci/run_all.sh` 或单独
# `bash scripts/ci/check_coverage.sh` 补跑一次——CI 上这道门禁仍然是
# 强制的，跳过只是本地默认行为，不是弱化检查本身。
#
# 用法
#   bash scripts/ci/run_all.sh              # 跳过覆盖率
#   RUN_COVERAGE=1 bash scripts/ci/run_all.sh  # 含覆盖率（慢，几分钟）
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

RUN_COVERAGE="${RUN_COVERAGE:-0}"

run_step() {
  local label="$1"
  shift
  echo ""
  echo "==> ${label}"
  "$@"
}

run_step "格式检查"                     bash scripts/ci/check_fmt.sh
run_step "Clippy"                       bash scripts/ci/check_clippy.sh
run_step "测试"                         bash scripts/ci/run_tests.sh
run_step "许可证与安全公告扫描"           bash scripts/ci/check_licenses.sh
run_step "禁止手写欧氏距离"               bash scripts/ci/check_no_manual_euclidean_distance.sh
run_step "硬编码用户可见字符串扫描（warn 模式）" python3 scripts/ci/check_i18n_strings.py
run_step "文档断链检查"                   bash scripts/ci/check_doc_links.sh
run_step "Markdown 死链检查"              python3 scripts/ci/check_markdown_links.py

if [ "${RUN_COVERAGE}" = "1" ]; then
  run_step "覆盖率门禁（较慢，几分钟）" bash scripts/ci/check_coverage.sh
else
  echo ""
  echo "==> 覆盖率门禁：已跳过（默认关闭，较慢；RUN_COVERAGE=1 bash scripts/ci/run_all.sh 可开启，CI 上仍强制跑）"
fi

echo ""
echo "本地清单全部通过（覆盖率$([ "${RUN_COVERAGE}" = "1" ] && echo "已" || echo "未")跑）。" \
     "注意：本机只覆盖 Windows target，CI 的 ubuntu-latest 与其余仅在 CI 跑的" \
     "job 仍需推送后确认，见脚本头注释「平台盲区」一节。"
