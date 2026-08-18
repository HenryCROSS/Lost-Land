# 设计文档总索引

本目录下五份文档共同描述「迷途大陆」的物品、装备、属性、社会、经济五个子系统。它们分开冻结、分次写成，彼此高度依赖但没有统一校对过——这份索引是校对结果：谁管什么、谁引用了谁、贯穿全局的原则用在哪几处、该按什么顺序读。

**交叉核对中发现的矛盾与缺口**，见 [conflicts.md](conflicts.md)（4 条待裁定、3 条设计缺口、2 条记录备查）。这些是文档之间还没对齐的地方，不代表某一方错了，读到相关章节时请留意。

---

## 一、五份文档各管什么、不管什么

| 文档 | 管什么 | 不管什么 |
|---|---|---|
| [物品系统](item-system.md) | `ItemDef`/`ItemStack` 定义与实例分离、堆叠合并规则、`Owner` 归属枚举、`ItemLocation` 位置模型、地面物品老化清理、六档品质与倍率表、耐久、重量与负重分档、`use_effect` 脚本接口 | 装备如何占用槽位（见装备文档）、物品如何提供属性加成的具体消费逻辑（`StatBonus` 未在任何文档定义，见缺口 5）、物品定价如何变成行会售价（见经济文档，换算关系未写明，见缺口 7）、`Owner::Faction` 背后的组织语义（见社会文档） |
| [装备栏位与占位掩码](equipment-slots.md) | 22 槽位定义、`SlotMask` 位运算、装备互斥/自动卸下规则、渲染层排序、装备流程的 Effect 序列 | 装备如何转化为攻防数值（见属性文档）、`ItemDef`/`ItemStack` 本身的字段（见物品文档）、单槽位 `EquipSlot` 类型的正式定义（未在任何文档给出，见缺口 6） |
| [属性系统](attribute-system.md) | 六维主属性（STR/DEX/CON/INT/WIS/CHA）、调整值公式、三系攻防、护盾、四种穿透、伤害公式、幸运、次级属性列表、d20 判定、与时间轴调度的接口 | 属性加成从哪来（`ItemDef.stat_bonuses` 的消费逻辑未定义）、装备如何贡献属性的具体聚合算法（未在装备文档给出）、议价/威望等次级属性如何真正接入经济结算（见经济、社会文档） |
| [社会系统](society-and-affiliation.md) | `Affiliation` 统一结构（势力/宗教/行会/文化/家族/职业六类共用一个数据结构）、`CultureDef` 生成权重、地图结构层（聚落/道路/遗迹/地标/资源点）、宗教如何运转、家族与代际、关系系统的默认派生与记忆偏移、性格 `Traits`、职业声望的局部化、LOD 兼容性 | 具体的悬赏任务结构、行会定价公式、钱包与货币守恒（见经济文档）、物品本身的字段（见物品文档） |
| [Agent 目标与经济](agent-goals-and-economy.md) | 目标—需求—任务—悬赏循环、行会中介贸易与定价、钱包与货币守恒、破产/致富/土匪负反馈、职业审计算法、商队与途中报价、背景/前景/具名三档精度与棘轮问题、惰性追赶的边界、`ll-econsim` 验收指标 | `Affiliation`/`CultureDef` 等归属结构本身（见社会文档）、物品与装备的字段定义（见物品、装备文档）、属性如何影响战斗结算（见属性文档） |

一句话版边界：**物品定义「是什么」，装备定义「戴在哪」，属性定义「打起来怎么算」，社会定义「谁跟谁什么关系」，经济定义「钱和活儿怎么流动」。** 五者共用同一个 `Agent`/`ItemStack` 底座，但没有一份文档试图覆盖别人的地盘——边界比内容更容易搞混，出现「这个概念该去哪份文档找」的疑惑时，先查下面的对照表。

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
| `Traits`（性格：勇敢/贪婪/忠诚/合群/记仇/虔诚） | [社会系统](society-and-affiliation.md) | [Agent 目标与经济](agent-goals-and-economy.md)（职业审计「性格允许」一档，但未显式列入判定公式，见冲突 3） |
| `Kinship`（血缘：父母/配偶/子女） | [社会系统](society-and-affiliation.md) | — |
| 关系默认派生基线 + 记忆偏移 | [社会系统](society-and-affiliation.md) | — |
| `Agent`（厚层实体） | [社会系统](society-and-affiliation.md) §五 与 [Agent 目标与经济](agent-goals-and-economy.md) §九 **共同**约束（两份文档各列一半字段，代码里合并成一个结构） | 双方互相依赖；已落地为 `crates/ll-world/src/entity/agent.rs` |
| `Goal`（目标链：类型/参数/进度/优先级） | [Agent 目标与经济](agent-goals-and-economy.md) | — |
| `Task`（任务/悬赏，`assignee: Option` 区分私人队列与公开池） | [Agent 目标与经济](agent-goals-and-economy.md) | [社会系统](society-and-affiliation.md)（行会/神殿的任务接取权由 `Affiliation` 判定） |
| `Caravan`（商队） | [Agent 目标与经济](agent-goals-and-economy.md) | [社会系统](society-and-affiliation.md)（沿道路走，与聚落道路正反馈是同一个循环） |
| `WealthTier`（破产/温饱/小康/富裕） | [Agent 目标与经济](agent-goals-and-economy.md) | — |
| 行会定价公式 | [Agent 目标与经济](agent-goals-and-economy.md) | [社会系统](society-and-affiliation.md)（「同势力/同行会打折」的具体落点，是否已含在公式内未写明，见冲突 2） |
| 近景/中景/远景 LOD（决策 21） vs 背景/前景/具名三档精度 | [社会系统](society-and-affiliation.md) §六 与 [Agent 目标与经济](agent-goals-and-economy.md) §七之二 分别给出 | 两套三档划分是否为同一件事的两种叫法未写明，见冲突 1（本索引里最要紧的一条） |

---

## 三、贯穿全局的设计原则

这几条原则不属于任何一份文档，是五份文档共用的思维方式。看到某处设计「为什么要这么绕」，多半能在这里找到答案。

### 「默认派生，只存偏差」

能用公式当场算出来的值，不存进世界状态；只存「偏离公式的那一点」。已识别的实例（原文档用「第三次」「第五次」这类编号互相指涉，但编号并未覆盖全部实例，见 [conflicts.md](conflicts.md) 条目 8，这里按文档出现顺序直接列全）：

1. **NPC 钱包**——`agent-goals-and-economy.md` §七之二：`钱包 = 批量公式(种子, ID, 时长) + 偏移量`，长期不交互就重定基准。已落地为 `ThinPopulation`（`crates/ll-world/src/entity/thin.rs`）。
2. **个体↔个体关系**——`society-and-affiliation.md` §四之三：一百万 NPC 的关系不存，由「组织↔组织」与「个体→组织」两层派生出基线，只存偏离基线的记忆偏移（定容 LRU 记忆槽）。
3. **背景 NPC 性格**——`society-and-affiliation.md` §四之三：`traits = f(种子, 文化均值)`，只有被剧情或玩家改变过的才存偏移。
4. **衍生属性**——`attribute-system.md` §七：`derive_stats` 是纯函数，衍生属性绝不进存档，思路与前三者一致（虽然文中没有用「派生只存偏差」这个原文措辞）。
5. **背景 NPC 的行为**——`agent-goals-and-economy.md` §七之二：职业对应固定任务模板，用 progress 插值算出「现在该在哪一步」，不逐 tick 模拟。

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

### 「被记住」与「被模拟」拆开（棘轮问题的解法）

数据持久化（便宜、可以是几百万份）与逐帧 AI 决策（昂贵、必须有界）是两件不同成本的事，混成一个开关就会有棘轮效应。`agent-goals-and-economy.md` §七之二 是这条原则的主战场，落地于 `ThinPopulation` 的 `wallet_rebase`/`wallet_delta`/`rebase_at` 三列（已核实与代码一致）；`society-and-affiliation.md` 的定容记忆槽（被记住的关系数量有界，被模拟的决策数量另算）是同一思路的第二个应用。

---

## 四、阅读顺序建议

1. **[属性系统](attribute-system.md)**——六维骨架最基础，其余四份都直接或间接依赖它（负重公式、CHA 驱动招募议价、威望次级属性）。
2. **[物品系统](item-system.md)**——第二基础，定义与实例分离是整个持久化模型的原型。
3. **[装备栏位与占位掩码](equipment-slots.md)**——依赖物品系统的 `equip_mask` 字段，读物品系统之后立刻读这份最顺。
4. **[社会系统](society-and-affiliation.md)**——引入 `Agent` 结构的一半字段（`affiliations`/`wallet`/`profession`），信息量最大，建议单独留出时间读完关系系统与职业声望两节。
5. **[Agent 目标与经济](agent-goals-and-economy.md)**——收束前面四份的全部概念，也补完 `Agent` 结构的另一半字段（`goals`）。读到这里才能看全整个 `Agent` 结构，以及行会/商队/职业审计如何把社会系统的静态结构变成会动的经济。

读完五份后再看 [conflicts.md](conflicts.md)——那里记录的疑点，大多要等你对五份都有印象之后才看得出问题所在。

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

三份「已部分落地」的文档中，真正验证过的只是 P3 阶段要求的字段布局与钱包机制；描述战斗结算、经济博弈、社会涌现的大部分内容仍是纸上设计，随时可能在 P5/P7/P8 实现时被推翻或调整。
