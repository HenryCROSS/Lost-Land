#!/usr/bin/env bash
# 覆盖率门禁：workspace 行覆盖率 + 核心 crate（ll-core/ll-sim/ll-world）行覆盖率。
#
# 不检查会漏掉什么
#
# 规格 §14.5 要求"行覆盖率 ≥ 80%，核心 crate ≥ 90%"，但 `cargo-llvm-cov`
# 在本项目从未被安装或运行过——不是"曾经达标后来退化"，是从 P0 到现在都
# 不知道真实数字。没有这道门禁，一段"被调用过但没有任何断言"的空转测试
# 可以无限混进代码库、把覆盖率数字撑高却什么都没测，而没有任何机制会
# 告诉你这件事；反过来，真正需要补测试的代码路径也完全可能常年没人碰过
# 却无人察觉，因为从来没有人量过数字。
#
# 阈值为什么不是 80% / 90%（棘轮阈值，只许调高不许调低）
#
# 2026-08-17 首次在本项目实测：workspace 行覆盖率 79.71%，
# ll-core+ll-sim+ll-world 合计行覆盖率 96.85%（详见提交历史与 PR 描述）。
# workspace 整体已经非常接近规格的 80% 目标，但直接把阈值设成 80% 意味着
# 从今天起第一次跑就是红的——团队面对"一上来就红"的 CI，第一反应通常是
# 关掉这项检查而不是立刻把覆盖率提上去，那就等于这道门禁从未存在过。
# 所以这里把阈值设成"当前真实值向下取整再留出运行间抖动的余量"：
#   - WORKSPACE_MIN_LINES=75（实测 79.71%，留约 4.7 个百分点余量——
#     本机重复跑同一套测试两次，数字在 79.71% 与 80.92% 之间浮动，
#     说明覆盖率本身有运行间抖动，阈值必须留出能吸收这种抖动的空间，
#     不能卡在实测值正下方）
#   - CORE_MIN_LINES=90（实测 96.85%，直接可以设到规格最终目标 90%，
#     还留了近 7 个百分点余量，不需要再降低）
# 这是起点，不是终点：能生效的低阈值胜过被关掉的高阈值，随着测试补齐，
# 应当只调高这两个数字，不得因为某次提交让覆盖率下降就把阈值往下改——
# 阈值下调等同于悄悄放弃这道门禁，如确需下调必须在 PR 描述里说明原因并
# 经过评审，不能是脚本改一行数字就默默滑过去。
#
# 用法：本地和 CI 都直接跑 `bash scripts/ci/check_coverage.sh`，
# 需要先装好 `cargo-llvm-cov`（`cargo install cargo-llvm-cov --locked`）
# 与 rustup 组件 `llvm-tools-preview`（`rustup component add llvm-tools-preview`）。

set -euo pipefail

WORKSPACE_MIN_LINES="${WORKSPACE_MIN_LINES:-75}"
CORE_MIN_LINES="${CORE_MIN_LINES:-90}"

echo "==> 采集覆盖率数据（workspace，仅运行一次测试，供下面两次不同口径的汇总复用）"
cargo llvm-cov --workspace --no-report

echo ""
echo "==> workspace 行覆盖率（阈值 ${WORKSPACE_MIN_LINES}%，规格 §14.5 终极目标 80%）"
if ! cargo llvm-cov report --fail-under-lines "${WORKSPACE_MIN_LINES}"; then
  echo "::error::workspace 行覆盖率低于棘轮阈值 ${WORKSPACE_MIN_LINES}%。这不是临时性失败——阈值只允许调高，请补测试把数字拉回阈值以上，不要通过下调 scripts/ci/check_coverage.sh 里的 WORKSPACE_MIN_LINES 来让检查通过。"
  exit 1
fi

echo ""
echo "==> 核心 crate 行覆盖率：ll-core / ll-sim / ll-world（阈值 ${CORE_MIN_LINES}%，即规格终极目标）"
if ! cargo llvm-cov report -p ll-core -p ll-sim -p ll-world --fail-under-lines "${CORE_MIN_LINES}"; then
  echo "::error::核心 crate（ll-core/ll-sim/ll-world）行覆盖率低于 ${CORE_MIN_LINES}%。" \
       "这三个 crate 是环面拓扑、确定性模拟与世界状态的地基层，规格 §14.5 对它们的" \
       "要求高于 workspace 整体均值，请为新增的未覆盖分支补测试。"
  exit 1
fi

echo ""
echo "覆盖率门禁通过。"
