#!/usr/bin/env python3
r"""扫描一份显式列出的「内容声明结构体/规则修正枚举」清单，检查每个字段
（或每个枚举变体）在「决策层」代码里是否至少有一处真实读取。

# 要抓什么问题

本项目最顽固的失败模式：字段被声明、被存储、被序列化、被哈希、有测试
守着往返——却没有任何游戏逻辑读它（见 `Agent.luck`、
`RaceDef.darkvision_cells`、`RuleModifier::Resistance` 三个已知实例）。
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
  点号）分开——`Agent.luck`/`RaceDef.darkvision_cells` 的实测证实了这一
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
    # 伤害类别/抗性接线批次：RuleModifier 的定义从 ll-mod/src/trait_def.rs
    # 挪到了 ll-sim/src/traits.rs（trait_def.rs 现在只 `pub use` 它），
    # 理由是决策层（ll-sim）需要直接引用这个类型才能把 Resistance 接进
    # 伤害管线，见 ll_sim::traits::RuleModifier 文档「类型定义现居
    # ll-sim」一节。这条 TARGET_TYPES 条目跟着改指向新的定义处——巧合的
    # 是新的定义处恰好落在决策层 glob（crates/ll-sim/src/*.rs）内，这不
    # 影响判定逻辑：变体名字面量的匹配不区分它出现在哪个决策层文件里。
    # 抗性多来源聚合批次：RuleModifier 的定义再次搬家——从
    # ll-sim/src/traits.rs 挪到 ll-sim/src/rule_modifier.rs（traits.rs 现在
    # 只 `pub use` 它）。理由是它已经不再是"天赋专属"的类型：装备这一路
    # 来源同样声明它，聚合点也搬去了新模块，见 ll_sim::rule_modifier 模块
    # 文档「为什么单独立一个模块」一节。新的定义处同样落在决策层 glob
    # （crates/ll-sim/src/*.rs）内，判定逻辑不受影响。
    ("crates/ll-sim/src/rule_modifier.rs", "enum", "RuleModifier"),
    ("crates/ll-world/src/terrain.rs", "struct", "TerrainDef"),
    ("crates/ll-sim/src/xp_curve.rs", "struct", "XpCurveDef"),
    ("crates/ll-world/src/entity/agent.rs", "struct", "Agent"),
    # 伤害公式引擎批次（b08ad7c）新增的第十三张内容表，此前一直漏在
    # TARGET_TYPES 之外——本条是字段门禁自查补齐批次新加的，见本文件
    # 「与内容值哈希门禁互校」一节。
    ("crates/ll-sim/src/formula.rs", "struct", "FormulaDef"),
    # 伤害类别/抗性接线批次（fe2bbad）新增的第十四、十五张内容表，同上，
    # 此前也漏在 TARGET_TYPES 之外。
    ("crates/ll-mod/src/weapon_category.rs", "struct", "WeaponCategoryDef"),
    ("crates/ll-mod/src/damage_category.rs", "struct", "DamageCategoryDef"),
    # 天气系统批次新增的第十七张内容表。与 SpaceProfile（登记在
    # CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE 里、字段门禁抓不到）
    # 不同，WeatherDef 的两个乘数在决策层文件里有真正的 `.field_name`
    # 点号读取（crates/ll-world/src/light.rs 的 ambient_light_under 读
    # `weather.light_scale`、sight_radius_under_weather 读
    # `weather.sight_scale`——读的是派生值 Weather 而不是 WeatherDef 实例
    # 本身，但字段同名，正则一视同仁），因此本表按正常方式进 TARGET_TYPES，
    # 不走豁免。
    ("crates/ll-world/src/weather.rs", "struct", "WeatherDef"),
    # 制作系统批次新增的第十八、十九张内容表。与 WeaponCategory/
    # DamageCategory 当初的情形不同——这两张表**同批次就有真实决策层
    # 消费者**：ll_sim::resolve::resolve_craft 逐个读 category/
    # ingredients/product/product_count/required_station/required_tool
    # （读的是 ll_sim::craft::RecipeRule 而不是 RecipeDef 实例本身，但
    # 字段同名，正则一视同仁，与 WeatherDef 那条同一种情形）。
    ("crates/ll-mod/src/recipe.rs", "struct", "RecipeDef"),
    ("crates/ll-mod/src/recipe_category.rs", "struct", "RecipeCategoryDef"),
    # 耐久标签批次新增的第二十张内容表（ContentTableKind::Tag）。与
    # WeaponCategory/DamageCategory 当初的情形不同：这张表**同批次就有
    # 真实决策层后果**（ll_sim::resolve::resolve_attack/resolve_craft 的
    # 两条耐久磨损通道），只是那条消费路径是间接的，见下面 TagDef.wear
    # 的豁免理由。
    ("crates/ll-mod/src/tag.rs", "struct", "TagDef"),
    # 加值类型批次新增的第二十一张内容表（ContentTableKind::ModifierType）。
    # 这张表**一个字段都没有**（加值类型是纯身份，理由见 ll_mod::modifier_type
    # 模块文档），因此本条进 TARGET_TYPES 之后扫描到的字段数是 0——它不是
    # 为了让某个字段被检查而加的，是为了让 check_content_hash_gate_cross_coverage
    # 那条互校有一个正经的落点：哪天有人给 ModifierTypeDef 加了字段，字段门禁
    # 立刻就能看见它，不需要有人记得同时更新这份清单。
    ("crates/ll-mod/src/modifier_type.rs", "struct", "ModifierTypeDef"),
    # 资源点批次新增的第二十二张内容表（ContentTableKind::Resource）。
    # 声明结构体叫 ResourceAttrs 而不是 *Def：这张表没有「本体注册入口」
    # 那一层（本体四种资源与任何 mod 的资源走同一份 resources.json5），
    # 因此不需要一个带 id 的 *Def 声明形状——凭空造一个只为了对上命名
    # 惯例的空壳结构体，正是 ADR 0021 拦的那件事。SpaceProfile 那条
    # 豁免记录的是同一类命名分歧，区别只在这张表**能**被字段门禁抓到。
    ("crates/ll-world/src/resource.rs", "struct", "ResourceAttrs"),
    # 文化批次新增的第二十三张内容表（ContentTableKind::Culture）。
    # 声明结构体叫 CultureAttrs 而不是 *Def，理由与 ResourceAttrs 逐字
    # 相同：这张表没有「本体注册入口」那一层，本体六条文化与任何 mod
    # 的文化走同一份 cultures.json5。
    #
    # 六个字段全部走豁免（逐条理由见下面的 EXEMPTIONS）：真实消费者
    # 分别在 ll_world::chronicle（选址与战争）、ll_world::settlement
    # （建材）、ll_mod::roster（建立者种族），三处都是本脚本判据下的
    # 「存储/派生层」，进不了 DECISION_LAYER_FILES——与 ResourceAttrs
    # 的 residents_supported/settlement_draw/exhaustible 三条完全同一种
    # 处境，见那三条豁免自己的文字。
    ("crates/ll-world/src/culture.rs", "struct", "CultureAttrs"),
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
    "WeatherDef.id": "同上（天气标识符）。",
    "WeatherDef.display_name_key": "同 RaceDef.display_name_key，指向 Fluent 本地化键。与其余几条不同的是它有一个真实且已接线的 UI 消费者：ll_ui::hud::status_bar::StatusBarData::weather_display_name_key 每帧把它交给 Catalog::resolve 显示在状态栏（见 crates/ll-game/src/app.rs::draw_hud）——但那是表现层，不是本门禁定义的决策层，判据上仍归入「结构性字段」。",
    # ---- (c) 消费者在派生层而不是决策层 glob 覆盖的文件里 ----
    "WeatherDef.season_weights": (
        "唯一消费者是 ll_world::weather::weather_kind_at 的加权选取（`table.season_weights(index)[slot]`）"
        "——它决定「这一刻是什么天气」，随后那个天气才经 light.rs 影响视野与画面亮度。"
        "这是本文件头注释「已知局限」第 2 条的多层间接：字段值先被 weather_kind_at 消化成一个 "
        "ContentIndex，决策层文件（light.rs）读到的是派生出来的 Weather 而不是原始的权重数组，"
        "字段名字面量因此不出现在决策层。刻意不把 crates/ll-world/src/weather.rs 加进 "
        "DECISION_LAYER_FILES：那个文件同时也是这张表的列式存储与注册期校验所在地，"
        "把它算作决策层会让 WeatherTable::define 里的 `attrs.season_weights` 写入被误判成「已接线」，"
        "这张表的全部字段从此对本门禁形同虚设——宁可在这里留一条写明理由的豁免，"
        "也不要换来一份看起来更绿、实际更弱的门禁。"
    ),
    # ---- (b) 已知死字段：本任务书点名的三处 ----
    # `Agent.luck`/`RaceDef.darkvision_cells` 两条已在暗视/幸运接线批次
    # 真正接上（见 crates/ll-sim/src/vision.rs、
    # crates/ll-sim/src/resolve.rs::resolve_attack「暴击」一节、
    # crates/ll-sim/src/combat.rs::crit_chance_permille），均已从本清单
    # 移除——两个字段名在 TARGET_TYPES 覆盖的其余结构体里都不存在同名
    # 字段，不存在 RaceDef.stat_modifiers 撞上 TraitDef.stat_modifiers
    # 那种误判风险，因此选择让决策层直接用字段真名（`.darkvision_cells`/
    # `.luck`），而不是像 race_stat_modifiers 那样刻意换名保留豁免。
    #
    # 后续更新（幸运并入 AttributeKind 批次）：`Agent.luck` 字段本身已经
    # 从 `Agent` 结构体上整体移除（不再是"接上了但字段还在原处"，是
    # "字段搬去了 BaseStats.luck"）——TARGET_TYPES 里的 Agent 条目会
    # 自动按当前源码重新枚举字段列表，`luck` 不会再出现在扫描结果里，
    # 不需要（也不应该）在这里补一条 "Agent.luck" 豁免。`BaseStats` 本身
    # 不在 TARGET_TYPES 里（其余六项主属性字段——strength/dexterity/
    # constitution/intelligence/willpower/charisma——同样从未被本门禁
    # 单独追踪过，只通过 `Agent.stats`/各 `*Def.stat_modifiers` 这一层
    # 字段名参与扫描），`luck` 现在与它们同一个待遇，不是被本门禁放过，
    # 是从一开始就不属于本门禁的追踪粒度，与其余六项保持一致。
    # `RuleModifier::Resistance` 已在伤害类别/抗性接线批次真正接上：
    # `ll_sim::traits::resistance_multiplier_permille` 在决策层
    # （crates/ll-sim/src/traits.rs）里真实 `match`/解构这个变体，
    # `crate::resolve::resolve_attack` 在减伤链路算完之后调用它把乘数
    # 应用到伤害上——见 crates/ll-sim/tests/resistance_resolve.rs 三条
    # 端到端测试与 mods/example_mod 的真实 mod 脚本证据。原豁免条目已
    # 移除。
    # 判定系统落地批次：RuleModifier::RerollOnce / Advantage / Disadvantage
    # 三条豁免已移除。三者曾以「本项目没有判定/检定系统」为由挂了很久,
    # 现在有了：ll_sim::check 的对抗判定,消费者是
    # ll_sim::rule_modifier::check_roll_bias（优势/劣势）与
    # check_reroll_value（重掷）,两处都写全名字面量。真实落点是
    # ll_mod::native_behavior 的卫兵盘查判定与
    # ll_sim::resolve::resolve_inspect 的逐件藏匿判定；端到端证据见
    # crates/ll-mod/tests/example_mod_rogue_passives.rs 与
    # mods/example_mod/traits.json5 里 examplemod:cutpurse_training 的
    # kind: "advantage" 声明。
    # ---- (b) 本次核实新发现：文档看似"已接线"，实测决策层无读取 ----
    "RaceDef.stat_modifiers": "第二十处「声明了但没接线」修复批次已真正接上：ll_game::world::build_player_agent 生成角色时改为调用 ll_sim::character::bake_race_stat_modifiers，内部经 ll_sim::character::RaceStatModifierSource（真实实现 ll_mod::race::RaceTable）查到六项修正并叠加进 BaseStats——crates/ll-sim/src/character.rs、crates/ll-mod/src/race.rs 的对应测试均已覆盖端到端与真实 mod 种族两条路径。留在本清单是本脚本正则匹配的已知局限（见头注释「已知局限」第 2 条同一类问题）：该 trait 方法故意没有叫 stat_modifiers，而是叫 race_stat_modifiers——因为 ll_mod::trait_def::TraitDef 恰好也有一个同名字段 stat_modifiers（下面 TraitDef.stat_modifiers 一条，至今仍是真正的死字段），若这里用回同名方法，本脚本的全文正则会把两个不同结构体的同名字段一并误判成「已接线」。为了不把一个字段的真实接线连带污染另一个字段的状态判定，这里选择保留本条豁免（并把理由写清楚），而不是删除后制造一次假阳性。",
    "RaceDef.xp_reward": "杀死该种族/生物的**基准**经验值（升级加点批次重新定义：它是 ll_sim::experience::kill_experience 这条公式的输入，不再是玩家最终拿到的数字）。真实消费路径已经接通、且已经跑在生产链路上：ll_mod::xp_curve 的 `impl ExperienceCatalog for RaceTable` 读 `view.xp_reward`，ll_game::content::RuntimeCatalogs::as_resolve_catalogs 把 RaceTable 作为 experience 目录交给 TurnEngine，resolve_dispatch 的 append_kill_experience 每次击杀都会查它。留在本清单是本脚本头注释「已知局限」第 2 条那类多层间接：决策层（crates/ll-sim/src）拿到的是一个 i64 参数（`base_reward`），不是对 RaceDef 实例做 `.xp_reward` 点号访问——依赖倒置的必然结果，ll-sim 根本不认识 RaceDef 这个类型。端到端证据见 crates/ll-mod/tests/example_mod_kill_experience.rs（真实 mods/ 目录 + TurnEngine，含把 experience 目录换成空实现就变红的反例）。",
    "RaceDef.traits": "该种族授予的天赋引用列表——需要额外调用 RaceTable::add_trait_grant 才会真正生效，读取路径是通过 grant 表而非 .traits 字段本身，决策层没有直接 .traits 读取。",
    "ClassDef.traits": "职业天赋接线批次新增，与 RaceDef.traits 同一条本脚本已知局限（头注释第 2 条「多层间接」）：真实消费路径是 ll_mod::class::ClassTable 的 TraitGrantSource impl → ll_sim::traits::effective_traits（决策层确实在读，但读到的是 ClassView.traits 这个中间产物，`.traits` 这个字段名在决策层同时还会撞上 RaceDef/RaceView 的同名字段，本脚本的全文正则无法区分两者）。已接线的证据：crates/ll-sim/src/resolve.rs 六处聚合调用点全部经 ll_sim::traits::agent_trait_sources 把 Agent.profession × ClassTable 作为第二路来源传入；端到端证据在 crates/ll-mod/tests/example_mod_class_traits.rs（真实 mods/ 目录 + mods/example_mod/gameplay.scm 的 register-class-trait 调用，含「职业来源换成空实现就放不出技能」的反例断言）。",
    "SubclassDef.traits": "副职天赋接线批次新增（项目所有者裁定「副职……带有技能的」），与 ClassDef.traits / RaceDef.traits 同一条本脚本已知局限（头注释第 2 条「多层间接」）：真实消费路径是 ll_mod::subclass::SubclassTable 的 TraitGrantSource impl → ll_sim::traits::effective_traits（决策层确实在读，但读到的是 SubclassView.traits 这个中间产物，`.traits` 这个字段名在决策层同时还会撞上 RaceDef/ClassDef 的同名字段，本脚本的全文正则无法区分三者）。已接线的证据：ll_sim::traits::agent_trait_sources 现在把 Agent.subclasses 逐个展开成第三路来源（返回类型因此从 [TraitSource; 2] 变成 Vec<TraitSource>），crates/ll-sim/src/resolve.rs 六处聚合调用点与 crates/ll-sim/src/rule_modifier.rs 的 agent_rule_modifiers 全部经它拿到来源；生产接线在 ll_game::content::RuntimeCatalogs::as_resolve_catalogs 的 subclass_traits 一行；端到端证据在 crates/ll-mod/tests/example_mod_subclass_traits.rs（真实 mods/ 目录 + mods/example_mod/subclasses.json5 的 traits 声明，含「副职来源换成空实现就放不出技能」与「不持有这个副职就放不出」两条反例断言）。",
    "RaceDef.starting_items": "出生携带物品列表——同 traits，声明与消费路径分离（starting_inventory 消费的是查表结果，不是对 RaceDef 实例做 .starting_items 点号访问），本脚本的字段名正则抓不到这条间接路径，见脚本头注释「已知局限」第 2 条。",
    # ---- (b) 其余本次扫描发现的死字段/占位字段，均有源码内注释自证 ----
    "ItemDef.base_weight": "ll-sim/src/item.rs 模块文档原文：resolve_pick_up/resolve_drop 不需要 base_weight/base_price/max_durability 中的任何一个，负重是后续批次的工作（YAGNI）。只收敛了 stack_limit 一个字段进 ItemCatalog 依赖倒置接口。",
    "ItemDef.base_price": "同 ItemDef.base_weight，同一处模块文档同一句话——经济系统（买卖定价）尚未落地。",
    "ClassDef.primary_attribute": "职业的主属性倾向。**已有真实消费者，但那个消费者在呈现层而不是决策层，且它永远不该进决策层**：项目所有者裁定「升级获得属性点技能点，然后就自己加点」，加到哪一项由玩家决定——主属性若驱动任何结算（自动成长/加点更便宜/额外发点），就是在玩家的决定之外偷偷替他选，与那条裁定直接冲突。字段自己的文档从一开始写的用途（「供职业选择界面展示」）因此就是它唯一正确的用途，升级加点批次已把它接进 ll_ui::hud::character_panel（CharacterPanelData::primary_attribute，由 ll_game::app 从 ClassTable 查出后传入），在「你还有 N 点属性点可以加」那一行旁边给出「你这个职业倾向于哪一项」的提示。本条豁免因此不是「等着被接线」的临时状态，而是一条**永久判定**：本脚本的决策层通配（crates/ll-sim/src）里不应该、也不会出现对这个字段的读取。",
    "SkillDef.owning_class": "skill.rs 字段文档原文：只是一个分类/展示字段，不是命名空间隔离的边界，主职副职共享同一份技能命名空间（P5-4 裁定）。",
    "QuestNodeDef.condition": "任务结算走的是 ll-sim::quest::QuestCatalog::kill_count_quests() 返回的窄接口 QuestKillRule（依赖倒置，同 ItemDef.stack_limit 的收敛手法），不是对 QuestNodeDef 实例整体做 .condition 点号访问——真实消费路径在 ll-mod 侧把 QuestCondition 拆解、按需要的字段喂给 QuestKillRule，本脚本按字段名正则抓不到这条间接路径，见脚本头注释「已知局限」第 2 条。",
    "TraitDef.stat_modifiers": "天赋授予的属性修正——同 RaceDef.stat_modifiers 同一类问题：trait_def.rs 模块文档「据此聚合出角色对某个标量池的有效容量」一节只论证了容量聚合用途，六项主属性的直接修正应用尚未接入 effective_attribute 一类决策层函数。",
    # `Agent.affiliations` 曾在本清单里（理由是「P3 阶段不消费，只保证
    # 存档格式不用在 P8 补迁移链」）——它其实一直有一个真实消费者
    # （`is_hostile`：没有任何势力归属的实体视为对谁都敌对），只是那份
    # 代码住在 `ll-script`，不在本脚本认的决策层路径里。行为树搬进
    # 引擎的批次把它移到了 `ll_sim::ai_query`（同一份实现，不是复制），
    # 本门禁当场报出「已被接线，请从 EXEMPTIONS 里删除这一条」，据此
    # 摘除——与 `Agent.subclasses` 当初同一条流程。
    "Agent.goals": "同一处「以下六个字段 P3 可以留空」注释覆盖的字段之一（见 agent.rs 字段声明处紧邻注释与 agent-goals-and-economy.md 第九节）：存档格式在 P5 冻结，当前阶段不消费，只保证不用在 P8 补迁移链。",
    # `Agent.subclasses` 曾在本清单里（理由是「未声明任何决策层消费点；
    # 副职系统的结算消费是后续批次工作」）——制作系统批次落地的
    # `ll_sim::resolve::resolve_craft` 副职闸门是它的**第一个**真实决策层
    # 消费者（`agent.subclasses.contains(needed)`），本门禁当场报出
    # 「已被接线，请从 EXEMPTIONS 里删除这一条」，据此摘除。
    "Agent.spawned_at": "字段文档原文：供死亡记录里「存活时长」一类未来统计使用——未来时态，当前没有任何结算逻辑读取存活时长。",
    # ---- (c) 字段门禁自查补齐批次（本次任务）新发现：三张此前漏在
    # TARGET_TYPES 之外的内容表补齐后，扫出的死字段。见本文件「与内容
    # 值哈希门禁互校」一节：这三张表在内容值哈希门禁（ContentTableKind
    # 编译期穷尽 match）里早已被覆盖，字段门禁这边此前是手工清单漏项，
    # 不是这三张表本身没做过盘点。
    "FormulaDef.id": "命名空间标识符，同 RaceDef.id 一类——formula_for 按 ContentIndex 查表取出整个 FormulaDef 后直接使用 instructions/needs_rng，取出后不再对 .id 做任何决策层读取。",
    "FormulaDef.needs_rng": "字段自身文档原文（crates/ll-sim/src/formula.rs）：resolve_attack 当前恒构造一条骰子流，这个字段只用于诊断/未来性能预估，不影响求值正确性，即使一条不含骰子的公式拿到随机流也不会调用 DetRng 的任何方法。",
    # ── 资源点批次新增的六条 ──────────────────────────────────────
    #
    # 六条**全部**挂豁免，理由是同一条结构性事实，不是六次各自的疏漏：
    # 这张表的真实消费者是 `ll_world::chronicle`（据点选址/规模/覆灭
    # 推演）与 `ll_world::resource`（资源点判定本身），两者都不在
    # DECISION_LAYER_GLOBS/FILES 里。
    #
    # **为什么不干脆把 chronicle.rs 加进 DECISION_LAYER_FILES**（那是
    # 先试过的做法）：chronicle.rs 里到处是 `site.id`/`state.id`，而
    # `.id` 正是本文件头注释「已知局限」第 2 条说的字段名撞车——加进去
    # 会让十三条 `*Def.id` 豁免同时被误判成「已接线」，把一道真的在起
    # 作用的检查换成一堆假绿。用豁免 + 逐条写清间接路径，是本文件
    # 已经为 SpaceProfile 选过的同一条路。
    #
    # 解除条件：字段门禁的判据哪天扩展到「世界生成期推演」这一层
    # （那需要先解决 `.id` 这类字段名撞车，例如改成类型感知匹配），
    # 这六条应当一起摘掉重新评估。
    "ResourceAttrs.display_name_key": "同 RaceDef.display_name_key，指向 Fluent 本地化键。它有一个已经存在的接线目标——SettlementDemise::ResourceExhausted 携带资源的 ContentIndex，传说浏览要靠这个键把它变成「铁矿」两个字——但传说浏览 UI 尚未落地，与 RecipeDef.display_name_key 是同一种「等 UI」而不是「永远没人读」。",
    "ResourceAttrs.source_terrain": "本文件头注释「已知局限」第 2 条的多层间接：真实消费者是 ll_world::resource::resource_node_at 的第一道筛（地形不对就没有这种资源），而那是同 crate 的 resource.rs——它是这张表的列式存储所在地，按本门禁的判据属于存储层，不在 DECISION_LAYER_FILES 里。决策层（chronicle.rs）读到的是 survey_resources 已经数好的处数，不再出现 .source_terrain 字面量。",
    "ResourceAttrs.abundance": "理由同 ResourceAttrs.source_terrain：真实消费者是 resource_node_at 的第二道筛（按丰度掷一次由种子与坐标完全确定的骰子），同样落在存储层文件 resource.rs 里。ll_world::resource 的单元测试「丰度越高的资源在同一片源地形上出现得越多」直接验收这个字段真的在改变输出。",
    "ResourceAttrs.residents_supported": "真实消费者是 ll_world::chronicle 的 EpochRun::capacity（一片领地能养活多少人 → 据点长多大 → 铺几栋房子），见本节开头那段说明为什么 chronicle.rs 进不了 DECISION_LAYER_FILES。ll_world::chronicle 的单元测试「资源丰富的世界据点规模明显更大」直接验收这个字段真的在改变输出。",
    "ResourceAttrs.settlement_draw": "真实消费者是 ll_world::chronicle 的 EpochRun::resource_draw_bonus（守着铁矿的地方更容易建城），同上。",
    "CultureAttrs.display_name_key": (
        "同 RaceDef.display_name_key，指向 Fluent 本地化键。它有一个已经存在的接线目标——"
        "编年史要能说出「哥布林部落攻灭了矮人矿城」，呈现层由这个键取名字——但传说浏览 UI "
        "尚未落地，与 ResourceAttrs.display_name_key 是同一种「等 UI」而不是「永远没人读」。"
    ),
    "CultureAttrs.economy": (
        "真实消费者是 ll_world::chronicle 的 EpochRun::culture_weights（资源画像的大类命中"
        "这份文化靠什么吃饭就加权重 → 守着铁矿的地方长出矿业文化）。chronicle.rs 按本脚本的"
        "判据属于存储/派生层，进不了 DECISION_LAYER_FILES，理由见 ResourceAttrs."
        "residents_supported 那一条。ll_world::chronicle 的单元测试「文化抽取跟着资源走」"
        "直接验收这个字段真的在改变输出。"
    ),
    "CultureAttrs.home_terrain": (
        "真实消费者同上（EpochRun::culture_weights 的地形项：锚点那一格的基础地形命中这份"
        "文化的选址偏好就加权重）。这是项目所有者定的四条依据「资源 + 地形 + 邻近据点 + "
        "一点随机」里的第二条。"
    ),
    "CultureAttrs.wall_terrain": (
        "真实消费者是 ll_world::settlement 的 wall_terrain → house_tiles/ruin_tiles（一座"
        "哥布林营地是木头搭的、一座矮人矿城是石头砌的）。settlement.rs 同样是存储/派生层。"
        "ll_world::settlement 的单元测试「文化的建材真的换掉了墙」与 ll-game 的端到端验收"
        "「哥布林营地与矮人矿城用的不是同一种建材」各验收一次。"
    ),
    "CultureAttrs.founder_races": (
        "真实消费者是 ll_mod::roster::settlement_founder_race（这座据点是谁建的 → 整座据点"
        "的主体人口）。roster.rs 与 ResourceAttrs.category 那条记录的情形相同：它按本脚本的"
        "判据属于存储层。ll-game 的端到端验收「矮人矿城被哥布林部落攻灭」逐个断言攻守双方"
        "的建立者种族。"
    ),
    # CultureAttrs.hostility 的豁免已在「文化归属与敌对判定」批次删掉：
    # ll_sim::ai_query::declared_hostile 现在直接读它（撞格路由把「走进
    # 对方那一格」判成攻击还是互换），ai_query.rs 落在决策层通配里，本
    # 门禁因此能自己抓到，不再需要豁免。原豁免理由（消费者只在
    # ll_world::chronicle 的 wage_wars/pick_target，那是存储/派生层）
    # 仍然属实，只是不再是**唯一**的消费者。
    "ResourceAttrs.exhaustible": "真实消费者是 ll_world::chronicle 的 EpochRun::capacity / exhaustible_reserve / dominant_exhaustible 三处（资源枯竭这条覆灭原因的全部来源），同上。ll_world::chronicle 的单元测试「推演里真的出现过资源枯竭导致的覆灭」直接验收它。",
    "WeaponCategoryDef.default_formula": "weapon_category.rs 模块文档「本批次没有给 ItemDef 加对应字段」一节：十九节默认公式挂载链条第 3 层（武器类别默认）不在本批次范围，字段是声明先行——同一份文档已经预告了这条一旦补进 TARGET_TYPES 就会命中本门禁。",
    "RecipeDef.id": "命名空间标识符，同 RaceDef.id/FormulaDef.id 一类——resolve_craft 按 ContentIndex 查 RecipeCatalog 取出 RecipeRule，那个最小视图刻意不含 id（见 ll_sim::craft::RecipeRule 文档），决策层不做任何 .id 读取。",
    "RecipeDef.display_name_key": "同 RaceDef.display_name_key，指向 Fluent 本地化键，UI 展示用。制作界面（UiMode 模式栈）尚未落地，这个键当前没有 UI 消费者——但那与本门禁的判据（决策层是否消费）无关，本地化键按定义就不是玩法数值。",
    "RecipeCategoryDef.display_name_key": "同上。配方类别是玩家会看见的分组维度（制作界面按类别分栏），比其余几条更明确地是「等 UI」而不是「永远没人读」，见 crate::recipe_category 模块文档「与那两张表的两处不同」一节。",
    "RecipeCategoryDef.required_subclasses": "副职闸门**有真实决策层消费者**，但它经 ll_sim::craft::RecipeCatalog::category_required_subclasses 这个方法调用取出（依赖倒置：真正的字段读取 `def.required_subclasses` 发生在 ll-mod 的 impl 里，不在决策层文件），resolve_craft 拿到的是一个普通 Vec 局部变量——这正是本文件头注释「已知局限」第 2 条那类间接路径，字段级正则抓不到。与 QuestNodeDef 那几条经 QuestCatalog 方法取出的字段同一种情形。",
    # ---- (d) 耐久标签批次：项目所有者裁定「每个物品可以有个标签的列表，
    # 带有多个标签」。标签在引擎侧唯一的后果是耐久磨损通道，而那条消费
    # 路径**刻意**是「注册期折算 → 结算期查一个位掩码」，不是「结算期
    # 遍历标签列表现算」——ADR 0016/0017「声明式，注册期物化，运行期
    # 查表」。两条豁免记的都是这同一条间接路径的两头。
    "ItemDef.tags": "有真实决策层消费者，但路径是间接的：ll_mod::item::ItemTable::add_tag 在**注册期**把这份标签列表折算成 ll_sim::item::ItemRule::wear_channels（一个位掩码），resolve_attack 的「挨打/使用」两条通道与 resolve_craft 的工具磨损读的都是 `.wear_channels`（决策层三处真实点号读取），不是 `.tags` 本身。折算刻意放在注册期而不是结算期：一件物品带哪些标签、每个标签走哪条通道全是装载期就固定的事实，把「遍历标签→逐个查标签表→求并集」搬进每一次攻击 × 每一件已装备物品，正是 ADR 0016/0017 要避免的事。ItemRule 也刻意不携带 tags 原始列表（它是「resolve 需要什么就给什么」的最小视图）。本脚本头注释「已知局限」第 2 条点名的那类间接路径。",
    "TagDef.wear": "同 ItemDef.tags，是同一条间接路径的另一头：这个字段的值在注册期被 do_register_item_tag 读出来喂给 ItemTable::add_tag，折进物品的 wear_channels 派生列；决策层读到的是折算结果，从不对 TagDef 实例做 `.wear` 点号访问。与 RecipeCategoryDef.required_subclasses 那条（经 RecipeCatalog 方法取出、字段读取发生在 ll-mod 的 impl 里）是同一种情形，不是「字段没人读」。",
    "DamageCategoryDef.display_name_key": "同 WeatherDef.display_name_key：指向 Fluent 本地化键，且有一个真实且已接线的消费者——角色面板的规则修正一段（ll_ui::hud::character_panel）把「火焰抗性 6」里的「火焰」两个字从这个键查出来，装配点在 crates/ll-game/src/app.rs::draw_hud 传给 ll_sim::rule_modifier::rule_modifier_displays 的 subject_name_key 回调。那是表现层不是本门禁定义的决策层，判据上归入「结构性字段」；同时这也是本文件头注释「已知局限」第 2 条那类间接路径（决策层看到的是回调返回的 NamespacedId 局部变量，不会对 DamageCategoryDef 实例做 `.display_name_key` 点号访问）。",
    "DamageCategoryDef.default_formula": "damage_category.rs 模块文档「本批次范围：注册表 + 校验，不接四层默认公式解析链条」一节：resolve_attack 仍然只用 DamageFormulaCatalog 现有的两层（显式引用 → 全局默认），四层解析链条（分项自身 → 伤害类别默认 → 武器类别默认 → 全局默认）依赖尚未落地的 DamageComponent（P6 范畴），此字段声明先行、消费留给后续批次。",
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


# ---------------------------------------------------------------------------
# 与内容值哈希门禁互校
#
# `ll-mod/src/content_hash.rs` 的 `ContentTableKind` 枚举 + `classify_index`
# 函数是编译期强制穷尽的（该模块文档「编译期强制：穷尽解构 tables」一
# 节）——新增一张内容表，若忘了同时给 `ContentTableKind` 加判别值、给
# `classify_index` 补分支，`cargo build` 直接不过。`TARGET_TYPES` 相反，
# 是纯手工维护的清单，编译器管不到它——这正是本文件开头「已知局限」
# 第 3 条自己承认的洞：`FormulaDef`/`WeaponCategoryDef`/`DamageCategoryDef`
# 三张表就是这样连续漏过三批。
#
# 这条互校把内容值哈希门禁当权威来源：只要一张内容表已经进了
# `ContentTableKind`（说明它已经被编译期强制盯上了），它对应的决策结构体
# 就必须也出现在 `TARGET_TYPES` 里——否则字段门禁对这张表形同虚设。新增
# 一张内容表，若只接了内容值哈希、忘了同步补 `TARGET_TYPES`，本函数会
# 让 `check_field_consumers.py` 退出码非零，把「新增内容表」与「字段门禁
# 免检」这两件事重新绑在一起，不再需要人记得手工同步两份清单。
#
# 覆盖范围的例外：`SpaceProfile` 与 `Clip` 两个判别值虽然在
# `ContentTableKind` 里,但从未被字段门禁追踪过——不是本次任务遗漏。
# `Clip` 是纯表现层内容（ADR 0020 甲区,`crate::clip` 模块文档），按定义
# 就不存在"游戏结算读它"这件事,字段门禁"决策层是否消费"这条判据对它不
# 适用,两条门禁在这张表上的分歧是预期的。`SpaceProfile` 的属性经
# `SpaceProfileTable` 的稠密位查询被 `fov.rs`/`light.rs` 消费,不是对
# `SpaceProfile` 结构体实例做 `.field_name` 点号访问这条路径,字段级
# 正则天然抓不到,属于本文件头注释「已知局限」第 2 条同一类间接路径——
# 这两个不是被本互校放过,是显式登记进下面
# `CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE`、附理由,与
# `EXEMPTIONS` 同一套纪律：不允许静默跳过。
CONTENT_HASH_KIND_TO_TARGET_TYPE: dict[str, str] = {
    "Terrain": "TerrainDef",
    "Class": "ClassDef",
    "Skill": "SkillDef",
    "Subclass": "SubclassDef",
    "Quest": "QuestNodeDef",
    "Race": "RaceDef",
    "Trait": "TraitDef",
    "ResourcePool": "ResourcePoolDef",
    "Item": "ItemDef",
    "XpCurve": "XpCurveDef",
    "Formula": "FormulaDef",
    "WeaponCategory": "WeaponCategoryDef",
    "DamageCategory": "DamageCategoryDef",
    "Weather": "WeatherDef",
    "Recipe": "RecipeDef",
    "RecipeCategory": "RecipeCategoryDef",
    "Tag": "TagDef",
    "ModifierType": "ModifierTypeDef",
    "Resource": "ResourceAttrs",
    "Culture": "CultureAttrs",
}

CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE: dict[str, str] = {
    "Opaque": "不是一张内容表，是「不落在任何表里的纯 id 引用」判别值本身，见 ContentTableKind::Opaque 文档。",
    "SpaceProfile": "定义结构体是 SpaceProfile（ll-world/src/space_profile.rs），不是 *Def 命名；其属性经 SpaceProfileTable 的稠密位查询被 fov.rs/light.rs 消费，不是对结构体实例做 .field_name 点号访问，字段级正则抓不到这条间接路径。内容值哈希覆盖面扩展批次落地时就没有被字段门禁认领，不是本次任务遗漏。",
    "Clip": "纯表现层内容（ADR 0020 甲区，crate::clip 模块文档）——按定义就不存在「游戏结算读它」这件事，字段门禁的判据（决策层是否消费）对这张表不适用。内容值哈希仍然覆盖它是刻意的（哈希覆盖面判据与字段门禁判据不同，见 content_hash.rs 模块文档「哈希覆盖哪些字段」一节），两条门禁在这一张表上的分歧是预期的，不是字段门禁的洞。",
}

# 枚举判别值形如 `    Opaque = 0,`/`    Terrain = 1,`——与文件顶部
# `ENUM_VARIANT_RE` 不同（那条要求变体名后紧跟 `{`/`(`/`,`，这里的判别值
# 枚举永远是显式赋值的单元变体，后面跟 `= 数字,`），单独定义一条更贴合
# 的正则，不复用 `ENUM_VARIANT_RE` 以免两种格式的匹配意图混在一起。
CONTENT_TABLE_KIND_VARIANT_RE = re.compile(r"^ {4}(\w+)(?:\s*=\s*\d+)?,", re.MULTILINE)


def check_content_hash_gate_cross_coverage() -> list[str]:
    """互校主体：见本节前面的大段注释。返回错误信息列表，空列表表示通过。"""
    content_hash_path = REPO_ROOT / "crates/ll-mod/src/content_hash.rs"
    text = content_hash_path.read_text(encoding="utf-8")
    found = _find_type_body(text, "enum", "ContentTableKind")
    if found is None:
        return [
            "找不到 `pub enum ContentTableKind` 的定义（互校用的锚点失效，"
            "类型可能被改名/删除/挪了文件），请更新 "
            "scripts/ci/check_field_consumers.py 的 "
            "check_content_hash_gate_cross_coverage。"
        ]
    _, body = found
    variants = [m.group(1) for m in CONTENT_TABLE_KIND_VARIANT_RE.finditer(body)]
    if not variants:
        return [
            "`ContentTableKind` 枚举体解析出 0 个变体——正则可能与实际格式"
            "脱节，请检查 CONTENT_TABLE_KIND_VARIANT_RE。"
        ]

    target_type_names = {name for (_file, _kind, name) in TARGET_TYPES}
    errors: list[str] = []
    for variant in variants:
        if variant in CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE:
            continue
        target_type = CONTENT_HASH_KIND_TO_TARGET_TYPE.get(variant)
        if target_type is None:
            errors.append(
                f"ContentTableKind::{variant} 既不在 "
                "CONTENT_HASH_KIND_TO_TARGET_TYPE 映射里，也不在 "
                "CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE 豁免里——这是"
                "内容值哈希门禁新收录了一张表，但字段门禁互校没跟上，请给"
                " check_field_consumers.py 补一条映射或豁免。"
            )
            continue
        if target_type not in target_type_names:
            errors.append(
                f"ContentTableKind::{variant} 对应的决策结构体 {target_type} "
                "不在 TARGET_TYPES 里——这正是本互校要拦的情形：一张内容表"
                "已经进了内容值哈希门禁（编译期强制覆盖），但字段门禁的手工"
                "清单没跟上，该表的死字段会从字段门禁溜过去。请把它加进 "
                "TARGET_TYPES。"
            )

    # 反向也检查：两份映射/豁免自己不能失效（变体改名/删除后残留的死条目，
    # 与 EXEMPTIONS 的 stale 检查同一类纪律）。
    for stale in sorted(set(CONTENT_HASH_KIND_TO_TARGET_TYPE) - set(variants)):
        errors.append(
            f"CONTENT_HASH_KIND_TO_TARGET_TYPE 里的 {stale!r} 已经不在 "
            "ContentTableKind 的变体列表里了（枚举改名/删除），是死映射，"
            "请清理。"
        )
    for stale in sorted(set(CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE) - set(variants)):
        errors.append(
            f"CONTENT_HASH_KINDS_NOT_TRACKED_BY_FIELD_GATE 里的 {stale!r} "
            "已经不在 ContentTableKind 的变体列表里了，是死豁免，请清理。"
        )

    return errors


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

    cross_coverage_errors = check_content_hash_gate_cross_coverage()
    print(
        f"\n与内容值哈希门禁互校：ContentTableKind 覆盖的表 "
        f"{'⊆' if not cross_coverage_errors else '⊄'} TARGET_TYPES 覆盖的表"
        f"（{len(cross_coverage_errors)} 条不一致）。"
    )
    if cross_coverage_errors:
        for msg in cross_coverage_errors:
            print(f"::error::{msg}")
        exit_code = 1

    if exit_code == 0:
        print("\n全部目标字段/变体：决策层已读取，或已在豁免清单里显式登记。")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
