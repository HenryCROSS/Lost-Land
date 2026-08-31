#!/usr/bin/env python3
r"""把**当前**每个超出规格 §13 行数上限的 Rust 源文件连同「为什么它可以
这么长」的理由记进一份由本脚本生成的快照（`scripts/ci/file_size_budget.json`），
并强制这份清单**只能往紧的方向转**：新的超限文件红、快照里的文件涨了红、
降了要求刷新快照、降到上限以下要求从快照里摘掉。

# 这道门禁**不是**「文件不许长」

规格 §13 写「文件 200–400 行为宜，800 行为上限」，落地至今**没有任何 CI
门禁管它**，于是十几个文件长期超限，每个批次都在如实登记「我没让它更糟」
却没人还账（见 `knowledge/handoff/2026-08-28-session-handoff.md` 第四节第
8 条）。

但直接把 §13 变成硬门禁是错的，理由有两条，第二条更要紧：

1. **当场全红的门禁会被关掉**，那比没有门禁更糟——这与
   `check_coverage.sh` 选棘轮阈值而不是规格原文 80% 是同一个道理（「能
   生效的低阈值胜过被关掉的高阈值」）。
2. **行数超标是症状，不是病。** 所有者 2026-08-29 的原话：「要合理规划，
   不是说超了就得砍。所以说是重构，让代码为之后的开发做准备。」一个职责
   单一、内聚良好的内容表或大型 `match` 一千五百行也可以不动；一个把三
   种不相干职责搅在一起的文件八百行也该拆。**行数管不住内聚度**，任何
   只看行数的门禁都会同时冤枉前者、放过后者。

所以本门禁的定位是**防回潮**，不是逼人砍行数：它拦住「悄悄累积」——新
文件一上来就超限、老文件继续膨胀——而**是否该拆、怎么拆，由「下一批要
往这里加什么」决定，不由这个数字决定**。

# 每条超限项必须写明理由，写不出理由本身就是信号

快照里每个条目都带一个 `reason` 字段，**空着就红**。这是本门禁真正的
判据：一个超限文件如果没有人能用一句话说清「它为什么可以这么长」，那
它多半就是该拆的那个。理由分两类，写法上不作强制，但建议区分：

- **「合理」**：说清它为什么内聚（例如「单一内容表，逐条目平铺」「一个
  语法层的解析器，拆开等于把语法拆成两半」）。
- **「待重构」**：承认它该拆、并写明**等哪一批**（例如「待重构：等对话
  批次落地时按意图族切开」）。这样债务是记名的，不是匿名沉淀。

`--bless` **不会覆盖已有的理由**，只更新行数；新进入快照的条目理由留空，
必须由人补上。这是本文件里唯一需要人手写的东西——**行数一个都不是手写
的**，全部由 `--bless` 从源码算出。

# 判据用「非空非注释行」，不用总行数

这个仓库的文档密度极高：落地本门禁时实测，超限文件里代码行只占总行数的
**38% 到 75%**（中位数约 55%），也就是说**接近一半的行是中文文档注释**。
而规格 §13 自己下一条就要求「所有公开项必须有文档注释说明『做什么、怎么
用、依赖什么』」「注释解释**为什么**」。

用总行数当判据，等于让这两条规约互相打架：把「写清楚为什么」直接惩罚成
行数负担，逼人要么删注释、要么把注释挪到一个没人会读的地方。本仓库反复
记录的教训恰恰是相反方向的——`atlas_coverage.rs` 的手写清单、交接文档的
常量表、`skin.rs` 的裸贴图名，每一次事故的代价都是「当初没写清楚为什么」。

因此判据是**非空、非注释行**：它量的是「这个文件里住了多少逻辑」，而
§13 的上限想管的正是这个。注释写多长都不进判据。

## 具体怎么数（以及数不准的地方）

一行**不计入**，当且仅当它满足下列之一：

- 去掉首尾空白后是空行；
- 去掉首尾空白后以 `//` 开头（含 `///` 文档注释与 `//!` 模块注释）；
- 落在一个**以 `/*` 开头的行**所开启的块注释内部。

其余全部计入，**包括行尾带注释的代码行**（`let x = 1; // 说明` 计一行
代码，因为那行确实有逻辑）。

**已知数不准的地方**（如实列出）：

1. **多行字符串字面量里以 `//` 开头的行会被当成注释漏掉。** 例如 Rust
   的 `r#"..."#` 原始字符串里嵌了一段注释样例。后果是**少算**，方向偏
   松。要精确处理必须真的做词法分析，与仓库其余检查脚本（
   `check_field_consumers.py`、`check_save_schema_version.py`）一样，这里
   刻意停在「正则近似」这一层：判据是棘轮，偏松一点不会让它失效，而引入
   一个自己会出错的 Rust 词法器会。
2. **块注释只认行首的 `/*`。** 代码行中间开启的块注释（`foo(); /* ...`）
   之后那几行会被当成代码计入，方向偏紧。本仓库 Rust 代码几乎不用块注
   释（`rustfmt` 与 `///` 惯例的自然结果），实测影响为零。
3. **`#[cfg(test)]` 里的测试代码照常计入。** 测试是代码，它挤占的也是同
   一份「一个文件里能装多少逻辑」的预算。把测试用 `#[path]` 挪进兄弟文件
   （仓库已有 `app_save_tests.rs` / `app_navigation_tests.rs` 两处先例）
   会如实地把行数搬到那个文件上，兄弟文件同样受本门禁管——**搬家不等于
   还账**，这是刻意的。

# 覆盖范围：只管 Rust 源文件

`SCAN_GLOBS` 只含 `crates/` 与 `tools/` 下的 `*.rs`。刻意不含
`scripts/` 下的 Python / Shell，理由是「怎样算一行注释」是语言相关的，
Python 的三引号文档串按上面的规则会被整段当成代码行——那正好把本门禁
最想保护的东西（写清楚为什么）反过来惩罚一遍。落地时实测：`scripts/`
下没有任何文件接近上限，纳进来买不到任何东西，只买到一条我没法验证的
计数规则。将来真要纳入，加一条 glob 加一套该语言的注释规则即可。

# 与 `check_save_schema_version.py` 的关系

形状一样：**由脚本自己生成的快照 + 比对 + `--bless`**，人手维护的只有
理由文本。这不是巧合——本仓库最稳定的缺陷来源就是「把真相源之外的副本
当判据」，而快照要成为判据又不成为漂移副本，唯一的办法就是它全文由脚本
从真相源算出、人只写脚本算不出来的那部分（那边是豁免理由，这边是超限
理由）。

# warn 还是阻断

**阻断**。存量在落地本门禁的同一批里用 `--bless` 冻成绿的，之后任何
「新超限文件」或「已超限文件继续涨」都必须显式过一次门。
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

REPO_ROOT = Path(__file__).resolve().parents[2]

# 规格 §13 的上限。改这个数等于改规格，请连同
# `docs/superpowers/specs/2026-08-16-lostland-design.md` §13 一起改。
LIMIT = 800

# 扫描范围。只含 Rust 源文件，理由见头注释「覆盖范围」一节。
SCAN_GLOBS = [
    "crates/**/*.rs",
    "tools/**/*.rs",
]

# 快照文件。行数全部由 `--bless` 生成；`reason` 是人写的，`--bless` 会原样
# 保留、不覆盖。
SNAPSHOT_PATH = REPO_ROOT / "scripts" / "ci" / "file_size_budget.json"

# 扫描时跳过的路径片段（构建产物）。
SKIP_PARTS = {"target"}


def count_code_lines(path: Path) -> tuple[int, int]:
    """返回 (非空非注释行数, 总行数)。计数规则与已知偏差见头注释。"""
    total = 0
    code = 0
    in_block = False
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        total += 1
        line = raw.strip()
        if in_block:
            if "*/" not in line:
                continue
            in_block = False
            line = line.split("*/", 1)[1].strip()
            if not line:
                continue
        if not line:
            continue
        if line.startswith("//"):
            continue
        if line.startswith("/*"):
            if "*/" not in line:
                in_block = True
                continue
            line = line.split("*/", 1)[1].strip()
            if not line:
                continue
        code += 1
    return code, total


def scan() -> dict[str, tuple[int, int]]:
    """量出扫描范围内每个文件的 (代码行, 总行)，键是相对仓库根的 POSIX 路径。"""
    measured: dict[str, tuple[int, int]] = {}
    for pattern in SCAN_GLOBS:
        for path in REPO_ROOT.glob(pattern):
            if not path.is_file():
                continue
            if SKIP_PARTS & set(path.relative_to(REPO_ROOT).parts):
                continue
            measured[path.relative_to(REPO_ROOT).as_posix()] = count_code_lines(path)
    return measured


def load_snapshot() -> dict | None:
    if not SNAPSHOT_PATH.exists():
        return None
    return json.loads(SNAPSHOT_PATH.read_text(encoding="utf-8"))


def write_snapshot(measured: dict[str, tuple[int, int]], previous: dict | None) -> list[str]:
    """重写快照：行数按当前实测取值，理由从旧快照原样搬过来。

    返回新进入快照、因而**没有理由**的文件清单——调用方要把它们喊出来，
    这些正是需要人补一句「为什么它可以这么长」的条目。
    """
    old_files = (previous or {}).get("files", {})
    files: dict[str, dict] = {}
    fresh: list[str] = []
    for name in sorted(measured):
        code, _total = measured[name]
        if code <= LIMIT:
            continue
        reason = old_files.get(name, {}).get("reason", "")
        if not reason:
            fresh.append(name)
        files[name] = {"code_lines": code, "reason": reason}
    payload = {
        "_note": (
            "本文件由 scripts/ci/check_file_size_budget.py --bless 生成。"
            "code_lines 一律由脚本从源码算出，不要手工编辑；"
            "reason 是人写的「为什么这个文件可以这么长」，--bless 会原样保留，"
            "空着门禁会红。判据是非空非注释行，理由见脚本头注释。"
        ),
        "limit": LIMIT,
        "files": files,
    }
    # 显式 newline="\n"：`.gitattributes` 规定仓库内一律存 LF，而 Windows 上
    # `write_text` 默认把 `\n` 翻成 CRLF，快照会整文件漂移。同
    # `check_save_schema_version.py::write_snapshot`。
    SNAPSHOT_PATH.write_text(
        json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    return fresh


def _check_new_offenders(
    measured: dict[str, tuple[int, int]], recorded: dict[str, dict]
) -> list[str]:
    """快照之外、却已经超限的文件——新债，直接红。"""
    errors = []
    for name in sorted(measured):
        if name in recorded:
            continue
        code, total = measured[name]
        if code <= LIMIT:
            continue
        errors.append(
            f"{name}：{code} 行代码（共 {total} 行）超过 §13 的 {LIMIT} 行上限，"
            "而它不在快照里——这是新增的超限文件。请**按职责拆开**它（不是按行数切），"
            "拆出来的每个模块要能用一句话说清它负责什么；"
            "确有理由维持现状的，跑 `python scripts/ci/check_file_size_budget.py --bless` "
            "把它记进快照，并在 reason 里写明为什么。"
        )
    return errors


def _check_recorded(
    measured: dict[str, tuple[int, int]], recorded: dict[str, dict]
) -> tuple[list[str], list[str]]:
    """比对快照里的每一条。返回 (红, 需要刷新快照的提示)。"""
    errors: list[str] = []
    stale: list[str] = []
    for name in sorted(recorded):
        entry = recorded[name]
        budget = entry.get("code_lines")
        if not entry.get("reason"):
            errors.append(
                f"{name}：快照里这一条没有写 reason。超限本身不是罪，"
                "**说不出它为什么可以这么长才是信号**——请补一句理由，"
                "或者写成「待重构：等 X 批次落地时按 Y 拆开」把这笔债记名。"
            )
        if name not in measured:
            errors.append(
                f"{name}：快照里有这一条，但文件已经不存在了（改名或删除）。"
                "跑 `--bless` 把这条死记录清掉——不允许豁免自己烂在那儿。"
            )
            continue
        code, total = measured[name]
        if code > budget:
            errors.append(
                f"{name}：代码行从 {budget} 涨到 {code}（共 {total} 行），"
                "本门禁是棘轮，已超限的文件只许缩不许涨。"
                "要往这个文件加东西，先把要加的那部分**按职责拆进一个新模块**；"
                "确有不得已的理由，改动与 `--bless` 一起提交，并在提交信息里说清楚。"
            )
        elif code <= LIMIT:
            stale.append(
                f"{name}：已经降到 {code} 行，不再超过 {LIMIT} 行上限——"
                "跑 `--bless` 把它从快照里摘掉。摘掉之后它按普通文件管，再超限就直接红。"
            )
        elif code < budget:
            stale.append(
                f"{name}：代码行从 {budget} 降到 {code}，是好事——"
                "跑 `--bless` 把预算收紧到新值。棘轮只往紧的方向转，"
                "不刷新的话省下来的余量会被下一批悄悄吃掉。"
            )
    return errors, stale


def _dump(measured: dict[str, tuple[int, int]], recorded: dict[str, dict]) -> None:
    over = sorted(
        ((c, t, n) for n, (c, t) in measured.items() if c > LIMIT),
        reverse=True,
    )
    print(f"\n超过 {LIMIT} 行上限的文件（{len(over)} 个），按代码行降序：")
    print(f"  {'代码行':>7} {'总行':>7} {'代码占比':>8}  文件")
    for code, total, name in over:
        mark = "" if name in recorded else "  ← 不在快照里"
        print(f"  {code:7d} {total:7d} {code * 100 // total:7d}%  {name}{mark}")


def main() -> int:
    parser = argparse.ArgumentParser(description="文件行数棘轮门禁（规格 §13）")
    parser.add_argument("--bless", action="store_true", help="用当前实测行数重写快照")
    parser.add_argument("--dump", action="store_true", help="打印全部超限文件，用于勘查")
    args = parser.parse_args()

    measured = scan()
    snapshot = load_snapshot()

    over_count = sum(1 for code, _ in measured.values() if code > LIMIT)
    print(
        f"扫描 {len(measured)} 个 Rust 源文件，"
        f"其中 {over_count} 个超过 §13 的 {LIMIT} 行上限（判据：非空非注释行）。"
    )

    if args.bless:
        fresh = write_snapshot(measured, snapshot)
        rel = SNAPSHOT_PATH.relative_to(REPO_ROOT).as_posix()
        print(f"\n已把当前实测行数写进 {rel}（{over_count} 条）。")
        if fresh:
            print(
                f"::error::下面 {len(fresh)} 条是新进快照的，reason 还空着，"
                "门禁会一直红到有人写上「为什么这个文件可以这么长」："
            )
            for name in fresh:
                print(f"  · {name}")
            return 1
        return 0

    if snapshot is None:
        rel = SNAPSHOT_PATH.relative_to(REPO_ROOT).as_posix()
        print(f"::error::找不到行数快照 {rel}，请跑 `--bless` 生成。")
        return 1

    if snapshot.get("limit") != LIMIT:
        print(
            f"::error::快照里的 limit={snapshot.get('limit')} 与脚本的 LIMIT={LIMIT} 不一致。"
            "上限来自规格 §13，改它要连同规格一起改，再跑 `--bless`。"
        )
        return 1

    recorded = snapshot.get("files", {})
    if args.dump:
        _dump(measured, recorded)

    errors = _check_new_offenders(measured, recorded)
    recorded_errors, stale = _check_recorded(measured, recorded)
    errors += recorded_errors

    for msg in errors:
        print(f"::error::{msg}")
    for msg in stale:
        print(f"::error::{msg}")

    if errors or stale:
        return 1

    print(
        f"快照里的 {len(recorded)} 个超限文件全部没有变长，且各自都写明了理由；"
        "快照之外没有新的超限文件——绿。"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
