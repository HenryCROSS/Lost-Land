# 设计文档总索引

本目录下十六份文档共同描述「迷途大陆」的物品、装备、属性、社会、经济、种族、世界历史、身份标识、命名与本地化、坐标与空间模型、脚本层数据句柄与批量查询、脚本状态存储、三轴战斗结算、增益与通用触发器、职业技能与任务、动画与视觉特效边界十六个子系统。它们分开冻结、分次写成，彼此高度依赖但没有统一校对过——这份索引是校对结果：谁管什么、谁引用了谁、贯穿全局的原则用在哪几处、该按什么顺序读。

前五份（物品、装备、属性、社会、经济）是最早冻结的一批，中间四份（种族、世界历史、身份与 ID 空间、命名与本地化）是在前五份基础上补的一批已拍板决定——它们大量引用前五份的既有结构（`Affiliation`、`Kinship`、`ContentIndex`、`BaseStats`……），几乎不新增底层机制，只是把前五份没覆盖到的角落（种族怎么算、历史怎么生成、生成物怎么引用、名字怎么本地化）填满。

第十份（[坐标系与空间模型](coordinate-system-and-layers.md)）冻结时间最晚，且不属于「社会/经济」这条主线——它管的是世界本身怎么组织（区块、连续地表、离散空间），是 P2 地形层的架构性替换，而不是在前九份的底座上再叠一层玩法系统。它引用前九份里的少数几处（`world-history.md` 的聚落粒度、`race-system.md` 的暗视接口、`agent-goals-and-economy.md` 的 LOD 与棘轮问题思路、`identity-and-ids.md` 的 `WorldId`），但反过来也被 P5/P7 的其余一切系统依赖——存档格式、世界生成、聚落归属都建在它定的区块/空间形状上（[2026-08-18 规格修订] 插入「物品与装备」新 P6 阶段后，世界生成所在的原 P6 顺移为 P7）。

第十三、十四份（[三轴战斗结算](combat-three-axis.md)、[增益与通用触发器](buffs-and-triggers.md)，均冻结于 2026-08-18）与第十份同批新增，但走的是完全不同的一条依赖线——它们不碰坐标/空间，而是直接扎进 P3 已经落地的 `resolve`/`apply` 战斗结算管线与新 P6「物品与装备」阶段的接口空白：`resolve_attack` 目前只是攻击力恒读力量、防御恒为零、穿透恒为 `NONE` 的占位实现，`Intent::Attack` 也只能表达单体目标。第十三份把"瞄准形状 × 伤害系别 × 投送方式"拆成三条正交轴，取代按武器类型分类实现的直觉方案；第十四份在此之上补齐增益的惰性到期判定与一套通用触发器框架，取代"每种命中效果各开一个专用钩子"的直觉方案。两份都直接点名了[属性系统](attribute-system.md) `derive_stats` 与[物品系统](item-system.md)/[装备栏位](equipment-slots.md) `StatBonus` 这处早已标注、迟迟未补的接线缺口（见下方表格「不管什么」栏与「缺口 5」），第十三份给出了具体的接线点建议，但**不代为定义 `StatBonus` 本身**——这项工作仍然留给新 P6 阶段。

第十五份（[职业 / 技能树 / 副职 / 任务系统](class-skill-quest-system.md)，冻结于 2026-08-19，晚于前十四份）走的是 P5-B 批次自己的一条独立主线——它不依赖坐标/空间、也不依赖脚本层数据句柄，只依赖第一份（[属性系统](attribute-system.md)，技能效果最终要落到属性/衍生数值上）。`ClassDef`/`SkillDef` 已经落地为 `crates/ll-mod/src/{class,skill}.rs`，是十六份文档里除坐标系与脚本状态存储之外第三个有实际代码支撑的。它同样点名了 `StatBonus`/装备接线这处缺口（本索引「五、技能效果的数值边界」一节称之为「P6 装备接线的硬边界」），与第十三、十四份指向同一处新 P6 阶段的开放项。

第十六份（[动画与视觉特效的边界](animation-and-vfx-boundary.md)，冻结于 2026-08-19，晚于前十五份）走的又是一条独立主线——它不依赖坐标/空间、脚本层数据句柄、脚本状态存储，也不依赖第十三至十五份的战斗/技能设计本身，只依赖两份既有的架构决定（[ADR 0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) 引擎层/玩法层判据、[ADR 0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md) 甲区/乙区判据）与一份已经部分落地的引擎代码（`crates/ll-render/src/anim.rs`）。它管的是「结算与表现的边界该画在哪」，与第十三、十四、十五份的关系是**消费而非依赖**——三轴战斗结算的命中判定、增益的生效状态、技能效果的触发，都是未来驱动动画播放的 `Effect` 来源，但本文档不改动它们的任何结论，只说明表现层该怎么订阅它们的输出。

**交叉核对中发现的矛盾与缺口**，见 [conflicts.md](conflicts.md)（4 条待裁定均已裁定、3 条设计缺口、2 条记录备查——均针对前五份文档，后四份及第十三、十四、十五、十六份写作时发现的新问题已直接记在各自文档正文或本索引「落地状态」一节里，未额外并入这份清单）。已裁定的结论已经落进对应文档正文，这里仅保留裁定记录供追溯；设计缺口与记录备查两类尚未处理，读到相关章节时请留意。

---

## 一、十六份文档各管什么、不管什么

| 文档 | 管什么 | 不管什么 |
|---|---|---|
| [物品系统](item-system.md) | `ItemDef`/`ItemStack` 定义与实例分离、堆叠合并规则、`Owner` 归属枚举、`ItemLocation` 位置模型、地面物品老化清理、六档品质与倍率表、耐久、重量与负重分档、`use_effect` 脚本接口 | 装备如何占用槽位（见装备文档）、物品如何提供属性加成的具体消费逻辑（`StatBonus` 未在任何文档定义，见缺口 5）、物品定价如何变成行会售价（见经济文档，换算关系未写明，见缺口 7）、`Owner::Faction` 背后的组织语义（见社会文档） |
| [装备栏位与占位掩码](equipment-slots.md) | 22 槽位定义、`SlotMask` 位运算、装备互斥/自动卸下规则、渲染层排序、装备流程的 Effect 序列 | 装备如何转化为攻防数值（见属性文档）、`ItemDef`/`ItemStack` 本身的字段（见物品文档）、单槽位 `EquipSlot` 类型的正式定义（未在任何文档给出，见缺口 6） |
| [属性系统](attribute-system.md) | 六维主属性（STR/DEX/CON/INT/WIS/CHA）、调整值公式、三系攻防、护盾、四种穿透、伤害公式、幸运、次级属性列表、d20 判定、与时间轴调度的接口 | 属性加成从哪来（`ItemDef.stat_bonuses` 的消费逻辑未定义）、装备如何贡献属性的具体聚合算法（未在装备文档给出）、议价/名望等次级属性如何真正接入经济结算（见经济、社会文档）、种族对主属性的修正（见种族文档） |
| [社会系统](society-and-affiliation.md) | `Affiliation` 统一结构（势力/宗教/行会/文化/家族/职业六类共用一个数据结构）、`CultureDef` 生成权重、地图结构层（聚落/道路/遗迹/地标/资源点）、宗教如何运转、家族与代际、关系系统的默认派生与记忆偏移、性格 `Traits`、职业声望的局部化、LOD 兼容性 | 具体的悬赏任务结构、行会定价公式、钱包与货币守恒（见经济文档）、物品本身的字段（见物品文档）、种族对关系派生基线的贡献（见种族文档「种族偏见」一节）、`Affiliation.org` 的具体 ID 类型（见身份文档） |
| [Agent 目标与经济](agent-goals-and-economy.md) | 目标—需求—任务—悬赏循环、行会中介贸易与定价、钱包与货币守恒、破产/致富/土匪负反馈、职业审计算法、商队与途中报价、背景/前景/具名三档精度与棘轮问题、惰性追赶的边界、`ll-econsim` 验收指标 | `Affiliation`/`CultureDef` 等归属结构本身（见社会文档）、物品与装备的字段定义（见物品、装备文档）、属性如何影响战斗结算（见属性文档） |
| [种族系统](race-system.md) | `Agent.race`/`RaceDef` 的形状、属性修正的烘焙时机、时间轴速度的数值预算、暗视接口与 FOV 对称性的关系、体型的取舍边界、寿命的三条平衡手段、薄层存储与 `birth_settlement`、混血规则、种族偏见的乘法分解、美术成本约束 | `RaceDef` 的具体数值表（数值定稿属于内容设计，非本文档范围）、种族分布场如何生成（见世界历史文档）、命名是否跟种族走（不跟，见命名文档，本文档只管属性/时间轴/暗视/体型/寿命/偏见/美术） |
| [世界历史生成](world-history.md) | 历史生成期「只模拟被记住的家族」这一核心架构、族谱有界规则、`Kinship` 单一真相源、三层时间粒度、生成流程七步、事件日志规模控制、选点白送的三件事、世界生成时间的规避手段、阶段归属与前置依赖 | `Settlement`/`WorldId` 等具体类型定义（见身份文档）、种族分布场具体怎么生成噪声（只说明是流程第二步，算法细节属于地形/噪声实现范畴）、`Kinship` 结构本身（见社会文档） |
| [身份与 ID 空间](identity-and-ids.md) | `ContentIndex`（类型）与 `WorldId`（实例）的分野、`OrgInstance` 形状、`WorldId` 永不复用与不需要代际号的理由、mod 定义具体势力的落点、脚本 API 查询式而非引用式的理由、存档如何记录 mod 集合（内容哈希、缺失 mod 的分类处理、生成期/当前两组 mod 集合分离、schema 版本与 mod 版本分开报错） | `Affiliation` 结构本身（见社会文档）、`WorldId` 的具体产出来源（见世界历史文档）、`EntityId` 本身的世代号机制（见 [0004](../decisions/0004-two-layer-entity-storage.md)）、规格 §10.4/§11.2 本身的存档格式定义（不改规格，只补规格没覆盖的策略缺口） |
| [命名、改名与本地化](naming-and-localization.md) | 命名跟「出生地文化」而非种族走的理由、`birth_settlement` 一列两用、多语言音素表索引对齐、改名作为 `Effect`/`HistoricalEvent`、改名权限的派生方式、派生名与覆盖名两条不同规则、mod 命名钩子与标签体系的前瞻判断 | `NamingRules`/`given_name`/`surname` 的具体实现（已落地，见 `crates/ll-world/src/naming.rs`，本文档只补机制之外的三层设计）、`birth_settlement` 列本身的引入理由（见种族文档，本文档只讲它的第二个用途） |
| [坐标系与空间模型](coordinate-system-and-layers.md) | 区块（zone）与两种坐标分辨率、`Space`（`Surface`/`Interior`）统一接口、地表连续无缝流式加载、离散空间的稀疏存储、`SpaceProfile` 层属性与光照的组合关系、FOV/相机在新模型下的复用边界、LOD 判定的区块化、存储块与区块的对齐决策 | `Settlement`/`WorldId` 的具体产出来源（见世界历史文档）、暗视本身的数值与对称性论证（见种族文档，本文档只讲光照来源的组合方式）、`SpaceProfile` 的具体数值表（内容设计范畴）、内容注册表本身的实现机制（见 P4 计划文档，本文档只给接口设想） |
| [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) | 脚本操作 Rust 侧数据的句柄语义（`ScriptEntityHandle`/`EntitySetHandle`/`GroupedEntitySetsHandle`，基于 `steel-core` 的 `Custom` trait）、防伪造论证、句柄跨帧持有与约束 C1 的关系、`Intent::Attack` 解禁形状、批量查询原语清单（筛选/投影/聚合/排序/分组）、确定性排序的平局规则、逃生舱的可见代价机制 | 具体属性词表的完整枚举（内容设计范畴）、`Arena` 世代号机制本身（见 [0004](../decisions/0004-two-layer-entity-storage.md)，本文档只说明如何直接复用它做失效检测）、Steel VM 沙箱能力边界本身（见 [0012](../decisions/0012-steel-capability-surface-verification.md)，本文档只在其基础上设计新接口） |
| [脚本状态存储](script-state-storage.md) | 脚本跨帧/跨存档持久化状态的受认可通道（`ScriptValue` 值类型系统、全局命名空间存储与每实体扩展数据两种形状）、能不能存实体引用的结论与依赖前提、命名空间隔离与跨 mod 只读查询、有界性配额与超限处理、mod 移除后孤儿状态的保留策略、约束 C1「隐式」限定词的修订表述、存档/读档边界强制重建 VM 的策略与代价评估 | `ScriptEntityHandle` 本身的防伪造机制（见[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md)，本文档直接复用其失效检测约定）、脚本内部能不能用浮点计算（见 [0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)，本文档只管"哪些类型能跨过 `register_fn` 边界落进世界状态"）、被禁能力的通盘盘点（见 [0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md)，本文档只是该通则的一个具名范例） |
| [三轴战斗结算](combat-three-axis.md) | 瞄准形状/伤害系别/投送方式三条正交轴、`TargetSpec`/`Intent::Attack` 扩展形状、`resolve` 范围展开管线、装备属性 → `DerivedStats` 的接线点、确定性展开顺序（`EntityId` 升序）、FOV/距离/批量查询三样既有机制的复用方式 | `StatBonus` 本身的正式定义（见物品/装备文档缺口 5/6，本文档只指出接线点）、护盾扣减的具体顺序（见属性文档 §二，本文档不重复设计）、技能树如何消费三轴（见 P5 技能树设计，未落笔） |
| [增益与通用触发器](buffs-and-triggers.md) | `ActiveEffect` 的惰性到期判定、`DerivedStats` 如何吸收生效增益、多个增益改同一属性的确定性合并顺序、`TriggerDef`/`TriggerResponse` 通用触发器框架、触发链的深度上限与队列化防递归、堆叠策略 `StackPolicy` 三选一 | 触发器一/二档具体编译产物的字节码形状（数值/实现细节，属新 P6 落地时决定）、`on_hit`/`on_kill` 等触发点本身在 `resolve`/`apply` 管线里的插桩位置（本文档只给出队列化调度模型，具体代码插桩留给实现） |
| [职业 / 技能树 / 副职 / 任务系统](class-skill-quest-system.md) | `ClassDef`（职业注册表）、`SkillDef`（技能树，前置关系用 DAG 不用线性序列）、`SubclassDef`（副职，命名空间裁定 P5-4）、`QuestNodeDef`（网状任务，前置列表是单一真相源，解锁视图是派生）、技能效果的数值边界 | 具体职业/技能/任务的数值内容（内容设计范畴）、`StatBonus` 本身的正式定义（见物品/装备文档缺口 5/6，本文档同样只指出接线点是「P6 装备接线的硬边界」） |
| [动画与视觉特效的边界](animation-and-vfx-boundary.md) | 结算（`WorldState`/`Effect`）与表现（动画/特效）的单向依赖边界、原地循环/一次性特效/投射物三类动画的差异与各自缺口、环境动画相位的零存储派生、非格子位置的类型与归属、连续输入下的动画调度策略（可跳过/排队/叠加）、`EffectDef` 注册表的判据与形状 | `anim.rs` 现有 `Clip`/`Playback` 的具体实现（已落地，本文档只指出它覆盖到哪、缺什么）、具体特效的美术内容（内容设计范畴）、`resolve_attack`/`ActiveEffect`/技能效果本身如何产生 `Effect`（见第十三、十四、十五份，本文档只消费它们的输出） |

一句话版边界：**物品定义「是什么」，装备定义「戴在哪」，属性定义「打起来怎么算」，社会定义「谁跟谁什么关系」，经济定义「钱和活儿怎么流动」，种族定义「先天差异有多少、体现在哪几处」，世界历史定义「世界是怎么变成现在这样的」，身份定义「东西怎么被引用而不会指错」，命名定义「叫什么、谁能改」，坐标与空间定义「世界本身怎么划分、怎么按需生成」，三轴战斗定义「打的时候具体算什么」，增益与触发器定义「效果怎么持续、怎么互相触发而不失控」，职业技能任务定义「玩家能学什么、接什么」，动画与视觉特效边界定义「算完的东西该怎么演给玩家看，演的过程绝不能反过来改算的结果」。** 十六者共用同一个 `Agent`/`ItemStack`/`Affiliation`/`TorusPos`/`DerivedStats`/`Effect` 底座，但没有一份文档试图覆盖别人的地盘——边界比内容更容易搞混，出现「这个概念该去哪份文档找」的疑惑时，先查下面的对照表。

---

## 二、核心概念对照表

| 概念 | 定义于 | 被引用/依赖于 |
|---|---|---|
| `ItemDef` / `ItemStack`（定义与实例分离） | [物品系统](item-system.md) | [装备栏位](equipment-slots.md)（`equip_mask` 字段类型来自装备文档的 `SlotMask`）、[Agent 目标与经济](agent-goals-and-economy.md)（`Caravan.cargo: Vec<ItemStack>`） |
| `Owner`（物品归属：无主/玩家/NPC/势力/商店） | [物品系统](item-system.md) | [社会系统](society-and-affiliation.md)（`Owner::Faction` 的组织语义由 `Affiliation` 提供、家族共有财产） |
| `SlotMask`（22 槽位位掩码） | [装备栏位](equipment-slots.md) | [物品系统](item-system.md)（`ItemDef.equip_mask`）、[属性系统](attribute-system.md)（护甲按已装备槽位聚合） |
| `EquipSlot`（单槽位标识，**未正式定义**，见缺口 6） | 无——[物品系统](item-system.md) 使用了这个类型名 | — |
| 六档品质与倍率表 | [物品系统](item-system.md) | [Agent 目标与经济](agent-goals-and-economy.md)（进入行会定价的「基础价」，换算关系未写明，见缺口 7） |
| 六维主属性 `BaseStats`（STR/DEX/CON/INT/WIS/CHA） | [属性系统](attribute-system.md) | [物品系统](item-system.md)（负重分档由 DEX 驱动的行动耗时公式反推）、[社会系统](society-and-affiliation.md)（CHA→招募/议价）、[Agent 目标与经济](agent-goals-and-economy.md)（CHA→议价、领导力→随从上限） |
| `Penetration`（四种穿透：破甲/破魔/破意/破盾） | [属性系统](attribute-system.md) | — |
| `StatBonus`（**未正式定义**，见缺口 5） | 无——[物品系统](item-system.md) 使用了这个类型名，两份相关文档区块都指向对方 | — |
| `Affiliation` / `AffiliationKind`（势力/宗教/行会/文化/家族/职业统一结构） | [社会系统](society-and-affiliation.md) | [物品系统](item-system.md)（`Owner::Faction`）、[Agent 目标与经济](agent-goals-and-economy.md)（行会中介贸易即 `AffiliationKind::Guild`、职业审计即 `AffiliationKind::Profession`） |
| `CultureDef`（文化生成权重） | [社会系统](society-and-affiliation.md) | — |
| `Traits`（性格：勇敢/贪婪/忠诚/合群/记仇/虔诚） | [社会系统](society-and-affiliation.md) | [Agent 目标与经济](agent-goals-and-economy.md)（职业审计「性格允许」一档；熟练度作为收益差距阈值的调制项已一并补入判定条件，原冲突 3，已裁定） |
| `Kinship`（血缘：父母/配偶/子女） | [社会系统](society-and-affiliation.md) | — |
| 关系默认派生基线 + 记忆偏移 | [社会系统](society-and-affiliation.md) | — |
| `Agent`（厚层实体） | [社会系统](society-and-affiliation.md) §五 与 [Agent 目标与经济](agent-goals-and-economy.md) §九 **共同**约束（两份文档各列一半字段，代码里合并成一个结构） | 双方互相依赖；已落地为 `crates/ll-world/src/entity/agent.rs` |
| `Goal`（目标链：类型/参数/进度/优先级） | [Agent 目标与经济](agent-goals-and-economy.md) | — |
| `Task`（任务/悬赏，`assignee: Option` 区分私人队列与公开池） | [Agent 目标与经济](agent-goals-and-economy.md) | [社会系统](society-and-affiliation.md)（行会/神殿的任务接取权由 `Affiliation` 判定） |
| `Caravan`（商队） | [Agent 目标与经济](agent-goals-and-economy.md) | [社会系统](society-and-affiliation.md)（沿道路走，与聚落道路正反馈是同一个循环） |
| `WealthTier`（破产/温饱/小康/富裕） | [Agent 目标与经济](agent-goals-and-economy.md) | — |
| 行会定价公式（`本地价` + 买家归属系数） | [Agent 目标与经济](agent-goals-and-economy.md) | [社会系统](society-and-affiliation.md)（「同势力/同行会打折」由 `成交价 = 本地价 × 买家归属系数` 实现，原冲突 2，已裁定） |
| 背景层/前景层/具名层三档精度（「被记住/被模拟」两轴） | [社会系统](society-and-affiliation.md) §六「LOD 兼容性」与 [Agent 目标与经济](agent-goals-and-economy.md) §七之二 共同给出，术语已统一 | 早期决策 21「近景/中景/远景」的距离式提法已废弃——距离不是决定精度的轴，「被记住/被模拟」才是（原冲突 1，已裁定，本索引里曾经最要紧的一条） |
| `RaceDef`（种族定义：属性修正/暗视地板/体型/寿命，注册表，mod 可扩展） | [种族系统](race-system.md) | [属性系统](attribute-system.md)（属性修正烘焙进 `BaseStats` 的时机）、[社会系统](society-and-affiliation.md)（`race_affinity` 挂在 `CultureDef` 上参与关系派生与职业审计） |
| `Agent.race: ContentIndex` / `ThinPopulation.race` | [种族系统](race-system.md) | 已落地为 `crates/ll-world/src/entity/agent.rs` 与 `thin.rs`；**但薄层落地方式与设计不符**——当前是显式存储列，设计要求零列现算，见种族文档「存储」一节的实现债务框 |
| `birth_settlement`（出生地，终身不变，区别于会随迁徙变化的 `settlement`） | [种族系统](race-system.md)「存储」一节首次提出 | [命名、改名与本地化](naming-and-localization.md)（同一列同时驱动命名依据的文化查询）；**未落地**，当前薄层只有 `settlement` 一列 |
| `WorldId` / `OrgInstance`（世界生成实例的持久标识，区别于 mod 定义类型用的 `ContentIndex`） | [身份与 ID 空间](identity-and-ids.md) | [社会系统](society-and-affiliation.md)（`Affiliation.org` 字段类型需要从 `ContentIndex` 改为 `WorldId`，尚未实现）、[世界历史生成](world-history.md)（势力/家族/聚落的产出方） |
| `Settlement`（聚落结构，历史生成的核心操作对象） | [世界历史生成](world-history.md)「阶段归属」列为前置依赖之一 | 正式字段定义**未在任何文档给出**，仅在社会系统「地图结构层」的 `StructureKind::Settlement` 变体与薄层 `settlement: Vec<u16>` 列里以不同粒度间接出现，三者尚未对齐——供后续实现时留意，不计入 conflicts.md（该清单只覆盖前五份文档间的交叉核对） |
| `NamingRules`（命名规则：音素表 + 拼接方式） | [社会系统](society-and-affiliation.md) 提出字段位置（`CultureDef.naming`）；真正的类型定义与实现在 `crates/ll-world/src/naming.rs`（已落地） | [命名、改名与本地化](naming-and-localization.md)（多语言版本表长对齐规则、命名依据出生地文化而非种族） |
| `Space`（`Surface`/`Interior`，统一地表与离散空间的接口） | [坐标系与空间模型](coordinate-system-and-layers.md) | 取代原本设想的单一 `(i,j,z)` 三元组；`Interior` 复用 [身份与 ID 空间](identity-and-ids.md) 的 `WorldId` 作为 `SpaceId` |
| 区块（zone，128×128 格，世界地图的逻辑单位） | [坐标系与空间模型](coordinate-system-and-layers.md) | [世界历史生成](world-history.md)（聚落播种、势力形成的操作粒度）；与既有存储粒度 `CHUNK_SIZE=32`（`crates/ll-world/src/chunk.rs`）按 4×4 写死对齐 |
| `SpaceProfile`（层/空间属性：环境光基准、是否露天、温度、可挖掘/可建造，注册表内容） | [坐标系与空间模型](coordinate-system-and-layers.md) | [种族系统](race-system.md)（暗视 `darkvision_floor` 的光照输入现在经由 `exposed_to_sky` 判断，接口本身不改）；走 P4 内容注册表同一套「静态声明/物化/注册期校验」模式 |
| `TargetSpec`（`Entity`/`Tile`/`Set` 三种瞄准载荷） | [三轴战斗结算](combat-three-axis.md) | 取代 `Intent::Attack` 原本只能表达 `target: EntityId` 单体的限制 |
| `AimShape` / `DamageSchool` / `DeliveryMode`（瞄准形状/伤害系别/投送方式三条正交轴） | [三轴战斗结算](combat-three-axis.md) | [属性系统](attribute-system.md)（`DamageSchool` 决定读取 `DerivedStats` 的哪一个字段、用哪一种 `Penetration`） |
| `ActiveEffect`（生效中的增益/减益实例：`expires_at`/`stacks`/`applied_at`/`source`） | [增益与通用触发器](buffs-and-triggers.md) | [属性系统](attribute-system.md)（`derive_stats` 入参新增「生效增益」一项，与装备贡献同一次算出 `DerivedStats`） |
| `TriggerDef` / `TriggerResponse`（通用触发器定义：`on_hit`/`on_kill`/`on_damaged`/`on_turn_start`/`on_death`） | [增益与通用触发器](buffs-and-triggers.md) | [物品系统](item-system.md)（`use_effect` 可以只是注册一个触发器，与 `Effect::MoveItem` 一样是少数通用原语覆盖大量内容的模式） |
| `StackPolicy`（`RefreshDuration`/`AddIntensity`/`Independent` 三选一，走注册表声明） | [增益与通用触发器](buffs-and-triggers.md) | — |
| `ClassDef`（职业注册表，`Agent.profession` 之外的职业身份） | [职业/技能树/副职/任务系统](class-skill-quest-system.md) | 已落地为 `crates/ll-mod/src/class.rs` |
| `SkillDef`（技能树节点，前置关系为 DAG） | [职业/技能树/副职/任务系统](class-skill-quest-system.md) | 已落地为 `crates/ll-mod/src/skill.rs`，注册期用拓扑排序校验无环 |
| `SubclassDef` / `QuestNodeDef`（副职、网状任务，均**未落地**） | [职业/技能树/副职/任务系统](class-skill-quest-system.md) | — |
| `Clip` / `Playback`（原地循环动画的帧序列与整数帧号播放，**已落地**） | `crates/ll-render/src/anim.rs` | [动画与视觉特效的边界](animation-and-vfx-boundary.md)（一次性特效/投射物在其外层包一层生命周期/位置管理，不改动这两个类型本身） |
| `EffectDef` / `EffectTable`（特效内容的注册表，**未落地**，本文档给出形状建议） | [动画与视觉特效的边界](animation-and-vfx-boundary.md) | 照抄 `SkillTable`/`TerrainTable`（[职业/技能树/副职/任务系统](class-skill-quest-system.md)）已验证的 `ContentIndex` + 列式存储模式 |

---

## 三、贯穿全局的设计原则

这几条原则不属于任何一份文档，是十六份文档共用的思维方式。看到某处设计「为什么要这么绕」，多半能在这里找到答案。

### 「默认派生，只存偏差」

能用公式当场算出来的值，不存进世界状态；只存「偏离公式的那一点」。已识别的实例（原文档用「第三次」「第五次」这类编号互相指涉，但编号并未覆盖全部实例，见 [conflicts.md](conflicts.md) 条目 8，这里按文档出现顺序直接列全）：

1. **NPC 钱包**——`agent-goals-and-economy.md` §七之二：`钱包 = 批量公式(种子, ID, 时长) + 偏移量`，长期不交互就重定基准。已落地为 `ThinPopulation`（`crates/ll-world/src/entity/thin.rs`）。
2. **个体↔个体关系**——`society-and-affiliation.md` §四之三：一百万 NPC 的关系不存，由「组织↔组织」与「个体→组织」两层派生出基线，只存偏离基线的记忆偏移（定容 LRU 记忆槽）。
3. **背景 NPC 性格**——`society-and-affiliation.md` §四之三：`traits = f(种子, 文化均值)`，只有被剧情或玩家改变过的才存偏移。
4. **衍生属性**——`attribute-system.md` §七：`derive_stats` 是纯函数，衍生属性绝不进存档，思路与前三者一致（虽然文中没有用「派生只存偏差」这个原文措辞）。
5. **背景 NPC 的行为**——`agent-goals-and-economy.md` §七之二：职业对应固定任务模板，用 progress 插值算出「现在该在哪一步」，不逐 tick 模拟。
6. **姓名**——`crates/ll-world/src/naming.rs`（已落地）：名与姓分别由 `hash(种子, 实体/家族 ID)` 现算，零存储，是这个思想的极端情形——它甚至不允许偏移，因为「同一个 NPC 永远同名」是设计要求本身，不是性能优化（[0009](../decisions/0009-derive-by-default-store-only-deviation.md) 已把姓名列为正式实例）。
7. **种族**（设计，未落地）——`race-system.md`「存储」一节：`race(entity) = weighted_pick(settlement_race_weights[birth_settlement], hash(...))`，理想情况下薄层零列现算；**但当前代码已把 `race` 做成显式存储列，与这条设计冲突，是一处待对齐的实现债务**，见该文档「存储」一节末尾的框注。
8. **平民家族与百万 NPC**——`world-history.md`「核心架构判断」：历史生成阶段只模拟「被记住」的家族与聚落，百万平民 NPC 全部是玩家落地后从聚落属性派生的，是这个思想在世界生成尺度上的应用。
9. **覆盖名**——`naming-and-localization.md`「两类名字，规则不同」：派生名零存储，只有被改名 `Effect` 动过的极少数才存一条覆盖字符串，与前三者（钱包、关系、性格）是同一模式的第四个独立实现（各自定义自己的偏移结构，互不共享代码）。
10. **增益/减益的生效状态**（设计，未落地）——`buffs-and-triggers.md` §一：只存 `expires_at`（到期的世界时钟刻度），「现在是否生效」永远是 `tick < expires_at` 的现算结果，与季节判定（[0009](../decisions/0009-derive-by-default-store-only-deviation.md) 之外、由 [0014](../decisions/0014-season-pure-function-derivation.md) 单独裁定但同一形状）是同一条思路的又一次复用，理由也相同：事件驱动到期在约束 C4 的后台跳跃推进下要补发大量历史事件，漏一次永久错位。
11. **环境动画的相位**（设计，未落地）——[动画与视觉特效的边界](animation-and-vfx-boundary.md) §三：`相位 = f(格子坐标, tick)`，零存储现算，且比前十项更彻底——不存在「偏离基线的个体」这一半，环境动画从不允许存在偏移，与姓名（本表第 6 项）是同一种「极端情形」。

### 「O(n²) 是敌人」

`agent-goals-and-economy.md` §零把这条立为全文总纲，援引矮人要塞的 FPS death 作为反面教材。具体应用：

- **行会中介贸易**替代居民互市——领地内 N 个居民两两议价是 O(N²)，改成人人对本地行会一次是 O(N)（`agent-goals-and-economy.md` §三）。
- **关系默认派生**替代两两存储——一百万 NPC 的 10¹² 条关系记录，改成零存储现算（`society-and-affiliation.md` §四之三）。
- **血缘查询限深**——「X 与 Y 是否有血缘」这类需要遍历的查询限制在 3 代以内，避免退化成图遍历（`society-and-affiliation.md` §四之二）。

### 「意图—结算—效果」架构，落到具体系统里

- 物品的拿起、丢下、交易、装备、存入箱子全部走同一个 `Effect::MoveItem`（`item-system.md` §四）。
- 装备是一个 `Effect` 序列（卸下相交槽位 → 移动被卸物品 → 装备新物品），不是一次原地修改（`equipment-slots.md`「装备流程」）。
- 属性派生 `derive_stats` 是 `resolve` 阶段的纯函数，只读世界（`attribute-system.md` §七）。

### 「本体即 Mod」

- 品质倍率表由注册表提供，可被 mod 覆盖（`item-system.md` §五）。
- 文化定义 `CultureDef` 由注册表提供，mod 加一份就能加一种文明（`society-and-affiliation.md` §二）。
- 装备槽位剩余 10 位留给 mod 扩展，位号由本体注册表分配（`equipment-slots.md`）。
- 种族定义 `RaceDef` 同理由注册表提供，混血也不是例外机制，只是新增的一条 `RaceDef` 条目（`race-system.md` §九）。
- mod 可以直接定义具体的势力实例（不只是「势力类型」），播种进世界后与生成的势力用同一个 `WorldId`、走同一套下游代码（`identity-and-ids.md` §四）。
- mod 可以给命名生成器挂条件匹配的命名钩子（如「所有海盗势力用特别的音素表」），这只是生成器钩子这个更大前瞻方向的第一个例子（`naming-and-localization.md` §六）。

### 「被记住」与「被模拟」拆开（棘轮问题的解法）

数据持久化（便宜、可以是几百万份）与逐帧 AI 决策（昂贵、必须有界）是两件不同成本的事，混成一个开关就会有棘轮效应。`agent-goals-and-economy.md` §七之二 是这条原则的主战场，落地于 `ThinPopulation` 的 `wallet_rebase`/`wallet_delta`/`rebase_at` 三列（已核实与代码一致）；`society-and-affiliation.md` 的定容记忆槽（被记住的关系数量有界，被模拟的决策数量另算）是同一思路的第二个应用。

---

## 四、阅读顺序建议

1. **[属性系统](attribute-system.md)**——六维骨架最基础，其余各份都直接或间接依赖它（负重公式、CHA 驱动招募议价、名望次级属性）。
2. **[物品系统](item-system.md)**——第二基础，定义与实例分离是整个持久化模型的原型。
3. **[装备栏位与占位掩码](equipment-slots.md)**——依赖物品系统的 `equip_mask` 字段，读物品系统之后立刻读这份最顺。
4. **[社会系统](society-and-affiliation.md)**——引入 `Agent` 结构的一半字段（`affiliations`/`wallet`/`profession`），信息量最大，建议单独留出时间读完关系系统与职业声望两节。
5. **[Agent 目标与经济](agent-goals-and-economy.md)**——收束前面四份的全部概念，也补完 `Agent` 结构的另一半字段（`goals`）。读到这里才能看全整个 `Agent` 结构，以及行会/商队/职业审计如何把社会系统的静态结构变成会动的经济。

读完前五份后再看 [conflicts.md](conflicts.md)——那里记录的疑点，大多要等你对五份都有印象之后才看得出问题所在。

后四份建立在前五份之上，建议按下面的顺序接着读：

6. **[种族系统](race-system.md)**——`Agent` 结构里最后一个还没细讲的字段（`race`），依赖属性系统（`BaseStats` 烘焙）、社会系统（`CultureDef.race_affinity`、职业声望）与经济系统（职业审计判定）。
7. **[身份与 ID 空间](identity-and-ids.md)**——篇幅最短，但概念上是七、八两份的地基：先明白 `ContentIndex` 与 `WorldId` 的分野，再读世界历史生成会顺畅得多。
8. **[世界历史生成](world-history.md)**——收束种族分布场、家族代际、身份标识三条线，讲清楚百万 NPC 的世界是怎么「生」出来的。
9. **[命名、改名与本地化](naming-and-localization.md)**——放在最后，因为它同时依赖种族文档的 `birth_settlement`、身份文档的查询式 API、世界历史文档的 `HistoricalEvent`，是四份里对其余三份依赖最重的一份。

第十份不属于这条依赖链，可以独立读，但读之前建议先看过 [世界历史生成](world-history.md)（理解「聚落在世界尺度上生成」这条前提）与 [种族系统](race-system.md)「暗视」一节（理解光照/暗视接口本身的设计，才能看懂坐标系文档「光照连锁反应」一节在组合什么）：

10. **[坐标系与空间模型](coordinate-system-and-layers.md)**——世界本身怎么组织的架构文档，冻结时间最晚，体量与前九份合起来相当，建议单独留出时间读，尤其是「三个块/两个 (i,j)」与「`Space` 统一接口」两节——这两节的名词最容易与前面九份文档里已经出现过的概念（存储块、`Layer`）混淆。

第十一份同样不属于社会/经济这条主线，管的是脚本与引擎之间怎么传数据，可以独立读，但读之前建议先看过 [ADR 0012](../decisions/0012-steel-capability-surface-verification.md)（Steel 沙箱能力边界的实测结论）与 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md)/[0017](../decisions/0017-tiered-declarations-materialize-columnar.md)（性能分档与列式物化的既有约定）：

11. **[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md)**——脚本如何不经拷贝地操作 Rust 侧数据、如何安全持有一个不可伪造的实体引用、如何一次跨界完成批量筛选/排序/聚合，是 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) 第二档「声明式受限公式」在集合操作上的推广。

第十二份直接建立在第十一份之上，紧接着读最顺——它复用第十一份定义的 `ScriptEntityHandle` 失效检测约定来回答「能不能存实体引用」这个问题：

12. **[脚本状态存储](script-state-storage.md)**——脚本如何安全保存跨帧/跨存档状态：给一条正路（`WorldState` 支持的显式读写 API），才能理直气壮地封死「VM 里悄悄攒状态」这条歪路，进而支撑「存档/读档边界强制重建 VM」这条纪律。读之前建议先看过 [ADR 0009](../decisions/0009-derive-by-default-store-only-deviation.md)（脚本状态是该原则的正当例外）与 [ADR 0010](../decisions/0010-single-source-of-truth-for-daylight.md)（`WorldState` 单一真相源在这里的应用）。

第十三、十四份回到「玩起来怎么算」这条最初的主线，脱离第十、十一、十二份的坐标/脚本支线，建议读完[属性系统](attribute-system.md)（第一份）之后就可以来读，不需要等到十二份全部读完——它们对第六至十二份没有任何依赖，只依赖第一至三份（属性、物品、装备）：

13. **[三轴战斗结算](combat-three-axis.md)**——先读「一、现状核实」看清楚 `resolve_attack` 目前的占位实现到底占位在哪三处，再读三条轴本身，最后读「四、接线点」——这一节会让你回头明白第一份属性文档 §七 `derive_stats` 与第一、二份「装备如何贡献属性」两处悬而未决的缺口具体缺在哪。

14. **[增益与通用触发器](buffs-and-triggers.md)**——建立在第十三份之上，紧接着读最顺：它的「一、失效惰性判定」直接引用 [ADR 0014](../decisions/0014-season-pure-function-derivation.md)，「三、通用触发器」直接引用 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md)，读之前建议对这两条 ADR 有印象；「四、必须结构性禁止递归」一节引用了 `knowledge/handoff/p3-to-p4.md` 记录的 `MAX_STEPS_PER_ADVANCE` 真实生效案例，不熟悉这段历史的话建议顺手翻一眼那份交接清单第一节第 2 条。

第十五份不属于前十四份的任何一条依赖链，可以独立读，只依赖第一份（属性系统）：

15. **[职业 / 技能树 / 副职 / 任务系统](class-skill-quest-system.md)**——是前十五份里唯一已经有两个内容注册表（`ClassDef`/`SkillDef`）落地代码的一份，建议对照 `crates/ll-mod/src/{class,skill}.rs` 读「与既有架构的接线点」一节，能直接看到设计与实现的对应关系；「五、技能效果的数值边界」一节点出的「P6 装备接线的硬边界」与第十三、十四份指向同一处开放项，建议三份一起对照。

第十六份同样不属于前十五份的任何一条依赖链，可以独立读，只依赖两份 ADR（[0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)/[0020](../decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)）与一份已落地的引擎代码：

16. **[动画与视觉特效的边界](animation-and-vfx-boundary.md)**——建议先读过 `crates/ll-render/src/anim.rs` 模块文档（尤其「整数帧号而非墙钟秒数」一节）再读本文档，能更清楚地看出本文档扩展的是它没覆盖的哪两类；读完第十三、十四份（三轴战斗、增益触发器）之后回头看本文档「一、`Effect` 流是表现层唯一的输入」一节会更有体感——那两份文档描述的正是驱动动画播放的 `Effect` 主要来源，但本文档不要求先读完它们才能理解自己的结论。

---

## 五、落地状态速览

详细依据（对应到 `crates/` 具体路径）写在每份文档开头的「落地状态」行，这里只给结论：

| 文档 | 状态 |
|---|---|
| 物品系统 | 纯设计，代码中无任何对应类型 |
| 装备栏位与占位掩码 | 纯设计，代码中无任何对应类型 |
| 属性系统 | 部分落地——六维字段布局（`BaseStats`）已落地，公式与次级系统未落地 |
| 社会系统 | 部分落地——`Affiliation`/`AffiliationKind` 与 `Agent` 归属字段已落地，`NamingRules` 已落地，其余（关系派生、家族、性格、文化生成器）未落地 |
| Agent 目标与经济 | 部分落地——`Goal` 与 `Agent.goals` 已落地，钱包的「偏移量+重定基准」机制已完整落地并与设计文档高度对应，任务/商队/行会定价/职业审计未落地 |
| 种族系统 | 部分落地，且有一处未对齐——`Agent.race`/`Agent.luck` 字段布局已落地且与设计一致；但 `ThinPopulation.race` 已落地为显式存储列，与本文档「薄层零列、现算取值」的设计结论冲突，是需要在 P9 落地时解决的实现债务（[2026-08-18 规格修订] 原 P8 顺移为 P9）；`RaceDef`、暗视接口、体型、混血、`birth_settlement` 均未落地 |
| 世界历史生成 | 纯设计，代码中无任何对应类型；地形生成（流程第一步）已实现，是本文档的既有基础，不是本文档新增的内容 |
| 身份与 ID 空间 | 纯设计，代码中无任何对应类型；`ContentIndex`（本文档要与之区分的另一半）已落地 |
| 命名、改名与本地化 | 部分落地——命名生成本体（`NamingRules`/`given_name`/`surname`/`full_name`）已完整落地并有测试覆盖；命名跟出生地文化走、i18n 音素表对齐、改名 `Effect`、覆盖名、mod 命名钩子均未落地 |
| 坐标系与空间模型 | 纯设计，代码中无任何对应类型（`Space`/`SpaceProfile`/区块索引均未落地）；它要替换的现有实现——`ChunkGrid`/`generate_terrain`/`compute_fov`/`Camera` 等——都是已落地且经过测试的 P2 成果，见该文档「时间窗与阶段归属」一节对影响范围的逐项评估 |
| 脚本层数据句柄与批量查询 | 纯设计，代码中无任何对应类型，明确标注不要求本次实现；它依赖的既有机制（`Arena` 世代号、`steel-core` 的 `Custom` trait、`ll-script` 的 `register_fn`/白名单）均已落地并在文档中逐项核实过 |
| 脚本状态存储 | 纯设计，代码中无任何对应类型，明确标注不要求本次实现；它依赖的既有机制（`ScriptEntityHandle` 失效检测、`NamespacedId`/`ContentIndex`、`ScriptDiagnostic`）均已落地或已有设计，但存在一条尚未满足的前提——`WorldState::actors`（`Arena<Agent>`）目前仍是 `#[serde(skip)]`，「能不能存实体引用」这条结论要到该债务还清后才能真正生效，见文档 3.3 节 |
| 三轴战斗结算 | 纯设计，代码中无任何对应类型；它要接线的对象——`resolve_attack`（`crates/ll-sim/src/resolve.rs`）、`damage_after_defense`（`crates/ll-sim/src/combat.rs`）——已落地且已核实是占位实现（攻击力恒读力量、防御恒为零、穿透恒为 `NONE`），见文档「一、现状核实」 |
| 增益与通用触发器 | 纯设计，代码中无任何对应类型；`crates/ll-sim/src/effect.rs` 目前只有六个既有变体，均已核实不含增益/触发器相关内容 |
| 职业 / 技能树 / 副职 / 任务系统 | 部分落地——`ClassDef`（`crates/ll-mod/src/class.rs`）与 `SkillDef`（`crates/ll-mod/src/skill.rs`，前置关系 DAG 校验）均已落地；`SubclassDef`、`QuestNodeDef` 仍是纯设计 |
| 动画与视觉特效的边界 | 部分落地——`crates/ll-render/src/anim.rs` 的 `Clip`/`Playback`（原地循环动画）已完整落地并有测试覆盖；一次性特效的生命周期判定、投射物、`EffectDef` 注册表均为纯设计，代码中无对应类型；结算/表现单向依赖这条边界规则本身不依赖任何新代码，已随本文档冻结立即生效 |

三份最早冻结、「已部分落地」的文档中，真正验证过的只是 P3 阶段要求的字段布局与钱包机制；描述战斗结算、经济博弈、社会涌现的大部分内容仍是纸上设计，随时可能在 P5/P8/P9 实现时被推翻或调整（[2026-08-18 规格修订] 原 P7/P8 顺移为 P8/P9）。中间四份里，种族系统与命名系统同样只有字段/函数布局落地，核心机制（现算公式、i18n 对齐、改名事件）尚未验证；世界历史生成与身份空间目前完全是纸面设计。第十份（坐标系与空间模型）虽是纯设计，但它要替换的对象是 P2 阶段验证过的真实代码，不是在空白处新增——这与前九份「在已有底座上补新系统」的性质不同，实现时的返工面积也更大，见该文档「对既有 P2 成果的影响范围」一节的诚实评估。
