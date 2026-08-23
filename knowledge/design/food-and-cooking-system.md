# 食物与烹饪系统：食材、菜谱、烹饪与饱食度

**落地状态**：纯设计。`crates/**` 全代码库检索无 `RecipeDef`/`register-recipe`/`Intent::Craft`/`Agent.satiety`/`ResourceKind::Satiety` 等任何匹配。本文档要建在其上的地基（`ItemDef`/`ItemStack`/`Intent::Use`/`use_effect: Option<SkillEffect>`/`Effect::ConsumeInventoryItem`/`Effect::MergeIntoInventory`/`Effect::ApplyStatModifier`/`Agent.active_stat_modifiers`）**已经实装**，不是纯设计——见一节逐项核实。

**冻结于** 2026-08-21，基线提交 `6bb0cb3`（`main` 分支，P6 第五批「耐久消耗与 `Intent::Use`」收尾之后）。

**并发声明**：写作时工作区里另有一路并行工作未提交（`crates/ll-sim/src/item.rs`、`crates/ll-sim/src/resolve.rs` 有未暂存改动，给攻击接武器引用，与本文档主题无关）。本文档不触碰 `crates/**`/`mods/**`/`assets/**` 任何路径，只新增本文件 + 更新 `README.md` 索引里属于本文档的那一行。

---

## 零、项目所有者的要求

> 「我突然想到需要食物系统，烹饪物品，菜谱之类的东西需要被设计出来」

没有更多细节——本文档需要自己把范围收窄到一个能验收的最小形状，见十节「不设计过头」的取舍。

---

## 一、现状核实：能在什么地基上建

物品系统是**已实装**的代码，不是设计文档里的形状：

- `ItemDef`（`crates/ll-mod/src/item.rs`）：`stack_limit`/`base_weight: Milli`/`base_price: Milli`/`max_durability: Option<i32>`/`equip_mask: SlotMask`/`stat_bonuses: Vec<StatBonus>`/`use_effect: Option<SkillEffect>`，`ItemTable::define` 注册期完整校验（ADR 0017）。
- `ItemStack`（`crates/ll-world/src/item.rs`）：`def: ContentIndex`/`count: u32`/`durability: Option<i32>`，`can_merge`/`merge_stacks`/`split_stack` 三个纯函数。
- `Agent.inventory: Vec<ItemStack>`、`WorldState.ground_items`（`crates/ll-world/src/state.rs`）。
- `Intent::PickUp`/`Drop`/`Equip`/`Unequip`/`Use`（`crates/ll-sim/src/intent.rs`）全部有对应的 `resolve_*` 与测试覆盖。
- `resolve_use_item`（`crates/ll-sim/src/resolve.rs:1527`）：查背包里是否有这个 `def`、查 `ItemCatalog` 拿 `use_effect`，产出 `Effect::ConsumeInventoryItem` + 按 `SkillEffect` 三个变体各自的效果。**吃东西这件事本身已经能做**——`register-item-use-effect "id" "restore-resource" "stamina" 20 0` 今天就能注册一颗「吃了回体力」的东西，`mods/example_mod/gameplay.scm:194` 有真实先例（`healing_potion` 用同一条链路回法力）。

**这条核实直接决定了本文档的范围**：吃东西（消耗 1 件物品、触发效果）已经不需要重新设计，本文档只需要解决两件新事——①东西从哪来（菜谱/烹饪），②长期不吃东西会怎样（饱食度）。下面二、三节分别处理。

**资源效果现状**（`crates/ll-sim/src/skill.rs`）：

```rust
pub enum ResourceKind { Mana, Stamina }              // 封闭枚举，只两个变体
pub enum ResourceCost { None, Amount(ResourceKind, u32), ... }
pub enum SkillEffect {
    DealDamage { base: i32 },
    RestoreResource { resource: ResourceKind, base: i32 },
    TemporaryStatModifier { attribute: AttributeKind, amount: i32, duration_ticks: u32 },
}
```

`Agent.mana: i32`/`Agent.stamina: i32`（`crates/ll-world/src/entity/agent.rs`）：两个专用字段，只存当前值，**不设上限，不做任何钳位**——`crates/ll-sim/src/apply.rs:162` 的 `Effect::AdjustResource` 分支原样 `agent.mana += delta`/`agent.stamina += delta`，没有 `min`/`max`。这条「无钳位」的既有事实是二节饱食度设计的关键依据。

**`Agent.active_stat_modifiers: BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>` 与 `Effect::ApplyStatModifier { target, attribute, delta, expires_at, source }` 均已实装**（`crates/ll-world/src/entity/agent.rs:298`、`crates/ll-sim/src/effect.rs:299`）——`buffs-and-triggers.md` 六节提议的「按 `(属性, 来源)` 键控、同源刷新、异源叠加」存储改法在本次核实时点已经落地，不再是待办。这条现状核实改变了六节「饥饿的后果」一节的结论，见其中「如实核实」小节。

**另一个已实装的相关系统**——`Agent.unlocked_skills: Vec<ContentIndex>`（`crates/ll-world/src/entity/agent.rs:248`）：技能解锁集合，五节「菜谱发现」讨论要不要照抄它时会核对这个字段。

**空间查询现状**——`WorldState::terrain_at(pos: TorusPos) -> Option<TerrainKind>`（`crates/ll-world/src/state.rs:674`）**在 `resolve` 侧已经是被使用的能力**：`resolve_move`（`crates/ll-sim/src/resolve.rs:1629`）用它查目的地地形。这与 `trait-system.md` 三节③「盗贼偷袭」核实的「`resolve` 没有战场空间上下文可查」**不是同一件事**——那里缺的是查询「目标附近有没有己方**实体**」这类以任意实体为圆心的批量查询（`script-entity-handles-and-batch-queries.md` 的批量查询原语没有接进 `resolve`），`terrain_at` 查的是「给定坐标是什么地形」，是完全不同的、已经接好线的能力。四节「烹饪的场地需求」详细展开这条区分。

---

## 二、饱食度的形态：`ResourceKind` 新增变体，不进 `ResourcePoolDef`/`RegenRule`

**结论：饱食度是 `Agent` 上一个新的专用字段（`satiety: i32`），走 `ResourceKind` 新增的第三个变体，与 `mana`/`stamina` 同一条既有轨道——不是天赋授予的 `ResourcePoolDef`（`resource-pools-and-rest.md` 那套），也不是持续性 buff。**

### 为什么不是「资源池机制已经实装，直接复用」（否决项目所有者的原始倾向）

`ResourcePoolShape::Scalar` + `RegenRule` 那套（`crates/ll-sim/src/resource_pool.rs`）确实已经实装，形状上看起来很贴合「一个会自然下降的标量池」。**但核实之后发现两处硬性不匹配，不是审美偏好，是这套机制本身的前提条件不成立**：

**不匹配一：容量必须由天赋授予,查不到授予关系时容量恒为零。** 这套机制服务的是法力池/法术位这类**职业限定**的资源——`effective_scalar_capacity`（`crates/ll-sim/src/resource_pool.rs`）遍历 `effective_traits(agent)`，对没有命中任何 `ResourcePoolGrant` 的角色，容量就是零，这是设计目标本身（`resource-pools-and-rest.md` 三节：「没有授予对应天赋的角色就是没有这个池」）。饱食度必须是**人人都有**的属性，不能靠「每个种族的 mod 脚本都记得声明一条天赋授予」这种可能被忘记的路径去保证——种族作者忘了写这一条，这个种族的角色就永远不会饿，且不会有任何报错或提示,是一个静默的正确性缺陷,不是「暂时没有」这种可以后补的缺口。

**不匹配二：`RegenRule` 现在不能表达「每回合减少」，具体到代码层面已核实**（`crates/ll-sim/src/resource_pool.rs`）：

```rust
pub enum RegenRule {
    None,
    OnTurnStart { amount: u32 },   // amount 是 u32，恒为正
    OnRest { amount: RestRecoveryAmount },
}
```

`resolve_resource_pool_regen`（`crates/ll-sim/src/resolve.rs:671`）对 `OnTurnStart` 只产出 `Effect::AdjustResourcePool { delta: amount as i32 }`——**`amount as i32` 恒非负，没有任何路径能让这条效果的 `delta` 变成负数**。这不是「碰巧没写」的疏漏：`OnTurnStart` 这个名字与它唯一的调用场景（`resource-pools-and-rest.md` 四节「法力池：每隔固定 tick 数回复固定量」）从一开始就只考虑了「恢复」这一个方向。

**结论（直接回答项目所有者点名的问题）：`RegenRule` 今天不能表达「每回合减少」。若真要让饱食度走这套机制，需要给 `RegenRule` 新增一个变体（例如 `DrainOnTurnStart { amount: u32 }`），不能改造现有 `OnTurnStart` 的语义——`amount: u32` 现在是「填正数=恢复正数」这个契约，已注册的内容（若有）与阅读过这段代码的人都会假设这一点，静默把它变成可正可负或反过来理解成「消耗」是破坏性的语义变更，不是纯增量。**但即使加了这个变体，不匹配一（容量必须靠天赋授予）仍然成立、仍然否决这条路——两处缺陷各自独立,单独修一处解决不了整体的不匹配。**

### 为什么不是持续性 buff

buff 系统（`buffs-and-triggers.md`）解决的是「一个短暂效果,可能来自多个来源,需要同源刷新异源叠加」这类问题。饱食度只有一个「来源」——时间流逝本身,不需要多来源叠加的合并语义;也不是「短暂」的,是从游戏一开始就持续存在、没有到期时刻的状态,`ActiveEffect.expires_at` 这个核心字段对饱食度没有自然的取值(填「永不过期」是在滥用一个为「有限期」设计的字段)。更根本的是,buff 系统目前**没有**「持续每回合改变一个数值」这个通用能力可用(六节详细核实,这正是持续伤害至今零实现的原因)——套用一个自己都还没有这个能力的框架,不会让饱食度获得任何东西。

### 采用的形状：`ResourceKind` 第三个变体，`Agent.satiety` 专用字段

`mana`/`stamina` 已经证明了这条形状对「人人都有、无需天赋授予、只存当前值不设硬上限」这类需求成立——饱食度结构上与它们完全同构,直接复用同一条轨道,不新开一套:

```rust
// crates/ll-sim/src/skill.rs（现状）
pub enum ResourceKind { Mana, Stamina, Satiety }   // 新增第三个变体

// crates/ll-world/src/entity/agent.rs（现状）
pub satiety: i32,   // 新字段，紧邻 mana/stamina，同一条「只存当前值」纪律
```

`crates/ll-sim/src/apply.rs:162` 的 `Effect::AdjustResource` 分支多加一支：

```rust
crate::skill::ResourceKind::Satiety => agent.satiety += delta,
```

**这一步是唯一需要改的执行路径**——`Effect::AdjustResource { resource, delta }` 本来就允许 `delta` 为负（文档原文：「调整量，可正可负」），不需要新增任何效果变体。

**吃东西恢复饱食度**：`SkillEffect::RestoreResource { resource: ResourceKind::Satiety, base }` 直接复用——`register-item-use-effect "id" "restore-resource" "satiety" 40 0` 与今天注册 `"mana"`/`"stamina"` 是同一条脚本路径,只是 `parse_use_effect`（`crates/ll-mod/src/script_item_api.rs`）「restore-resource」分支的 `tag` 匹配列表要加一个 `"satiety"` 分支——**这是新增一个字符串标签能识别的取值，不是改 `register-item-use-effect` 的参数个数**,不违反「不改既有 `register-*` 参数个数」的先例。

**饱食度自然下降（衰减）**：**不经过 `RegenRule`**——因为饱食度根本不是 `ResourcePoolDef` 的实例（二节已论证）,`RegenRule` 这个类型挂在 `ResourcePoolDef` 上，饱食度从一开始就没有注册进那张表，谈不上「用不用得上 `RegenRule`」。衰减走一个新的、独立的 `resolve` 步骤，与 `resolve_resource_pool_regen`（`crates/ll-sim/src/resolve.rs:671`）**挂在同一个触发点**（`resolve.rs:617`,「结算一个实体的意图=这个实体自己的回合」,对全部 `Intent` 变体统一触发,不只是 `Wait`）,但**不查任何 `TraitCatalog`/`ResourcePoolCatalog`,对每一个实体无条件产出**:

```rust
// 设计形状，非最终实现——crates/ll-sim/src/resolve.rs 新增函数
fn resolve_satiety_decay(actor: EntityId) -> Vec<Effect> {
    vec![Effect::AdjustResource {
        actor,
        resource: ResourceKind::Satiety,
        delta: -SATIETY_DECAY_PER_TURN,   // 常量，示意值，非最终数值曲线
    }]
}
```

**不设硬上限，不做钳位**——与 `mana`/`stamina`/`health` 同一条既有纪律（`health` 原文：「可以为零或负……不在这个字段本身设下限」）。饱食度可以无限升高（暴食）或降到很低的负数（长期挨饿）,「这算不算问题」是读取时的规则判断,不是字段自身的约束——这条路径**完全不需要新增任何 `Effect` 变体**,只需要 `ResourceKind` 多一个变体、`apply` 的既有 `match` 多一支、`AttributeKind`/`ResourceKind` 相关的字符串解析多认识一个标签。改动面比二节开头设想的「资源池路线」小得多。

`WorldState::hash()` 插入点（`crates/ll-world/src/state.rs`，紧邻既有 `agent.mana`/`agent.stamina` 两行）：

```rust
hasher.write_i64(i64::from(agent.satiety));
```

---

## 三、食材、成品与 `RecipeDef`：只有菜谱是新类型

**结论：食材与成品都不是新类型，就是普通 `ItemDef`（用 `ContentIndex` 引用）——`RecipeDef` 是唯一需要新增的内容类型，装进新表 `RecipeTable`。**

### 为什么食材/成品不需要专门的类型（ADR 0021 检验）

ADR 0021 的判据只有一条：**有没有一份算法要被两种类型共用？** 检验「食材」要不要单独建类型：一把铁剑（装备）与一块生肉（食材）如果都只是「被 `RecipeDef.ingredients` 引用的 `ContentIndex`」，两者需要的操作完全一样——查 `ItemCatalog` 拿 `stack_limit` 判断能不能堆叠、查背包里有没有这个 `def`、够不够数量。**没有任何一段「食材专属」的算法**：不需要额外的腐败/保质期字段（十节已标为将来扩展）、不需要区分「能吃的」和「不能吃的」（这是 `use_effect: Option<SkillEffect>` 是否为 `Some` 已经回答的问题，不需要第二个字段重复表达）。给 `ItemDef` 加一个 `is_ingredient: bool` 或新开一个 `IngredientDef` 类型，只会在没有消除任何重复逻辑的情况下多出一层没有实际内容的类型区分——这正是 ADR 0021 要拦住的「看起来该对称」。

「成品」同理——一块烤熟的肉就是一件普通 `ItemDef`（`use_effect` 挂 `RestoreResource`），与箭矢、药水没有任何结构性差异，`RecipeDef.product: ContentIndex` 直接指过去即可。

### 为什么菜谱需要新类型

「N 种食材各自消耗若干数量、产出 1 种成品若干数量」——这条转换算法本身，任何一个既有类型都表达不了：`ItemDef` 描述的是「一件物品是什么」，不是「一件物品怎么从别的物品变出来」；`SkillEffect` 描述的是「使用一件已有的物品会发生什么」，同样不是「怎么获得这件物品」。这是一份**真正需要共享**的新算法（校验食材是否齐全、扣减食材、产出成品——五花肉烤肉与鹿肉炖菜共用同一段校验/扣减/产出逻辑，只是数据不同），ADR 0021 的判据在这里成立，值得开一个新类型。

### 形状

```rust
// crates/ll-mod/src/recipe.rs（新文件，照抄 item.rs 的注册表模式）
pub struct RecipeDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    /// 需要的食材与各自数量——恒非空（零食材的"菜谱"没有意义，
    /// register-recipe 注册期拒绝空列表，同 register-item 拒绝
    /// stack-limit 为 0 同一条"非法组合即拒绝"纪律）。
    pub ingredients: Vec<RecipeIngredient>,
    /// 产出物品，指向 ItemDef。
    pub product: ContentIndex,
    /// 产出数量，恒 >= 1。
    pub product_count: u32,
}

pub struct RecipeIngredient {
    pub item: ContentIndex,   // 指向 ItemDef
    pub count: u32,           // 需要数量，恒 >= 1
}
```

`RecipeTable` 照抄 `ItemTable`/`ResourcePoolTable` 已验证的列式存储手法（ADR 0016/0017 一档：声明式，注册期物化）：`display_name_key: Vec<Option<NamespacedId>>`/`ingredients: Vec<Vec<RecipeIngredient>>`/`product: Vec<ContentIndex>`/`product_count: Vec<u32>`/`defined: Vec<bool>`，`RecipeTable::define` 注册期完整校验（食材/成品的 `ContentIndex` 必须已经在 `Registry` 里 intern 过——但**不要求**它们已经通过 `register-item` 定义，理由同 `register-item-use-effect` 允许引用尚未定义细节的索引，`RecipeTable::define` 只校验索引本身合法，不跨表校验「这个索引是不是一件真的物品」，跨表强校验会让注册顺序产生不必要的耦合，与 `TraitTable`/`ItemTable` 目前互相不做这类跨表校验一致）。

`resolve` 侧最小视图（与 `ItemRule`/`ResourcePoolRule` 同一条「只收敛真正要读的字段」先例）：

```rust
pub struct RecipeRule {
    pub ingredients: Vec<RecipeIngredient>,
    pub product: ContentIndex,
    pub product_count: u32,
}
pub trait RecipeCatalog {
    fn recipe(&self, recipe: ContentIndex) -> Option<RecipeRule>;
}
```

---

## 四、烹饪这个动作：新 `Intent::Craft`，效果全部复用既有变体，场地需求核实为"能做但先不做"

### 为什么是新 `Intent::Craft`，不是复用 `Intent::Use`

`Intent::Use` 的语义是「用掉背包里的一件东西，触发它自带的效果」——`resolve_use_item` 的输入只有「用哪个 `def`」，输出永远是「消耗这一件 + 效果」的一对一映射。烹饪是「消耗 N 种、每种若干份，产出 1 种若干份」，输入形状（一个菜谱索引，而不是一个物品索引）与效果形状（多条消耗 + 一条产出，而不是单条消耗 + 单条效果）都对不上——把烹饪硬塞进 `Intent::Use`，需要让「使用」一个不存在于背包里的「菜谱物品」，或者让 `use_effect` 表达"消耗其它 N 件东西"，两者都是在扭曲 `Intent::Use` 已经验证过的简单语义去迁就一个形状不同的操作。新增一个变体、复用既有效果原语，比硬凑更诚实：

```rust
Intent::Craft {
    actor: EntityId,
    recipe: ContentIndex,
},
```

与 `Intent::Use`/`Intent::UseSkill` 同一条纪律：只携带「想做哪个菜谱」这条裸请求，不做任何合法性判断——食材够不够、产出多少，全部留给 `resolve_craft` 结合 `Agent.inventory` 与 `RecipeCatalog` 现算。

### `resolve_craft`：零新增 `Effect` 变体

这是本节最值得记录的一点——**烹饪的效果产出可以完全复用已经存在的两个变体，不需要新开任何 `Effect`**：

1. 查 `agent = world.actors.get(actor)`，查不到返回空（既有纪律）。
2. 查 `RecipeCatalog::recipe(recipe)`，查不到返回空（ADR 0015：未注册当作没有）。
3. 对每一条 `RecipeIngredient`，在 `agent.inventory` 里找 `def` 匹配的堆，`count` 不够就整体返回空（不产出任何效果，与 `resolve_use_skill` 资源不足时静默不产出效果同一条既有纪律）。**已知简化**：与 `resolve_use_item` 现在的 `agent.inventory.iter().find(...)` 一样，只认第一条匹配的堆，不会跨多个同 `def` 堆合并计数——正常情况下 `resolve_pick_up` 的合并逻辑会让同一种可堆叠食材只存在一堆，只有食材数量超过 `stack_limit` 被迫拆成多堆时才会失真，本节两个验收示例的食材数量（1~3）远低于任何合理的 `stack_limit`，不受这条简化影响,更大数量的菜谱是留给未来的已知边界,不在本次范围内解决。
4. 食材齐全时，对每种食材产出 `Effect::ConsumeInventoryItem { actor, def, durability: None }`，**重复 `count` 次**——`ConsumeInventoryItem` 本身「恒扣一，不带 `amount` 字段」是既有设计决定（`crates/ll-sim/src/effect.rs:615` 原文：「真要支持『一次用 N 个』……应该是……连续提交」），本节不违反这条决定、不给它加字段，而是让 `resolve_craft` 在**同一次结算产出的效果批次内**把它重复 N 次——`resolve` 产出 `Vec<Effect>` 本来就允许包含同一个变体的多条记录（`append_kill_history`/多条 `AdjustResourcePool` 都是这么做的），不是新增能力。
5. 用 `merge_stacks`/`can_merge`（`ll_world::item`，`resolve_pick_up` 已经在用的同一套算法，ADR 0021：真正共享的算法，直接复用不重新实现）算出产出物品要不要与背包已有堆合并，产出 `Effect::MergeIntoInventory { actor, replaced, resulting }`。

**零新增 `Effect`——`ConsumeInventoryItem` 与 `MergeIntoInventory` 原样够用**，因为烹饪的本质就是「拾取的逆过程接产出」：拾取是"从地面搬进背包并合并"，烹饪是"从背包扣减并把产出合并进背包"，两者共用同一套合并算法（步骤 5），扣减这一半烹饪比拾取多了"扣好几种、每种好几份"，但既有的单件消耗原语重复调用就能表达，不需要一个专门表达"批量扣减"的新变体。

### 场地需求核实：能查询,但两个验收示例不需要,MVP 不做

**核实结论，纠正一处可能的悲观预期**：`resolve` 现在**确实**能查询「某个坐标是什么地形」（一节已核实 `WorldState::terrain_at`，`resolve_move` 是真实调用点）。若把「灶台」建模成一种 `TerrainKind`（例如注册一种 `lostland:campfire` 地形），`resolve_craft` 完全可以在结算前加一句 `world.terrain_at(agent.pos) == Some(required_terrain)`，甚至能扩展到"是否与灶台相邻"（`TorusPos` 加减邻居坐标，各自调一次 `terrain_at`，纯算术，不需要任何新的批量查询原语）——这条能力**不卡在** `trait-system.md` 三节③"盗贼偷袭"核实过的那个缺口。

**那个缺口指的是另一件事，两者不要混为一谈**：「盗贼偷袭」需要的是「以目标实体为圆心，查半径 N 格内有没有己方**实体**」——这是一次以任意实体为中心的批量实体查询，`script-entity-handles-and-batch-queries.md` 定义了这类查询原语，但没有接进 `resolve`。「站在灶台格子上」只需要查"我自己当前坐标的地形是什么"，是单点查询，不涉及任何其它实体，`terrain_at` 已经直接支持。**真正做不到的是"灶台是玩家建造/放置的一个物件"这条路**——本代码库没有任何"可放置物件/构筑物"系统（`world-history.md`/`society-and-affiliation.md` 提到的 `StructureKind` 是世界生成期的地图结构层，不是玩家可建造的东西，且这部分本身也是纯设计未落地），若"灶台"要求是玩家亲手放下的一个可移动物件而不是固定地形，这条路径确实不存在,与"地形判定"是完全不同的两件事。

**MVP 决定：不做场地需求，`RecipeDef` 不携带任何位置字段。** 不是因为做不到（上面已经论证能做），是因为项目所有者点名的诉求只是"食物系统、烹饪物品、菜谱"，两个验收示例（十一节）都不需要"必须在灶台旁"这条限制才能成立——加上它是在没有真实需求驱动的情况下预先设计一层复杂度（YAGNI）。若未来真的需要，`RecipeDef` 加一个 `required_terrain: Option<ContentIndex>` 字段、`resolve_craft` 加一句地形比对，是纯粹的新增，不影响本节已经定形的其余部分。

---

## 五、菜谱怎么被发现：全部已知，不设解锁门槛

**结论：MVP 阶段，任何角色只要凑齐食材、提交 `Intent::Craft`，就能做出对应菜谱——不需要"学会"这一步。**

核对了项目所有者点名的三个选项：

- **全部已知**——零新增状态，`RecipeTable` 本身就是全部已注册菜谱的清单，`resolve_craft` 只检查食材，不检查"这个角色会不会做"。
- **学会才能做，复用 `unlocked_skills`**——`Agent.unlocked_skills: Vec<ContentIndex>`（一节已核实，已实装）技术上能装下一个指向 `RecipeTable` 的 `ContentIndex`（`ContentIndex` 是贯穿全部内容类型的同一个全局号段，机制上不会混淆），但**语义上会是一次静默的概念污染**——"已解锁的技能"这个字段名字与既有全部消费点（技能树相关判定）都假设它装的是技能，往里塞菜谱索引，任何遍历这个字段的既有/未来代码都要多想一层"这条记录到底是技能还是菜谱"。真要做"学会菜谱"，应该是新增一个同构但独立的字段（`Agent.known_recipes: BTreeSet<ContentIndex>`），不是复用 `unlocked_skills` 本身。
- **试出来的**——没有任何现成机制可用（没有"随机组合食材"的判定路径，也没有对应的 UI/交互设计），是一个全新的子系统，成本远超其余两个选项。

**为什么选"全部已知"**：项目所有者的原始要求只提到"需要食物系统、烹饪物品、菜谱"，没有提出"要有获取/解锁菜谱的进度感"这层诉求——在没有这个真实需求的情况下预先设计一套解锁机制（不论是新字段还是复用 `unlocked_skills`）都是 YAGNI。"全部已知"是唯一一个**不需要 `Agent` 新增任何字段**就能让二个验收示例成立的选项，且不排除未来加解锁——`known_recipes` 字段是纯粹的新增，加上它的那一天，`resolve_craft` 多一句"若 `RecipeDef` 声明需要解锁，检查 `agent.known_recipes.contains(recipe)`"即可，不需要回头改本节已经定形的其余部分。

### ⚠ 更正记录：本节结论已被项目所有者推翻（配方发现批次）

**上面"全部已知，不设解锁门槛"那条结论不再成立。以上原文一字未删，保留在这里供对照，但它描述的不是当前系统。**

- **被谁推翻**：项目所有者，原话：「科研可以通过加点解锁，最开始设有初始可以通过阅读获取经验，也或者通过研究其他物品获取经验。**菜谱就是通过随机丢入东西煮获取或者阅读书籍的时候获取。**」
- **因为什么**：本节当初选"全部已知"的**唯一**理由是"所有者没有提出解锁诉求，因此预先设计解锁是 YAGNI"。那句裁定直接提供了那个诉求——判断的前提消失了，结论随之失效。这不是本节论证有误，是它依赖的输入变了。
- **本节哪一句仍然成立**：预留的那条演进路径**逐字兑现**了。落地方式正是本节写下的那两句：新增独立字段 `Agent.known_recipes`（**不**复用 `unlocked_skills`，理由就是本节列出的"静默的概念污染"，一字未改地采纳），并在 `resolve_craft` 多加一道"若配方声明需要发现，检查 `known_recipes.contains(recipe)`"的闸门。本节其余全部结论（食材/成品不建新类型、`Intent::Craft` 不复用 `Intent::Use`、`ConsumeInventoryItem` 不加 `amount`）继续沿用。
- **与本节"试出来的"那一档的关系**：本节曾把它判为"没有任何现成机制可用……成本远超其余两个选项"。这条评估**当时是准确的**；配方发现批次真的把那个子系统做出来了（`Intent::Experiment` + `resolve_experiment`），代价确实落在本节预计的地方——一个新意图、一段新结算、一次 `DetRng` 掷骰。
- **落地位置**：`Agent.known_recipes`（`crates/ll-world/src/entity/agent.rs`）、`Effect::LearnRecipe`（`crates/ll-sim/src/effect.rs`）、`Intent::Read`/`Intent::Experiment`（`crates/ll-sim/src/intent.rs`）、`resolve_read`/`resolve_experiment`/`resolve_craft` 第 4 道闸门（`crates/ll-sim/src/resolve.rs`）、`RecipeDef.requires_discovery` 与 `ItemDef.taught_recipes`（`crates/ll-mod/`）。端到端验收见 `crates/ll-mod/tests/example_mod_recipe_discovery.rs`。
- **一并订正**：`crafting-system.md` 十四节①把这条冲突记为"待裁决"，该节已同步更新。

---

## 六、食物的效果：`use_effect` 今天能表达什么，缺什么

`ItemDef.use_effect: Option<SkillEffect>`（一节已核实实装）三个变体逐一核对能不能覆盖食物需要的效果：

- **`RestoreResource { resource, base }`**——恢复法力/耐力/（二节新增的）饱食度，**今天就能表达**，烤肉回体力、大餐回饱食度都走这条。
- **`TemporaryStatModifier { attribute, amount, duration_ticks }`**——大餐"吃了之后一段时间内属性提升"，**今天就能表达**，是本节唯一需要的"临时增益"形状，`resolve_use_item` 已经把它接成 `Effect::ApplyStatModifier`（一节已核实）。
- **`DealDamage { base }`**——食物一般用不到（除非是"毒蘑菇"这类反例），形状上够用，不展开。

**食物需要它表达不了的东西——持续效果（"接下来 N 回合每回合缓慢回血/回饱食度"）**：`SkillEffect` 三个变体都是**一次性**结算（吃下去那一刻立刻发生完，不会在未来的回合里再触发一次）。持续效果需要的是"一个会在未来的回合被重新触发"的机制——这正是 `buffs-and-triggers.md` 七节 7.1"持续伤害"核实过的那个缺口的同构问题（持续回血与持续掉血是同一个架构问题的一体两面：都需要"谁在第 N 个 tick 主动做点什么"）。**核实现状：该文档 7.1 已经给出架构结论（甲：挂在既有 `on_turn_start`/`next_action_at` 调度节点上，不新开时间轴），且明确标注"现在能做"（八节落地顺序表第 1 项）——但这是纯设计的落地顺序建议，不是已经写好的代码，全代码库检索确认没有任何 DoT/HoT 相关的 `Effect`/`SkillEffect` 变体存在。** 因此：

**如实标注依赖：食物"持续 N 回合缓慢回血/回饱食度"这条效果，需要等 `buffs-and-triggers.md` 7.1 的持续效果架构真正落地（目前零实现，只有设计结论）——这不是本文档要重新设计的东西，本文档的两个验收示例（十一节）刻意只用一次性效果，不依赖这条尚未落地的能力。**

**饥饿的后果——如实核实，比原判断更细致**：

- **临时属性下降**——**结构上今天就能做**，不需要等任何东西：`Effect::ApplyStatModifier`/`Agent.active_stat_modifiers` 均已实装（一节已核实），"同源刷新"语义（`(attribute, source)` 相同视为同一效果的重复施加）天然防止"饥饿惩罚"在每回合重复判定时无限叠加——`resolve_satiety_decay`（二节）判定 `agent.satiety <= 0` 时，可以顺带（重新）产出一条 `Effect::ApplyStatModifier { source: <饥饿惩罚的 ContentIndex>, expires_at: 下一回合, ... }`，每回合刷新一次到期时刻，不会因为连续多回合挨饿而叠加出离谱的惩罚。**唯一缺的是一个稳定的 `source: ContentIndex`**——工程实现时随手注册一个占位内容条目当"饥饿"这个来源的身份即可，不需要任何新机制，是实现阶段的一个小细节，不构成设计缺口。
- **持续掉血直至饿死**——**做不了**，与"持续回血"卡在完全同一处：需要 DoT 架构真正落地（`buffs-and-triggers.md` 7.1，纯设计零实现）。

**MVP 决定：饥饿的后果本身不在本次设计范围内**——项目所有者的要求没有点名"要有饿死机制"，两个验收示例也不需要它成立。本节的核实结论是给排期用的（七节"现在能做的 vs 等什么"会精确列出），不代表现在就要把"临时属性下降"接上——那是一个可以现在做、但没有被要求做的可选项。

---

## 七、mod 能不能注册菜谱：`register-recipe`，一档，新增构造器 `recipe-ingredient`

### 签名与档位

**一档（ADR 0016/0017：声明式，注册期物化，运行期查表）**——菜谱是纯静态数据（固定的食材表、固定的产出），不存在任何"运行期才能决定"的动态成分，没有理由让它走脚本回调（三档），与 `register-item`/`register-trait`/`register-resource-pool` 现有全部内容注册函数走同一档。

```rust
// 新文件 crates/ll-mod/src/script_recipe_api.rs，模式照抄
// crates/ll-mod/src/script_resource_pool_api.rs
engine.register_fn("register-recipe", register_recipe);
```

`(register-recipe id display-name-key ingredients product-id product-count)`：

- `id`/`display-name-key`：完整命名空间标识符字符串，同 `register-item`。
- `ingredients`：食材列表，`(list (recipe-ingredient item-id count) ...)`——**变长列表直接作为一次调用的参数，不拆成"先注册骨架、再逐条追加"两步**，与 `register-trait` 的 `granted_skills`/`stat_modifiers`/`rule_modifiers`/`granted_resource_pools` 四个 `(list ...)` 参数是同一个既有模式（`resource-pools-and-rest.md` 十一节示例：`(list (resource-pool-grant "id" (fixed 20)))`）。**不照抄 `register-item-stat-bonus` 那种"先注册、再用独立函数追加"的模式**——那条模式服务的是"这个字段可能在物品定义之后、由不同批次的代码追加"（装备槽位是 P6 第三批才补上的能力，物品本身在更早的批次已经存在），菜谱的食材表在注册那一刻就是完整、确定的，没有"以后再追加一条食材"这种真实场景，没有理由拆成两步。
- `product-id`：产出物品的完整标识符字符串，必须指向一个已经/将要通过 `register-item` 注册的物品（跨表校验的取舍见三节）。
- `product-count`：产出数量，`i64`，必须 `>= 1`。

新增构造器函数，供 `ingredients` 列表内部使用（照抄 `resource-pool-grant`/`trait-grant` 这类"内容内部小结构"构造器的既有模式）：

```
(recipe-ingredient item-id count)   ; 返回一条 RecipeIngredient 供 register-recipe 的 ingredients 列表使用
```

### 注册期校验

- `ingredients` 恒非空——零食材的"菜谱"没有意义，同 `register-item` 拒绝 `stack-limit` 为 0 的既有纪律。
- 每条 `count`、`product-count` 恒 `>= 1`。
- `id` 不能与已注册的菜谱重复（`RecipeTable::define` 的 `DuplicateDefinition` 错误，同 `ItemTable`/`RaceTable` 既有模式）。

---

## 八、被否决的方案

- **饱食度走 `ResourcePoolShape::Scalar` + `RegenRule`（项目所有者原始倾向）**——二节已详细论证否决：容量必须靠天赋授予这条前提对"人人都有"的属性不成立；`RegenRule::OnTurnStart` 现在也不能表达负值。
- **饱食度做成持续性 buff**——二节已论证：没有多来源合并的真实需求、没有自然的"到期时刻"、且 buff 系统本身还没有"持续每回合改变数值"这个通用能力可复用。
- **食材/成品单独建类型（`IngredientDef`）**——三节已用 ADR 0021 论证：没有专属算法，只会制造一层没有实际内容的类型区分。
- **烹饪复用 `Intent::Use`**——四节已论证：输入/输出形状与 `Intent::Use` 已验证过的"一件东西换一个效果"语义不匹配，硬塞会扭曲既有语义。
- **`Effect::ConsumeInventoryItem` 加 `amount` 字段**——四节已论证：违反该字段"恒扣一"是既有设计决定的原文，`resolve_craft` 在同一批效果里重复产出这个变体已经足够表达"扣好几份"，不需要改动既有效果的形状。
- **菜谱发现复用 `Agent.unlocked_skills`**——五节已论证：技术上不会崩，但会造成字段语义的静默污染，真要做应该开一个独立的 `known_recipes` 字段。
- **`register-recipe` 的食材表拆成"先注册骨架、再用独立函数逐条追加"**——七节已论证：这个模式服务的是"以后的批次要追加字段"这种真实场景，菜谱的食材表在注册那一刻就是完整的，没有这个场景，不需要照抄。
- **现在就给 `RecipeDef` 加场地需求字段**——四节已论证：场地判定的机制本身能做（`terrain_at` 已经接进 `resolve`），但没有真实需求驱动，且两个验收示例都不需要它，YAGNI。

---

## 九、两个验收示例

两处生僻数值均为示意，非最终数值曲线（同 `resource-pools-and-rest.md` 十一节先例）。

### 简单示例：烤肉——生肉 → 熟肉，回体力

```scheme
;; 物品：生肉、熟肉
(register-item "examplemod:raw_meat" "examplemod:raw_meat_display_name" 20 200 500 -1)
(register-item "examplemod:cooked_meat" "examplemod:cooked_meat_display_name" 20 200 800 -1)

;; 吃熟肉恢复 20 点耐力——今天就能表达（六节已核实 restore-resource 够用）
(register-item-use-effect "examplemod:cooked_meat" "restore-resource" "stamina" 20 0)

;; 菜谱：1 份生肉 → 1 份熟肉
(register-recipe "examplemod:grill_meat" "examplemod:grill_meat_display_name"
  (list (recipe-ingredient "examplemod:raw_meat" 1))
  "examplemod:cooked_meat" 1)
```

玩家背包里有 1 份（或更多）`raw_meat`，提交 `Intent::Craft { recipe: grill_meat }`——`resolve_craft` 查到 `raw_meat` 数量够，产出 `Effect::ConsumeInventoryItem { def: raw_meat }`（一次）+ `Effect::MergeIntoInventory`（把 1 份 `cooked_meat` 合并进背包）。再对 `cooked_meat` 提交既有的 `Intent::Use`，走一节已核实的既有链路恢复 20 点耐力。

### 复杂示例：猎人炖菜——多种食材，给临时属性加成的大餐

```scheme
;; 食材：鹿肉（已有的 raw_meat 之外的第二种主料）、野菜
(register-item "examplemod:venison" "examplemod:venison_display_name" 10 300 1200 -1)
(register-item "examplemod:wild_herb" "examplemod:wild_herb_display_name" 30 50 200 -1)

;; 成品：猎人炖菜
(register-item "examplemod:hunters_stew" "examplemod:hunters_stew_display_name" 5 400 3000 -1)

;; 吃一份炖菜：600 tick 内体质 +2——今天就能表达（temporary-stat-modifier 够用）
(register-item-use-effect "examplemod:hunters_stew" "temporary-stat-modifier" "constitution" 2 600)

;; 菜谱：2 份鹿肉 + 3 份野菜 + 1 份生肉 → 1 份炖菜
(register-recipe "examplemod:hunters_stew_recipe" "examplemod:hunters_stew_recipe_display_name"
  (list (recipe-ingredient "examplemod:venison" 2)
        (recipe-ingredient "examplemod:wild_herb" 3)
        (recipe-ingredient "examplemod:raw_meat" 1))
  "examplemod:hunters_stew" 1)
```

三种食材、任一种数量不够都会让 `resolve_craft` 整体不产出任何效果（四节步骤 3）——玩家需要同时凑齐 2 份鹿肉、3 份野菜、1 份生肉才能做出这份炖菜，产出效果批次是 3 条 `ConsumeInventoryItem`（鹿肉两条、野菜三条、生肉一条，共六条同变体记录）+ 1 条 `MergeIntoInventory`。吃下炖菜后走既有 `Intent::Use` 链路，`resolve_use_item` 产出 `Effect::ApplyStatModifier { attribute: Constitution, delta: 2, expires_at: 当前 tick + 600, source: hunters_stew 的 ContentIndex }`。

**两个示例共同验证**：菜谱注册（`register-recipe`/`recipe-ingredient`）、`Intent::Craft` 的多食材消耗与单一产出、以及烹饪产出的东西完全通过既有 `Intent::Use`/`use_effect` 链路生效——烹饪与进食是两个独立、但能无缝衔接的动作，不需要为"吃"这件事重新设计任何东西。

---

## 十、现在能做的 vs 等什么

**现在就能落地的设计形状（不代表可以立刻写代码，代表设计本身没有阻塞）：**

1. `ResourceKind::Satiety` 新变体、`Agent.satiety: i32` 新字段、`apply()` 的 `AdjustResource` 分支多一支、`hash()` 精确插入点（二节）——零新增 `Effect`，只在既有闭合枚举上加一个变体。
2. `resolve_satiety_decay`：新 `resolve` 函数，挂在既有"结算一个实体的意图"触发点，无条件对每个实体产出一条 `Effect::AdjustResource { resource: Satiety, delta: -N }`（二节）。
3. `RecipeDef`/`RecipeIngredient`/`RecipeTable`/`RecipeCatalog`（三节）——一档声明式，物化方式照抄 `ItemTable`。
4. `Intent::Craft` + `resolve_craft`（四节）——零新增 `Effect`，全部复用 `Effect::ConsumeInventoryItem`/`Effect::MergeIntoInventory`。
5. `register-recipe`/`recipe-ingredient` 脚本 API（七节）。
6. `register-item-use-effect` 的 `"restore-resource"` 分支新增 `"satiety"` 标签（二节）——纯字符串标签新增，不改函数签名。
7. 十一节两个验收示例——一旦 1-5 落地即可直接跑通，不依赖任何尚未落地的其它系统。

**等什么（明确阻塞，且不阻塞上面 1-7）：**

1. **食物"持续 N 回合缓慢回血/回饱食度"这类持续效果**——阻塞在 `buffs-and-triggers.md` 7.1 的持续伤害/持续恢复架构上，该文档已给出架构结论但零实现（全代码库检索无对应 `Effect`/`SkillEffect` 变体）。六节已标注，两个验收示例刻意不依赖它。
2. **饥饿的"临时属性下降"后果的具体接线**（`resolve_satiety_decay` 顺带产出 `Effect::ApplyStatModifier`）——结构上今天就能做（六节已核实 `Effect::ApplyStatModifier`/`active_stat_modifiers` 均已实装），只是本次设计范围没有要求做,标为可选项,不是阻塞项。
3. **饥饿的"持续掉血直至饿死"后果**——与第 1 项同一处阻塞（DoT 架构零实现）。
4. **烹饪要求靠近灶台/固定地形**——机制上不阻塞（四节已核实 `terrain_at` 已接进 `resolve`），只是本次没有把它纳入 `RecipeDef` 形状，YAGNI，真需要时是纯粹的字段新增。
5. **烹饪要求玩家先建造一个可放置的灶台物件**——真正阻塞，本代码库没有任何"可放置构筑物"系统（四节已核实），这与第 4 项（固定地形判定）是完全不同的两件事，不要混淆。
6. **菜谱解锁/学会机制**——五节已论证 MVP 不做（YAGNI），若未来需要，新增独立的 `Agent.known_recipes` 字段是纯粹的新增，不阻塞任何已定形的部分。
7. **食材腐败/保质期、农业与种植、营养均衡（不能天天只吃一种食物）**——项目所有者的原始要求没有点名任何一项，本文档明确标为将来扩展，不在本次设计范围内给出形状。

---

## 相关文档

- [物品系统](item-system.md) — `ItemDef`/`ItemStack`/`use_effect` 既有形状的原始设计来源，本文档一节核实其已实装
- [装备栏位与占位掩码](equipment-slots.md) — `register-item-equip-mask`/`register-item-stat-bonus`/`register-item-use-effect` "新增能力用新函数，不改既有签名参数个数"这条先例的具体范例，七节 `register-recipe` 的档位判断同一条纪律
- [资源池与休息系统](resource-pools-and-rest.md) — `ResourcePoolShape`/`RegenRule` 既有形状的核实来源，二节否决"饱食度走这套机制"的直接依据；`mana`/`stamina`/`ResourceKind` 现状核实同样来自本文档一节已核实的代码
- [增益与通用触发器](buffs-and-triggers.md) — `active_stat_modifiers`/`Effect::ApplyStatModifier` 已实装的现状核实（六节）、持续伤害/持续恢复架构的零实现现状（六节、十节依赖清单）
- [天赋/特性系统](trait-system.md) — 三节③"盗贼偷袭"空间查询缺口的原始核实来源，四节据此精确区分"实体批量查询"（阻塞）与"单点地形查询"（不阻塞）
- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) — 批量实体查询原语的设计来源，四节引用以说明它没有接进 `resolve`，与已接线的 `terrain_at` 是两件事
- [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017](../decisions/0017-tiered-declarations-materialize-columnar.md) — 七节 `register-recipe` 一档判据
- [ADR 0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) — 三节"食材/成品是否需要新类型"核心判断的直接依据
- `crates/ll-mod/src/item.rs`/`crates/ll-world/src/item.rs`/`crates/ll-sim/src/resolve.rs`/`crates/ll-sim/src/effect.rs`/`crates/ll-sim/src/apply.rs`/`crates/ll-world/src/entity/agent.rs`/`crates/ll-world/src/state.rs`/`crates/ll-mod/src/script_item_api.rs` — 一节现状核实依据
