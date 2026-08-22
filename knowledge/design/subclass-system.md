# 副职系统设计

**落地状态**：部分落地——`SubclassDef`/`SubclassTable`（`crates/ll-mod/src/subclass.rs`）与
`register-subclass`（`crates/ll-mod/src/script_subclass_api.rs`）已经落地，`Agent.subclasses:
Vec<ContentIndex>` 也已经落地（`crates/ll-world/src/entity/agent.rs`）——**但目前是一个空壳**：
`SubclassDef` 只有 `id`/`display_name_key` 两个字段，没有任何数值/效果载荷，也没有任何代码路径会
往 `Agent.subclasses` 里写入东西。本文档设计的获得机制、上限与放弃机制、`traits` 字段、
`skill-requires!` 副职闸门的真实接线，**均为纯设计，无实现代码**，见各节「现在能做的 vs 等什么」。

**冻结于** 2026-08-22，基线提交 `f46c363`（`main` 分支）。

**并发声明**：本次任务与另一路并行工作共享同一工作树——卫兵盘查批次，改动
`crates/ll-mod/src/class.rs`、`crates/ll-sim/**`、`crates/ll-script/**`，与本文档主题（副职）无关。
本文档只新增这一个文件 + 更新 `README.md` 索引，不触碰 `crates/**`/`mods/**`/`assets/**`。

---

## 零、一处必须先纠正的现状：`SubclassDef`/`QuestNodeDef` 其实已经落地

`class-skill-quest-system.md`（冻结于 2026-08-19）与 `README.md`「落地状态速览」都写着「`SubclassDef`、
`QuestNodeDef` 仍是纯设计」——**这条记录已经过期**。`git log` 核实：

```
a4f180f feat: SubclassDef 副职注册表——共享技能命名空间（裁定 P5-4）
4a728ba feat: 技能与任务接进真实结算——resolve_with_skills_and_quests 接线
bf01c3e feat: 补齐职业/技能/副职/任务的脚本注册绑定（register-class/skill/subclass/quest）
```

三个提交都晚于两份文档冻结的时间点。`crates/ll-mod/src/subclass.rs`（`SubclassDef`/`SubclassTable`/
`register-subclass`）与 `crates/ll-mod/src/quest.rs`（`QuestNodeDef`/`QuestTable`，含 `KillCount`
一档条件，`ll_sim::quest::kill_progress_effects` 已经真实接线进 `resolve_with_skills_and_quests`）
**均已落地并有测试覆盖**。本文档写权限只到本文件 + `README.md`，不越权改写那两份文档，只在此如实
记录这处漂移，供 README 索引更新时一并提醒读者。

**这对本文档的意义**：本文档要设计的不是「从零建一个副职系统」，而是「给一个已经能注册、已经能被
`Agent.subclasses` 持有、但除了展示名字什么都不做的空壳，补上获得机制与最小闸门接线」。

---

## 一、现状核实：候选表逐项核对

先核实框架里给出的候选表。**结论：整体成立，补充几处关键细节，一处判断需要修正。**

| 方向 | 成长挂钩现状核实 | 与原描述的出入 |
|---|---|---|
| 工匠/锻造 | 耐久 `ItemStack.durability: Option<i32>` 确实已落地；修理确实不存在（`crates/ll-sim` 检索无任何修理相关 `Intent`/`Effect`） | **补充一处更深的问题**：这条方向的成长挂钩「用装备」本身站不住——耐久是消耗性的（用装备→耐久下降），不是产出性的，「越用装备耐久越低」推不出「锻造技艺越高」的因果。真正该挂的动作是「修理装备」，而修理不存在。这不是「缺一个依赖模块」这么简单，是这个方向的成长挂钩动作本身还没被创造出来，比原表格暗示的更靠后 |
| 鉴定/秘术 | 未鉴定状态确实不存在（`item-system.md`/`crates/ll-world/src/item.rs` 检索「未鉴定/unidentified/identify」零匹配） | 原描述准确。补充：`ItemStack.durability: Option<i32>` 是「逐实例可选状态」的现成先例，加一个 `identified: bool` 字段本身成本不高；真正贵的是「生成/掉落时标记为未鉴定」要动世界生成路径，以及需要一个新 `Intent::Identify`——比复用现成 `Effect` 的方向贵一档 |
| 炼金/草药 | `RecipeDef` 已在 `food-and-cooking-system.md` 定形，`resolve_craft`（设计）复用 `Effect::ConsumeInventoryItem`/`Effect::MergeIntoInventory` 两个既有 `Effect`，零新增 `Effect` | 原描述准确。**补充一个关键点**：`RecipeDef.product` 是任意 `ItemDef`，配方不限于食物——药水配方可以是同一套 `RecipeDef` 的另一批内容，不需要为「炼金」新开一个类型 |
| 厨艺 | `RecipeDef` + `Satiety`（设计） | 原描述准确。**补充一处会削弱这条方向价值的细节**：`food-and-cooking-system.md` 已经裁定「菜谱全部已知不设解锁门槛」——副职在这条方向上不能靠「解锁菜谱」当奖励，只能靠「解锁副职专属技能」（如野外烹饪），价值比炼金弱一档 |
| 测绘/制图 | 探索记忆、世界地图确实已落地 | 原描述准确。**补充：这是全表成长挂钩成本最低的一项**——探索记忆本身就是一个已经随移动实时更新的位图，「探明了多少格」不需要新增任何计数字段，直接读现有位图即可，零新增存储 |
| 求生/野营 | 休息、季节确实已落地 | 原描述准确。补充：「休息次数」这个计数目前不存在（全项目检索 `Agent`/脚本状态均无通用计数器先例，唯一先例是任务系统的击杀计数，是特化实现不是通用机制），需要新增，成本中等 |
| 殡葬/掘尸 | 尸体系统确实已落地 | 原描述准确。**补充：`Effect::Kill` 已存在，且 `level-and-experience-system.md` 已经示范「击杀直接挂经验，不新开事件」这条先例**——杀敌计数走同一挂载点（`kill_progress_effects` 的既有存储键）成本很低，与测绘并列最低成本梯队 |
| 骑术 | 载具设计尚未落地（`crates/` 检索无 `MountDef`/`register-vehicle` 实现） | 原描述准确 |
| 斥候/侦知 | 陷阱/暗门/足迹确实全不存在 | 原描述准确。补充：三者都缺，不是缺一个模块，没有「先做一半」的余地 |
| 驯兽/训导 | 同伴系统、NPC 态度确实全不存在（`crates/` 检索 `companion`/`Attitude` 零匹配） | 原描述准确 |
| 驮运 | 负重确实不存在（`ll-sim/item.rs` 模块文档原文核实：`base_weight`/`base_price`/`max_durability` 均未接线） | 原描述准确。补充：这是最能低成本复活 `ItemDef.base_weight` 死字段的方向，但按 YAGNI 不在本次最小可用集范围内 |
| 商贾 | 交易系统确实不存在（`crates/` 检索 `resolve_trade`/`Intent::Trade`/`register-trade` 零匹配） | 原描述准确 |

**一处需要修正的判断**：原框架把「工匠/锻造」列在候选表首位，暗示它是较优先的方向。核实后应当垫底——不是因为耐久/修理这两个依赖模块本身多难补，而是因为它的成长挂钩动作在因果上是反的（见上表）。详见六节排序。

---

## 二、副职是资格，天赋是授予——两道闸的现状

框架给出的核心判据（副职不给数值，只当"能不能学"的资格闸门；给东西的是天赋）在代码里对应两个独立机制，**都已经设计、都还没有真实的运行期消费者**：

1. **`skill-requires!` / `SkillRequirement`**（`skill-learn-requirements.md`，纯设计）——三道闸之一读
   `agent.subclasses`：`req.subclasses.is_empty() || req.subclasses.iter().any(|s|
   subclasses.contains(s))`。该文档已经核实：`SkillRequirement` 类型本身、`skill-requires!`
   注册函数、消费它的 `resolve_learn_skill` 全部不存在于 `crates/**`；更关键的是，**`Intent::LearnSkill`
   本身也不存在**——「学习技能」这个动作在真实玩法路径里还没有落点，`unlocked_skills` 目前只被测试/
   验收代码直接 `push`。
2. **`TraitGrant` / `TraitDef.traits`**（`trait-system.md`，纯设计）——该文档「十、现在能做的 vs 等什么」
   第 4 项已经提出 `ClassDef`/`SubclassDef` 新增 `traits: Vec<TraitGrant>` 字段，判定为「无阻塞，纯
   结构体扩展」。`TraitGrant { trait_id, unlock_level }` 里种族/副职/装备/buff 恒填 `unlock_level = 1`
   （拥有即生效，不随等级变化），这正是「副职给天赋」这条通道的现成形状——**本文档不重新设计
   `TraitGrant`/`TraitDef` 本身，只确认这是副职系统唯一应该拥有的「给东西」字段**，且它给的是通过
   `TraitTable` 间接兑现的东西（技能/属性修正/规则修正/资源池），不是副职自己直接持有数值。

**两道闸都不依赖对方**：`traits` 字段生效不需要 `skill-requires!` 落地（拥有副职即刻生效，见六节
`effective_traits` 的并集聚合），`skill-requires!` 生效也不需要 `traits` 字段（它只读
`agent.subclasses` 成员关系）。这意味着副职系统即使在 `skill-requires!`/`resolve_learn_skill`
仍未落地的情况下，只要 `traits` 字段+其消费路径落地，就已经能产出可感知的游戏效果——不必等待
更大的技能学习管线整体完工才有价值，见五节「最小可用集」的依赖排序。

---

## 三、`SubclassDef` 的最小形状

```rust
pub struct SubclassDef {
    /// 已落地，不变。
    pub id: NamespacedId,
    /// 已落地，不变。
    pub display_name_key: NamespacedId,
    /// 新增：这个副职自己授予的天赋——见二节，trait-system.md 六节
    /// TraitGrant 的直接消费方。空列表表示这个副职本身不直接授予任何
    /// 天赋（纯粹作为技能学习的资格闸门存在，例如"学徒"这类通用副职）。
    pub traits: Vec<TraitGrant>,
}
```

**只加这一个字段**。理由回应框架第二节的红线：

- **不加任何数值字段**（不加属性修正、不加资源池容量、不加护甲加成……）——那些是 `TraitDef` 的职责，
  `SubclassDef.traits` 只是一份引用列表，真正的效果载荷全部在 `TraitTable` 里，副职本身零存储数值。
- **不把获得条件放在 `SubclassDef` 上**——四节会说明，获得条件是一套独立的声明（照抄
  `skill-requires!` 不与 `register-skill` 合并的先例），不塞进 `register-subclass` 的参数列表，
  理由同该文档六节：分类展示与强制闸门是两件独立的事，混在一起会在将来某天有人想"只展示不强制"或
  "只强制不展示"时变成一处隐藏耦合。
- **不需要独立的命名空间字段**——`subclass.rs` 模块文档「裁定 P5-4」一节已经论证清楚：技能命名空间
  与主职共享，副职不需要自己管一段号区。

---

## 四、怎么获得副职

### 结论：使用计数，理由是它是唯一不违反红线的选项；成本评估——中等，且已有可直接复制的落地先例

**候选方案与否决理由**：

1. **训练（打开界面手动升级）**——否决。这就是框架红线明确禁止的「另开一个采集/训练小游戏」的变体：
   打开一个界面、点一个按钮"训练"，与做菜/挖矿一类专门小游戏在结构上没有区别——玩家要为了这个副职
   专门腾出时间做一件游戏其余部分不需要的事。
2. **任务（完成特定 `QuestNodeDef` 后授予）**——部分否决。任务系统（`QuestNodeDef`/`KillCount`）
   已经落地，用一条任务节点触发"获得副职"在**技术上**完全可行（`Effect::SetScriptState` 之外
   再产出一个"写入 `Agent.subclasses`"的效果即可），**但它不是本文档要选的默认路径**：任务本质是
   "被安排好的一次性关卡"，与"用什么练什么"的连续成长感不是同一种体验——任务更适合当**触发某个
   特定副职的入口叙事包装**（比如"帮草药商跑一趟腿"解锁炼金学徒身份的第一步），而不是承担整条
   成长曲线。**结论：任务不是主机制，但不排斥某个具体副职的内容设计里用一条任务节点作为"报名"
   门槛，叠在使用计数之上**——两者不互斥，本文档的最小形状默认不需要任务参与。
3. **使用计数（做某件事满 N 次自动获得）**——**采纳**。这是唯一直接满足框架红线"成长挂钩必须是
   玩家已经在做的动作"的方案：玩家不需要知道"我在攒炼金副职的进度"，捡材料、杀敌、探路这些动作
   本身就是游戏其余部分要求玩家做的事，副职只是在这些既有动作上"顺手"累加。

**成本评估：中等，不是从零发明，是复制一个已经跑通的模式。**

关键证据——`crates/ll-sim/src/quest.rs` 的 `kill_progress_effects` 已经是一个**完整落地**的
"使用计数 + 达标授予"实现：

```rust
pub fn kill_progress_effects(actor, killed_kind, agent, quests) -> Vec<Effect> {
    // 1. 读 agent.script_state 里已有的击杀计数（未写入过时为 0）
    // 2. +1，产出一条 Effect::SetScriptState 写回新计数
    // 3. 遍历该 killed_kind 关联的 KillCount 任务，达标且前置任务已完成
    //    且尚未完成时，追加一条 mark_quest_completed 写入
}
```

这不是"设计文档里画的形状"，是接进 `resolve_with_skills_and_quests` 的真实代码。它证明：

- **计数存储通道已经落地**：`Agent.script_state`（`crates/ll-world/src/script_state.rs`）+
  `ScriptValue::Int(i64)`，按 `(mod_namespace, key)` 命名空间隔离，本体可以用 `"lostland"`
  命名空间存引擎级统计（击杀计数正是这么做的），不需要给 `Agent` 加新的直接字段。
- **"累加 + 达标检查 + 产出效果"这套结构已经跑通一次**，副职获得只是把"达标时产出的效果"从
  "标记任务完成"换成"标记副职获得"，把"触发点"从 `Effect::Kill` 扩展到另外两个既有的结算点。

**本文档提出的最小形状**（复用同一套存储通道，不新建 `Agent` 字段）：

```rust
/// 一条"做过什么、多少次，就能获得这个副职"的声明。与 SkillRequirement
/// 是两个独立的闸——SkillRequirement 回答"已经拥有副职后能不能学某个
/// 技能"，本类型回答"副职本身怎么拿到"。
pub struct SubclassUnlock {
    pub subclass: ContentIndex,        // 指向 SubclassTable，必须已注册
    pub trigger: SubclassUnlockTrigger,
    pub threshold: u32,
}

/// 只列最小可用集需要的三种——见五节，不为已推迟的方向（厨艺/求生/
/// 鉴定/工匠……）预先造变体，YAGNI。
pub enum SubclassUnlockTrigger {
    /// 累计击杀某类目标达到 threshold 次——挂载点 kill_progress_effects
    /// （已落地），复用同一个击杀计数存储键，不新起第二套计数。
    Kills(ContentIndex),
    /// 累计拾取某个物品类别达到 threshold 次——挂载点
    /// resolve_pick_up/merge_into_inventory_effect（已落地，需要新增
    /// 一个 item_pickup_progress_effects，结构完全照抄
    /// kill_progress_effects，只是触发点从 Effect::Kill 换成拾取）。
    ItemsPickedUp(ContentIndex),
    /// 探明的世界地图格数达到 threshold 格——挂载点 ExplorationMemory
    /// 现有位图（已落地）。**这一档不需要新增任何计数写入**——阈值
    /// 判定直接读位图当前的已探明格数（一次 popcount），零新存储。
    TilesExplored,
}
```

`(subclass-unlocks-via! subclass-id trigger-kind trigger-target threshold)`——一档声明式
（ADR 0016/0017），独立函数，不与 `register-subclass` 合并，理由同 `skill-requires!` 六节
「分类展示与强制闸门是两件独立的事」。

**代价诚实标注**：`Kills`/`TilesExplored` 两档挂载点已经落地或接近零成本；`ItemsPickedUp`
需要新增一个与 `kill_progress_effects` 同构的函数，接进 `resolve_pick_up` 的效果产出——这不是
新范式，是把已经验证过的模式再抄一份，成本可控但不是零。

---

## 五、有没有上限——结论：有，一个小整数，且必须配放弃机制

**结论：需要上限。** 理由不是「无限累积会让存档变大」这类工程借口，是框架自己在六节给出的判据本身
要求稀缺性：**「build 多样性来自直觉搭配 vs 错位搭配的化学反应」这条价值主张，前提是副职是稀缺资源**。
若使用计数机制跑得足够久，任何一个角色理论上能集齐全部副职——那时候「法师配驯兽」不再是一个需要
取舍的选择，只是「反正迟早都会有」的时间问题，错位搭配产生的化学反应就被拖成了必然结果，框架想要
的「取舍」张力消失。

**上限的具体数值不是本文档要拍板的内容设计参数**——结构上"是一个小整数"就够，建议区间 2~3
（1 太严格，等于没有搭配空间；4 及以上稀缺感明显减弱），具体数值留给内容设计阶段按实际游玩节奏微调。

**放弃机制**（上限存在就必须有对应的放弃路径，否则玩家攒到上限后系统直接锁死，无法再体验任何新
副职带来的搭配变化）：

```rust
// 新增一个与既有移除类操作同构的效果——Agent.subclasses 是 Vec，
// 移除是一次 retain，不需要复杂的清理逻辑。
Effect::RemoveSubclass { actor: EntityId, subclass: ContentIndex }
```

**放弃不追溯**：与「可学」和「可用」是两件事（`skill-learn-requirements.md` 五节）同一条纪律——
副职闸门只在"写入 `unlocked_skills` 之前"判定一次，一旦某个技能已经学会，它此后是否仍然满足
`SkillRequirement` 与"能不能用"无关。放弃一个副职，**不会**让已经通过它学会的技能变得不可用，
只是（a）关闭了继续用这个副职学新技能的入口，（b）`effective_traits` 聚合里不再包含它授予的天赋
（天赋本身零存储，纯派生，见 `trait-system.md` 八节，放弃后下一次聚合直接不再读到），
（c）腾出一个上限槽位供下一次使用计数达标时授予新副职。

**放弃本身不设代价**（不扣经验、不设冷却）——保持与"副职不给数值"同一条极简纪律：加代价意味着
要为这个代价设计数值，那是本文档明确要避免的范围扩张。

---

## 六、最小可用集

### 结论：测绘/制图 + 殡葬/掘尸 + 炼金/草药，三个

**排序**（按"成长挂钩成本 + 依赖是否已落地"从低到高，一节候选表核实的直接结论）：

1. **测绘/制图**——零新增存储，`TilesExplored` 直接读现有位图。
2. **殡葬/掘尸**——复用 `kill_progress_effects` 现成挂载点，与经验系统同一先例。
3. **炼金/草药**——`RecipeDef` 已定形，只需新增一个与击杀计数同构的拾取计数函数。
4. 求生/野营——休息已落地但计数需新增，成本中等。
5. 厨艺——同炼金但价值打折（食物系统已裁定配方不设门槛，副职收益只剩技能闸）。
6. 驮运——YAGNI 暂不需要，但标注为"最省事复活 `base_weight` 死字段"的候选，供未来参考。
7. 鉴定/秘术——需要新 `Intent` + 触及世界生成路径，成本高于 1~5。
8. 工匠/锻造——成长挂钩动作本身因果倒置（一节已论证），需要先设计"修理"这个新机制才谈得上。
9~12. 骑术/斥候/驯兽/商贾——阻塞于载具/陷阱/同伴/交易四个尚未落地的系统，标为扩展，等对应系统
落地后再评估。

**为什么前三个算"一个完整的补给循环"，不是三个互不相干的功能点**：

框架第一节定的正交轴是"你怎么在战斗之外补给自己"。前三个方向对应补给循环的三个环节，且全部
挂在玩家已经在做的动作上：

- **殡葬/掘尸**（杀敌）——战斗本身产出的原始材料（尸体已经落地，掉落已经能捡）。
- **炼金/草药**（捡材料）——把探索/战斗顺手捡到的材料，通过 `RecipeDef` 转化为可消耗的补给品
  （药水），这一步是循环里"加工"的环节。
- **测绘/制图**（走路）——探索本身产出"去哪能找到更多材料/去哪能避开危险"的信息优势，是循环里
  "决策"的环节，让补给行为更有效率而不是盲目瞎走。

三者共用同一批底层机制（`Effect::Kill`/拾取/`ExplorationMemory`），互相独立、不共享代码路径
（符合 ADR 0021——不是因为它们"看起来都是副职"就该抽出共同基类，是因为它们各自的挂载点确实不同），
但拼起来覆盖了「杀敌产出材料→加工材料成补给品→更有效地找到更多材料」这一条从战斗到再补给的完整
链路，不需要发明任何一个新的游戏动作。

**求生/野营与厨艺标为紧邻的下一批扩展**——它们同样契合补给循环的叙事（休息回资源、吃饭回饱食度），
只是各自欠一个尚未落地的计数或被食物系统自身的设计决定削弱了价值，不进最小可用集，但不代表它们
方向错误，等对应基础设施补齐后应当优先于表格靠后的方向。

---

## 七、`register-subclass` 的签名与档位

**现有签名不变**：`(register-subclass id display-name-key)`——`SubclassDef` 加 `traits` 字段
不改这个签名的参数个数,理由同 `class-skill-quest-system.md` 对 `register-skill` 的处理：
`traits` 走独立函数，不塞进 `register-subclass` 的位置参数列表。

**新增两个独立函数,均为一档（ADR 0016/0017：声明式,注册期物化,运行期查表,零脚本回调）：**

```scheme
(subclass-grants-trait! subclass-id trait-id)
(subclass-unlocks-via! subclass-id trigger-kind trigger-target threshold)
```

- `subclass-grants-trait!`：`subclass-id`/`trait-id` 均须已经 `register-subclass`/`register-trait`
  注册过（跨表存在性校验,与 `skill-requires!` 六节"必须是已注册技能"同一条纪律）。可对同一个
  `subclass-id` 多次调用,每次追加一条 `TraitGrant { unlock_level: 1 }` 到 `traits` 列表——不做
  去重校验（与 `RaceDef.traits`/`ClassDef.traits` 未来若追加同类函数时应保持的行为一致,重复授予
  同一个天赋两次在效果聚合层是幂等的并集操作,不需要在注册期拒绝）。
- `subclass-unlocks-via!`：`trigger-kind` 是 `"kills"`/`"items-picked-up"`/`"tiles-explored"`
  三选一字符串（对应四节 `SubclassUnlockTrigger` 三个变体，`"tiles-explored"` 时 `trigger-target`
  参数被忽略——不强制传空字符串还是直接省略参数,留给实现阶段按 Steel 现有变参惯例决定）；
  `threshold` 是达标次数/格数。同一个 `subclass-id` 只允许注册一条 `SubclassUnlock`——与
  `register-class-xp-curve`"一个职业只能绑一条曲线"同一条纪律,一个副职有多条互相竞争的解锁路径
  会让"我还差多少"这句 UI 文案没法唯一地展示。

档位判据（三步）：有自由度（mod 能声明任意天赋授予/解锁条件）；自由度落在纯数据上（阈值/触发类型
都是注册期定死的值,运行期只做整数比较,不消费脚本回调）；调用频率——达标判定发生在每次相关动作
结算时（杀敌/拾取/移动),属于战斗/探索结算的常规路径,量级与既有击杀计数同一档,必须一档。

---

## 相关文档

- `knowledge/design/class-skill-quest-system.md`——`ClassDef`/`SkillDef`/`SubclassDef`/`QuestNodeDef`
  的原始基线设计，零节已指出其「落地状态」记录过期
- `knowledge/design/skill-learn-requirements.md`——`SkillRequirement`/`skill-requires!` 的完整设计，
  二节直接引用其「可学」闸门现状与「可学/可用是两件事」的纪律
- `knowledge/design/trait-system.md`——`TraitDef`/`TraitGrant`/`effective_traits` 聚合，二节/三节
  直接复用其「有效技能=并集」「拥有即生效」两条既有结论，不重新设计
- `knowledge/design/food-and-cooking-system.md`——`RecipeDef`/`Satiety`，六节最小可用集「炼金/草药」
  「厨艺」两项的地基
- `knowledge/design/level-and-experience-system.md`——「经验挂 `Effect::Kill`，不新开事件」的先例，
  四节使用计数机制直接类比
- `crates/ll-mod/src/subclass.rs`/`script_subclass_api.rs`——`SubclassDef` 现状代码
- `crates/ll-mod/src/quest.rs`/`crates/ll-sim/src/quest.rs`——`kill_progress_effects` 现状代码，
  四节使用计数机制的直接复制来源
- `crates/ll-world/src/script_state.rs`——`Agent.script_state`/`ScriptValue`，四节计数存储通道
- `crates/ll-mod/src/class.rs`——`ClassDef.primary_attribute` 死字段现状（一节候选表核实间接依据）
- `scripts/ci/check_field_consumers.py`——`ClassDef.primary_attribute` 豁免条目原文
- `knowledge/decisions/0016-mod-performance-tiers-by-declaration.md`/
  `0017-tiered-declarations-materialize-columnar.md`——七节档位判据
- `knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md`——六节「三个方向
  不共享代码路径」的判据来源
