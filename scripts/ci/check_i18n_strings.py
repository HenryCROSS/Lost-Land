#!/usr/bin/env python3
"""扫描 crates/**/src/**/*.rs，找出疑似硬编码的用户可见字符串字面量。

# 不检查会漏掉什么

规格 §11.3："代码中不得出现任何硬编码的用户可见字符串，由 CI 检查强制"。
项目还没有 `ll-ui`、还没有任何 `.ftl` 本地化文件，风险目前是潜在的而非
现实的——但风险会在 UI 层落地、菜单文案第一次被写进代码那一刻突然出现。
到那时如果这道检查还不存在，第一批硬编码字符串会在无人察觉的情况下进
代码库，往后每一条新字符串都只能靠人眼在 review 里揪，而字符串字面量
恰恰是 review 里最容易被漏看的一类改动（它不像逻辑错误那样让测试变红）。
现在先把检查的骨架和豁免约定定下来，好过等第一批硬编码字符串已经进了
代码库之后再回头补规则、回头逐条甄别哪些是真违规、哪些是历史包袱。

# 现状：为什么是"先警告、不阻断"（--strict 之前的默认模式）

本脚本 2026-08-17 首次在当前代码库上跑，命中的全部是下面两类：

1. `.expect()` / `panic!()` / `assert!()` 等断言与调试宏里的中文消息——
   这些是给开发者看的诊断信息（测试失败时打印），不是玩家会看到的游戏
   文本，已经用宏名过滤掉，不会被本脚本判定为命中。
2. 内部错误类型的 `Display`/`Err` 构造里的中文文案（例如
   `ll-render/src/atlas.rs` 里 `RenderError::AtlasMetadata` 的校验错误、
   `ll-world/src/state.rs` 里存档反序列化的错误消息）——这类文案的先例
   是 `ll-core/src/error.rs`：那里的 `CoreError::Display` 实现明确写了
   注释"此处文案面向开发者与日志，不面向玩家，故不走 i18n"。上述几处
   结构上是同一类东西，只是还没有补上同款的豁免标记。

给这些位置补 `// i18n-exempt` 标记属于 `crates/**` 源码改动，不在本次
CI 门禁补齐任务的授权范围内（该范围只允许改 `.github/` 与本目录），因此
本脚本默认以"警告"模式运行：报告命中但不让 CI 失败。一旦上述历史命中
补上标记（或改用同款豁免注释），或者 `ll-ui`/i18n 机制真正落地，应当把
CI 里调用本脚本时的 `I18N_CHECK_MODE` 改成 `strict`，让它真正阻断。
不要一直留在 warn 模式——那样这道门禁就只是摆设。

# 检查什么 / 怎么判定豁免

- 扫描 `crates/*/src/**/*.rs`（不含 `tests/`、`examples/`、`benches/`——
  那些是非发布路径，不是玩家会接触到的运行时代码）。
- 一行里出现被双引号包住、且至少含一个中日韩统一表意文字（CJK）字符的
  字符串字面量，视为疑似命中。中文是本项目首发语言之一，代码标识符、
  类型名、关键字都不可能含中文字符，用它做信号误报率低；英文字面量的
  误报率高得多（大量合法的技术性英文字符串，如日志格式、类型名字符串），
  本脚本暂不处理，留待需要时换用基于 `syn` 的 AST 工具。
- 命中行若含有以下任一子串，判定为开发者/日志向诊断信息，跳过：
  `.expect(`、`panic!(`、`unreachable!(`、`todo!(`、`unimplemented!(`、
  `assert!(`、`assert_eq!(`、`assert_ne!(`、`debug_assert!(`、
  `debug_assert_eq!(`、`debug_assert_ne!(`、`tracing::`、`log::`、
  `eprintln!(`、`println!(`。
- 注释行（`//`、`///`、`//!` 开头，含前导空白后判断）跳过。
- 命中行含 `// i18n-exempt` 标记的，跳过（约定的豁免方式）。

# 已知局限（非本次范围）

- 不做真正的 Rust 语法解析，字符串边界靠正则近似，多行字符串字面量、
  字符串内转义引号等边界情况可能漏检或误判。
- 只抓 CJK 字面量，纯英文的用户可见字符串抓不到（规格要求首发中英双语，
  这是本脚本目前最大的盲区，需要后续升级为 AST 工具才能覆盖）。
- 不理解宏展开后的实际语义，只按源码文本表面模式过滤。
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

REPO_ROOT = Path(__file__).resolve().parents[2]
CRATES_ROOT = REPO_ROOT / "crates"
EXCLUDED_DIR_NAMES = {"tests", "examples", "benches"}
EXEMPT_MARKER = "i18n-exempt"

DIAGNOSTIC_MACROS = [
    ".expect(",
    "panic!(",
    "unreachable!(",
    "todo!(",
    "unimplemented!(",
    "assert!(",
    "assert_eq!(",
    "assert_ne!(",
    "debug_assert!(",
    "debug_assert_eq!(",
    "debug_assert_ne!(",
    "tracing::",
    "log::",
    "eprintln!(",
    "println!(",
]

STRING_LITERAL = re.compile(r'"(?:[^"\\]|\\.)*"')
HAN_CHAR = re.compile(r"[一-鿿]")


def is_scannable_source_file(path: Path) -> bool:
    rel_parts = path.relative_to(CRATES_ROOT).parts
    # crates/<crate>/src/... 形状；跳过不含 src 的路径（tests/examples/benches 等
    # 与 src 同级的目录）。
    if "src" not in rel_parts:
        return False
    src_index = rel_parts.index("src")
    if any(part in EXCLUDED_DIR_NAMES for part in rel_parts[:src_index]):
        return False
    return True


def find_hits() -> list[tuple[Path, int, str]]:
    hits: list[tuple[Path, int, str]] = []
    if not CRATES_ROOT.is_dir():
        return hits

    for rs_file in sorted(CRATES_ROOT.rglob("*.rs")):
        if not is_scannable_source_file(rs_file):
            continue

        lines = rs_file.read_text(encoding="utf-8", errors="replace").splitlines()
        for line_no, line in enumerate(lines, start=1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if EXEMPT_MARKER in line:
                continue
            if any(marker in line for marker in DIAGNOSTIC_MACROS):
                continue

            for match in STRING_LITERAL.finditer(line):
                literal = match.group(0)
                if HAN_CHAR.search(literal):
                    hits.append((rs_file, line_no, literal))
                    break

    return hits


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--strict",
        action="store_true",
        help="发现命中即以非零退出码失败（默认只警告，不阻断——见文件头注释）。"
        "也可用环境变量 I18N_CHECK_MODE=strict 达到同样效果。",
    )
    args = parser.parse_args()

    strict = args.strict or __import__("os").environ.get("I18N_CHECK_MODE") == "strict"

    hits = find_hits()
    if not hits:
        print("未发现疑似硬编码的用户可见字符串（crates/*/src）。")
        return 0

    level = "error" if strict else "warning"
    for path, line_no, literal in hits:
        rel = path.relative_to(REPO_ROOT)
        print(
            f"::{level} file={rel},line={line_no}::"
            f"疑似硬编码用户可见字符串：{literal}。规格 §11.3 要求用户可见文本必须"
            f"经由 Fluent .ftl 走 i18n，不得直接写字符串字面量。若这是开发者/日志向"
            f"诊断信息（而非玩家会看到的文本），比照 crates/ll-core/src/error.rs 的"
            f"先例在行尾加 `// i18n-exempt` 标记并说明理由。"
        )

    print(f"\n共发现 {len(hits)} 处疑似命中，见上方逐条标注。")
    if strict:
        print("当前为 strict 模式：命中即失败。")
        return 1

    print(
        "当前为 warn 模式（默认）：本次不阻断 CI。这是过渡状态，不是终态——"
        "见 scripts/ci/check_i18n_strings.py 文件头注释，说明了收紧到 strict 的条件。"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
