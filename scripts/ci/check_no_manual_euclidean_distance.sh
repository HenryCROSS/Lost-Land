#!/usr/bin/env bash
# 禁止手写欧氏距离：对应 .github/workflows/ci.yml
# `no-manual-euclidean-distance` job 的「禁止手写欧氏距离」步骤。
#
# 规格 §7.1 明写「此项由 CI 静态检查强制」，但门禁一直没有真正实现
# （见 knowledge/handoff/p2-to-p3.md 第五节）。环面世界里手写欧氏距离是
# 极难定位的一类缺陷——代码看起来能跑、单测也未必会红，只在两点恰好
# 跨南北或东西接缝时才会算出一条绕了半个世界的错误「最短路」，因为普通
# `sqrt(dx²+dy²)` 不知道世界在边界会绕回。`TorusSize::chebyshev`/
# `squared_euclidean`/`delta`（见 crates/ll-core/src/torus.rs）已经处理
# 了绕接缝取最短位移这件事，手写距离公式应一律改用它们。
#
# 逻辑与 ci.yml 里原本内联的版本逐字一致，只是搬到这个文件里，本地和
# CI 现在调用同一份脚本。
set -euo pipefail

violations=0
while IFS= read -r -d '' file; do
  while IFS=: read -r line_no content; do
    trimmed="$(printf '%s' "$content" | sed -e 's/^[[:space:]]*//')"
    case "$trimmed" in
      //*) continue ;;
    esac
    echo "::error file=$file,line=$line_no::检测到疑似手写欧氏距离：${content}。环面世界必须改用 TorusSize 的 chebyshev/squared_euclidean/delta（crates/ll-core/src/torus.rs），不得手写 sqrt/powi(2)/hypot 计算距离。"
    violations=$((violations + 1))
  done < <(grep -nE '\bsqrt\(|\.powi\(2\)|\bhypot\(' "$file" || true)
done < <(find crates -type f -name '*.rs' -print0)
if [ "$violations" -gt 0 ]; then
  echo "共发现 $violations 处疑似手写欧氏距离，见上方逐条标注。"
  exit 1
fi
echo "未发现手写欧氏距离。"
