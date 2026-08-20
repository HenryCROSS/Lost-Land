# 载具与骑乘系统

**冻结于** 2026-08-20。**落地状态**：纯设计，`crates/` 中无任何对应类型——已核实：`ItemDef`/`ItemStack`/`Owner`/`ItemLocation`（[物品系统](item-system.md)）、`SlotMask`/`EquipSlot`（[装备栏位](equipment-slots.md)）全代码库检索无匹配；`Agent` 没有 `mounted_on`/`rider`/`mount_profile`/`suspended_action_offset` 字段；`TerrainDef`/`TerrainTable` 没有地表分类；`ll-sim::effect::Effect` 没有 `Mount`/`Dismount`/`PlaceVehicle` 变体；`Agent.skill_cooldowns`/`active_stat_modifiers`/`unlocked_skills`/`Timeline::remove` **均已落地**（见一节，本文档大量复用它们）。**实现阶段**：P6（物品与装备）——项目所有者原话点名「这将会是物品与装备的一部分」，但 `Agent` 新增字段与既有先例（种族、职业技能）同样的理由，宜尽早落地，不必等 P6 全部完成才动 `Agent` 的形状。**冻结时对应 git 提交**：`7ec16b9`（`main` 分支，本文档写作时的仓库 HEAD）。

**本文档只给出「能支撑一匹马和一条船」的最小形状**，多人载具、马车、载具改装、载具驯养/繁殖、载具战斗深度定制（除本文档已裁定的攻防加成/技能授予外）明确排除在外，见九节。

---

## 零、项目所有者的要求（原话，含三轮追加裁定）

> 「能不能存在一个载具系统，这东西的贴图会覆盖在人物贴图的下半部分。这样就存在马，牛，船之类的东西，并且存在某些功能，例如提供掩体，跨域海洋等等功能，你看看怎么设计，这将会是物品与装备的一部分，因为有的可能是能自己动的生物，而有的是被制作出来，可以被放置在地图某处。」

第一轮追加（掩体的真实语义）：

> 「提供掩体或者说作为一种装备，提供防御或者攻击属性」

> 「也或者提供一种远程攻击的手段，当然也会有增加攻击属性」

第二轮追加（回合经济）：

> 「骑乘时，移动回合肯定是算玩家的，马的属性会被切换，也就不在时间轴内了。等下马以后再重新回到时间轴。」

第三轮追加（能力授予与渲染的两条裁定）：

> 「1.技能冷却跟随载具。2.我同意你的『有效技能 = 已学会的 ∪ 当前载具授予的』」

> 「渲染的话，有的载具可以选择渲染人物，有的不渲染，这样就算覆盖人物了也没问题了」

这些追加把原本模糊的「掩体」「贴图覆盖」两处直接钉死成具体机制，本文档正文按最终裁定写，不重复展示中间被推翻的方案（除非该方案的否决理由本身有复用价值）。

---

## 一、现状核实（写作本文档前已去代码核实）

- **物品/装备系统全部是纯设计**：[物品系统](item-system.md)、[装备栏位与占位掩码](equipment-slots.md)全代码库检索无 `ItemDef`/`ItemStack`/`SlotMask` 匹配。P6 尚未开工。
- **地形通行性是静态查表**：`crates/ll-world/src/terrain.rs` 的 `TerrainTable::blocks_move`/`move_cost` 是按 `ContentIndex` 下标的纯列式查表，注册期一次性 `define`，运行期 `O(1)` 数组访问。
- **`resolve_move` 的真实形状**（`crates/ll-sim/src/resolve.rs:409` 起）：撞墙仍消耗 `action_cost(BASE_ACTION_COST, speed)`；可通行时产生 `MoveTo` + `ScheduleNext`；`speed` 全程来自 `effective_speed_from_dexterity(agent.stats.dexterity)`。
- **`resolve_attack` 仍是占位实现，攻防聚合尚未接线**（`crates/ll-sim/src/resolve.rs:495`）：`let attack_power = attacker.stats.strength;`，`damage_after_defense(attack_power, 0, Penetration::NONE)`——**攻击力直接读原始 `BaseStats` 字段，防御恒为 `0`，两者都没有经过任何聚合函数**，`Agent.active_stat_modifiers` 在这个函数里完全没有被读取。这不是本文档新发现的缺口，是 [三轴战斗结算](combat-three-axis.md) 已经点名、独立于载具存在的既有缺口，二节据此展开。
- **`Agent.active_stat_modifiers: BTreeMap<AttributeKind, ActiveStatModifier>` 已经落地**（`crates/ll-world/src/entity/stats.rs:82`、`agent.rs:207`）：由 `Intent::UseSkill`（`resolve_use_skill` 的 `TemporaryStatModifier` 分支，`resolve.rs:763`）写入，`apply.rs:168` 响应 `Effect::ApplyStatModifier` 完成写入，**已经进 `WorldState::hash()`**（`crates/ll-world/src/state.rs:929`）。按 `AttributeKind` 键控，「同一项属性同一时刻只能有一条生效的修正」——`StackPolicy::RefreshDuration`（后写覆盖先写），不是叠加多条。
- **`Agent.unlocked_skills: Vec<ContentIndex>`/`Agent.skill_cooldowns: BTreeMap<ContentIndex, Tick>` 已经落地**（`crates/ll-world/src/entity/stats.rs:174`/`192`），**均已进 `WorldState::hash()`**（`state.rs:922`/`924`）。`skill_cooldowns` 存的是**到期时刻**（绝对 `Tick`），不是剩余时长——「惰性判定，不要求主动清理过期条目」，与 [增益与通用触发器](buffs-and-triggers.md) `ActiveEffect.expires_at` 同一模式。
- **「能不能放某技能」的真实查询路径只有一处**：`resolve_use_skill`（`resolve.rs:684`）四道门——门一 `agent.unlocked_skills.contains(&skill)`、门二 `agent.skill_cooldowns.get(&skill)` 与 `world.clock` 比较、门三查技能目录、门四资源是否充足。全代码库没有第二处真实的技能可用性判定（其余匹配全是测试夹具构造 `Agent` 字面量）。
- **`Timeline::remove(actor: EntityId)` 已经落地**（`crates/ll-sim/src/timeline.rs:87`）：「移除某实体在队列中的全部条目……用于实体死亡：时间轴可能残留它此前排入的行动，若不清理，死后队列弹出到它时会对一个已不存在的实体执行动作」——`BinaryHeap::retain`，与插入历史无关，结果只由「谁还在队列里」这个集合决定，满足约束 C5。`Timeline::schedule(actor, at)` 是唯一的入队入口。**四节直接复用这两个既有方法，不新增队列机制。**
- **`EntityId` 不是跨存储唯一的**（`crates/ll-world/src/entity/id.rs` 模块文档）：「两层各自维护自己的 `(索引, 世代)` 分配……同一个 `EntityId` 值分别喂给厚层与薄层是两次独立查询，互不相关」——二节据此否决「新开一个实体池装船」的方案。
- **`AttributeKind` 是六个定长变体**（`stats.rs:43`）：`Strength`（物理攻击、负重）、`Dexterity`（时间轴速度、闪避、命中）、`Constitution`、`Intelligence`（魔法攻击）、`Willpower`（精神攻防、视野）、`Charisma`。**没有对应「护甲/防御」的属性变体**——`DerivedStats.armor` 按 [属性系统](attribute-system.md) §七的设计来自 `derive_stats(基础属性, 装备, 状态效果, 负重)`，不是某个 `AttributeKind` 的直接映射，这一点决定了六节「攻击加成」与「防御加成」不能走完全相同的具体通道。
- **精灵渲染的既有原语**（`crates/ll-render/src/sprite.rs`）：`Footprint`（占地格数）与 `SpriteSize`/`Pivot`（视觉像素尺寸/锚点）刻意解耦；`footprint_anchor_pixel`/`sprite_draw_position` 是唯一被允许的锚点换算路径；`DrawOrder`（`Layer` + 屏幕 `foot_y` + `entity: u64`）是唯一的绘制顺序键。

---

## 二、核心判断：载具是关系，不是实体类型——采纳，给出精确形状

### 复核项目所有者的初判

**采纳「骑乘是关系」这个结论，但关系的两端必须落在同一个可寻址空间里，关系字段才有意义**——`EntityId` 不是跨存储唯一的（一节已核实），若骑手与坐骑分别落在两个不同的实体池，`rider.mounted_on: EntityId` 这一个字段本身就无法告诉你该去哪个池里查。

### 被否决的方案：新开一个 `Arena<Prop>`（轻量占位物实体池）

马是 `Agent`（有 AI、属性、会死），船是一个新的、极小的 `Arena<Prop>` 条目——**否决**，代价不是「多写一个 struct」，是打破了 `EntityId` 目前唯一成立的使用前提：`Intent::Attack { target: EntityId }`、`Effect::Damage { target: EntityId }`、脚本层的 `ScriptEntityHandle` 全部隐式假设「一个 `EntityId` 就指向厚层 `Agent`」。引入第二个厚层实体池意味着这些调用点全部要么改签名、要么各自猜测，波及面远超「载具」这一个系统。

### 最终形状：一个实体存储，一份可选的关系数据，不是一个共享的 `Vehicle` 父类型

**马与船都落在既有的 `Arena<Agent>`。** 二者的差异不靠 Rust 类型系统区分，靠内容注册表数据区分——[ADR 0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)「抽象的理由是有算法要共享，不是看起来该对称」的正面应用：马与船共享的不是一个 `trait Vehicle`，是**同一份数据 schema**（`MountDef`，见三/六/八节）与**同一段算法**（速度查询、地表通行判定、渲染层序、技能授予查询——这几处确实要跑同一套代码）；船有没有 AI，是数据层面的一个布尔（`MountDef.autonomous`），不是类型层面的分叉。

```rust
// Agent（新增四个字段，其余不变）
pub struct Agent {
    // …… 既有字段不变
    /// 我正骑乘/驾驶的实体，`None` 表示未骑乘。写入口唯一是 apply
    /// 响应 Effect::Mount/Effect::Dismount（C1）。
    pub mounted_on: Option<EntityId>,
    /// 正骑在我身上的实体，`None` 表示空载。与 mounted_on 成对写入，
    /// 同一次 apply 内保持双向一致——见四节「唯一写入口」。
    pub rider: Option<EntityId>,
    /// 这个 Agent 是否「可被骑乘/驾驶」，指向注册表条目。`None` 表示
    /// 这是一个普通实体，不能被骑。
    pub mount_profile: Option<ContentIndex>,
    /// 被骑乘期间挂起的时间轴进度——四节「重入 tick 不可滥用」的
    /// 存储位置，仅在 mounted_on/rider 关系建立期间短暂非空。
    pub suspended_action_offset: Option<i64>,
}
```

**厚层「数百个」的规模下，一条船携带 `profession`/`goals`/`affiliations` 这类死字段的代价是概念噪音，不是字节数**（`Vec` 字段为空时只占 `ptr+len+cap`，无堆分配）——真正的解法是「永远不去读一条船的 `profession`/`goals`」，与厚层现有「哪些字段被谁消费」的既有纪律（`race` 未落地阶段就是「建布局但零消费」）同一个模式，不需要靠拆分实体池来解决。

**一个直接的红利**：马与船都携带 `skill_cooldowns`/`active_stat_modifiers`/`stats` 这些既有字段，六、七节的技能授予与属性聚合因此**不需要为载具另开一套存储**——完全复用 `Agent` 已经有的、已经进 `hash()`、已经参与存档往返的字段，见六节。

---

## 三、通行性：有条件的地形穿越

### 判定从「查一个 bool」变成什么

**不改 `TerrainKind::blocks_move` 的签名**——它在 `resolve_move` 之外还被其他调用方使用，若签名接受一个「可选坐骑」参数，所有既有调用方都要多传一个 `None`。正确做法是只在 `resolve_move` 这一个调用点新增一层判定。

新增一条**闭集**分类轴（不是给开放的 `ContentIndex` 地形集合挂白名单——理由见下）：

```rust
// crates/ll-world/src/terrain.rs（设计）
/// 地表分类：闭集位标志，与 SlotMask（equipment-slots.md）、
/// ActionCapability（action-capability-and-input-context.md）同一个
/// 既有惯用法——本体保留低位，高位留给 mod 扩展。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceKind(u8);
impl SurfaceKind {
    pub const NONE: SurfaceKind = SurfaceKind(0);
    pub const WATER: SurfaceKind = SurfaceKind(0b0001);
    // 高四位留给 mod（熔岩、深渊……）。
}
```

`TerrainDef`/`TerrainAttrs`/`TerrainTable` 各新增一个 `surface: SurfaceKind` 字段/列，默认 `NONE`。`lostland:deep_water`/`lostland:shallow_water` 声明为 `WATER`。

**为什么是闭集分类而不是坐骑持有一份「能穿越哪些具体地形 ID」的白名单**：地形本身是开放集合，若坐骑直接持有具体地形 ID 列表，mod 新增一种「浅沼泽」地形后，所有已声明「能过水」的坐骑不会自动识别它，除非沼泽作者记得同步所有坐骑的白名单——`O(地形数 × 坐骑数)` 的手工同步负担。分类轴反过来：地形声明「我属于哪一类」，坐骑声明「我能过哪几类」，新增一种沼泽只要打上 `WATER` 标签就自动被识别。

### `resolve_move` 的新分支

```rust
// crates/ll-sim/src/resolve.rs（设计，插入既有 blocks_move 分支之前）
let mover = agent.mounted_on.and_then(|id| world.actors.get(id)); // None 时零开销短路
let mover_speed = effective_speed_from_dexterity(
    mover.map_or(agent.stats.dexterity, |m| m.stats.dexterity)
);
let mount_def = mover.and_then(|m| m.mount_profile).and_then(|i| world.mount_table.get(i));

if terrain.blocks_move(&world.terrain_table) {
    let passable = mount_def
        .is_some_and(|def| def.grants_passage.intersects(terrain.surface(&world.terrain_table)));
    if !passable {
        let cost = action_cost(BASE_ACTION_COST, mover_speed);
        return vec![Effect::ScheduleNext { actor, at: schedule_after(world, cost) }];
    }
    let cost = action_cost(mount_def.unwrap().surface_move_cost, mover_speed);
    let mut effects = vec![
        Effect::MoveTo { actor, pos: dest },
        Effect::ScheduleNext { actor, at: schedule_after(world, cost) },
    ];
    if let Some(mount_id) = agent.mounted_on {
        effects.push(Effect::MoveTo { actor: mount_id, pos: dest }); // 坐骑视觉同步
    }
    return effects;
}
// …… 原有可通行分支不变，只是 speed 换成 mover_speed
```

**对未骑乘的绝大多数移动，这条新分支零开销**：`agent.mounted_on` 是 `None` 时 `mover`/`mount_def` 直接短路成 `None`，普通移动只多一次 `Option` 判断。

### 性能代价：一档，物化列式表，不是新开一档

按 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md)/[0017](../decisions/0017-tiered-declarations-materialize-columnar.md)：`grants_passage`/`surface_move_cost` 都是注册期一次性声明，不依赖任何运行期才存在的输入（与 [伤害公式 mod API](damage-formula-mod-api.md) 必须落二档的理由恰好相反）。`MountTable` 按 `ContentIndex` 存定长列，与 `TerrainTable` 同一物化方式。

### 确定性影响

- **`TerrainTable`/`MountTable` 本身不进 `WorldState::hash()`**——内容注册表，与 `RaceDef`/`TerrainDef` 现有处理方式一致。
- **`Agent.mounted_on`/`Agent.rider`/`Agent.suspended_action_offset` 必须进 `WorldState::hash()`**——与一节已核实的既有先例（`skill_cooldowns`/`active_stat_modifiers` 已经进 `hash()`，`state.rs:922`~`933`）同一条纪律，四节给出具体的哈希写入位置。`mount_profile` 与 `race: ContentIndex` 同一类（spawn 时定死不变），可选是否单独覆盖，不是本文档的重点。

### 对 FOV/探索记忆的影响：明确不改

`compute_fov` 只认 `origin`/`radius`，骑乘不改变视野半径。`MarkExplored` 仍只在 `actor == world.player_entity` 时追加——玩家骑乘时 `actor` 依旧是玩家自己的 `EntityId`。

### 继承的一处未验证风险，不在本文档解决

[种族系统](race-system.md) 十二节已经标注「2×2 `footprint` 的碰撞与寻路是否已支持……未经核实」——船的占地格数同样依赖这项能力，本文档不重复验证。

---

## 四、回合经济：骑乘时谁在行动（项目所有者已裁定，本节只给具体形状）

裁定原话：「骑乘时，移动回合肯定是算玩家的，马的属性会被切换，也就不在时间轴内了。等下马以后再重新回到时间轴。」——本节回答「不在时间轴内」在现有结构里具体怎么做、「重新回到时间轴」按什么时刻、哪些属性是「切换」哪些是「叠加」。

### ① 退出/重入时间轴：复用既有的 `Timeline::remove`/`schedule`，真移除，不是取出时跳过

**一节已核实：`Timeline::remove(actor)` 已经落地，且已经是「死亡清理残留条目」的既有用途**——`Effect::Mount` 应用时对坐骑调用一次 `timeline.remove(mount_id)`；`Effect::Dismount` 应用时对坐骑调用一次 `timeline.schedule(mount_id, 重入tick)`（见②）。

**为什么是真移除，不是「取出时判断被骑乘就跳过」**：`remove` 用 `BinaryHeap::retain`，是对队列内容的一次性过滤，结果只由「谁还在队列里」这个集合决定，与插入历史无关，满足约束 C5；且这个方法**已经存在、已经被死亡场景验证**，直接复用比新写一条「弹出时二次判断」的分支更省——「取出时跳过」还要求调度器主动查一次 `world.actors.get(entry.actor)?.rider`/`mounted_on` 这类世界状态，而**约束 C2 规定时间轴队列只放朴素数据**（`TimelineEntry { at, actor }`，不含任何需要回查世界状态的字段）——调度器弹出一条记录后，若要跳过还得反查 `WorldState`，这打破了「队列本身就是唯一权威顺序来源」的既有简单性；`remove`/`schedule` 在事件发生的那一刻（骑乘关系建立/解除）一次性同步队列内容，弹出阶段不需要知道任何骑乘信息，这是代价更低、也更符合约束的做法。

**代价**：`retain` 是 `O(n)`，`n` 是当前队列长度——厚层「数百个」规模下可忽略，与死亡清理承受的是同一个代价，不是新引入的性能顾虑。

### ② 重入 tick：记录「挂起时距离下次行动还差多少」，不可被反复上下马滥用

**新增字段 `Agent.suspended_action_offset: Option<i64>`（二节已给出）**——一节已核实这个量当前不存在，必须新增，新增即进世界状态、进 `hash()`（见下）。

```
Effect::Mount 应用时（mount 是坐骑一侧的 Agent）：
    offset = max(0, mount.next_action_at.0 - world.clock.0)
    mount.suspended_action_offset = Some(offset)
    timeline.remove(mount_id)
    // mount.next_action_at 本身保持不动，此刻它已经不在队列里，
    // 这个字段的旧值在挂起期间不被任何代码读取，直到下面被重新写入。

Effect::Dismount 应用时：
    offset = mount.suspended_action_offset.take().unwrap_or(0)
    new_at = Tick(world.clock.0 + offset)
    mount.next_action_at = new_at
    timeline.schedule(mount_id, new_at)
```

**为什么不可滥用，正面论证**：设 `T₀` 为上马时刻、`D` 为原本还差的 tick 数（`= 原 next_action_at − T₀`，已被钳制到 `≥ 0`）。无论骑手在上马后经过多久（`Δ` 个 tick）才下马，重入时刻都是 `下马时刻 + D = (T₀ + Δ) + D`——**这与「坐骑从未被打断，在 `T₀ + D` 那一刻本该行动，而骑手恰好在那之后的某个时间点才让它下马」得到的『它欠世界多少 tick』是同一个量，与骑乘持续了多久（`Δ`）无关**。反复上下马 `k` 次、每次挂起 `Δᵢ`，重入偏移量在每一次挂起时都被**重新计算**（不是累加），因为每次 `Effect::Mount` 都是「以当前 `next_action_at` 为基准，钳制到非负，记一次新的 `offset`」——链条的正确性来自「`offset` 永远只表示『相对于此刻还差多少』，而不是『已经过去了多久』」：
- 若坐骑在挂起前已经**过期未弹出**（`world.clock ≥ next_action_at`），`offset` 钳制为 `0`，下马时立即重新入队——它不会因为被骑乘而错过一次早已到期的行动机会，但也不会因为反复上下马而额外攒出行动次数（`0` 就是 `0`，不会变负）。
- 若坐骑挂起前还没到期（`offset > 0`），无论中途上下马多少次，只要**没有让时间在“未挂起”状态下流逝**，`offset` 的值不会被消耗——它只在坐骑真正脱离时间轴（挂起）期间被“冻结”，脱离期间世界时钟照常前进，但这段时间不计入坐骑的等待，重入时原样把冻结前的差值补上。

**必须有的测试（本文档钉死断言，不留给实现者自行补）**：构造一个坐骑，记录它在「从不被骑乘」情形下累计到某个世界时刻为止的行动次数；再构造同一个坐骑、同一段世界时间窗口内插入任意次数、任意时长的上下马操作（骑乘期间不产生任何 `Intent` 给它），断言两种情形下坐骑最终的累计行动次数与 `next_action_at` **完全一致**——这是本节论证的可执行版本，不是口头保证。

**必须进 `WorldState::hash()`**：`mounted_on`/`rider`/`suspended_action_offset` 三个字段都要在一节已核实的每-`Agent` 哈希循环（`state.rs:901` 起）里补上写入，紧邻 `next_action_at` 之后是自然的位置——`mounted_on`/`rider` 用 `EntityId::as_u64()` 包一层 `Option` 写法（`write_optional_...` 这类既有帮手函数的同一模式，例如 `write_optional_content_index`/`write_optional_world_id` 已经是这个套路），`suspended_action_offset` 直接 `hasher.write_i64`（`Option` 用 `0`/`1` 标志位 + 值，与 `write_optional_world_id` 同一惯例）。

### ③ 哪些属性替换、哪些叠加

**判据：这个字段回答的是「谁的身体在承担这件事」——只能有一个真相源的，替换；回答的是「骑手自身能力上叠加了多少」的，叠加。**

| 字段 | 替换 or 叠加 | 理由 |
|---|---|---|
| 移动速度（`effective_speed_from_dexterity` 的输入） | **替换**——骑乘期间读坐骑的 `stats.dexterity`，不读骑手的，不相加 | 只有一具身体在物理上移动（坐骑的脚/桨），「骑手敏捷 + 坐骑敏捷」没有对应的物理意义——一个跑得慢的人骑上快马应该变得和马一样快，不是「慢一点」 |
| 攻击力加成（战马） | **叠加**，走 `active_stat_modifiers` | 骑手仍是挥剑的那个人，坐骑只是让他打得更狠，不是取代他的攻防人格——与既有 `TemporaryStatModifier` 走同一条已落地的通道，见六节 |
| 防御力加成（盾车） | **叠加**（设计上），但通道本身还不存在——见六节 | 同上，只是 `DerivedStats.armor` 没有 `AttributeKind` 对应，聚合入口是 [属性系统](attribute-system.md) `derive_stats`「状态效果」入参，尚待补齐，见六节 |
| 技能可用性 | 项目所有者已裁定为「并集」，既不是替换也不是简单叠加，是**派生集合运算** | 见六节 |

**移动代价的分子**（`action_cost` 第一个参数）不在这张表里——它不是「骑手 vs 坐骑」的属性归属问题，是三节已给出的「查地形表还是查 `MountDef.surface_move_cost`」的地形判定问题，两者正交。

---

## 五、渲染：载具自己声明「画不画骑手」

项目所有者的简化裁定：「有的载具可以选择渲染人物，有的不渲染，这样就算覆盖人物了也没问题了。」——**这消掉了「怎么把两张精灵的上下半拼合对齐」这个最麻烦的问题**：不需要专门画一份「只有上半身」的骑乘 `Clip`，不需要发明新的轴心概念，需要处理的只剩两个普通场景。

### 两种场景，都复用既有机制，不新造

- **`MountDef.renders_rider == true`（马）**：骑手精灵**正常绘制**（用骑手自己的 `Footprint`/`Pivot`，走 [`footprint_anchor_pixel`]/[`sprite_draw_position`] 原样不改的既有换算），坐骑精灵也正常绘制（用坐骑自己的 `Footprint`/`Pivot`）——**就是两个普通实体各自绘制**，唯一需要额外保证的是层序（见下）。
- **`MountDef.renders_rider == false`（船）**：骑手精灵**完全不提交绘制**——船的精灵本身已经表达了「人在船里」，没有两张图要对齐的问题，也不存在轴心换算。

**没有裁剪、没有多精灵合成、没有新的锚点概念**——一节已核实 `ll-render::sprite` 目前没有任何精灵局部绘制/透明遮罩机制，本设计**不需要**这类机制，`renders_rider` 这一个布尔就把「贴图覆盖人物下半部分」的视觉目标转化成了「压根不画那张会被覆盖的图」，这是比像素级合成简单得多、也更不容易踩本项目在轴心对齐上栽过跟头的方案。

### ① 层序稳定性：`DrawOrder` 现有结构不能保证，需要新增一个层内子序号

**核实结论：不能**。`DrawOrder` 现有的平局规则是 `entity: u64`（打包后的 `EntityId`）——坐骑与骑手是两个不同的 `EntityId`，谁的数值更小完全取决于各自的 spawn 顺序，**不能保证骑手恒排在坐骑之后**（`renders_rider == true` 时若坐骑的 `entity` 数值恰好更大，骑手会先绘制、坐骑后绘制，骑手视觉上「钻进马肚子」）。

**新增一个排在 `foot_y` 之后、`entity` 之前的字段**，不改动 `Layer` 本身（`Layer` 数值即协议，可能被图集元数据引用；`DrawOrder` 是每帧从世界状态现算的运行期排序键，从不落盘，改它的形状没有协议兼容性代价）：

```rust
pub struct DrawOrder {
    layer: Layer,
    foot_y: i32,
    composite_order: u8,  // 新增：坐骑取更早绘制的值，骑乘中的骑手取更晚绘制的值，
                           // 普通实体恒为同一个中性默认值，不受影响。
    entity: u64,
}
```

坐骑与骑手共享同一个 `foot_y`（二者共处一格，见四节 `Effect::MoveTo` 对坐骑的视觉同步），`composite_order` 保证坐骑恒先于骑手绘制，与 `entity` 的具体数值无关；未涉及骑乘的普通实体全部使用同一个默认值，现有排序行为与现有单元测试的结论不受影响。

### ② 大于一格的 footprint：现有机制在类型层面支持，碰撞/寻路层未验证——如实分层回答

**渲染层**：`Footprint`（占地格数）与 `SpriteSize`/`Pivot`（视觉像素尺寸）刻意解耦，`footprint_anchor_pixel`/`sprite_draw_position` 本来就是为「占地格数与视觉像素尺寸不必匹配」设计的（模块文档原话「重点目标的精灵是 32×48 像素，却只占 2×2 格」），船用一个 `Footprint { width: 3, height: 2 }` 或任意更大的数值，走的是**已经写好、已经被单元测试锁定**的同一套换算，不需要为「船比人大」新增任何渲染代码——这一层**支持**，不是将来扩展。

**碰撞/寻路层**：不支持——如实标注，与三节末尾指出的是同一个未验证项（[种族系统](race-system.md) 十二节：「2×2 `footprint` 的碰撞与寻路是否已支持……未经核实」）。占地格数目前只在渲染换算里被消费，**没有任何证据表明 `resolve_move`/寻路会把一个大于 1×1 的 `Footprint` 当成真正阻挡其他实体的碰撞体积**。本文档不现在解决这一层，标为将来扩展——最小形状下，船只需要「视觉上占几格、绘制位置对不对」，「其他 NPC 会不会穿过停靠的船」这类碰撞判定留给下一个批次核实与实现。

---

## 六、能力授予：走既有的技能系统，攻防加成走既有的属性修正通道，都不新造机制

### 结论：掩体不是独立机制，也不是伤害公式的操作数——载具本身就是一种属性/能力来源

项目所有者的裁定推翻了本文档更早版本「掩体是伤害公式的一个新操作数」的方案：「提供掩体或者说作为一种装备，提供防御或者攻击属性」「也或者提供一种远程攻击的手段，当然也会有增加攻击属性」——**载具的贡献是两种性质不同的东西，分开处理**：

| 贡献 | 性质 | 通道 |
|---|---|---|
| 攻击/防御数值加成（战马加攻击、盾车加防御） | 被动数值 | `Agent.active_stat_modifiers`（叠加），见下 |
| 远程攻击手段 | 能力授予 | 技能系统（`unlocked_skills`/`skill_cooldowns` 的派生扩展），见下 |

「多了 5 点攻击」和「多了一个能放的技能」是两回事，硬塞进同一个机制会两头不像——数值加成不需要冷却/资源消耗/前置条件这些技能才有的概念，技能也不该被简化成一个数值。

### 攻击加成：复用 `active_stat_modifiers`，走既有通道，但要点破一个真实的缺口

**这个通道已经存在、已经被写入、已经进 `hash()`（一节已核实）**——`Effect::Mount` 应用时，若坐骑的 `MountDef.attack_modifier: Option<(AttributeKind, i32)>` 非空（例如 `(Strength, 5)`），对骑手插入一条 `Effect::ApplyStatModifier { target: rider, attribute, delta, expires_at: Tick(i64::MAX) }`（不设自然到期，靠 `Effect::Dismount` 显式清除，见下）；`Effect::Dismount` 应用时插入一条 `delta: 0, expires_at: world.clock` 的同键覆盖（`RefreshDuration` 语义——一节已核实「同一项属性同一时刻只能有一条生效的修正」，后写覆盖先写），使这条修正立即读作失效，不需要新增「移除」这个动作。

**必须诚实指出的缺口**：`resolve_attack` 目前**根本不读** `active_stat_modifiers`（一节已核实，攻击力直接是 `attacker.stats.strength`）。这不是载具引入的新缺口——[三轴战斗结算](combat-three-axis.md) 「四、接线点」早就点名「`derive_stats` 的具体实现是新 P6 阶段的工作」，载具的攻击加成与技能的 `TemporaryStatModifier`、未来装备的 `StatBonus` 面对的是**同一个尚未接线的聚合函数**，载具不需要、也不应该为自己单独造一条捷径——它只是这个已知缺口的第三个消费者（技能、装备、载具）。**载具落地前，`resolve_attack` 必须先学会读 `active_stat_modifiers`**，这条要求已经写入七节清单，不是新增负担，是把既有 TODO 补上。

**附带说明的耦合**：`AttributeKind::Strength` 同时驱动「物理攻击」与「负重上限」（一节已引用其字段文档），战马的攻击加成因此会顺带让骑手负重上限也变高——这是复用属性级通道而非专属战斗字段的自然结果，判断为可接受的副作用（骑乘理应能多带东西），不是需要额外隔离的泄漏。

### 防御加成：设计上走同一条思路，但聚合入口本身不存在——如实标注，不假装已解决

`DerivedStats.armor` 没有对应的 `AttributeKind`（一节已核实），不能像攻击力一样直接塞进 `active_stat_modifiers`。[属性系统](attribute-system.md) §七 `derive_stats(基础属性, 装备, 状态效果, 负重)` 的「状态效果」这个入参位置，正是防御类加成该去的地方——但**这个入参目前没有任何消费逻辑**，与装备的 `StatBonus`（[设计文档总索引](README.md) 「缺口 5」）是同一个尚未补齐的洞。**本文档不代为设计这条聚合逻辑**，只指出接线点：盾车的防御加成走「状态效果」入参，与装备的属性加成走同一条尚待实现的路径，不是载具专属的第二个洞。落地顺序：`derive_stats` 补齐「状态效果」消费逻辑（P6 装备批次的既定工作）→ 载具的防御加成才能真正生效；在此之前，`MountDef.defense_modifier` 字段可以先声明（八节），但注册后实际不生效，与「P3 加零成本的字段占位」是同一个先例。

### 远程攻击/特殊能力：授予技能，不新造机制

**采纳「载具授予一个技能」的判断**——项目里已有的技能系统（`register-skill` 注册接口、`Agent::skill_cooldowns` 冷却、`ResourceCost` 资源消耗、`prerequisites` 前置、[三轴战斗结算](combat-three-axis.md) 已覆盖的远程/范围伤害）**全部白拿**，不需要为「载具的攻击能力」重新实现命中判定/冷却/消耗。

**有效技能集：派生，不写进 `unlocked_skills`**——项目所有者已同意「有效技能 = 已学会的 ∪ 当前载具授予的」，与本项目复用了十二次的「默认派生，只存偏差」同一模式：

```rust
fn skill_source(agent: &Agent, mount: Option<(&Agent, &MountDef)>, skill: ContentIndex) -> SkillSource {
    if agent.unlocked_skills.contains(&skill) {
        return SkillSource::SelfTaught;
    }
    if let Some((_, def)) = mount {
        if def.granted_skills.contains(&skill) {
            return SkillSource::Mount;
        }
    }
    SkillSource::None
}
```

**要动的查询路径只有一处，一节已核实**：`resolve_use_skill` 的门一（解锁判定）与门二（冷却判定）——门一从 `agent.unlocked_skills.contains(&skill)` 改成 `skill_source(...) != SkillSource::None`；门二与冷却写入的**归属**（下面详述）一起改。全代码库没有第二处真实的技能可用性判定需要同步修改。

### 冷却记在载具上——项目所有者已裁定，推翻本文档更早版本「记在骑手身上」的建议

裁定原话：「1. 技能冷却跟随载具。」——**这比记在骑手身上更合理**，具体后果必须写清楚，避免将来被当成 bug「修掉」：

| 情形 | 冷却行为 | 理由 |
|---|---|---|
| 同一辆载具，下马再上 | **保持** | 冷却在载具自己的 `skill_cooldowns` 上，不随骑手走 |
| 换乘另一辆同类载具 | **各自独立** | 两台弩车就该能各射一发——冷却属于机器不属于操作者，玩家想要两发就得真的准备两台车，那本身是资源成本，**不是漏洞** |

**存储位置：与骑手完全同一套，零新增机制**——二节已经确立马与船都落在 `Arena<Agent>`，二者都天然拥有 `Agent.skill_cooldowns: BTreeMap<ContentIndex, Tick>` 这个既有字段（一节已核实已落地、已进 `hash()`）。`SkillSource::Mount` 的技能，冷却读写目标是**坐骑自己的** `skill_cooldowns`（`world.actors.get(mount_id)`），不是骑手的；`SkillSource::SelfTaught` 的技能，冷却读写目标不变（骑手自己的）。**生物载具与物件载具的存储位置完全一样**——都是「某个 `EntityId` 对应的 `Agent.skill_cooldowns`」，不存在两套存储，这正是二节「都落在 `Arena<Agent>`」这个选型的直接红利，不需要为载具专门设计一套冷却存储。

**绝对到期时刻：核实成立，不是新裁定**——`skill_cooldowns` 存的从来就是到期 `Tick`（一节已核实，「惰性判定，不要求主动清理」），载具的冷却与骑手自己的冷却用的是**同一个字段类型、同一套既有语义**，没有新增任何机制。好处（无人骑乘的载具冷却也照常流逝）是这个既有惰性判定本身就自带的性质，不是为载具专门设计出来的。

**换乘同类载具不会绕过冷却：核实成立，是既有结构的自然结果**——冷却按 `ContentIndex` 键控在**各自的** `Agent.skill_cooldowns` 上（不同 `EntityId`），两辆载具即使 `mount_profile` 指向同一个 `MountDef`，各自的 `skill_cooldowns` 是两张完全独立的 `BTreeMap`，天然互不相干，不需要任何额外代码去「保证」这一点，是数据结构本身的直接推论。

**载具被摧毁时冷却随之消失，无悬挂引用**——`skill_cooldowns` 是坐骑 `Agent` 整体的一部分，坐骑被 `Arena::despawn` 时随整个 `Agent` 一起回收，不是独立存储，没有第二份需要单独清理的数据。唯一的悬挂风险是**骑手**一侧的 `mounted_on` 仍指向已销毁坐骑的陈旧 `EntityId`——这正是四节（原三节，现并入四节末）已经要求的：任何产出对坐骑 `Effect::Kill` 的 `resolve_*` 函数，必须在其之前配对追加 `Effect::Dismount { rider, mount: target }`，清空骑手的 `mounted_on`。

### 坐骑被杀死时骑手怎么办：resolve 侧配对产生 `Effect::Dismount`

**决策必须留在 `resolve`，不能让 `apply` 悄悄替 `Effect::Kill` 追加隐藏副作用**——这是「决策在 resolve」（约束 C1）的直接应用。任何会产生 `Effect::Kill { target }` 的函数，在产出 `Kill` 之前先查一次 `target` 的 `Agent.rider`，若非 `None`，追加一条 `Effect::Dismount { rider, mount: target }` 排在 `Kill` 之前。

**骑手死后的落点、是否受到摔落伤害——如实标注为开放问题，本文档不裁定**：`Effect::Dismount` 只保证骑手的 `mounted_on` 被清空、移动规则立刻恢复步行，骑手的 `pos` 保持在坐骑死亡那一刻两者共享的位置不变。

### 下船的安全阀：只允许下到非阻挡地形

水中主动下船的后果本文档不裁定，只给最小安全阀——`Intent::Dismount` 在 `resolve` 阶段检查骑手下船后落脚的格子（复用 `resolve_move` 的 `blocks_move` 判定，`mount_def` 视为 `None` 重新查一次），若目的地阻挡未骑乘的移动，静默作废这次下船尝试，与撞墙、被眩晕挡下同一条纪律（[行动能力与输入上下文](action-capability-and-input-context.md) 四节）。

### 明确不做：目标重定向

「攻击有一定概率打中坐骑而不是骑手」需要在瞄准形状展开阶段引入新的判定，超出最小形状，标为将来扩展（呼应九节）。

---

## 七、P6 必须先提供什么

以下是**载具落地前**必须先交付的清单——按本次三轮裁定重新核算，比原先估计的更早能落地（大量条目复用已落地的技能/属性修正基础设施）。

| # | 必须先有 | 状态 |
|---|---|---|
| 1 | `ItemDef`/`ItemStack` 定义与实例分离本身 | **未落地**，P6 尚未开工 |
| 2 | `Owner` 归属枚举 | **未落地** |
| 3 | 「物品变实体」转换路径（`ItemStack` → 占地 `Agent`，逆向亦然） | **未落地，本文档发现的最大缺口**——`item-system.md` 现有四种 `ItemLocation` 没有一种对应「已放置、占地、可进入」这类状态 |
| 4 | `ItemLocation::Ground` 的 30 日老化清理需对停靠的载具让路 | 一旦第 3 条落地，被放置的载具已经不是 `ItemStack`，天然不在 `Ground` 清理范围内——依赖第 3 条，不是独立工作 |
| 5 | `Agent` 新增 `mounted_on`/`rider`/`mount_profile`/`suspended_action_offset` 四个字段 | **未落地**，二/四节已给出形状，越早加入越省事 |
| 6 | `TerrainDef`/`TerrainTable` 新增 `surface: SurfaceKind` 列 | **未落地**，三节，是对已落地代码的一处扩展 |
| 7 | `Effect` 新增 `Mount`/`Dismount`（`PlaceVehicle` 见第 3 条） | **未落地** |
| 8 | 调度器复用 `Timeline::remove`/`schedule`——**已核实两者都已落地**，四节已给出具体接线，不需要新增队列机制 | **已满足**，本文档改变了原先「需要核实 `TurnEngine`」的判断——真正需要的方法已经存在 |
| 9 | `resolve_attack` 学会读 `active_stat_modifiers` | **未落地，但不是载具专属**——这是 [三轴战斗结算](combat-three-axis.md) 已经点名的既有缺口，载具的攻击加成是它的第三个消费者（技能、未来装备、载具），载具落地前这条必须先补，但补的工作量不该记在「载具专属」账上 |
| 10 | `derive_stats`「状态效果」入参的消费逻辑（`StatBonus` 缺口 5） | **不阻塞载具的攻击加成/技能授予**，只阻塞载具的**防御**加成——六节已如实标注，防御加成字段可以先声明（八节），实际生效需要等这条 |

**明确不阻塞载具落地的两项**：

- **`SlotMask`/`EquipSlot`（装备栏位系统）**——载具不占用任何装备槽位，骑乘是独立于「穿戴」的另一种关系（二节已论证）。
- **技能/属性修正的底层存储与哈希**——**已经落地**，一节已核实 `unlocked_skills`/`skill_cooldowns`/`active_stat_modifiers` 全部已经存在且进 `hash()`，载具直接复用，不需要为此新增任何存储或哈希代码（除四节明确要求的四个新 `Agent` 字段本身）。

---

## 八、mod 可注册性：`register-vehicle`

### 签名

```scheme
(register-vehicle "lostland:rowboat"
  3 2          ;; footprint: 占地宽 高
  24 40        ;; pivot: 图像内锚点像素 x y（仅 renders-rider=#f 时，坐骑自身精灵仍用它；
               ;;   renders-rider=#t 时，骑手精灵走自己原有的 Footprint/Pivot，不复用这一对）
  #f           ;; autonomous?：船=#f，若为生物坐骑则=#t（配合正常的 race/行为脚本注册）
  #f           ;; renders-rider?：船=#f（不画骑手），马=#t（画骑手）
  1            ;; grants-passage：SurfaceKind 位掩码，1 = WATER
  120          ;; surface-move-cost：在被特批的地表上移动的代价
  '()          ;; attack-modifier：'() 表示无，或 '(strength 5) 这样的 (属性 增量) 对
  0            ;; defense-modifier：整数，0 表示无——字段先声明，实际生效依赖七节第 10 条
  '("lostland:bola_throw"))  ;; granted-skills：本载具授予骑手的技能 ID 列表，需已通过 register-skill 注册
```

返回 `Result<bool, String>`——与既有全部 `register-*` 同一模式。`register-vehicle` 只声明 `MountDef`（骑乘相关的纯数据），**不声明生物性**——马依然走既有的种族/行为注册路径拿到 AI 与属性，`mount_profile` 只是一个额外挂在马的 `Agent` 实例上的引用。

### 档位：一档，理由按 [ADR 0018](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) 三步判据

1. **有没有设计自由度**——有（占地、速度、可穿越地表、攻防加成、授予的技能都因载具而异）。
2. **自由度落在算法还是数据上**——纯数据。`attack-modifier`/`defense-modifier`/`granted-skills` 都是注册期一次性交出的值，**不依赖任何运行期才存在的输入**——授予技能只是「这个 `ContentIndex` 集合里有没有某个 ID」的静态成员判断（六节 `skill_source`），不是每次调用都要重新计算的公式，与 `move_cost[某种地形]` 同一类静态查表值。
3. **调用频率**——移动/战斗/渲染热路径都会读，但读的是注册期已经物化好的表，`O(1)`/`O(k)`（`k` = 授予的技能数，通常个位数）访问。

**结论：一档，`MountTable` 与 `TerrainTable` 同一物化方式**——按 `ContentIndex` 下标存各字段的定长列（`granted_skills: Vec<Vec<ContentIndex>>` 这类变长字段与 `TerrainDef.opens_into` 解析成 `Option<TerrainKind>` 是同一类「注册期解析成内部引用，运行期零解析开销」的处理方式）。注册期校验：`footprint` 宽高非零、`surface_move_cost` 非零非 `u32::MAX`、`attack-modifier`/`defense-modifier` 数值落在合理范围（内容设计范畴，不定案）、`granted-skills` 里的每个 ID 必须已经通过 `register-skill` 注册（查不到即报错，与既有 `register-*` 对交叉引用的处理一致）。

### `SurfaceKind` 位分配：复用既有先例

本体保留低位，高位留给 mod 扩展，规则与 [装备栏位](equipment-slots.md) `SlotMask` 剩余位、[行动能力与输入上下文](action-capability-and-input-context.md) `ActionCapability` 高四位完全同一个先例。

---

## 九、明确排除的范围（不设计过头）

- **多人载具**（马车、多座位船）——「关系」目前是单一 `rider`/`mounted_on` 一对一字段，扩展成多座位需要重新设计。
- **载具改装/升级**——载具属性完全由 `MountDef`（内容注册）决定，不存在「同一个 `ContentIndex` 的载具因为玩家投入资源而属性变化」这类个体差异化。
- **载具驯养/繁殖**——本文档不做，属生物系统本身的问题，与「骑乘关系怎么表达」正交。
- **目标重定向**（攻击有概率打中坐骑而非骑手）——见六节末尾，需要瞄准形状展开阶段的新判定，超出最小形状。
- **载具耐久/维修**——一旦「物品变实体」路径落地，载具理论上可以复用[物品系统](item-system.md)已有的耐久字段，但「变成 `Agent` 之后耐久怎么体现、船体破损是否影响 `grants_passage`/攻防加成」需要额外设计，本文档不做。

---

## 十、开放问题（如实标注，不强行圆）

1. **`resolve_attack` 读取 `active_stat_modifiers` 的具体聚合公式**——本文档只要求它必须发生（七节第 9 条），不设计聚合的具体形状（例如多个来源的修正是否需要区分优先级），这属于 [三轴战斗结算](combat-three-axis.md)/`derive_stats` 本身的既有待办。
2. **`derive_stats`「状态效果」入参的消费逻辑**（防御加成的真正生效路径）——同上，不是载具专属，六节已如实标注。
3. **多格实体的碰撞/寻路是否已支持**——五节已指出这是继承自 [种族系统](race-system.md) 十二节的既有未验证项。
4. **水中被迫下船/坐骑死亡后的具体后果**——六节两处标注为「本文档只给安全阀，不裁定后果」。
5. **`attack-modifier`/`defense-modifier` 的具体数值区间、`granted-skills` 的数量上限**——内容设计范畴，本文档不定案，只给字段形状。
6. **世界范围内载具数量的规模假设**——本文档假设「玩家实际拥有/骑乘的少量载具」，与厚层「数百个」规模假设相符；若未来出现「每个渔村都有二十条船」这类数量级，需要重新评估，本文档不解决这个规模问题。

---

## 相关文档

- [物品系统](item-system.md) — `ItemDef`/`ItemStack`/`Owner`/`ItemLocation`，七节指出的「物品变实体」缺口
- [装备栏位与占位掩码](equipment-slots.md) — `SlotMask` 位标志先例，本文档 `SurfaceKind` 位分配方式照抄；七节「明确不阻塞」的一项
- [属性系统](attribute-system.md) — `effective_speed_from_dexterity`（四节坐骑速度直接复用）、`derive_stats`「状态效果」入参（六节防御加成的接线点）
- [三轴战斗结算](combat-three-axis.md) — `resolve_attack` 占位实现现状、`Effect::RecordHistoricalEvent`「决策在 resolve」先例（六节坐骑死亡处理直接复用同一条纪律）
- [伤害公式 mod API](damage-formula-mod-api.md) — 三节「一档 vs 二档」判据的对照参照（本文档 `MountDef` 全部落一档，与该文档 `FormulaDef` 必须落二档的理由恰好相反）
- [增益与通用触发器](buffs-and-triggers.md) — `ActiveEffect.expires_at` 绝对到期时刻的先例，六节冷却机制与四节挂起偏移量的设计参照
- [种族系统](race-system.md) — 体型/`footprint` 十二节的未验证项，三节、五节两处继承
- [行动能力与输入上下文](action-capability-and-input-context.md) — 「调度层不生成 `Intent` 即可让实体完全不行动」的既有先例，四节直接复用；`ActionCapability` 位标志与 mod 扩展位分配方式，八节引用
- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) — `EntityId` 寻址厚层的既有假设，二节否决 `Arena<Prop>` 方案时引用
- [0004 — 两层实体存储替代 ECS](../decisions/0004-two-layer-entity-storage.md) — `EntityId` 跨存储不唯一的既有事实，二节核心论证的直接依据
- [0016 — mod 性能分档按声明方式，不按作者身份](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017 — 声明式分档物化为列式数据](../decisions/0017-tiered-declarations-materialize-columnar.md) — 三节、八节的分档论证
- [0018 — 引擎层与玩法层脚本边界](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) — 八节三步判据
- [0021 — 抽象的理由是有算法要共享，不是看起来该对称](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) — 二节核心论证直接引用
- [0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 三节、四节确定性影响一节的直接依据
- `crates/ll-world/src/terrain.rs`（`TerrainTable`/`TerrainDef`，已落地，三节的扩展对象）
- `crates/ll-sim/src/resolve.rs`（`resolve_move`/`resolve_attack`/`resolve_use_skill`，已落地，三、四、六节的扩展对象）
- `crates/ll-sim/src/timeline.rs`（`Timeline::remove`/`schedule`，已落地，四节直接复用）
- `crates/ll-world/src/entity/stats.rs`/`agent.rs`（`active_stat_modifiers`/`unlocked_skills`/`skill_cooldowns`，已落地，六节直接复用）
- `crates/ll-world/src/state.rs`（`WorldState::hash()` 的每-`Agent` 哈希循环，已落地，四节指出新字段的写入位置）
- `crates/ll-render/src/sprite.rs`（`Footprint`/`Pivot`/`DrawOrder`，已落地，五节的复用/扩展对象）
- `crates/ll-world/src/entity/id.rs`（`EntityId`，已落地，二节否决方案的直接依据）
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) §5（crate 分层）、§8（时间轴调度器）
