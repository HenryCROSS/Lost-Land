# 副职系统设计

**落地状态**：部分落地，且**本批次核实后比冻结时前进了一大截**——`SubclassDef`/`SubclassTable`
（`crates/ll-mod/src/subclass.rs`）、`register-subclass`（`crates/ll-mod/src/script_subclass_api.rs`）、
`Agent.subclasses: Vec<ContentIndex>`（`crates/ll-world/src/entity/agent.rs`）均已落地，且
**`Agent.subclasses` 已经有了第一个真实的运行期消费者**：`resolve_craft`
（`crates/ll-sim/src/resolve.rs:2340`）读 `RecipeCategoryDef.required_subclasses` 做 any-of 闸门
判定（提交 `08cdeb0`）。仍未落地的是：`SubclassDef` 的 `traits` 字段、`SubclassUnlock`/
`SubclassUnlockTrigger` 整套获得机制、上限与放弃机制、`skill-requires!` 那一路闸门。
**没有任何代码路径会往 `Agent.subclasses` 里写入东西**——副职现在能被读，不能被拿到。

**冻结于** 2026-08-22，基线提交 `08cdeb0`（`main` 分支）。

**2026-08-22 第一次订正**（由 `crafting-system.md` 触发，基线提交 `e81e03c`）：一节候选表
「工匠/锻造」与「厨艺」两行的判断、四节 `SubclassUnlockTrigger` 的变体集合、六节排序的第 3/5/8
三项，均已按制作系统的核实结论修订——修订处就地标注 **【2026-08-22 订正】**，原文保留以便对照。
核心变化：**「制作」这个动作本身是工匠的成长挂钩，本文档当初漏检了它**；四个制作副职
（工匠/裁缝/炼金/厨艺）共用同一个新增触发器变体 `ItemsCrafted(配方类别)`。

**2026-08-22 第二次订正（本批次，基线 `08cdeb0`）**：项目所有者要求把陆续提过的十七个方向
「有重叠的放一个合适的副职里，或者是缺少了什么」。本次新增 **一之二节（名册合并的独立复核）**
与 **一之三节（缺口复核）**，并按 `61d2e73` 冻结之后落地的十个批次逐条核实了全文的「落地状态」
描述，过时处就地更正、标注 **【第二次订正】**。**结论层面变动最大的五处**：

1. **协调者要求「用 ADR 0021 复核九条合并」是一处范畴误用**——ADR 0021 裁决的是 Rust 类型与
   函数，副职是注册表里的一行内容，不携带算法。一之二节先拆开这处混淆，给出合并副职真正该用的
   判据，再逐条复核。九条里**三条不成立**（采集的四合一、驭兽、行商的驮运那一半）。
2. **制作类三个副职（工匠/裁缝/调剂）的闸门已经是真代码**，不再是设计——它们缺的只剩成长挂钩，
   见一之四节。
3. **「医疗」比协调者判断的便宜得多**：`Intent::UseSkill { target: Option<EntityId> }` +
   `SkillEffect::RestoreResource` 今天就能治疗别人，一之三③给出核实。
4. **保暖（`c12c04f`）改变了裁缝的处境，但没解决它的不对称**——一节末尾的「隐忧」框已按核实结果
   重写，并给出一条本批次才成立、且不需要任何裁定的第四条补法。
5. **二节「`TraitGrant` 纯设计」已过期**——种族与职业两路天赋授予都已落地，`agent_trait_sources`
   的文档里已经**显式预留并命名了副职这一路的接入点**。

**并发声明**：本次是纯设计任务，只改本文件与 `knowledge/design/README.md` 的索引行，
不触碰 `crates/**`/`mods/**`/`assets/**`/`scripts/**`。

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

**【第二次订正】这处漂移本批次复核仍然存在**（`README.md` 第 261 行的速览行已被上一批次就地标注
「本状态行已过期」，`class-skill-quest-system.md` 未动）。但漂移的**内容**变了：现在过期的不再是
「`SubclassDef` 是否已落地」，而是「`SubclassDef` 有没有真实消费者」——`08cdeb0` 之后有了。

**这对本文档的意义**：本文档要设计的不是「从零建一个副职系统」，也不再是「给一个除了展示名字什么
都不做的空壳补接线」——`resolve_craft` 已经在真实玩法路径里读 `Agent.subclasses` 了。要补的是
**唯一还缺的那一半：怎么把副职拿到手**（获得机制 + 上限 + 放弃），以及 `traits` 这条授予通道。

---

## 一、现状核实：候选表逐项核对

先核实框架里给出的候选表。**结论：整体成立，补充几处关键细节，一处判断需要修正。**

**【第二次订正】本表冻结于 `61d2e73`，此后十个批次落地，表中六行的「落地状态」已经过期。**
过期行就地追加 **【第二次订正】** 段落，原文保留以便对照；合并后的九副职名册见一之二节。

| 方向 | 成长挂钩现状核实 | 与原描述的出入 |
|---|---|---|
| 工匠/锻造 | 耐久 `ItemStack.durability: Option<i32>` 确实已落地；修理确实不存在（`crates/ll-sim` 检索无任何修理相关 `Intent`/`Effect`） | **补充一处更深的问题**：这条方向的成长挂钩「用装备」本身站不住——耐久是消耗性的（用装备→耐久下降），不是产出性的，「越用装备耐久越低」推不出「锻造技艺越高」的因果。真正该挂的动作是「修理装备」，而修理不存在。这不是「缺一个依赖模块」这么简单，是这个方向的成长挂钩动作本身还没被创造出来，比原表格暗示的更靠后 <br>**【2026-08-22 订正，见下方「订正」小节】** 本行漏检了「制作」这个动作本身——它才是工匠的成长挂钩，`crafting-system.md` 八节已完整论证。本行的「修理」结论作废 <br>**【第二次订正】** 制作系统已于 `08cdeb0` 整套落地。工匠的**资格闸门已经是真代码**（`RecipeCategoryDef.required_subclasses`），缺的只剩成长挂钩计数。修理仍不存在，但它已经与工匠无关 |
| 鉴定/秘术 | 未鉴定状态确实不存在（`item-system.md`/`crates/ll-world/src/item.rs` 检索「未鉴定/unidentified/identify」零匹配） | 原描述准确。补充：`ItemStack.durability: Option<i32>` 是「逐实例可选状态」的现成先例，加一个 `identified: bool` 字段本身成本不高；真正贵的是「生成/掉落时标记为未鉴定」要动世界生成路径，以及需要一个新 `Intent::Identify`——比复用现成 `Effect` 的方向贵一档 <br>**【第二次订正】** 本批次复核 `identified`/`unidentified` 在 `crates/**` 仍然零命中，结论不变。**但要点名一处新出现的混淆源**：`e81e03c` 落地的 `Intent::Inspect`/`Effect::Inspect`（卫兵物品盘查）名字与「鉴定」极像，实为社交动作，与「认出未知物品」无关，勿误判为已落地 |
| 炼金/草药 | `RecipeDef` 已在 `food-and-cooking-system.md` 定形，`resolve_craft`（设计）复用 `Effect::ConsumeInventoryItem`/`Effect::MergeIntoInventory` 两个既有 `Effect`，零新增 `Effect` | 原描述准确。**补充一个关键点**：`RecipeDef.product` 是任意 `ItemDef`，配方不限于食物——药水配方可以是同一套 `RecipeDef` 的另一批内容，不需要为「炼金」新开一个类型 <br>**【第二次订正】** `resolve_craft` 不再是「设计」，已落地（`crates/ll-sim/src/resolve.rs`），且确实零新增 `Effect`。本行原判断全部兑现 |
| 厨艺 | `RecipeDef` + `Satiety`（设计） | 原描述准确。**补充一处会削弱这条方向价值的细节**：`food-and-cooking-system.md` 已经裁定「菜谱全部已知不设解锁门槛」——副职在这条方向上不能靠「解锁菜谱」当奖励，只能靠「解锁副职专属技能」（如野外烹饪），价值比炼金弱一档 <br>**【2026-08-22 订正】** 这条「弱一档」的判断已被 `crafting-system.md` 七节的**类别闸门**部分推翻：闸门挂在配方**类别**而不是单条配方上，内容设计因此可以让基础烹饪类别人人可做（食物系统五节裁定原样成立）、另设一个要求厨艺副职的高阶类别。厨艺不再天然弱一档 <br>**【第二次订正】** `Satiety` 本批次核实为**零落地**——`crates/**` 只在 `crates/ll-sim/src/exposure.rs:33` 一句对照注释里提到饱食度，且原文记录它「当初被判定不该用资源池」。**厨艺唯一区别于炼金的那个消费者今天不存在**，这是把厨艺并进「调剂」的真正理由，见一之二③ |
| 测绘/制图 | 探索记忆、世界地图确实已落地 | 原描述准确。**补充：这是全表成长挂钩成本最低的一项**——探索记忆本身就是一个已经随移动实时更新的位图，「探明了多少格」不需要新增任何计数字段，直接读现有位图即可，零新增存储 <br>**【第二次订正，本行有两处错】** ①`crates/ll-world/src/exploration.rs` 的公开接口只有 `mark_explored`/`is_explored`/`zone_has_any_explored`/`visited_zone_count`，**没有「已探明格数」这个方法**，四节说的「一次 popcount」并不存在；真正常数级且已落地的是 `visited_zone_count()`。②`Effect::MarkExplored` **只在玩家移动时产出**（`crates/ll-sim/src/resolve.rs:1698` 有整节文档解释为何不给 NPC 加），这一档触发器结构上只对玩家有效 |
| 求生/野营 | 休息、季节确实已落地 | 原描述准确。补充：「休息次数」这个计数目前不存在（全项目检索 `Agent`/脚本状态均无通用计数器先例，唯一先例是任务系统的击杀计数，是特化实现不是通用机制），需要新增，成本中等 <br>**【第二次订正】** `c12c04f` 落地了温度与保暖（`crates/ll-sim/src/exposure.rs` 按已装备物品的 `StatTarget::Insulation` 求和）。「野外过夜不被冻死」从此是一条已经在跑的真实玩法压力——求生的**需求侧**比冻结时强了一档，挂钩侧（休息计数）仍需新增 |
| 殡葬/掘尸 | 尸体系统确实已落地 | 原描述准确。**补充：`Effect::Kill` 已存在，且 `level-and-experience-system.md` 已经示范「击杀直接挂经验，不新开事件」这条先例**——杀敌计数走同一挂载点（`kill_progress_effects` 的既有存储键）成本很低，与测绘并列最低成本梯队 <br>**【第二次订正】** 本行漏了一个更贴切的挂载点：`Intent::Loot`/`resolve_loot`（`crates/ll-sim/src/resolve.rs:1795`）**已落地**，它挑「本格上 `contents` 非空的容器」（尸体）、整个移除、把 `contents` 逐条并进背包——**「剥取」这个动作本身已经是真代码**，比「杀敌计数」更贴合殡葬的语义 |
| 骑术 | 载具设计尚未落地（`crates/` 检索无 `MountDef`/`register-vehicle` 实现） | 原描述准确。**【第二次订正】** 本批次复核仍然零命中。但骑术**不该单列成一个副职**——项目所有者自己的原话已经把载具劈成「能自己动的生物」与「被制作出来、可以被放置」两类，见一之二⑦ |
| 斥候/侦知 | 陷阱/暗门/足迹确实全不存在 | 原描述准确。补充：三者都缺，不是缺一个模块，没有「先做一半」的余地。**【第二次订正】** 本批次复核仍然全缺。另需划一条边界：`bb6cda8` 落地了潜行（`Agent.stealthed`/`Intent::ToggleStealth`），但**潜行不属于斥候，也不属于副职轴**——项目所有者已裁定它归盗贼主职业，见一之三① |
| 驯兽/训导 | 同伴系统、NPC 态度确实全不存在（`crates/` 检索 `companion`/`Attitude` 零匹配） | 原描述准确。**【第二次订正】** 本批次复核仍然零命中 |
| 驮运 | 负重确实不存在（`ll-sim/item.rs` 模块文档原文核实：`base_weight`/`base_price`/`max_durability` 均未接线） | 原描述准确。补充：这是最能低成本复活 `ItemDef.base_weight` 死字段的方向，但按 YAGNI 不在本次最小可用集范围内 <br>**【第二次订正，本行的判断本身要推翻】** 驮运的全部内容是「能背更多」——**那是一个数值**，而二节的红线是「副职不给数值」。**它根本不是一个副职方向，是一条 `TraitGrant`**，见一之二⑨。`base_weight` 仍是死字段（`crates/**` 只有 `content_audit.rs`/`content_hash.rs` 碰它） |
| 商贾 | 交易系统确实不存在（`crates/` 检索 `resolve_trade`/`Intent::Trade`/`register-trade` 零匹配） | 原描述准确。**【第二次订正】** 本批次复核补一处：`Effect::AdjustWallet` **已落地**（`crates/ll-sim/src/effect.rs:177`，`apply.rs:141` 消费），但在 `resolve` 里**零产出者**——钱这个字段在真实玩法里从不变化。交易缺的不是 `Effect`，是 `Intent` 与 resolve 函数 |

**一处需要修正的判断**：原框架把「工匠/锻造」列在候选表首位，暗示它是较优先的方向。核实后应当垫底——不是因为耐久/修理这两个依赖模块本身多难补，而是因为它的成长挂钩动作在因果上是反的（见上表）。详见六节排序。

### 订正（2026-08-22，由 `crafting-system.md` 触发）

上一段的「垫底」结论**作废**。本文档当初检查工匠的成长挂钩时，只检查了「用装备」（因果反了）与
「修理」（不存在），**漏了「制作」这个动作本身**——而制作恰恰是工匠的定义性动作。
`crafting-system.md` 八节的完整论证归纳如下，此处只记结论：

1. 「制作」在本文档写作时确实不存在，但它**无论如何都要为烹饪造出来**（项目所有者已点名食物/
   菜谱系统）。工匠挂钩的**边际成本**因此是「在一个正在造的动作上加一个计数器」，与本文档给
   `ItemsPickedUp` 的评价（「把已经验证过的模式再抄一份」）完全同级，不是「先造一个新机制」。
2. 这条订正顺带修正了**炼金**的挂钩：本文档给炼金挂的是 `ItemsPickedUp`（捡材料），
   但捡草药是所有人都在做的通用动作，等于「探索得够多就自动变成炼金术士」。
   `ItemsCrafted("lostland:alchemy")` 精确得多——熬过多少锅药才是炼金练了多久。
3. **四个制作副职（工匠/裁缝/炼金/厨艺）因此共用同一个触发器变体**，各自填不同的配方类别；
   一次制作系统的投资同时解锁四个方向。

**订正后的排序：工匠从第 8 位升到与炼金并列的第 3 梯队，不是升到第一**——它依赖
`Intent::Craft`/`RecipeTable`/`RecipeCategoryTable` 三样尚未落地的东西，仍然贵于
「测绘/制图」（零新增存储）与「殡葬/掘尸」（挂载点已落地）。见六节修订后的排序。

**【第二次订正】上面这句「三样尚未落地」已经过期**——三样在 `08cdeb0` 全部落地。制作类副职因此
从「阻塞在一个正在设计的系统上」变成「闸门已通、只差计数」，见一之四节与六节的新排序。

### 一处订正解决不了的隐忧：裁缝的长期需求不对称【第二次订正，整段重写】

**原文（`61d2e73`）**：「成长挂钩回答的是『怎么获得这个副职』，不回答『获得之后它还有没有用』。
项目所有者已裁定只有装备武器才有耐久——武器会磨损，玩家会反复回来找工匠；衣服永远不坏，裁缝的
每件产出都是一次性买卖。这条长期需求的不对称需要『耐久适用范围』或『物品能否授予抗性』的裁定。」

**本批次核实后，三条候选补法里有一条已经落地，结论要改**：

1. **温度/保暖——已落地**（`c12c04f`）。`StatTarget::Insulation`（`crates/ll-world/src/item.rs:506`）
   是真枚举变体，脚本侧 `register-item-stat-bonus` 认 `"insulation"`
   （`crates/ll-mod/src/script_item_api.rs:431`），`crates/ll-sim/src/exposure.rs` 按已装备物品求和。
2. **物品授予抗性——仍未落地**。`StatTarget` 只有 `Attribute`/`Armor`/`Insulation` 三个变体
   （本批次复核，无 `Resistance`）。`crafting-system.md` 十四节④已把这条挂在「多来源抗性聚合」批次上。
3. **扩大耐久适用范围——仍是一条待裁定的口子**，`crafting-system.md` 六节末尾标注过。

**保暖落地改变了什么、没改变什么——这是本节的核心结论**：

- **改变了「有没有需求」。** `c12c04f` 之前，衣服在结算层面什么都不做（除 `equip_mask` 外无任何
  派生消费者）；之后它进了 `exposure.rs` 的一条真实判定。原文那句「裁缝做完一件就再也不需要他」
  里的**那一件，现在是有意义的一件**——裁缝的产出不再是纯风味。
- **没改变「需求会不会重复」。** 工匠的重复需求来自武器耐久的**递减曲线**；`Insulation` 是从已装备
  物品求和出来的**常数**，不随时间下降。一件不坏的厚衣服仍然是一次性买卖。
- **把三条补法收敛成了一条真问题**：抗性那条卡在别的批次，保暖那条已经做完，**剩下唯一需要项目
  所有者裁定的是「衣物要不要有耐久」**。原文说「需要两个裁定」，现在只需要一个。

**一条本批次才成立、且不需要任何裁定的第四条补法**：温度是按空间与季节派生的
（`SpaceProfile.base_temperature` + 季节 + 天气三路都已落地，`d5215f1`/`c12c04f`），
**所以同一个玩家在不同气候带需要不同的衣服**——去冰原要厚的，去沙漠要另一件。
这是一条**不依赖耐久的重复需求**，机制支撑今天就全部到位，**完全落在内容设计层**。
它不能完全替代耐久（气候带数量有限，需求会饱和），但它把「裁缝一次性买卖」这个问题从
「必须先有裁定才能动」降级成了「现在就能先做起来，裁定可以慢慢等」。

---

## 一之二、名册合并：十七个方向收敛成九个副职（独立复核）

项目所有者的原话：「我最开始讲的副职只是提供了个方向，还缺少了很多方面的设定，你可以看看有没有
什么重叠的，就放一个合适的副职里，或者是缺少了什么」。他陆续点过的方向是：炼金/草药、工匠、驯兽、
斥候、鉴定、科研、采集矿物、种植、制作工具武器装备、制作衣服，以及「生活琐事」这个大方向；
协调者另外提过厨艺、测绘/制图、求生/野营、骑术、殡葬/掘尸、驮运、商贾、博识。

协调者给出的合并是十七 → 九。**本节独立复核，不采信任何一条。**

### 先纠正一处判据误用：ADR 0021 管的是代码，不是内容

协调者要求「用 ADR 0021 的判据独立复核每一条合并」。**这条要求本身有一处必须先拆开的混淆，
不拆开的话九条合并没有一条能被正确评价。**

ADR 0021 的判据是「有没有一份算法要被共用」，且是双向的——既拦「为对称而抽象」（`Camera`/
`BoundedCamera` 不抽 trait），也拦「把同一份算法复制四遍」（`compute_fov` 抽 `SightGrid`）；
后一个方向正是 `crafting-system.md` 二节论证四类制作统一时用的那一侧。**但这条判据的裁决对象
是 Rust 类型与函数，不是 `SubclassTable` 里的行。** 一个副职不携带任何算法：它是注册表里的一条
`(id, display_name_key)` 记录，加上别人拿它做的一次 `contains` 判定。

`crafting-system.md` 自己就是这处区分的现成证据：它用 ADR 0021 论证出**一套** `RecipeDef`/
`resolve_craft`/`craft_progress_effects`，同时**保留了四个互不相同的副职**——工匠/裁缝/炼金/厨艺
共用同一个 `SubclassUnlockTrigger::ItemsCrafted` 变体、同一个计数函数，各自填不同的
`RecipeCategoryDef.required_subclasses`。**「一份算法」与「一个副职」在那份文档里已经是解耦的。**

所以「用 ADR 0021 判断两个方向该不该合成一个副职」是范畴错误。它能回答的只有
「这两个方向的成长挂钩要不要写成两个函数」，而那个问题的答案几乎恒为「不要」——
`kill_progress_effects`（`crates/ll-sim/src/quest.rs:229`）那套「读计数 → 加一 → 达标检查 →
产出写入」是六行结构，任何新触发器都是同一份结构换一个键。**从「所以计数函数只写一份」跳到
「所以这两个方向是同一个副职」，是从代码结论跳到内容结论。**

### 那么合并副职的正确判据是什么

副职按二节只有两件职责：**当闸门**（`RecipeCategoryDef.required_subclasses` 已落地、
`SkillRequirement` 待落地）与**授天赋**（`traits` 待落地）。因此：

> **两个方向该合成一个副职，当且仅当不存在任何一份合理的内容设计，会想把一扇门对其中一个打开、
> 对另一个关上。**

这条判据可证伪（举一个反例即可推翻一条合并），而且**它的两个方向代价严重不对称**：

- **合并的代价不可逆。** `crates/ll-content/src/remap.rs:612` 的 `remap_subclasses` 对解析不到的
  索引**直接丢弃**（本批次核实）。把 A、B 合成 C 之后再想拆回去，已经持有 C 的存档既拿不到 A
  也拿不到 B——那是一次会让玩家丢东西的迁移。
- **拆开的代价是一行内容。** 多一个副职就是多一次 `register-subclass` 加一次
  `recipe-category-requires-subclass!`（两个函数都已落地），零 Rust 改动、零存档影响。

**因此拿不准的时候，正确的默认是不合并。** 这与 ADR 0021 在代码那一侧的默认（拿不准时不抽象）
是同一条 YAGNI，只是在内容这一侧，「不抽象」对应的动作叫「不合并」。

### 九条合并的复核结论

| 副职 | 协调者的合并 | 复核结论 | 挂钩动作的真实落地状态（本批次 grep 核实） |
|---|---|---|---|
| **工匠** | 工具/武器/护甲 | **成立** | 闸门**已是真代码**；计数未落地 |
| **裁缝** | 衣物 | **成立**（1→1，不是合并） | 同上 |
| **调剂** | 炼金 + 厨艺 | **结论可接受，但给出的理由站不住** | 同上 |
| **采集** | 采矿 + 种植 + 草药 + 剥取 | **不成立**（作为一行成本标注） | 四个挂载点：两个已落地、一个待造、一个连所属系统都没有 |
| **学者** | 鉴定 + 博识 + 科研 | **合并无害，但排位错了** | 挂钩动作**不存在**，且没有任何批次会顺手造出它 |
| **斥候** | 侦察 + 测绘/制图 | **成立**，但成本描述有两处错 | 测绘那一半已落地，且**只对玩家有效** |
| **驭兽** | 驯兽 + 骑术 | **不成立**——所有者自己的原话把它劈成两半 | 两半都零落地 |
| **求生** | 野营 | **成立**，且比冻结时强了一档 | `Intent::Rest` 已落地，`c12c04f` 之后有真实压力 |
| **行商** | 驮运 + 商贾 | **驮运那一半违反二节自己的红线** | 两半都零落地 |

### ① 工匠（工具/武器/护甲）——成立

本批次核实，**这一条已经不是设计了**：`crates/ll-mod/src/recipe_category.rs:72` 有
`pub required_subclasses: Vec<ContentIndex>`，`crates/ll-sim/src/resolve.rs:2340` 的 `resolve_craft`
第③步读它、对 `agent.subclasses` 做 any-of 判定、空列表即人人可做。

「工具/武器/护甲」要不要在副职层面三分，**这个问题不需要现在回答，因为旋钮已经在代码里了**：
三者是三条 `RecipeCategoryDef`，闸门是一个 any-of 列表——内容设计可以让三个类别都要求
`lostland:artisan`，也可以拆成 `lostland:smith`/`lostland:armorer` 各管一段。副职名册不必替内容
设计做这个决定；把它合成一个「工匠」是一个安全的起点，因为**往细里拆是加一行 `register-subclass`**，
而往粗里并是一次会丢存档的迁移。

### ② 裁缝（衣物）——成立

同上。**这不是一次合并**（只有一个来源方向），列在这里是因为它与工匠共用全部机制。
裁缝真正的问题不在合并，在长期需求的不对称，见一节末尾已按保暖落地重写的「隐忧」小节。

### ③ 调剂（炼金 + 厨艺）——结论可接受，但协调者给的理由站不住

协调者的理由：「两者都是『配方消耗材料 → 产出带 `use_effect` 的消耗品』，机制上一模一样，
只有风味不同」。**两处问题**：

**(a) 这条理由对工匠同样成立，而协调者没有把工匠并进来。** 铁剑也是「配方消耗材料 → 产出一个
`ItemDef`」。若「机制一模一样」足以合并，正确的结论是把工匠/裁缝/调剂**全部**合成一个「制作者」
副职；协调者显然不要那个结论，说明真正在起作用的判据不是机制，是内容身份——**那就该按内容身份
论证，不该借机制说事。** 用一条会把自己另外两行一并吞掉的理由去支持第三行，是论证不成立，
不必然是结论不成立。

**(b)「产出带 `use_effect` 的消耗品」根本不是机制层的性质。** `crafting-system.md` 二节③已核实、
本批次复核确认：`resolve_craft` 从头到尾**不读** `use_effect`，也不读 `equip_mask`——成品能不能喝
是 `resolve_use_item` 的事。用一个 `resolve_craft` 看不见的字段论证 `resolve_craft` 层面的合并，
论证与结论对不上。

**按正确判据重新问一遍**：存不存在一份想把某扇门对药开、对饭关的内容设计？——存在，而且很好想
（「野外急救包」这个类别显然该对药师开、对厨子关）。但反向的设计同样好想。**这说明这条合并是一个
纯粹的内容取舍，不是机制结论。**

**结论：可以接受，但必须如实标成取舍，并把两个方向的代价一并写出来**——合并后再拆，已持有的
存档拿不回；而保留「厨艺」独立的代价恰好是**一行 `register-subclass`**。这一条建议明示给项目
所有者，让他知道这不是一个技术约束，是一次可以随时反悔（但要趁早反悔）的内容决定。

**附一条支持合并的真实证据（协调者没给，本批次核实得到）**：**饱食度（`Satiety`）零落地**——
`crates/**` 只在 `crates/ll-sim/src/exposure.rs:33` 一句对照注释里出现，原文记录饱食度
「当初被判定**不该**用资源池，因为资源池是『显式授予』而饱食度是『人人都有』」。
**厨艺唯一区别于炼金的那个消费者，今天在引擎里不存在。** 这才是支持合并的论证：不是「两者机制
一样」，而是「其中一个的差异化基础还没建成，现在为它留一个独立副职槽位是 YAGNI」。
**并且这条论证自带失效条件**：饱食度一旦落地，厨艺就有了炼金没有的消费者，那时候拆开的理由
就成立了——**所以这条合并应当在饱食度落地之前完成拆分决策，不要拖到玩家已经攒了存档之后。**

### ④ 采集（采矿 + 种植 + 草药 + 剥取）——**不成立**

协调者的理由：「都是『从世界拿原材料』。作为**系统**它们各不相同，但作为**副职**是同一条轴」。
**把「采集者」当成一个内容身份是能成立的；不成立的是这一行的成本标注。** 逐条核实四个挂载点：

| 子方向 | 挂载点 | 落地状态（本批次核实） |
|---|---|---|
| 草药 | `Intent::PickUp` → `resolve_pick_up` | **已落地** |
| 剥取 | `Intent::Loot` → `resolve_loot`（`crates/ll-sim/src/resolve.rs:1795`） | **已落地**，且与拾取是**两个不同的函数** |
| 采矿 | 将来的 `resolve_mine` | **不存在**——`crates/**` 唯一命中是 `crates/ll-mod/src/recipe.rs:118` 的一句「将来的 `resolve_mine`」 |
| 种植 | 无 | **连所属系统都不存在** |

三处必须写下来的具体问题：

1. **「挂钩动作：采集次数」读起来像一个已落地的单一挂载点，实际是四个。** `resolve_loot` 挑的是
   「本格上 `contents` 非空的容器」（尸体），把容器整个 `RemoveGroundItem` 掉、再把 `contents`
   逐条并进背包；`resolve_pick_up` 挑的是「本格上任意一堆地面物品」。两者结构不同，计数要挂两处。
2. **采矿的产出通道与另外三者不同**——它改的是地形（`Effect::SetTerrain`），不是「地上多一堆
   物品」。按 ADR 0021 那一侧看，它与拾取/剥取**没有算法可共用**；按本节判据看，它作为副职内容
   并进「采集」没问题，但**并进来的时间点应当是 `resolve_mine` 落地之后**，而且届时并进来是免费的
   （计数键是 `NamespacedId` 字符串，多一个键不影响已有存档，见四节）。
3. **种植按协调者自己写的合并轴，字面上就不属于这一条。** 「都是从世界拿原材料」——种植是先给
   世界、等时间、再收，方向相反。而且它是四者里依赖最贵的一个：「逐格随时间演变的状态」在引擎里
   **没有任何先例**（地形是每格一个 `TerrainKind`，`ChunkGrid` 没有逐格计时器），比载具、交易、
   同伴这几个「形状清楚只是没做」的系统还要靠后。**把它折进一行「采集次数」，等于把整份名册里
   最贵的一项藏进了看起来最便宜的一行**——而「设计文档记录过期，害得后续判断出错」正是这个项目
   已经踩过四次的坑。

**修正建议**：采集 = 草药 + 剥取（两个都已落地，现在就能做）；采矿在 `resolve_mine` 落地后并入
同一个副职（并入零代价）；**种植单列一行、不给承诺**——它不是「采集的一个子项」，它是一个尚未
立项的系统。

### ⑤ 学者（鉴定 + 博识 + 科研）——合并本身无害，但排位错了

逐条核实：

- **鉴定**：`identified`/`unidentified` 在 `crates/**` **零命中**，确认仍不存在。
  混淆风险见一节候选表「鉴定/秘术」行的第二次订正（`Intent::Inspect` 是卫兵盘查，不是鉴定）。
- **博识**：无任何存储需求，纯闸门/天赋，零成本。
- **科研**：需要 `Agent.known_recipes` 新字段 + `resolve_craft` 新一道判定 + 存档新 `ContentKind`。
  且它压着一条未裁定冲突，见八节①。

**合并本身没问题**（三者都是「知道某件事」这一类门），**但「挂钩动作：阅读/研究」在引擎里根本
不存在**——`Intent` 现有十七个变体（`Move`/`Attack`/`Wait`/`OpenDoor`/`EnterSpace`/`ExitSpace`/
`UseSkill`/`Rest`/`PickUp`/`Drop`/`Equip`/`Unequip`/`Use`/`Loot`/`Inspect`/`ToggleStealth`/`Craft`），
没有任何一个是读书或研究。

**这一点让学者与其余八行不在一个量级上，而这正是本文档一节当初对工匠犯过的错的镜像。**
当时的错是「漏检了制作这个动作」；这次要避免的是反过来——**「阅读/研究」与「制作」不同的地方
在于，制作无论如何都要为烹饪造出来（边际成本论证成立），而没有任何已落地或已排期的批次会顺手
造出一个阅读动作。** 学者的挂钩是全表唯一一个连边际成本论证都用不上的挂钩，**它必须排在名册最末**，
不能与其余八行并列。

### ⑥ 斥候（侦察 + 测绘/制图）——成立，但成本描述有两处错

合并成立：两者都是「走在前面、把信息带回来」这一个内容身份，找不到一份想把某扇门对测绘开、
对侦察关的设计。**但冻结版对测绘成本的两句描述本批次核实为错**，已在一节候选表「测绘/制图」行
就地更正，此处只记结论：

1. **没有「已探明格数」这个方法**，四节说的「一次 popcount」并不存在。已落地且真的是常数级的是
   `visited_zone_count()`——触发器变体名相应地应当是 **`ZonesVisited`** 而不是 `TilesExplored`。
2. **`Effect::MarkExplored` 只在玩家移动时产出**，这一档触发器结构上只对玩家有效。
   这不只是斥候一个方向的问题，见一之三④第 1 条。

**一条必须写下的边界**：斥候**不吸收潜行**。理由见一之三①。

### ⑦ 驭兽（驯兽 + 骑术）——**不成立**

核实：`MountDef`/`register-vehicle`/`CompanionDef`/`Attitude` 在 `crates/**` **全部零命中**，两半
都没落地。合并的问题不在成本，在**项目所有者自己的原话已经把它劈成了两半**——
`vehicle-and-mounting.md` 第 11 行引用的原文：

> 「因为有的可能是能自己动的生物，而有的是被制作出来，可以被放置在地图某处。」

**载具按所有者的定义天然分两类：活的（马、牛）与造的（船）。** 骑术若定义成「骑乘载具」，
它一半落在驯兽这边、一半落在工匠/营造那边（造船是制作，放置是营造）。

用本节判据检验：存不存在一份想把船开放给不会驯兽的人的内容设计？——**存在，而且它就是所有者
那句话本身。** 驯兽的门是「这只生物听不听我的」（需要态度/同伴状态），骑术的门是「我会不会驾驭
这个载具」（需要载具表）。两扇门守的不是同一件事。

**修正建议**：**驭兽 = 驯兽（活物），骑术不单列成副职**，而是**跟着载具类型走**——活物载具的门
是驭兽，造物载具的门是工匠/营造。这样零新增副职、与所有者原话一致，且不需要在载具系统落地之前
就替它决定门该长什么样。

### ⑧ 求生（野营）——成立，且比冻结时强了一档

`Intent::Rest`/`Effect::BeginRest`/`Effect::ClearResting` 已落地，季节已落地。**冻结版没有的新增
需求来源**：`c12c04f` 落地了温度与保暖（`crates/ll-sim/src/exposure.rs` 按已装备物品的
`StatTarget::Insulation` 求和）。「在野外过夜不被冻死」从此是一条真实的、已经在跑的玩法压力，
不再只是叙事包装。

**求生因此从冻结版的第 4 位（「休息已落地但计数需新增，成本中等」）提到第 2 梯队**：它的挂钩是
一个已落地的**单点**，比采集的四个挂载点干净，也比斥候的玩家专属挂钩通用。

### ⑨ 行商（驮运 + 商贾）——驮运那一半违反二节自己的红线

核实两半：

- **商贾**：`Effect::AdjustWallet` **已落地**（`crates/ll-sim/src/effect.rs:177`，`apply.rs:141`
  消费），**但在 `resolve` 里零产出者**——全仓检索只命中 `apply` 与测试。钱这个字段在真实玩法里
  从不变化。`Intent::Trade`/`resolve_trade` 零命中。所以「等交易系统」等的不是一个 `Effect`，
  是一个 `Intent` 和一个 resolve 函数。
- **驮运**：`ItemDef.base_weight` 仍是死字段——`crates/**` 里只有 `content_audit.rs` 与
  `content_hash.rs` 碰它（审计与哈希，不是玩法消费），负重未落地。

**但驮运的问题比「没落地」严重得多**：它的全部内容是「能背更多」——**那是一个数值**。
二节的红线原文是「副职不给数值，只当『能不能学』的资格闸门；给东西的是天赋」。
驮运没有任何「能不能做某件事」的门可以守，它只有一个上限数字。

**按本文档自己的判据，驮运根本不是一个副职方向，它是一条 `TraitGrant`。** 而天赋授予这条路
**已经落地了两路**（种族 `register-race-trait`、职业 `register-class-trait`，见二节第二次订正），
副职那一路只差 `SubclassDef.traits` 一个字段。

**修正建议**：行商 = 商贾（等交易系统）；**驮运删出名册**，改记成一句「行商副职将来可以通过
`traits` 授予一条提升负重的天赋」——一句话，不占一个副职槽位。

---

## 一之三、缺口复核：已有系统上还没有副职挂着的地方

判据：**已落地的系统里，哪些没有任何副职挂在上面。** 协调者提了三条，本节逐条复核，再补上本次
自己找到的。

### ① 潜行/盗窃——**不是副职。这一条记录为一次有据可查的更正**

核实：**潜行已落地**（`bb6cda8`）——`Agent.stealthed: bool`、`Intent::ToggleStealth`、
`Effect::SetStealth`，走「改判定不改视野」。**盗窃仍未落地**：`Owner`/`stolen_marker` 零落地
（`crates/ll-sim/src/effect.rs:698` 与 `crates/ll-sim/src/apply.rs:416` 两处注释都明说「尚未落地，
未来批次」），`Effect::Inspect` 已经预留了将来比对 `owner`、转成 `HistoricalEventKind::Crime`
的位置。

协调者当初把「盗窃/潜行」列成「最大的副职缺口」。**项目所有者已裁定它不属于副职**，原话：
「潜行和盗窃或许可以安排成盗贼主职业的一种被动技能 buff？」**本节把这次更正记录下来，
并给出为什么这条裁定在结构上是对的、而不只是所有者的偏好。**

**理由一：它站错了轴。** 本文档一节定的副职轴是「你怎么在**战斗之外**补给自己」。潜行不是一种
补给方式，它是一种**接敌方式**——`bb6cda8` 把它接进的是攻击判定（潜行中攻击直通偷袭），
不是任何补给循环。

**理由二：把它做成副职会拆掉六节想要的那条张力。** 六节的价值主张是「build 多样性来自直觉搭配
vs 错位搭配的化学反应」。**错位搭配之所以有味道，前提是主职业限定了你是谁，副职只改变你怎么活
下去。** 潜行若是副职，任何一个厨子攒够次数就能变成刺客——主职业不再限定任何东西，
「错位」这个概念本身就不存在了。**这不是「潜行放哪都行」的品味问题，是放错了会让另一条设计
主张失效。**

**理由三：所有者选的落点恰好是两条路里已经通了的那一条。** `5f6bae5` 落地了 `ClassDef` 授予
天赋（`register-class-trait`，带 `unlock_level`），`13eea2d` 落地了多来源规则修正聚合点
`crates/ll-sim/src/rule_modifier.rs`。「盗贼主职业的一种被动技能 buff」在今天的代码里就是一条
`(register-class-trait "lostland:rogue" "lostland:shadow_step" N)`——**零新增机制**。
副职那一路反而还差 `SubclassDef.traits` 字段。

### ② 建造/营造——缺口成立，且比协调者说的更有依据

核实三条，每条都比协调者的说法更具体：

- **`Effect::SetTerrain` 已落地，且已经是真实玩法路径的产出**——不只是 `WorldState.terrain`
  上那个存储方法：`resolve_open_door`（`crates/ll-sim/src/resolve.rs:3029`）与 `resolve_move`
  撞门那一支（同文件 2443 行）都产出它，`crates/ll-sim/src/apply.rs:138` 消费它。
  **「在世界里改一格地形」这个原语已经是存档/重放安全的**，营造不需要发明它。
- **`SpaceProfile.buildable` 确实存在且确实无消费者**——`crates/ll-mod/src/script_space_profile_api.rs:116`
  原文：「`diggable`/`buildable`：布尔。同样**暂无消费方**——采矿/营造动作……」。
  **注意 `diggable` 是同一行的另一个死字段**：采矿与营造是这两个死字段唯一的、成对的消费者。
- **缺的是 `Intent`**：现有十七个变体里没有 `Build`（清单见一之二⑤）。

**与工匠的界限**（协调者给的「便携 vs 世界」是对的，本节补上它在代码里的形状）：工匠的产出走
`merge_into_inventory_effect`（进背包），营造的产出走 `Effect::SetTerrain`（进地形）。
这是两个不同的产出通道，不是同一份算法喂不同数据——**按 ADR 0021，这一处确实不该并进
`resolve_craft`**（与制作系统把四类制作合成一套不矛盾：那四类的产出通道是同一个）。

副职层面合不合并是另一个问题，按一之二节的判据：存不存在一份想把「造房子」开放给不会打铁的人的
内容设计？——**存在（木匠/石匠）。所以营造应当是独立副职，不并进工匠。**

### ③ 医疗——缺口成立，但**比协调者判断的便宜得多**，排序要往前提

协调者说「资源池和休息都落地了，但『治疗别人』没有归宿」。**这条只对了一半，另一半本批次核实
为已落地**：

- `Intent::UseSkill { actor, skill, target: Option<EntityId> }` 已落地，`target` 是 `Option`
  （`crates/ll-sim/src/intent.rs:138`）。
- `resolve_use_skill`（`crates/ll-sim/src/resolve.rs:3310`）里 `let effect_target = target.unwrap_or(actor);`，
  而 `SkillEffect::RestoreResource` 分支产出 `Effect::AdjustResource { actor: effect_target, .. }`。

**也就是说「对另一个人释放一个回复资源的技能」今天就能跑通，一行 Rust 都不用改。**

真正缺的只有两条，两条都很小：

1. **用药不能对别人用**——`Intent::Use { actor, def }` 没有 `target` 字段，`resolve_use_item`
   把 `Effect::AdjustResource` 的 `actor` 写死成使用者自己。「给伤员灌药」需要给 `Intent::Use`
   加一个 `Option<EntityId>`，与 `Intent::UseSkill` **完全同构**——照抄，不是发明。
2. **没有「治疗了多少次」的计数**——与其余八个副职缺的是同一样东西（`SubclassUnlock` 整套）。

**结论：医疗是全部缺口里唯一一个「资格闸门那一路已经完全跑通、只差成长挂钩」的方向**，
与工匠/裁缝/调剂三个制作副职处境相同，**应当与它们同列，不该排在学者/驭兽/行商之后。**

**它与调剂的界限清楚且不重叠**：调剂**造**药（`resolve_craft`，产出一个 `ItemDef`），
医疗**用**药与施术（`resolve_use_item`/`resolve_use_skill`，产出 `Effect::AdjustResource`）——
两个不同的 `Intent`、两个不同的 resolve 函数、两条不同的产出通道。按一之二节判据检验：
存不存在一份想把「战地急救」开放给不会熬药的人的设计？——存在（军医不必是药剂师）。
**所以医疗是独立副职，不并进调剂。**

### ④ 本批次自己找到的缺口

按同一条判据把已落地系统逐个过一遍：

1. **副职获得机制整套是隐式「只对玩家」的——这是本节找到的最大问题。**
   `5862dbe` 之后行为树经 `TurnEngine` 真的在驱动 NPC，`Agent.subclasses` 是**每个** `Agent` 都
   有的字段，但四节设计的触发器里 `TilesExplored`/`ZonesVisited` 结构上只对玩家有效
   （`MarkExplored` 只在玩家移动时产出）；`Kills`/`ItemsCrafted` 虽然对 NPC 也能跑，
   但没有任何路径会给 NPC 提交 `Intent::Craft`。**「NPC 铁匠的副职从哪来」这个问题本文档从未
   回答过。** 最省事的答案是世界生成或 `register-class` 时直接写死一份初始 `subclasses`——
   而那需要一个本文档同样没有命名的效果，见下一条。
2. **`Effect::GrantSubclass` 从未被命名——一处真实的设计漏洞。**
   四节说「把达标时产出的效果从『标记任务完成』换成『标记副职获得』」，但没给这个效果一个名字，
   也没论证它必须存在。ADR 0023「脚本状态写入必须过 `apply`」加上 `Agent.subclasses` 是
   `WorldState` 的一部分（不是 `script_state`），意味着这个写入**不能**塞进
   `Effect::SetScriptState`，必须是一个新的 `Effect` 变体。五节已经命名了
   `Effect::RemoveSubclass`，**成对的授予效果反而漏了**。本次在四节补上。
3. **`Effect::AdjustWallet` 已落地但在 `resolve` 里零产出者**——见一之二⑨。这不是副职缺口，
   是行商那一行「等交易系统」的具体含义。
4. **天气/温度已落地，副职覆盖是够的**——`d5215f1`/`c12c04f` 落地了天气与温度，`exposure.rs`
   已经按装备的 `Insulation` 求和。裁缝（做保暖衣物）与求生（野外过夜）两个副职吃这个系统，
   不缺第三个。**这是一条正面结论，记在这里是为了说明本节判据被完整跑过一遍，不是只挑了有缺口
   的看。**
5. **卫兵盘查（`Intent::Inspect`/`Effect::Inspect`，`e81e03c`）没有副职挂着，且不应该有**——
   它是主职业（`lostland:guard`，`mods/lostland/classes.scm`）的行为，与潜行同理，属于
   「你是谁」而不是「你怎么补给」。同样记为一条查过并排除的项。
6. **修理仍然不存在，且现在它是无主的。** 一节最初把修理当成工匠的挂钩，第一次订正把工匠改挂到
   制作上之后，**修理这个动作就没有任何副职声称它了**。它不是一个副职缺口（工匠已经有挂钩），
   而是一条待记录的观察：若将来做修理，它天然属于工匠，且是裁缝不对称问题的第三条补法
   （扩大耐久范围）的配套动作。

### ⑤ 一条本节判据判不出、但项目所有者应当知道的事

**九个副职里，只有工匠/裁缝/调剂的闸门、求生的休息、采集的拾取与剥取是真代码；其余全部挂钩动作
今天都不存在。但这不是九个方向各自的问题，是同一个瓶颈：`SubclassUnlock`/`SubclassUnlockTrigger`/
`Effect::GrantSubclass` 这套获得机制一天不落地，九个副职一个也拿不到。**

**先落地获得机制、再逐个补方向，比先想清楚九个方向要划算得多**——方向是内容数据（一行
`register-subclass`），获得机制是代码。这也是六节新排序把「共同前置」单列成第 0 梯队的理由。

---

## 一之四、制作类副职的地基已经变了：闸门是代码，缺的只是成长挂钩

**这一节独立成节，因为它是全文档唯一一处「从设计变成实现」的地方，后来人最容易照着过期描述
做错判断。**

### 已经是真代码的部分（`08cdeb0`）

| 组成 | 位置 | 语义 |
|---|---|---|
| `RecipeCategoryDef.required_subclasses: Vec<ContentIndex>` | `crates/ll-mod/src/recipe_category.rs:72` | 一个配方类别要求哪些副职 |
| `recipe-category-requires-subclass!` | `crates/ll-mod/src/script_recipe_category_api.rs:44` | mod 侧注册入口，注册后追加，不挤进 `register-recipe-category` 的位置参数 |
| `RecipeCatalog::category_required_subclasses` | `crates/ll-sim/src/craft.rs:98`（trait）/ `crates/ll-mod/src/recipe.rs:359`（impl） | 依赖倒置，决策层不直接读字段 |
| `resolve_craft` 第③步 | `crates/ll-sim/src/resolve.rs:2340` | **any-of**：`required_subclasses` 为空即人人可做，非空时 `agent.subclasses` 命中任意一个即通过 |

**语义要点，本批次逐条核实**：闸门是 **any-of** 不是 all-of；空列表是**合法且常见**的
（`crates/ll-mod/src/content_audit.rs:1474` 原文）；闸门挂在**类别**上不挂在配方上，
所以「基础烹饪人人可做、高阶烹饪要厨艺」这种内容设计不需要任何代码支持。

### 还缺的部分——只有一样

`SubclassUnlockTrigger::ItemsCrafted(类别)` + `craft_progress_effects`。**它依赖 `SubclassUnlock`
整套先落地**（见一之三⑤），不能单独做。

### 做 `craft_progress_effects` 的人必须先读的一条纪律

**计数键用 `NamespacedId` 字符串拼，不要照抄 `kill_count_key` 的索引数值键。** 完整理由见四节
「计数键」小节——这条纪律写在这里是因为**实现它的人会先读本文档**，而隐患藏在
`crates/ll-content/src/remap.rs` 里，不看不会撞上。

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

### 【第二次订正】上面两条各自过期一半

**第 1 条原样成立，未过期。** 本批次复核：`skill-requires!`/`SkillRequirement`/`Intent::LearnSkill`/
`resolve_learn_skill` 在 `crates/**` 仍然**全部零落地**（检索只命中设计文档，以及
`crates/ll-mod/src/recipe_category.rs:151` 与 `crates/ll-mod/src/race.rs:70` 两处引用它作先例的
注释）。

**第 2 条已过期。** `5f6bae5` 落地了 `ClassDef` 授予天赋：`ClassTable::add_trait_grant`、脚本入口
`register-class-trait`（`crates/ll-mod/src/script_class_api.rs:54`）、
`impl TraitGrantSource for ClassTable`（`crates/ll-mod/src/class.rs:290`）；`RaceDef` 那一路更早
就有（`crates/ll-mod/src/race.rs:463`）。`effective_traits` 的签名同批改成收 `&[TraitSource]` 切片
（`crates/ll-sim/src/traits.rs:250`），`13eea2d` 又在其上落地了多来源规则修正聚合点
`crates/ll-sim/src/rule_modifier.rs`（天赋 + 装备两路）。

**这对副职的意义比「少一条待办」大得多**：`crates/ll-sim/src/traits.rs` 的 `agent_trait_sources`
现在返回 `[TraitSource; 2]`（种族 + 职业），**它的文档里有一整节标题就叫「其余三路（副职/载具/buff）
为什么不在这里」**，原文写着「按 YAGNI，接线那一批再往返回值里加元素，届时本函数的返回类型从
`[TraitSource; 2]` 变成 `[TraitSource; 3]`（或 `Vec`），调用点不需要改一行」。
**副职授予天赋的接入点已经被代码显式预留并命名了**——三节提的 `SubclassDef.traits` 字段，落地时
的形状不再是「照着种族那一路照抄一份」，而是「给 `SubclassTable` 补一个 `impl TraitGrantSource`，
再往 `agent_trait_sources` 里加一路」。

**但有一处结构差异必须点出来**：种族与职业都是 `Agent` 上的**单值**字段（`race`/`profession`），
`TraitSource::new(owner, grants)` 因此一路一个 `ContentIndex`。**`Agent.subclasses` 是 `Vec`**——
一个角色持有 N 个副职就要展开 N 路 `TraitSource`。这正是 `agent_trait_sources` 文档里
「`Agent::subclasses` 是一个集合而非单值」那句话指的东西，也是它的返回类型将来必须从定长数组变成
`Vec` 的原因。**五节的上限（2~3）在这里有了一个本文档冻结时没有的、代码层面的支撑理由**：
返回值长度 = 2 + 副职数，上限让这个长度保持在小常数量级。

**两道闸都不依赖对方**：`traits` 字段生效不需要 `skill-requires!` 落地（拥有副职即刻生效，见六节
`effective_traits` 的并集聚合），`skill-requires!` 生效也不需要 `traits` 字段（它只读
`agent.subclasses` 成员关系）。这意味着副职系统即使在 `skill-requires!`/`resolve_learn_skill`
仍未落地的情况下，只要 `traits` 字段+其消费路径落地，就已经能产出可感知的游戏效果——不必等待
更大的技能学习管线整体完工才有价值，见五节「最小可用集」的依赖排序。

**【第二次订正】这段结论现在有了第三条独立通路，而且它是三条里唯一已经通了的**：
`RecipeCategoryDef.required_subclasses` 读 `agent.subclasses`（`08cdeb0`）——**既不需要
`skill-requires!`，也不需要 `traits`**。制作类副职因此可以在另外两条闸都没落地的情况下产出
完整的游戏效果，只要获得机制落地就行。这把「副职系统最短的可交付路径」缩短了一大截，
是六节新排序的直接依据。

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
    /// 天赋（纯粹作为技能学习或制作类别的资格闸门存在，例如"学徒"这类
    /// 通用副职，以及所有只靠 required_subclasses 起作用的制作类副职）。
    pub traits: Vec<TraitGrant>,
}
```

**只加这一个字段**。理由回应框架第二节的红线：

- **不加任何数值字段**（不加属性修正、不加资源池容量、不加护甲加成……）——那些是 `TraitDef` 的职责，
  `SubclassDef.traits` 只是一份引用列表，真正的效果载荷全部在 `TraitTable` 里，副职本身零存储数值。
  **【第二次订正】这条红线本批次抓到了一次真实的违反**：协调者名册里的「驮运」全部内容是一个
  负重上限数字，按这条红线它不是副职，是一条 `TraitGrant`，见一之二⑨。
- **不把获得条件放在 `SubclassDef` 上**——四节会说明，获得条件是一套独立的声明（照抄
  `skill-requires!` 不与 `register-skill` 合并的先例），不塞进 `register-subclass` 的参数列表，
  理由同该文档六节：分类展示与强制闸门是两件独立的事，混在一起会在将来某天有人想"只展示不强制"或
  "只强制不展示"时变成一处隐藏耦合。**【第二次订正】这条先例现在有了一个已落地的同款证据**：
  `recipe-category-requires-subclass!` 就是注册后追加的独立函数，没有挤进
  `register-recipe-category` 的位置参数（`crates/ll-mod/src/recipe_category.rs:126` 原文：
  「`required_subclasses` 恒以空列表开始」）。
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
   已经落地，用一条任务节点触发"获得副职"在**技术上**完全可行，**但它不是本文档要选的默认路径**：
   任务本质是"被安排好的一次性关卡"，与"用什么练什么"的连续成长感不是同一种体验——任务更适合当
   **触发某个特定副职的入口叙事包装**（比如"帮草药商跑一趟腿"解锁炼金学徒身份的第一步），
   而不是承担整条成长曲线。**结论：任务不是主机制，但不排斥某个具体副职的内容设计里用一条任务
   节点作为"报名"门槛，叠在使用计数之上**——两者不互斥，本文档的最小形状默认不需要任务参与。
3. **使用计数（做某件事满 N 次自动获得）**——**采纳**。这是唯一直接满足框架红线"成长挂钩必须是
   玩家已经在做的动作"的方案：玩家不需要知道"我在攒炼金副职的进度"，捡材料、杀敌、探路这些动作
   本身就是游戏其余部分要求玩家做的事，副职只是在这些既有动作上"顺手"累加。

**【第二次订正】第 2 条现在有了一条本文档当初没想到的用途**：一之三④第 1 条指出，NPC 永远不会
提交 `Intent::Craft`，因此使用计数对 NPC 结构上无效。**「世界生成或职业注册时直接写死一份初始
`subclasses`」是 NPC 唯一可行的路径**，它与任务授予共用同一个效果（下面新增的
`Effect::GrantSubclass`）。这不推翻「使用计数是玩家的主机制」，只是补上另一半人口。

**成本评估：中等，不是从零发明，是复制一个已经跑通的模式。**

关键证据——`crates/ll-sim/src/quest.rs:229` 的 `kill_progress_effects` 已经是一个**完整落地**的
"使用计数 + 达标授予"实现：

```rust
pub fn kill_progress_effects(world, actor, killed_kind, quests) -> Vec<Effect> {
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
  "标记任务完成"换成"标记副职获得"，把"触发点"从 `Effect::Kill` 扩展到另外几个既有结算点。

### 【第二次订正】漏掉的那个效果：`Effect::GrantSubclass`

上面那句「换成『标记副职获得』」**从来没有给这个效果一个名字，也没论证它必须存在**——
五节命名了 `Effect::RemoveSubclass`，成对的授予效果反而漏了。补上：

```rust
// Agent.subclasses 是 WorldState 的一部分，不是 script_state。
// ADR 0023「脚本状态写入必须过 apply」的同一条纪律在这里意味着：
// 这个写入不能塞进 Effect::SetScriptState（那只写 script_state），
// 必须是一个独立的 Effect 变体，由 apply 写进 Agent.subclasses。
Effect::GrantSubclass { actor: EntityId, subclass: ContentIndex }
```

**它必须做一次去重**（`Agent.subclasses` 是 `Vec` 不是集合，重复授予同一个副职会让
`contains` 判定仍然正确但存档里多一份垃圾），以及**一次上限检查**（五节）。两件事都放在
`apply` 里，与 `Effect::RemoveSubclass` 的 `retain` 对称。

**这个效果同时是三条授予路径的唯一出口**：使用计数达标、任务节点授予、世界生成/职业注册时的
初始副职。三条路径不需要各自的效果——**这一处才是 ADR 0021 那条「不要把同一份算法复制三遍」
真正适用的地方**（不是副职名册那边）。

### `SubclassUnlock` 的形状

```rust
/// 一条"做过什么、多少次，就能获得这个副职"的声明。与 SkillRequirement
/// 是两个独立的闸——SkillRequirement 回答"已经拥有副职后能不能学某个
/// 技能"，本类型回答"副职本身怎么拿到"。
pub struct SubclassUnlock {
    pub subclass: ContentIndex,        // 指向 SubclassTable，必须已注册
    pub trigger: SubclassUnlockTrigger,
    pub threshold: u32,
}

/// 【第二次订正后的变体集合】只列六节最小可用集需要的三种，
/// 不为已推迟的方向预先造变体，YAGNI。
pub enum SubclassUnlockTrigger {
    /// 累计制作某个配方类别的成品达到 threshold 次——挂载点是
    /// crafting-system.md 八节的 craft_progress_effects，结构照抄
    /// kill_progress_effects。ContentIndex 指向 RecipeCategoryTable
    /// （不是某一条具体配方），让键空间保持在"类别数量"这个小量级上。
    /// 工匠/裁缝/调剂三个制作副职共用这一个变体，各自填不同的类别。
    ItemsCrafted(ContentIndex),
    /// 累计采得某个物品类别达到 threshold 次——挂载点有两个而不是
    /// 一个：resolve_pick_up（拾取，草药）与 resolve_loot（剥取尸体）。
    /// 两个函数各接一次同构的计数产出，写同一个键。
    ItemsGathered(ContentIndex),
    /// 累计完成休息达到 threshold 次——挂载点 resolve_rest/resolve_wait
    /// （已落地），同样是一次同构的计数产出。
    RestsTaken,
}
```

**【第二次订正】相对冻结版删掉的三个变体，逐条给理由**：

- **`Kills(ContentIndex)` 删除**——它冻结时只为「殡葬/掘尸」一个方向存在，而一之二④把殡葬的挂钩
  从「杀敌」改到了更贴切的「剥取」（`resolve_loot` 已落地）。**没有任何副职再用它，按 YAGNI 删。**
  保留它的代价不是零：多一个变体就多一处要在 `subclass-unlocks-via!` 里解析的字符串、多一处
  注册期校验分支。将来若真需要一个战斗侧副职，`kill_progress_effects` 的挂载点还在，加回来很便宜。
- **`ItemsPickedUp(ContentIndex)` 改名并扩容成 `ItemsGathered`**——第一次订正已经把炼金从它改挂到
  `ItemsCrafted`，本次核实发现 `resolve_loot` 也已落地，两个挂载点该写同一个计数。
- **`TilesExplored` 删除**——两个理由。①它依赖的「已探明格数」方法**不存在**（一之二⑥），
  真正常数级的是 `visited_zone_count()`，所以正确的变体名是 `ZonesVisited`；
  ②`MarkExplored` 只对玩家产出，这一档结构上只对玩家有效。**斥候不在六节的最小可用集里，
  所以这个变体按 YAGNI 现在不造**；将来造它时用 `ZonesVisited` 这个名字，并在文档里写明玩家专属。

### 计数的键：**不要**照抄 `kill_count_key`，它有一处存档隐患

本批次逐行核实（不采信 `crafting-system.md` 的转述）：

- `crates/ll-sim/src/quest.rs:206`：`fn kill_count_key(kind) -> String { format!("{KILL_COUNT_KEY_PREFIX}{}", kind.get()) }`
  ——把 `ContentIndex` 的**数值**拼进键。
- `crates/ll-content/src/remap.rs:343`：`remap_agent` 的解构里是 `script_state: _,`
  ——**明确不参与存档重映射**。

两条合起来：**mod 集合变化导致索引重编号后，这些键会静默指向别的内容。** 这是既有击杀计数就有的
隐患，不是新引入的，但**新造的计数不该再抄一遍。**

**副职的全部计数键一律用 `NamespacedId` 字符串拼**：`"craft_count:lostland:forging"`、
`"gather_count:lostland:herbs"`。命名空间标识符跨 mod 集合变化保持稳定，天然免疫重编号。

**这不需要给计数函数传一份 `Registry`**——`QuestCatalog::kill_count_quests()` 已经示范了正确
做法：需要反查的 `NamespacedId` 在 **catalog 构建期**一次性解析好，随规则一起返回
（`crafting-system.md` 八节给出了 `CraftUnlockRule` 的完整形状）。

**这条纪律写在副职文档里，是因为实现它的人会先读副职文档**——隐患藏在
`crates/ll-content/src/remap.rs` 里，不专门去看不会撞上，而撞上之后的表现是「玩家装了个 mod，
副职进度悄悄跑到别的类别上去了」，属于最难被测试发现的那一类缺陷。

### 注册函数

`(subclass-unlocks-via! subclass-id trigger-kind trigger-target threshold)`——一档声明式
（ADR 0016/0017），独立函数，不与 `register-subclass` 合并，理由同 `skill-requires!` 六节
「分类展示与强制闸门是两件独立的事」。签名细节见七节。

**代价诚实标注**：三个变体的挂载点（`resolve_craft`/`resolve_pick_up`+`resolve_loot`/`resolve_rest`）
**全部已落地**，每个变体需要新增一个与 `kill_progress_effects` 同构的六行计数函数，
接进对应 resolve 的效果产出——这不是新范式，是把已经验证过的模式再抄三份，成本可控但不是零。
**加上 `Effect::GrantSubclass` 与 `apply` 侧的去重/上限检查，这就是获得机制的全部工作量。**

---

## 五、有没有上限——结论：有，一个小整数，且必须配放弃机制

**结论：需要上限。** 理由不是「无限累积会让存档变大」这类工程借口，是框架自己在六节给出的判据本身
要求稀缺性：**「build 多样性来自直觉搭配 vs 错位搭配的化学反应」这条价值主张，前提是副职是稀缺资源**。
若使用计数机制跑得足够久，任何一个角色理论上能集齐全部副职——那时候「法师配驯兽」不再是一个需要
取舍的选择，只是「反正迟早都会有」的时间问题，错位搭配产生的化学反应就被拖成了必然结果，框架想要
的「取舍」张力消失。

**【第二次订正】这条结论现在有了第二条、独立于游戏体验的支撑理由**：见二节末尾——
`agent_trait_sources` 的返回值长度是「2 + 副职数」，上限让它保持在小常数量级，
是它能从定长数组平滑长成 `Vec` 而不引入逐帧分配的前提。**两条理由指向同一个数值区间，
这是一个好信号，不是巧合**：稀缺的东西本来就该少到能放进一个小数组里。

**上限的具体数值不是本文档要拍板的内容设计参数**——结构上"是一个小整数"就够，建议区间 2~3
（1 太严格，等于没有搭配空间；4 及以上稀缺感明显减弱），具体数值留给内容设计阶段按实际游玩节奏微调。

**放弃机制**（上限存在就必须有对应的放弃路径，否则玩家攒到上限后系统直接锁死，无法再体验任何新
副职带来的搭配变化）：

```rust
// 新增一个与既有移除类操作同构的效果——Agent.subclasses 是 Vec，
// 移除是一次 retain，不需要复杂的清理逻辑。
// 与四节新增的 Effect::GrantSubclass 成对。
Effect::RemoveSubclass { actor: EntityId, subclass: ContentIndex }
```

**放弃不追溯**：与「可学」和「可用」是两件事（`skill-learn-requirements.md` 五节）同一条纪律——
副职闸门只在"写入 `unlocked_skills` 之前"判定一次，一旦某个技能已经学会，它此后是否仍然满足
`SkillRequirement` 与"能不能用"无关。放弃一个副职，**不会**让已经通过它学会的技能变得不可用，
只是（a）关闭了继续用这个副职学新技能的入口，（b）`effective_traits` 聚合里不再包含它授予的天赋
（天赋本身零存储，纯派生，见 `trait-system.md` 八节，放弃后下一次聚合直接不再读到），
（c）腾出一个上限槽位供下一次使用计数达标时授予新副职。

**【第二次订正】「放弃不追溯」有一处例外必须写明，因为制作闸门已经落地**：
`resolve_craft` 的副职闸门**是每次制作都判一遍的**（`crates/ll-sim/src/resolve.rs:2340`，
不是「第一次做的时候判一次然后记下来」）。**所以放弃工匠之后，工匠类别的配方立刻做不了了**——
这与技能那一路的「学会了就永远能用」是相反的行为。**这不是缺陷，是两种闸门的语义本来就不同**
（`SkillRequirement` 闸的是「获得一个永久能力」，`required_subclasses` 闸的是「执行一次动作」），
但它必须写下来，否则玩家会以为放弃副职只是失去学新东西的资格。**并且它给了放弃机制一个真实的
代价**，不需要额外设计任何惩罚数值。

**放弃本身不设代价**（不扣经验、不设冷却）——保持与"副职不给数值"同一条极简纪律：加代价意味着
要为这个代价设计数值，那是本文档明确要避免的范围扩张。上一段说的「制作立刻做不了」不是设计出来
的代价，是闸门语义的自然后果。

---

## 六、最小可用集与全名册排序

### 冻结版结论（`61d2e73`）：测绘/制图 + 殡葬/掘尸 + 炼金/草药，三个

**排序**（按"成长挂钩成本 + 依赖是否已落地"从低到高，一节候选表核实的直接结论）：

1. **测绘/制图**——零新增存储，`TilesExplored` 直接读现有位图。
2. **殡葬/掘尸**——复用 `kill_progress_effects` 现成挂载点，与经验系统同一先例。
3. **炼金/草药**——`RecipeDef` 已定形，只需新增一个与击杀计数同构的拾取计数函数。
4. 求生/野营——休息已落地但计数需新增，成本中等。
5. 厨艺——同炼金但价值打折（食物系统已裁定配方不设门槛，副职收益只剩技能闸）。
6. 驮运——YAGNI 暂不需要，但标注为"最省事复活 `base_weight` 死字段"的候选，供未来参考。
7. 鉴定/秘术——需要新 `Intent` + 触及世界生成路径，成本高于 1~5。
8. 工匠/锻造——成长挂钩动作本身因果倒置（一节已论证），需要先设计"修理"这个新机制才谈得上。

**【2026-08-22 订正：3/5/8 三项的排序与理由改写】** 见一节「订正」小节。制作系统
（`crafting-system.md`）把工匠/裁缝/炼金/厨艺四个方向收敛到**同一个挂钩**
（`ItemsCrafted(配方类别)`）与**同一笔投资**。9~12 位骑术/斥候/驯兽/商贾阻塞于载具/陷阱/同伴/交易
四个尚未落地的系统，标为扩展。

### 【第二次订正】全名册重排——上面整份排序作废，理由是它的两条排序轴都失效了

冻结版按「成长挂钩成本」排。**制作系统落地之后，制作类三个副职的闸门已经是真代码，而其余方向的
闸门一个也没有**——「闸门是否已落地」现在是一条比「挂钩成本」更有区分力的轴。新排序按两轴：
**资格闸门是否已落地** + **成长挂钩的挂载点是否已落地**。

**第 0 梯队——共同前置，不是一个副职**：`SubclassUnlock`/`SubclassUnlockTrigger`/
`Effect::GrantSubclass`（四节）。**九个副职一个也绕不开它**，见一之三⑤。

**第 1 梯队——闸门已是真代码，只差成长挂钩**：

1. **工匠**、2. **裁缝**、3. **调剂**——`resolve_craft` 的 any-of 闸门已落地（`08cdeb0`），
   三者共用 `ItemsCrafted(类别)` 一个变体、一个 `craft_progress_effects`。
   **三者是一份实现，所以在最小可用集里排除其中两个省不下任何东西。**
4. **医疗**——`Intent::UseSkill { target }` + `SkillEffect::RestoreResource` 已经能治疗别人
   （一之三③核实）。它的闸门这一路走 `SubclassDef.traits` 授予治疗技能，与制作三个走
   `required_subclasses` 不同，但同样不缺代码。缺的同样只有计数。

**第 2 梯队——挂载点已落地，闸门内容待定**：

5. **求生**——`Intent::Rest` 单点，且 `c12c04f` 之后有真实压力（低温/保暖）。
6. **采集（草药 + 剥取两半）**——`resolve_pick_up`/`resolve_loot` 两个已落地函数。
   注意是两个挂载点，不是一个。
7. **斥候（测绘那一半）**——`visited_zone_count()` 已落地且真的是常数级，
   但**只对玩家有效**（`MarkExplored` 玩家专属）。

**第 3 梯队——阻塞在一个尚未落地的系统上，但形状清楚**：

8. **营造**——差一个 `Intent::Build`；`Effect::SetTerrain` 与 `buildable`/`diggable` 已就位。
9. **采集（采矿那一半）**——差 `resolve_mine`；届时并入第 6 项零代价。
10. **驭兽（驯兽）**——差同伴/态度系统整套。
11. **行商（商贾）**——差 `Intent::Trade`/`resolve_trade`；`Effect::AdjustWallet` 已就位但无产出者。

**第 4 梯队——连挂钩动作本身都还没被发明**：

12. **学者**——「阅读/研究」在引擎里没有对应的 `Intent`，且**没有任何已排期批次会顺手造出它**；
    其三个来源（鉴定/博识/科研）是三个互相独立的未落地批次，科研还压着一条与食物系统五节的
    未裁定冲突（八节①）。

**未列入名册、需要项目所有者知道的三项**：

- **种植**——不是「采集的一个子项」，是一个未立项的系统（一之二④）。单列，不给承诺。
- **驮运**——违反二节红线，改记为「行商将来可以通过 `traits` 授予一条提升负重的天赋」（一之二⑨）。
- **骑术**——按所有者原话分裂：活物载具的门归驭兽，造物载具的门归工匠/营造（一之二⑦）。不单列。

### 新的最小可用集：采集 + 工匠 + 裁缝 + 调剂 + 求生，五个副职、三个触发器变体

**为什么是五个而不是三个**：工匠/裁缝/调剂**共用一份实现**（同一个 `ItemsCrafted` 变体、同一个
`craft_progress_effects`），排除其中两个一行代码都省不下来，只会让最小可用集少两个可玩的方向。
**「最小」的对象是实现工作量，不是名册长度。**

**触发器变体仍然是三个**（`ItemsCrafted`/`ItemsGathered`/`RestsTaken`），与冻结版数量相同——
换掉了全部三个，但工作量同级。

**为什么这五个是"一个完整的补给循环"**：框架第一节定的正交轴是"你怎么在战斗之外补给自己"。
五者恰好覆盖循环的三个环节，且**每一个的挂载点都已经是真代码**：

- **采集**（拾取 + 剥取）——原材料进来。`resolve_pick_up`/`resolve_loot` 已落地。
- **工匠 / 裁缝 / 调剂**（制作）——原材料变成装备、衣物、药水。`resolve_craft` 与副职闸门已落地。
- **求生**（休息）——把补给品换成继续走下去的能力，且 `c12c04f` 之后有真实的低温压力逼着玩家
  要有一件保暖衣物——**这一条恰好把裁缝的产出接回了循环**，是冻结版三件套（测绘/殡葬/炼金）
  没有的闭合。

五者互相独立、不共享代码路径（各自的 resolve 挂载点确实不同），但拼起来覆盖了
「采材料 → 做东西 → 活下去 → 再去采」这条闭环，**不需要发明任何一个新的游戏动作**。

**为什么冻结版的「测绘/制图」掉出了最小可用集**：两个理由，都是本批次核实出来的——
它依赖的「已探明格数」方法不存在，而它真正可用的替代（`visited_zone_count`）只对玩家有效。
它没有变差，只是不再是「零成本」，而其余五个现在都有已落地的挂载点。

---

## 七、`register-subclass` 的签名与档位

**现有签名不变**：`(register-subclass id display-name-key)`——`SubclassDef` 加 `traits` 字段
不改这个签名的参数个数,理由同 `class-skill-quest-system.md` 对 `register-skill` 的处理：
`traits` 走独立函数，不塞进 `register-subclass` 的位置参数列表。

**新增两个独立函数,均为一档（ADR 0016/0017：声明式,注册期物化,运行期查表,零脚本回调）：**

```scheme
(register-subclass-trait subclass-id trait-id)
(subclass-unlocks-via! subclass-id trigger-kind trigger-target threshold)
```

**【第二次订正】第一个函数改名了。** 冻结版写的是 `subclass-grants-trait!`。本批次核实
`crates/ll-mod/src/**` 全部三十余个已注册脚本函数名之后发现，**已落地的代码里有两条并行的命名
惯例，而冻结版那个名字两条都不属于**：

| 惯例 | 语义 | 已落地实例 |
|---|---|---|
| `register-<主体>-<附加物>` | 给一份已注册的定义**追加一条声明** | `register-class-trait`、`register-race-trait`、`register-race-starting-item`、`register-item-stat-bonus`…… |
| `<主体>-requires-<条件>!` | 给一份已注册的定义**追加一道闸/前置** | `recipe-requires-station!`、`recipe-requires-tool!`、`recipe-category-requires-subclass!` |

「副职授予一个天赋」与 `register-class-trait`/`register-race-trait` **是完全同一件事的第三个
来源**（三者都是 `TraitGrantSource` 的实现方），**必须叫 `register-subclass-trait`**——
用一个新名字会让「哪三路来源在授予天赋」这件事在脚本层面看起来像三件不同的事。

`subclass-unlocks-via!` 保留 `!` 后缀：它给一份已注册的副职追加一道获得条件，落在第二条惯例里。

- `register-subclass-trait`：`subclass-id`/`trait-id` 均须已经 `register-subclass`/`register-trait`
  注册过（跨表存在性校验,与 `skill-requires!` 六节"必须是已注册技能"同一条纪律）。可对同一个
  `subclass-id` 多次调用,每次追加一条 `TraitGrant { unlock_level: 1 }` 到 `traits` 列表——不做
  去重校验（与已落地的 `register-race-trait`/`register-class-trait` 行为一致,重复授予同一个天赋
  两次在 `effective_traits` 的并集聚合层是幂等的,`crates/ll-sim/src/traits.rs:254` 的
  `!result.contains(&grant.trait_id)` 已经去重,不需要在注册期拒绝）。
  **注意与职业那一路的一处签名差异**：`register-class-trait` 收三个参数（多一个 `unlock-level`），
  副职这一路**只收两个**——副职没有等级，`unlock_level` 恒为 1（二节，与种族/装备/buff 同）。
- `subclass-unlocks-via!`：`trigger-kind` 是 `"items-crafted"`/`"items-gathered"`/`"rests-taken"`
  三选一字符串（对应四节订正后的 `SubclassUnlockTrigger` 三个变体，`"rests-taken"` 时
  `trigger-target` 参数被忽略——不强制传空字符串还是直接省略参数,留给实现阶段按 Steel 现有变参
  惯例决定）；`threshold` 是达标次数。同一个 `subclass-id` 只允许注册一条 `SubclassUnlock`——与
  `register-class-xp-curve`"一个职业只能绑一条曲线"同一条纪律,一个副职有多条互相竞争的解锁路径
  会让"我还差多少"这句 UI 文案没法唯一地展示。

档位判据（三步）：有自由度（mod 能声明任意天赋授予/解锁条件）；自由度落在纯数据上（阈值/触发类型
都是注册期定死的值,运行期只做整数比较,不消费脚本回调）；调用频率——达标判定发生在每次相关动作
结算时（制作/拾取/剥取/休息),属于常规结算路径,量级与既有击杀计数同一档,必须一档。

---

## 八、仍然悬着的冲突（记录，不代项目所有者裁决）

**本节新增于第二次订正。** 四条都不是本文档能解决的，记在这里是为了它们不再被反复重新发现。

### ① 「科研」与食物系统五节「菜谱全部已知、不设解锁门槛」直接冲突

`food-and-cooking-system.md` 五节裁定「任何角色只要凑齐食材、提交 `Intent::Craft`，就能做出对应
菜谱——不需要『学会』这一步」。项目所有者点名的「科研」方向本质就是给配方加解锁门槛。

**目前没有触发冲突**，因为 `crafting-system.md` 十四节①把两件事拆开了：**类别访问权**
（`required_subclasses`，已实现，读 `Agent.subclasses`）与**配方解锁**（`known_recipes`，未实现）。
本文档的九个副职**没有一个依赖配方解锁**。

**但冲突本身还在**：一之二⑤把科研并进了学者。**学者副职如果要有科研这一半，就必须推翻食物系统
五节那条裁定，或者把科研限定在非食物类别上。** 本文档不代为决定，只指出它是学者排在第 4 梯队的
理由之一。

### ② 裁缝的结构性不对称

完整分析见一节末尾「隐忧」小节（已按保暖落地重写）。一句话结论：**保暖落地（`c12c04f`）给了裁缝
一条真实的首次需求，但没给它重复需求；三条候选补法里已落地一条、卡在别的批次一条，
剩下唯一需要裁定的是「衣物要不要有耐久」。** 另有一条不需要裁定、今天就能做的第四条补法
（按气候带分化衣物需求）。

### ③ 技艺浮动与工具磨损

`crafting-system.md` 九节⑦「产出数量/品质随属性或技艺浮动」与本文档二节「副职不给数值」直接
冲突——「锻造技艺 87 点」在当前设计里没有合法的存放位置。九节⑩「工具因制作而磨损」与所有者
「只有装备武器才有耐久」的裁定直接冲突。

**本批次核实**：`resolve_craft` **确实没有产出任何 `Effect::AdjustEquipmentDurability`**——
工具前置只查「装着且耐久不为 `Some(0)`」（`crates/ll-sim/src/resolve.rs:2359`），做完不磨损。
两条冲突都仍然悬着，都需要所有者裁定。

### ④ 抗性跨来源 tie-break 按 `origin` 升序

`13eea2d` 落地的 `crates/ll-sim/src/rule_modifier.rs` 里，多来源抗性的 tie-break 按 `origin` 升序，
只反映注册顺序不反映强弱。与副职无直接关系，但**会影响将来「副职授予抗性天赋」这条路径的价值**：
若一件装备与一个副职天赋给同一种抗性，谁赢取决于注册顺序而不是设计意图。待裁定。

---

## 相关文档

- `knowledge/design/class-skill-quest-system.md`——`ClassDef`/`SkillDef`/`SubclassDef`/`QuestNodeDef`
  的原始基线设计，零节已指出其「落地状态」记录过期
- `knowledge/design/skill-learn-requirements.md`——`SkillRequirement`/`skill-requires!` 的完整设计，
  二节直接引用其「可学」闸门现状与「可学/可用是两件事」的纪律
- `knowledge/design/trait-system.md`——`TraitDef`/`TraitGrant`/`effective_traits` 聚合，二节/三节
  直接复用其「有效技能=并集」「拥有即生效」两条既有结论，不重新设计
- `knowledge/design/food-and-cooking-system.md`——`RecipeDef`/`Satiety`；五节「菜谱不设门槛」是
  八节①冲突的一方，`Satiety` 零落地是一之二③合并论证的依据
- `knowledge/design/crafting-system.md`——制作系统（配方类别、`Intent::Craft`、场地/工具前置、
  副职闸门）；本文档第一次订正的来源，`SubclassUnlockTrigger::ItemsCrafted` 的完整论证，
  二节是「ADR 0021 双向性」的原始论证
- `knowledge/design/vehicle-and-mounting.md`——项目所有者关于载具的原话，一之二⑦拆分骑术的依据
- `knowledge/design/resource-pools-and-rest.md`——`Intent::Rest` 与资源池，六节求生与一之三③医疗
  的地基
- `knowledge/design/level-and-experience-system.md`——「经验挂 `Effect::Kill`，不新开事件」的先例，
  四节使用计数机制直接类比
- `crates/ll-mod/src/subclass.rs`/`script_subclass_api.rs`——`SubclassDef` 现状代码
- `crates/ll-mod/src/recipe_category.rs`/`script_recipe_category_api.rs`——
  `RecipeCategoryDef.required_subclasses` 与 `recipe-category-requires-subclass!`，
  `Agent.subclasses` 的第一个真实消费者的注册侧（一之四节）
- `crates/ll-sim/src/resolve.rs`——`resolve_craft` 的副职闸门（2340 行）、`resolve_loot`（1795 行）、
  `resolve_use_skill` 的 `effect_target`（3310 行）、`MarkExplored` 玩家专属（1698 行）
- `crates/ll-sim/src/traits.rs`——`TraitSource`/`agent_trait_sources`/`effective_traits`，
  二节第二次订正的核实来源，副职这一路的接入点已在其文档里显式预留
- `crates/ll-sim/src/rule_modifier.rs`——多来源规则修正聚合点（`13eea2d`），八节④
- `crates/ll-sim/src/exposure.rs`——温度/保暖（`c12c04f`），一节「隐忧」与六节求生的依据
- `crates/ll-mod/src/quest.rs`/`crates/ll-sim/src/quest.rs`——`kill_progress_effects` 现状代码，
  四节使用计数机制的直接复制来源，以及 `kill_count_key` 那处存档隐患的原文
- `crates/ll-content/src/remap.rs`——`script_state: _`（343 行，计数键隐患的另一半）与
  `remap_subclasses`（612 行，合并副职不可逆的证据）
- `crates/ll-world/src/script_state.rs`——`Agent.script_state`/`ScriptValue`，四节计数存储通道
- `crates/ll-world/src/exploration.rs`——`ExplorationMemory` 的真实公开接口，一之二⑥的核实来源
- `crates/ll-mod/src/script_space_profile_api.rs`——`buildable`/`diggable` 两个死字段的原文注释，
  一之三②的核实来源
- `knowledge/decisions/0016-mod-performance-tiers-by-declaration.md`/
  `0017-tiered-declarations-materialize-columnar.md`——七节档位判据
- `knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md`——一之二节
  「这条判据管代码不管内容」的原文依据
- `knowledge/decisions/0023-script-state-writes-go-through-apply.md`——四节
  `Effect::GrantSubclass` 必须是独立效果变体的依据
