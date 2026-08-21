#!/usr/bin/env python3
r"""扫描一份显式列出的「内容声明结构体/规则修正枚举」清单，检查每个字段
（或每个枚举变体）在「决策层」代码里是否至少有一处真实读取。

# 要抓什么问题

本项目最顽固的失败模式：字段被声明、被存储、被序列化、被哈希、有测试
守着往返——却没有任何游戏逻辑读它（见 `Agent.luck`、
`RaceDef.darkvision_floor`、`RuleModifier::Resistance` 三个已知实例）。
`cargo` 的 `dead_code` lint 抓不到这类字段，因为它们确实「被用了」——
写进列式存储、读出来构造 view、混入内容值哈希、在测试里断言存取往返都
算「用了」，但没有一处是「游戏结算会不会因为这个字段的值不同而算出不
同结果」。本脚本把这两类「用」分开。

# 存储层 vs 决策层，怎么区分

- **决策层**（`DECISION_LAYER_FILES`）：`ll-sim/src/*.rs`（`resolve`/
  `apply`/`combat`/`effect`/`intent`/`behavior`/`skill`/`item`/
  `resource_pool`/`traits`/`xp_curve`/`experience`/`quest`/`timeline`，
  不含 `tests/`、`examples/`）与 `ll-world/src/{fov,light}.rs`——这些是
  真正驱动模拟结算、影响玩法输出的地方。
- **存储层**（不在上面清单里的一切，包括但不限于）：`ll-mod`
  的列式存储/脚本注册 API、`ll-world::state::hash`、
  `ll-content::remap`、任意 crate 的 `tests/`、`examples/` 目录——这些
  只是把字段存进去、读出来、混进哈希、或在测试夹具里往返验证,不代表
  玩法逻辑真的消费了这个值。

这条边界照抄自本次任务书的判据,不是本脚本自己发明的分类——存量分歧
（例如某个字段该不该算"决策层"）应该体现在下面 `EXEMPTIONS`
清单的理由文字里，不是悄悄改判据。

# 判定"读取"的方法（以及为什么不做更精确的判定）

- **结构体字段**：在决策层文件全文里正则搜 `\.field_name\b`（点号 +
  字段名 + 单词边界）。选它是因为它天然把"结构体字面量写入"
  （`field_name: value,`，没有前导点号）和"真正读取"
  （`agent.field_name`、`self.field_name`、`view.field_name()`，有前导
  点号）分开——`Agent.luck`/`RaceDef.darkvision_floor` 的实测证实了这一
  点：两者在全仓库范围内都有几十处 `field_name:` 形式的写入（测试夹具
  逐个构造实例），但决策层里一处 `.field_name` 都没有。
- **枚举变体**（目前只有 `RuleModifier` 一例）：在决策层文件全文里正则
  搜 `EnumName::VariantName` 字面量。枚举变体的字段要被读到，前提是这个
  变体本身先在某处 `match`/构造里被点名——如果连变体名字都没在决策层
  出现过，它的字段不可能被读到，因此按变体（而不是变体内每个字段）汇报
  一条,与任务书里"`RuleModifier::Resistance` 战斗里没人读"的粒度一致。

# 已知局限（这类静态检查必然抓不到的东西）

1. **读了但结果被丢弃**：只要决策层文件里出现了 `.field_name`，本脚本
   就判定"已接线"，不检查这次读取的返回值是否真的参与了后续计算、还是
   读完就扔（例如 `let _ = agent.field_name;` 或读到之后传给一个空函数）。
2. **多层间接**：字段值先被拷进另一个局部变量/中间结构体，决策层只读
   那个中间产物、从不出现原始字段名字面量，本脚本会误判成"未接线"
   （假阳性,应加进 `EXEMPTIONS` 并写清楚间接路径）。反过来,如果决策层
   之外的某处代码恰好出现了同名字段的 `.field_name`（字段名撞车，例如
   另一个无关结构体也有个 `id` 字段）,本脚本会误判成"已接线"
   （假阴性）——这是选用"全文正则搜字段名"而不是真正做类型感知的
   代价,与 `check_i18n_strings.py`/`check_no_manual_euclidean_distance.sh`
   两个既有先例接受的同一类权衡一致。
3. **只覆盖 `TARGET_TYPES` 里显式列出的结构体/枚举**：这是"手工维护清单
   会漏新字段"与"全量扫描整个 workspace 噪音过大、无法在本次任务书
   授权范围内评估完成"之间的折中——见仓库里对应的门禁补齐提交说明。
   `TARGET_TYPES` 覆盖当前已知的十二张内容表中有独立 `*Def`/`*Attrs`
   结构体声明的部分，以及模拟运行期的 `Agent` 结构体；新增一张内容表
   或新增一个游戏逻辑结构体，若想被本门禁覆盖，需要把它加进
   `TARGET_TYPES`——这本身也是一处需要人工介入的地方，请在 code review
   里核对新表是否补了这一行。
4. **不解析真正的 Rust 语法**：结构体/枚举体的抽取靠"找到
   `^pub struct/enum Name` 这一行、再做括号配对"这种近似方法，多行宏、
   `cfg` 条件编译分支等边界情况可能取错字段列表。

# warn 还是阻断

**阻断**。豁免清单（`EXEMPTIONS`）已经把当前扫描到的全部未接线字段
显式收录，见每条的理由与预期接线阶段——存量已冻结，新增未豁免字段会
让 CI 变红，逼一个决定：要么接上决策层逻辑，要么显式补一条豁免理由
（在 code review 里可见）。与 `check_i18n_strings.py` 的 warn 模式不同：
那里 warn 是因为把历史命中标记为豁免需要改 `crates/**` 源码，不在当时
任务的改动授权范围内；这里的豁免清单在 `scripts/ci/` 本文件内维护，不
需要碰 `crates/**` 就能让门禁一开始就是绿的,没有"先 warn 免得 CI 一上
来就红"的顾虑。
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

REPO_ROOT = Path(__file__).resolve().parents[2]

# 「应当影响玩法的字段」清单的来源结构体/枚举。file 是相对仓库根的路径，
# kind 是 "struct" 或 "enum"，name 是类型名。见脚本头注释「已知局限」
# 第 3 条：这是显式维护的清单，不是全 workspace 自动发现。
TARGET_TYPES: list[tuple[str, str, str]] = [
    ("crates/ll-mod/src/race.rs", "struct", "RaceDef"),
    ("crates/ll-mod/src/item.rs", "struct", "ItemDef"),
    ("crates/ll-mod/src/class.rs", "struct", "ClassDef"),
    ("crates/ll-mod/src/skill.rs", "struct", "SkillDef"),
    ("crates/ll-mod/src/subclass.rs", "struct", "SubclassDef"),
    ("crates/ll-mod/src/quest.rs", "struct", "QuestNodeDef"),
    ("crates/ll-mod/src/resource_pool.rs", "struct", "ResourcePoolDef"),
    ("crates/ll-mod/src/trait_def.rs", "struct", "TraitDef"),
    ("crates/ll-mod/src/trait_def.rs", "enum", "RuleModifier"),
    ("crates/ll-world/src/terrain.rs", "struct", "TerrainDef"),
    ("crates/ll-sim/src/xp_curve.rs", "struct", "XpCurveDef"),
    ("crates/ll-world/src/entity/agent.rs", "struct", "Agent"),
]

# 决策层文件：真正驱动模拟结算、影响玩法输出的地方。见脚本头注释
# 「存储层 vs 决策层」一节。ll-sim/src 用整目录非递归通配（排除
# tests/、examples/ 子目录），ll-world 只挑 fov/light 两个文件。
DECISION_LAYER_GLOBS: list[str] = ["crates/ll-sim/src/*.rs"]
DECISION_LAYER_FILES: list[str] = [
    "crates/ll-world/src/fov.rs",
    "crates/ll-world/src/light.rs",
]

# 豁免清单：格式 "TypeName.field_name" 或 "EnumName::VariantName" ->
# (理由, 预期接线阶段/追踪出处)。照抄内容值哈希门禁（6e79783）的
# `ContentTableKind::Opaque` 豁免清单先例——未接线必须显式出现在这里，
# 理由与预期接线阶段写清楚，这样"断着"从"没人发现"变成"清单上明摆着"。
#
# 收录方式：本脚本 2026-08-21 首次在当前代码库上以阻断模式跑通，下面
# 是当时扫描到的全部未接线命中,逐条核实后分两类：
#   (a) 结构性字段——标识符/本地化键/尚未落地的碰撞与寻路输入,这类字段
#       语义上就不是"决策层要读的数值",不是"忘了接线",是"这类字段永远
#       不会被这份门禁要求接线"。
#   (b) 真正的死字段——数值/规则声明了,但没有任何决策层逻辑读它,已在
#       模块文档里承认（例如 RuleModifier 变体文档"当前无消费者"）或者
#       是当时核实新发现的死字段（例如 RaceDef.stat_modifiers——种族
#       属性修正接线批次已经补上真实决策层消费者
#       ll_sim::character::bake_race_stat_modifiers，见
#       crates/ll-sim/src/character.rs，但因与 TraitDef.stat_modifiers
#       同名字段撞车会污染本脚本的正则判定，本条豁免特意保留，理由见
#       下方该条目自己的文字）。
EXEMPTIONS: dict[str, str] = {
    # ---- (a) 结构性字段：id/本地化键/尚未有寻路消费者的体型 ----
    "RaceDef.id": "命名空间标识符，通过 ContentIndex 间接寻址，不是决策层直接按字段名读取的数值。",
    "RaceDef.display_name_key": "指向 Fluent 本地化键，UI 展示用，不是玩法数值，见规格 §11.3 i18n 边界。",
    "RaceDef.footprint": "占位格数——race-system.md 十二节明确标注碰撞/寻路是否支持 >1x1 占位尚未核实，声明先行、消费后补。预期随占位系统落地一并接线。",
    "RaceDef.lifespan_years": "只提供数据本身，race-system.md 七节论证了不需要额外硬编折扣系数；寿命对结算的影响（老化/死亡判定）是后续批次范围，此刻只建布局。",
    "ItemDef.id": "命名空间标识符，通过 ContentIndex 间接寻址，同 RaceDef.id。",
    "ClassDef.id": "同上。",
    "SkillDef.id": "同上。",
    "SubclassDef.id": "同上。",
    "QuestNodeDef.id": "同上（任务节点标识符）。",
    "ResourcePoolDef.id": "同上。",
    "TraitDef.id": "同上。",
    "TerrainDef.id": "同上。",
    "XpCurveDef.id": "同上。",
    "ItemDef.display_name_key": "同 RaceDef.display_name_key，指向 Fluent 本地化键，UI 展示用。",
    "ClassDef.display_name_key": "同上。",
    "SubclassDef.display_name_key": "同上。",
    "ResourcePoolDef.display_name_key": "同上。",
    "TraitDef.display_name_key": "同上。",
    # ---- (b) 已知死字段：本任务书点名的三处 ----
    "Agent.luck": "进 WorldState::hash()（state.rs），全项目零决策层逻辑读取——见 attribute-system.md 五节「未来将驱动」：暴击率/优势掷骰/掉落权重/稀有事件权重均未落地。P3 阶段只建布局，预期随暴击/优势判定系统一并接线。",
    "RaceDef.darkvision_floor": "存取完整、两条测试断言往返正确，但没有任何函数实现 race-system.md 五节文档写的 effective_light = max(实际光照, darkvision_floor)。预期随视野/光照系统消费暗视一并接线（ll-world/src/{fov,light}.rs）。",
    "RuleModifier::Resistance": "能从天赋声明、进内容值哈希，战斗里没人读——trait_def.rs 模块文档与枚举文档均已承认「当前没有任何 resolve 侧消费者」。预期随伤害结算的抗性乘数挂载点落地一并接线。",
    "RuleModifier::RerollOnce": "同 RuleModifier::Resistance，trait_def.rs 文档同一处承认无消费者——需要 roll_one_die 钩子，尚未落地。",
    "RuleModifier::Advantage": "trait_def.rs 变体文档原文「占位变体，当前无消费者（本项目没有判定/检定系统）」。",
    "RuleModifier::Disadvantage": "语义同 RuleModifier::Advantage，方向相反，同样没有判定系统可挂载。",
    # ---- (b) 本次核实新发现：文档看似"已接线"，实测决策层无读取 ----
    "RaceDef.stat_modifiers": "第二十处「声明了但没接线」修复批次已真正接上：ll_game::world::build_player_agent 生成角色时改为调用 ll_sim::character::bake_race_stat_modifiers，内部经 ll_sim::character::RaceStatModifierSource（真实实现 ll_mod::race::RaceTable）查到六项修正并叠加进 BaseStats——crates/ll-sim/src/character.rs、crates/ll-mod/src/race.rs 的对应测试均已覆盖端到端与真实 mod 种族两条路径。留在本清单是本脚本正则匹配的已知局限（见头注释「已知局限」第 2 条同一类问题）：该 trait 方法故意没有叫 stat_modifiers，而是叫 race_stat_modifiers——因为 ll_mod::trait_def::TraitDef 恰好也有一个同名字段 stat_modifiers（下面 TraitDef.stat_modifiers 一条，至今仍是真正的死字段），若这里用回同名方法，本脚本的全文正则会把两个不同结构体的同名字段一并误判成「已接线」。为了不把一个字段的真实接线连带污染另一个字段的状态判定，这里选择保留本条豁免（并把理由写清楚），而不是删除后制造一次假阳性。",
    "RaceDef.xp_reward": "杀死该种族/生物应授予的经验值——归并键设计已经在文档里论证清楚（与 Effect::IncrementKillCount 共享），但决策层目前没有 .xp_reward 读取点，经验授予的具体触发点尚未接上这张表。预期随击杀经验结算读取本字段一并接线。",
    "RaceDef.traits": "该种族授予的天赋引用列表——需要额外调用 RaceTable::add_trait_grant 才会真正生效，读取路径是通过 grant 表而非 .traits 字段本身，决策层没有直接 .traits 读取。",
    "RaceDef.starting_items": "出生携带物品列表——同 traits，声明与消费路径分离（starting_inventory 消费的是查表结果，不是对 RaceDef 实例做 .starting_items 点号访问），本脚本的字段名正则抓不到这条间接路径，见脚本头注释「已知局限」第 2 条。",
    # ---- (b) 其余本次扫描发现的死字段/占位字段，均有源码内注释自证 ----
    "ItemDef.base_weight": "ll-sim/src/item.rs 模块文档原文：resolve_pick_up/resolve_drop 不需要 base_weight/base_price/max_durability 中的任何一个，负重是后续批次的工作（YAGNI）。只收敛了 stack_limit 一个字段进 ItemCatalog 依赖倒置接口。",
    "ItemDef.base_price": "同 ItemDef.base_weight，同一处模块文档同一句话——经济系统（买卖定价）尚未落地。",
    "ItemDef.max_durability": "同 ItemDef.base_weight，同一处模块文档同一句话——耐久扣减是后续批次的工作，当前耐久收窄仅落到武器攻击穿透（600f458），不含最大耐久上限本身的结算消费。",
    "ClassDef.primary_attribute": "class.rs 字段文档原文：P5 阶段只是分类字段，供职业选择界面展示，不驱动结算逻辑。",
    "SkillDef.owning_class": "skill.rs 字段文档原文：只是一个分类/展示字段，不是命名空间隔离的边界，主职副职共享同一份技能命名空间（P5-4 裁定）。",
    "QuestNodeDef.condition": "任务结算走的是 ll-sim::quest::QuestCatalog::kill_count_quests() 返回的窄接口 QuestKillRule（依赖倒置，同 ItemDef.stack_limit 的收敛手法），不是对 QuestNodeDef 实例整体做 .condition 点号访问——真实消费路径在 ll-mod 侧把 QuestCondition 拆解、按需要的字段喂给 QuestKillRule，本脚本按字段名正则抓不到这条间接路径，见脚本头注释「已知局限」第 2 条。",
    "TraitDef.stat_modifiers": "天赋授予的属性修正——同 RaceDef.stat_modifiers 同一类问题：trait_def.rs 模块文档「据此聚合出角色对某个标量池的有效容量」一节只论证了容量聚合用途，六项主属性的直接修正应用尚未接入 effective_attribute 一类决策层函数。",
    "TraitDef.rule_modifiers": "存放的正是本清单已收录的四个 RuleModifier 变体——变体本身在决策层没有消费者，持有它们的这个字段自然也没有，见 RuleModifier::Resistance 等四条豁免。",
    "Agent.affiliations": "agent.rs 字段声明处紧邻注释原文：以下六个字段 P3 可以留空，但字段必须现在就有——见 society-and-affiliation.md 第五节，存档格式在 P5 冻结，P3 阶段不消费，只保证存档格式不用在 P8 补迁移链。",
    "Agent.goals": "同 Agent.affiliations，同一处「以下六个字段 P3 可以留空」注释覆盖的字段之一，见 agent-goals-and-economy.md 第九节。",
    "Agent.subclasses": "字段文档说明容器形状（允许同时持有多个副职）与设计裁定，但未声明任何决策层消费点；副职系统的技能号段/结算消费是后续批次工作。",
    "Agent.spawned_at": "字段文档原文：供死亡记录里「存活时长」一类未来统计使用——未来时态，当前没有任何结算逻辑读取存活时长。",
}


@dataclass(frozen=True)
class FieldTarget:
    """一个待检查的「字段」或「枚举变体」。"""

    reference: str  # "TypeName.field_name" 或 "EnumName::VariantName"
    def_file: str  # 声明处相对仓库根路径，报错定位用
    def_line: int  # 声明处行号（1-based）
    read_pattern: re.Pattern[str]  # 判定「决策层已读取」用的正则


def _line_no(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def _find_type_body(text: str, kind: str, name: str) -> tuple[int, str] | None:
    """定位 `^pub struct/enum Name` 这一行（要求真的在行首,不是文档注释
    里的代码示例——见脚本头注释「判定读取的方法」前一节踩过的坑：
    race.rs 模块文档里就有一段 `//!     pub struct RaceDef {` 的示例代码,
    行首正则天然跳过它)，返回 (方法体在原文里的起始行号, 方法体文本)。
    """
    pattern = re.compile(rf"^pub {kind} {re.escape(name)}\b[^\n{{]*\{{", re.MULTILINE)
    m = pattern.search(text)
    if m is None:
        return None
    brace_idx = m.end() - 1
    depth = 0
    i = brace_idx
    while i < len(text):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                body = text[brace_idx + 1 : i]
                return _line_no(text, brace_idx), body
        i += 1
    raise ValueError(f"{name}: 括号不配对，无法定位类型体结尾")


STRUCT_FIELD_RE = re.compile(r"^[ \t]*pub[ \t]+(\w+)[ \t]*:", re.MULTILINE)
# 枚举变体名：rustfmt 输出的四空格缩进,变体名后紧跟 `{`（结构体变体）、
# `(`（元组变体）或 `,`（单元变体），跳过文档注释行（`///`/`//!`）。
ENUM_VARIANT_RE = re.compile(r"^ {4}(\w+)[ \t]*[{(,]", re.MULTILINE)


def collect_targets() -> list[FieldTarget]:
    targets: list[FieldTarget] = []
    for rel_file, kind, name in TARGET_TYPES:
        path = REPO_ROOT / rel_file
        text = path.read_text(encoding="utf-8")
        found = _find_type_body(text, kind, name)
        if found is None:
            print(
                f"::error file={rel_file}::找不到 `pub {kind} {name}` 的定义"
                "（TARGET_TYPES 里的条目失效，类型可能被改名/删除/挪了文件），"
                "请更新 scripts/ci/check_field_consumers.py 的 TARGET_TYPES。"
            )
            sys.exit(1)
        body_start_line, body = found
        if kind == "struct":
            for fm in STRUCT_FIELD_RE.finditer(body):
                field = fm.group(1)
                line_no = body_start_line + body.count("\n", 0, fm.start())
                targets.append(
                    FieldTarget(
                        reference=f"{name}.{field}",
                        def_file=rel_file,
                        def_line=line_no,
                        read_pattern=re.compile(r"\." + re.escape(field) + r"\b"),
                    )
                )
        else:  # enum：按变体粒度汇报，理由见脚本头注释。
            for vm in ENUM_VARIANT_RE.finditer(body):
                variant = vm.group(1)
                line_no = body_start_line + body.count("\n", 0, vm.start())
                targets.append(
                    FieldTarget(
                        reference=f"{name}::{variant}",
                        def_file=rel_file,
                        def_line=line_no,
                        read_pattern=re.compile(
                            re.escape(f"{name}::{variant}")
                        ),
                    )
                )
    return targets


_LINE_COMMENT_RE = re.compile(r"(?<!:)//.*$")


def _strip_line_comments(text: str) -> str:
    """去掉整行/行尾的 `//`、`///`、`//!` 注释，避免文档注释里提到别的类型
    的字段名（例如 `traits.rs` 文档里写"RaceDef.traits 是注册表..."）被
    误判成决策层真的读取了那个字段——`RaceDef.traits` 一例是本脚本开发
    过程中实测踩到的假阳性，靠这一步过滤掉。`(?<!:)` 是为了不误伤
    `https://` 这类字符串里的 `//`（本代码库的决策层文件目前没有这种
    写法，加上纯粹是防御性的）。不处理块注释 `/* ... */` 与字符串字面量
    内的 `//`——本代码库这两类场景在决策层文件里未观察到，属于脚本头
    注释「已知局限」第 4 条同一类"近似而非真解析"的取舍。
    """
    return "\n".join(_LINE_COMMENT_RE.sub("", line) for line in text.splitlines())


def decision_layer_text() -> str:
    files: list[Path] = []
    for pattern in DECISION_LAYER_GLOBS:
        files.extend(sorted((REPO_ROOT).glob(pattern)))
    for rel in DECISION_LAYER_FILES:
        files.append(REPO_ROOT / rel)
    chunks = [_strip_line_comments(f.read_text(encoding="utf-8")) for f in files]
    return "\n".join(chunks)


def main() -> int:
    targets = collect_targets()
    layer_text = decision_layer_text()

    unwired: list[FieldTarget] = []
    for t in targets:
        if t.read_pattern.search(layer_text) is None:
            unwired.append(t)

    stale_exemptions = set(EXEMPTIONS) - {t.reference for t in targets}
    reported = [t for t in unwired if t.reference not in EXEMPTIONS]
    wired_refs = {t.reference for t in targets} - {t.reference for t in unwired}
    stale_because_wired = sorted(set(EXEMPTIONS) & wired_refs)

    print(f"扫描到 {len(targets)} 个目标字段/变体，决策层覆盖 "
          f"{len(DECISION_LAYER_GLOBS) + len(DECISION_LAYER_FILES)} 组文件路径。")
    print(f"未接线：{len(unwired)}（其中 {len(unwired) - len(reported)} 条已在豁免清单里）")

    exit_code = 0

    if reported:
        print("\n以下字段/变体在决策层没有任何读取，且不在豁免清单里：")
        for t in reported:
            print(
                f"::error file={t.def_file},line={t.def_line}::"
                f"{t.reference} 在决策层（{', '.join(DECISION_LAYER_GLOBS + DECISION_LAYER_FILES)}）"
                "没有任何 `.field`/`Enum::Variant` 形式的读取。要么补上消费它的游戏逻辑，"
                "要么在 scripts/ci/check_field_consumers.py 的 EXEMPTIONS 里加一条，"
                "写明理由与预期接线阶段。"
            )
        exit_code = 1

    if stale_because_wired:
        print(
            "\n以下豁免条目对应的字段/变体现在已经在决策层被读取到了，"
            "豁免清单没有同步删除（这条本身就是门禁该提醒的事——接线了就该把豁免摘掉）："
        )
        for ref in stale_because_wired:
            print(f"::error::{ref} 已被接线，请从 EXEMPTIONS 里删除这一条。")
        exit_code = 1

    if stale_exemptions:
        print(
            "\n以下豁免条目的字段/类型已经不在 TARGET_TYPES 扫描结果里"
            "（类型改名/字段删除/TARGET_TYPES 条目被移除），是死豁免，请清理："
        )
        for ref in sorted(stale_exemptions):
            print(f"::error::{ref} 是死豁免条目，请从 EXEMPTIONS 里删除。")
        exit_code = 1

    if exit_code == 0:
        print("\n全部目标字段/变体：决策层已读取，或已在豁免清单里显式登记。")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
