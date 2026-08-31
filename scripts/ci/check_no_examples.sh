#!/usr/bin/env bash
# 工作区不得有任何 example target：对应 .github/workflows/ci.yml `test`
# job 的「禁止 example target」步骤。
#
# 本脚本的前身是 `run_acceptance_demos.sh`
# ==========================================
#
# 前身做两件事：
#
# 1. **真的运行** RUN_LIST 里那些无头验收 demo 的 `main()`——因为
#    `cargo test --workspace` 与 `cargo clippy --all-targets` 都只**编译**
#    example，不运行它的 `main()`，而验收断言全写在 `main()` 里。这条
#    在 2026-08 被真实撞上：`p5_save_acceptance` 因为「决策二」硬门禁
#    落地而在主干上**恒定 panic 了几个批次**，整条 CI 却全绿。
# 2. **完整性检查**：每一个 example target 必须被显式登记进 RUN_LIST 或
#    SKIP_LIST，否则报错。前身的脚本头注释把这一半称作「本门禁真正的
#    价值所在」。
#
# 2026-08-29，项目所有者裁定去掉 `examples/`（原话「我觉得应该要去掉
# example。然后有用的东西搬迁了。剩下的后面考虑。」，记录见
# `knowledge/decisions/0030-remove-examples-acceptance-demos.md`）。
#
# **第 1 件事失去了对象**：RUN_LIST 里那两个 demo 的断言已经搬进
# `crates/ll-content/tests/{save_acceptance,gameplay_acceptance}.rs`，
# 由 `cargo test --workspace` 直接执行——`main()` 里藏着没人跑的断言这
# 个失败模式，从此在结构上不存在。
#
# **第 2 件事反而更重要了**：裁定之后，「工作区里冒出一个 example」本身
# 就是需要当场变红的事。留一个没人跑的 demo 慢慢烂掉，正是前身诞生的
# 原因；而删掉这道门禁，等于把「不许悄悄加回来」这条保护一起删掉。
#
# 所以本脚本保留前身的第 2 件事，并把判据收紧成「一个都不许有」。
#
# 真要再加 example，先改裁定
# ==========================
#
# 本脚本变红不是「把名字加进某张清单」就能消掉的——那正是它与前身的
# 区别。要重新引入 example，需要一次新的所有者裁定，并同批更新
# ADR 0030 与规格 §15，否则这里恒红。
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

# 用 `cargo metadata` 而不是 `ls crates/*/examples`：前者是 cargo 自己
# 认定的 target 清单，`[[example]]` 里用 `path =` 改过位置、或以目录
# （`examples/<name>/main.rs`）形式组织的 target 都能正确枚举到，后者
# 会漏。这一段与前身逐字相同。
#
# 走 `sys.stdout.buffer` 而不是 `print`：Windows 上 Python 的文本模式
# stdout 会把换行翻译成 CRLF，行尾多出来的那个 \r 会让下游的文本比较
# 认为所有条目都对不上（前身第一版就是这么翻车的）。写 bytes 绕开文本
# 模式的换行翻译，两个平台上都只出 \n。
export LC_ALL=C

ALL_EXAMPLES="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c '
import sys, json
meta = json.loads(sys.stdin.buffer.read().decode("utf-8"))
for package in meta["packages"]:
    for target in package["targets"]:
        if "example" in target["kind"]:
            line = "%s:%s\n" % (package["name"], target["name"])
            sys.stdout.buffer.write(line.encode("utf-8"))
' | sort)"

if [ -n "${ALL_EXAMPLES}" ]; then
  echo "错误：工作区里出现了 example target：" >&2
  echo "${ALL_EXAMPLES}" | sed 's/^/  - /' >&2
  echo "" >&2
  echo "2026-08-29 由项目所有者裁定去掉 examples/（见" >&2
  echo "knowledge/decisions/0030-remove-examples-acceptance-demos.md）。" >&2
  echo "验收改由「本体二进制实机试玩 + 分层自动化测试」承担。" >&2
  echo "" >&2
  echo "若这段代码有真实断言且无头跑得动，请把它写成 tests/ 下的普通" >&2
  echo "#[test]；若它要开窗/要 GPU/等人输入，那正是这次裁定要去掉的那一" >&2
  echo "类。确实需要重新引入 example 的话，先取得一次新的裁定并同批更新" >&2
  echo "ADR 0030 与规格 §15——本脚本不接受「加进某张清单」这种消红方式。" >&2
  exit 1
fi

echo "工作区没有任何 example target（2026-08-29 所有者裁定，ADR 0030）。"
