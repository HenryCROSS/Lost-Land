#!/usr/bin/env python3
r"""从 `WorldState` 出发自动求出「会被序列化进存档主体的类型闭包」，
把每个类型的**线格式形状**记进一份**由本脚本自己生成**的快照
（`scripts/ci/save_body_shape.json`），并强制：**形状变了，
`CURRENT_SCHEMA_VERSION` 必须在同一批递增。**

# 要抓什么问题

存档**主体**走的是 `postcard`（见
`crates/ll-content/src/save_file.rs::save_to_file`：
`postcard::to_allocvec(world)`），那是 **non-self-describing** 的二进制
格式——字节流里根本没有字段名，反序列化完全按声明顺序逐字段吃字节。
后果有两条，都已在本仓库实测过：

1. **`#[serde(default)]` 在存档主体这条路上完全是空操作。** 它需要格式
   能报告「这个字段缺席」，`postcard` 报告不了。实测（独立最小探针）：
   老结构体三字段编码 → 新结构体四字段带 `#[serde(default)]` 解码 →
   直接 `Err("Hit the end of buffer, expected more data")`。
2. **新字段若不在结构体末尾，后果比报错更糟**：后续字段的字节被错位读成
   **合法值**——静默数据损坏，没有任何东西会报错。

这个仓库已经犯过两次：`Agent::gender`（角色创建批次，2026-08-28）与
`GroundItemStack::placed`（家具放置批次）都往存档主体加了字段、都只加了
`#[serde(default)]`、都**没有**递增 `CURRENT_SCHEMA_VERSION`。两批各自的
「老存档读得回来」测试走的是 `serde_json::Value`——**自描述**格式，
`serde(default)` 在那里确实生效，于是测试全绿，**却一个字节都没碰到真正
的 postcard 主体那条路**。归属批次（2026-08-29）查出这件事，把常量 2 → 3
补上，同时留下本门禁，让第三次犯不出来。

# 勘查结论：存档主体到底涵盖哪些类型

**锚点不是一句话能列全的类型清单，而是一个可以机械求解的闭包**：

- 存档主体 = `postcard::to_allocvec(world)`，`world: &WorldState`。因此
  **根类型有且只有一个**：`ll_world::state::WorldState`。
- 从根出发沿「字段/变体载荷的类型」做可达性闭包，就是主体涵盖的全部
  类型。本脚本在 `DEF_SCAN_GLOBS` 覆盖的 crate 源码里建立「类型名 →
  定义」索引（`struct`/`enum`/`type` 别名），然后走这个闭包。
- 闭包沿三种边推进：① 结构体字段声明的类型；② 枚举变体载荷的类型；
  ③ **容器级 `#[serde(try_from/from/into = "X")]` 指向的中转类型**——
  `WorldState` 自己就是这一类（`#[serde(try_from = "WorldStateRepr")]`），
  漏了这条边等于漏掉真正决定反序列化布局的那半边。

闭包有多大、包含哪些类型，**跑一次就是答案**：

```bash
python scripts/ci/check_save_schema_version.py --dump
```

**不要把那张表抄进任何文档**（包括本注释）——它会漂移，这个仓库已经因为
「文档里存了一份会漂移的副本」出过三次事故（交接文档的常量表在两个会话
内过期三次，三个互不相干的代理各撞上一次）。

# 判据：形状快照 + 版本号联动

本脚本把闭包里每个类型的**线格式形状**（见下一节）写成一份 JSON 快照，
连同当时的 `CURRENT_SCHEMA_VERSION` 一起存在
`scripts/ci/save_body_shape.json`。每次 CI 重新计算，然后：

| 快照形状 | 版本号 | 结论 |
|---|---|---|
| 一致 | 一致 | **绿** |
| **变了** | **没动** | **红**——这正是要抓的缺陷 |
| 变了 | 已递增 | 红，但换一条信息：「跑 `--bless` 刷新快照」 |
| 一致 | 变了 | 红：版本白升了，或者快照没跟上，二选一说清楚 |

「已递增还要跑一次 `--bless`」不是折磨人：快照不刷新，下一批的形状变化
就会与**上上批**的快照比，版本号也与上上批比，于是「加了字段没升版本」
会被判成「版本已递增」而漏掉。快照必须与常量同步前进，门禁才咬得住。
**`--bless` 写进快照的内容全部由本脚本从源码算出，没有任何一个数字是人
手打的**——这是它与本仓库三次「手写清单」事故的根本区别（见下）。

# 「线格式形状」具体指什么，以及**为什么不含字段名**

`postcard` 的字节布局只取决于三件事：字段的**顺序**、字段的**类型**、
枚举变体的**下标**。它**不写字段名**。因此本脚本记录的形状是：

- 结构体：按声明顺序的 `[字段类型文本]` 列表（`#[serde(skip)]` 的字段
  **不计入**——它们真的不进字节流），外加每个字段上会改变线格式的
  serde 属性文本（`with`/`skip_serializing_if`/…）。
- 枚举：按声明顺序的变体列表，每个变体记 `[载荷类型文本]`。
- 容器级 `#[serde(...)]` 属性文本，以及该类型是否派生 `Serialize`/
  `Deserialize`。
- 若该类型有**手写** `impl Serialize`/`impl Deserialize`，额外记一条那段
  impl 正文的短哈希——手写实现的线格式与字段列表无关，字段列表管不住
  它，正文哈希才管得住。闭包里现在就有好几个这样的类型，`--dump` 会给
  它们打上 `[手写 serde]` 标记（**这里不列名字**：那又是一份会漂移的
  副本，见下一节）。

**故意不含字段名**：改名不改变 `postcard` 的任何一个字节，把改名判成
「形状变了、必须升版本」是纯粹的假红。宁可漏掉「改名」这种无害变化，
也不要制造一类会让人想去关掉门禁的假红（代价见「已知局限」第 1 条）。

# 为什么这不是本仓库的第四张手写清单

本仓库最稳定的缺陷来源就是「把真相源之外的副本当判据」：
`atlas_coverage.rs` 的手写地形清单（加地形不加行、移走贴图也不红）、
交接文档里的常量表（两个会话内过期三次、三个代理各撞一次）、
`skin.rs` 查裸贴图名（五张 UI 贴图全部静默退回纯色）。本门禁刻意不重蹈：

- **没有类型清单**：闭包从 `WorldState` 机械求出，新增类型自动进闭包，
  新增字段自动改变形状。要让它漏掉一个类型，必须让那个类型从
  `WorldState` 不可达——而那样它本来就不在存档主体里。
- **没有人手写的数值**：快照全文由 `--bless` 生成。
- **豁免有名有姓、有理由**（`EXEMPTIONS`），且**死豁免会红**——豁免的
  类型如果已经不在闭包里，门禁会要求清理，不允许豁免自己烂在那儿。

唯一人手维护的常量是 `ROOT_TYPE` 与 `SCHEMA_VERSION_FILE`：前者是
`save_to_file` 的实参类型，后者是常量所在的文件。两者都由本脚本在启动时
**回查源码验证**（找不到就直接红），不是无人看管的字符串。

# 已知局限（如实列出，覆盖不到的部分）

1. **同类型字段互换顺序抓不到。** `a: u32, b: u32` 换成 `b: u32, a: u32`
   语义变了、字节布局没变（类型序列相同），形状哈希不变。这是「不含
   字段名」的代价，见上一节的取舍理由。
2. **`#[serde(with = "模块")]` 只记属性文本，不看模块内部。** 那个模块的
   实现改了线格式（例如 `mod_state::serde_map` 换一种编码），本门禁不红。
   闭包里现在就有几处 `with`——具体几处、在哪几个类型上，查快照里的
   `attrs` 字段，不在这里抄一份。
3. **手写 `impl Serialize` 只比对正文哈希**，因此反过来也会假红：纯粹
   重构那段 impl（改局部变量名、拆函数）而线格式一个字节没变，也会红。
   这是刻意选的方向——手写序列化正文变了却不让人看一眼，风险更大。
4. **闭包外的类型不管。** 存档**头部**（`SaveHeader`）走的是明文 JSON，
   自描述格式，`#[serde(default)]` 在那里**确实生效**，加字段不需要升
   schema 版本，因此本门禁刻意不覆盖头部。`ll-sim` 的 `Timeline` 等类型
   不从 `WorldState` 可达，也不在覆盖范围内。
5. **不解析真正的 Rust 语法。** 类型定义靠正则找 `struct`/`enum`/`type`
   行再做括号配对，与 `check_field_consumers.py` 同一类近似方法；
   `cfg` 条件编译分支、宏生成的类型可能取错或取不到。宏生成的类型会
   表现为「未解析的叶子名」，见 `--dump` 的输出。
6. **外部 crate 的类型是不透明叶子。** `std`/`glam` 等类型只按名字记录，
   它们自己的布局变化（例如换一个依赖版本）本门禁看不见。
7. **同名类型冲突。** 闭包里若出现一个在扫描范围内有多份定义的类型名，
   本脚本**直接红**并要求在 `TYPE_DISAMBIGUATION` 里指明取哪一份——
   不猜，猜错等于门禁指着一个无关的类型算形状。

# 文件切分

本文件只放**判据与策略**；「怎么从 Rust 源码里把形状读出来」在
`scripts/ci/save_shape_lib.py`。两者是独立会变的事，写在一起会越过仓库
800 行的文件上限，也会逼着读判据的人先翻四百行正则。

# warn 还是阻断

**阻断**。存量在落地这道门禁的同一批里用 `--bless` 冻成绿的，之后任何
形状变化都必须显式过一次「升版本 + 刷新快照」的门。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from save_shape_lib import IDENT_RE, TypeDef, build_index, shape_of  # noqa: E402

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

REPO_ROOT = Path(__file__).resolve().parents[2]

# 存档主体的根类型。它不是随手选的：`ll-content/src/save_file.rs` 的
# `save_to_file` 里唯一那次 `postcard::to_allocvec(world)` 的实参就是
# `&WorldState`。`verify_anchors` 会回查这一行，改了签名而没同步这里，
# 门禁直接红。
ROOT_TYPE = "WorldState"

# 回查锚点用的源码位置与判据正则。
BODY_ENCODE_FILE = "crates/ll-content/src/save_file.rs"
BODY_ENCODE_PATTERN = re.compile(r"postcard::to_allocvec\(\s*world\s*\)")
SCHEMA_VERSION_FILE = "crates/ll-content/src/save_file.rs"
SCHEMA_VERSION_PATTERN = re.compile(r"pub const CURRENT_SCHEMA_VERSION:\s*u32\s*=\s*(\d+)\s*;")

# 建立「类型名 → 定义」索引时扫描的源码范围。存档主体的闭包当前完全落在
# ll-world / ll-core 两个 crate 里；把 ll-mod 也纳进来是因为
# `ContentIndex`/`NamespacedId` 这类内容层类型会出现在闭包边缘。范围放宽
# 的代价是同名类型冲突变多（见 TYPE_DISAMBIGUATION）。
DEF_SCAN_GLOBS = [
    "crates/ll-world/src/**/*.rs",
    "crates/ll-core/src/**/*.rs",
    "crates/ll-mod/src/**/*.rs",
]

# 快照文件。全文由 `--bless` 生成，任何一行都不该手写。
SNAPSHOT_PATH = REPO_ROOT / "scripts" / "ci" / "save_body_shape.json"

# 同名类型的取舍。键是类型名，值是相对仓库根的文件路径——闭包里出现同名
# 冲突时必须在这里指明取哪一份，脚本不猜（猜错等于门禁指着一个无关的
# 类型算形状）。
TYPE_DISAMBIGUATION: dict[str, str] = {
    # `ResourceKind` 在 ll-world（世界资源种类，进存档）与 ll-sim（技能
    # 消耗的资源池种类，不进存档）各有一个同名类型。存档主体里的那个是
    # ll-world 的：`SettlementSite::resources` 引用的就是它。
    "ResourceKind": "crates/ll-world/src/resource.rs",
}

# 豁免：键是类型名，值必须写清楚**为什么**它可以不参与形状比对。死豁免
# （豁免的条目已经不在闭包里）会让门禁变红，不允许豁免自己烂在那儿。
#
# 目前为空：闭包里那五个手写 `impl Serialize` 的类型不走豁免，走「impl
# 正文哈希」这条更严的路（见头注释「已知局限」第 3 条）。
EXEMPTIONS: dict[str, str] = {}


def resolve(name: str, index: dict[str, list[TypeDef]]) -> tuple[TypeDef | None, str]:
    """按类型名取唯一定义；有歧义时返回错误信息而不是瞎猜一个。"""
    candidates = index.get(name)
    if not candidates:
        return None, ""
    if len(candidates) == 1:
        return candidates[0], ""
    picked = TYPE_DISAMBIGUATION.get(name)
    if picked:
        for c in candidates:
            if c.file == picked:
                return c, ""
        return None, (
            f"TYPE_DISAMBIGUATION[{name!r}] 指向 {picked}，但那里没有 {name} 的定义"
            "（文件改名或类型搬家了），请更新这条。"
        )
    where = ", ".join(f"{c.file}:{c.line}" for c in candidates)
    return None, (
        f"类型名 {name} 在扫描范围内有 {len(candidates)} 份定义（{where}），"
        "本脚本不猜取哪一份——请在 TYPE_DISAMBIGUATION 里指明。"
    )


def compute_closure(
    index: dict[str, list[TypeDef]],
) -> tuple[dict[str, dict], set[str], list[str]]:
    """从 ROOT_TYPE 出发求「会被序列化进存档主体的类型闭包」。"""
    shapes: dict[str, dict] = {}
    unresolved: set[str] = set()
    errors: list[str] = []
    seen: set[str] = set()
    stack = [ROOT_TYPE]
    while stack:
        name = stack.pop()
        if name in seen:
            continue
        seen.add(name)
        defn, err = resolve(name, index)
        if err:
            errors.append(err)
            continue
        if defn is None:
            unresolved.add(name)
            continue
        shape = shape_of(defn)
        shapes[name] = {
            "kind": shape.kind,
            "file": defn.file,
            "container": shape.container_attrs,
            "members": shape.members,
            "manual_serde": dict(sorted(shape.manual_serde.items())),
        }
        stack.extend(ref for ref in sorted(shape.refs) if ref not in seen)
    return shapes, unresolved, errors


def fingerprint(shapes: dict[str, dict]) -> str:
    canonical = json.dumps(shapes, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def verify_anchors() -> tuple[int | None, list[str]]:
    """回查两处锚点，确认本门禁盯的还是真实存在的那条编码路径与那个常量。"""
    errors: list[str] = []
    body_src = (REPO_ROOT / BODY_ENCODE_FILE).read_text(encoding="utf-8")
    if BODY_ENCODE_PATTERN.search(body_src) is None:
        errors.append(
            f"在 {BODY_ENCODE_FILE} 里找不到 `postcard::to_allocvec(world)`——存档主体的"
            f"编码入口换了写法或搬家了。本门禁的根类型锚点 ROOT_TYPE={ROOT_TYPE!r} "
            "依赖这一行，请一并核对。"
        )
    ver_src = (REPO_ROOT / SCHEMA_VERSION_FILE).read_text(encoding="utf-8")
    m = SCHEMA_VERSION_PATTERN.search(ver_src)
    if m is None:
        errors.append(
            f"在 {SCHEMA_VERSION_FILE} 里找不到 "
            "`pub const CURRENT_SCHEMA_VERSION: u32 = N;`——常量改名或换了类型，"
            "本门禁无法判定版本号，请更新 SCHEMA_VERSION_PATTERN。"
        )
        return None, errors
    return int(m.group(1)), errors


def load_snapshot() -> dict | None:
    if not SNAPSHOT_PATH.exists():
        return None
    return json.loads(SNAPSHOT_PATH.read_text(encoding="utf-8"))


def write_snapshot(version: int, shapes: dict[str, dict]) -> None:
    payload = {
        "_note": (
            "本文件由 scripts/ci/check_save_schema_version.py --bless 生成，不要手工编辑。"
            "它记录存档主体（postcard 编码的 WorldState 闭包）的线格式形状，"
            "以及生成它时的 CURRENT_SCHEMA_VERSION。"
        ),
        "schema_version": version,
        "fingerprint": fingerprint(shapes),
        "types": shapes,
    }
    # 显式 newline="\n"：仓库 `.gitattributes` 规定 `*.json text eol=lf`，
    # 而 Windows 上 `write_text` 默认把 `\n` 翻成 CRLF，快照会整文件漂移，
    # 每次在 Windows 上 `--bless` 都产出一份「只有行尾变了」的假 diff。
    SNAPSHOT_PATH.write_text(
        json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def _diff_one(name: str, old: dict, new: dict) -> tuple[list[str], list[str]]:
    breaking: list[str] = []
    benign: list[str] = []
    if old.get("kind") != new.get("kind"):
        return [f"{name}：类型种类从 {old.get('kind')} 变成 {new.get('kind')}"], benign
    if old.get("container") != new.get("container"):
        breaking.append(
            f"{name}：容器级 serde 属性变了 {old.get('container')} → {new.get('container')}"
        )
    if old.get("manual_serde") != new.get("manual_serde"):
        breaking.append(f"{name}：手写 impl Serialize/Deserialize 的正文变了")
    om, nm = old.get("members", []), new.get("members", [])
    if old.get("kind") != "enum":
        if om != nm:
            breaking.append(f"{name}：字段序列变了（{len(om)} → {len(nm)} 个字段）")
        return breaking, benign
    common = min(len(om), len(nm))
    if om[:common] != nm[:common]:
        breaking.append(f"{name}：已有枚举变体的下标或载荷变了（会错位读老存档）")
    elif len(nm) > len(om):
        benign.append(
            f"{name}：在枚举末尾追加了 {len(nm) - len(om)} 个变体"
            "（老存档里不会出现这些下标，读得回来）"
        )
    elif len(nm) < len(om):
        breaking.append(f"{name}：删掉了 {len(om) - len(nm)} 个枚举变体")
    return breaking, benign


def diff_shapes(old: dict[str, dict], new: dict[str, dict]) -> tuple[list[str], list[str]]:
    """返回 (必须升版本的变化, 向后兼容但仍须刷新快照的变化)。"""
    breaking = [f"{n}：不再属于存档主体闭包（字段被删/类型被移出）" for n in sorted(set(old) - set(new))]
    breaking += [f"{n}：新进入存档主体闭包（有字段引用了它）" for n in sorted(set(new) - set(old))]
    benign: list[str] = []
    for name in sorted(set(old) & set(new)):
        b, g = _diff_one(name, old[name], new[name])
        breaking += b
        benign += g
    return breaking, benign


def _dump(shapes: dict[str, dict], unresolved: set[str]) -> None:
    print("\n闭包内的类型：")
    for name in sorted(shapes):
        s = shapes[name]
        extra = "  [手写 serde]" if s["manual_serde"] else ""
        print(f"  {name:<28} {s['kind']:<6} {s['file']}{extra}")
    print("\n不透明叶子（外部 crate 类型 / 基础类型 / 解析不到的名字）：")
    print("  " + ", ".join(sorted(unresolved)))


def _print_changes(breaking: list[str], benign: list[str]) -> None:
    for msg in breaking:
        print(f"  · {msg}")
    for msg in benign:
        print(f"  · （向后兼容，但仍须刷新快照）{msg}")


def _verdict(version: int | None, snapshot: dict, shapes: dict[str, dict]) -> int:
    old_version = snapshot.get("schema_version")
    old_shapes = snapshot.get("types", {})
    breaking, benign = diff_shapes(old_shapes, shapes)
    shape_changed = fingerprint(old_shapes) != fingerprint(shapes)

    if not shape_changed and version == old_version:
        print("\n存档主体形状与快照一致，CURRENT_SCHEMA_VERSION 未变——绿。")
        return 0

    if shape_changed and version == old_version:
        print(
            f"\n::error::存档主体的线格式形状变了，但 CURRENT_SCHEMA_VERSION 仍然是 "
            f"{version}。存档主体走 postcard（non-self-describing），加/删/换序字段会让"
            "老存档要么读不出来、要么被错位读成合法值（静默数据损坏），"
            "`#[serde(default)]` 在这条路上是空操作、救不了。请把 "
            f"{SCHEMA_VERSION_FILE} 里的 CURRENT_SCHEMA_VERSION 递增，再跑 "
            "`python scripts/ci/check_save_schema_version.py --bless` 刷新快照。"
        )
        _print_changes(breaking, benign)
        return 1

    if not shape_changed:
        print(
            f"\n::error::CURRENT_SCHEMA_VERSION 从 {old_version} 变成 {version}，"
            "但存档主体的形状一个字节都没变。要么这次升版本是多余的（改回去），"
            "要么形状变化落在本门禁覆盖不到的地方（见脚本头注释「已知局限」），"
            "后者请跑 `--bless` 刷新快照并在提交信息里写明变的是什么。"
        )
        return 1

    print(
        f"\n::error::存档主体形状变了，CURRENT_SCHEMA_VERSION 也已经从 {old_version} "
        f"递增到 {version}——这一半是对的。剩下一步：跑 "
        "`python scripts/ci/check_save_schema_version.py --bless` 把新形状写进快照。"
        "（快照不刷新，下一批的形状变化就会与上上批的快照比，"
        "「加了字段没升版本」会被误判成「版本已递增」而漏掉。）"
    )
    _print_changes(breaking, benign)
    return 1


def main() -> int:
    parser = argparse.ArgumentParser(description="存档主体形状 ↔ schema 版本联动检查")
    parser.add_argument("--bless", action="store_true", help="用当前源码重新生成形状快照")
    parser.add_argument("--dump", action="store_true", help="打印闭包与不透明叶子，用于勘查")
    args = parser.parse_args()

    version, errors = verify_anchors()
    index, scanned = build_index(REPO_ROOT, DEF_SCAN_GLOBS)
    shapes, unresolved, closure_errors = compute_closure(index)
    errors += closure_errors

    print(
        f"扫描 {len(scanned)} 个源文件，索引到 {len(index)} 个类型名；"
        f"从 {ROOT_TYPE} 出发的存档主体闭包含 {len(shapes)} 个本地类型，"
        f"{len(unresolved)} 个不透明叶子。"
    )
    print(f"CURRENT_SCHEMA_VERSION = {version}；形状指纹 = {fingerprint(shapes)[:16]}…")

    if args.dump:
        _dump(shapes, unresolved)

    for ref in sorted(set(EXEMPTIONS) - set(shapes)):
        errors.append(f"EXEMPTIONS 里的 {ref!r} 已经不在存档主体闭包里了，是死豁免，请清理。")

    if errors:
        for msg in errors:
            print(f"::error::{msg}")
        return 1

    if args.bless:
        if version is None:
            print("::error::拿不到 CURRENT_SCHEMA_VERSION，不能生成快照。")
            return 1
        write_snapshot(version, shapes)
        print(f"\n已把当前形状写进 {SNAPSHOT_PATH.relative_to(REPO_ROOT).as_posix()}"
              f"（schema_version={version}）。")
        return 0

    snapshot = load_snapshot()
    if snapshot is None:
        print(
            f"::error::找不到形状快照 {SNAPSHOT_PATH.relative_to(REPO_ROOT).as_posix()}，"
            "请跑 `python scripts/ci/check_save_schema_version.py --bless` 生成。"
        )
        return 1
    return _verdict(version, snapshot, shapes)


if __name__ == "__main__":
    sys.exit(main())
