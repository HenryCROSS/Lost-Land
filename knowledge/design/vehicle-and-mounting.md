# 载具与骑乘系统

**冻结于** 2026-08-20。**落地状态**：纯设计，`crates/` 中无任何对应类型——已核实：`ItemDef`/`ItemStack`/`Owner`/`ItemLocation`（[物品系统](item-system.md)）、`SlotMask`/`EquipSlot`（[装备栏位](equipment-slots.md)）全代码库检索无匹配；`Agent` 没有 `mounted_on`/`rider`/`mount_profile`/`suspended_action_offset` 字段；`TerrainDef`/`TerrainTable` 没有地表分类；`ll-sim::effect::Effect` 没有 `Mount`/`Dismount`/`PlaceVehicle` 变体；`Agent.skill_cooldowns`/`active_stat_modifiers`/`unlocked_skills`/`Timeline::remove`/`ExplorationMemory` 的 `Vec<u64>` 位图**均已落地**（见一节，本文档大量复用它们）。**实现阶段**：P6（物品与装备）——项目所有者原话点名「这将会是物品与装备的一部分」，但 `Agent` 新增字段与既有先例（种族、职业技能）同样的理由，宜尽早落地。**冻结时对应 git 提交**：`7ec16b9`（`main` 分支，本文档写作时的仓库 HEAD）。

**本文档只给出「能支撑一匹马和一条船」的最小形状**，多人载具、马车、载具改装、载具驯养/繁殖、载具战斗深度定制（除本文档已裁定的攻防加成/技能授予外）明确排除在外，见九节。

---

## 零、项目所有者的要求（原话，含四轮追加裁定）

> 「能不能存在一个载具系统，这东西的贴图会覆盖在人物贴图的下半部分。这样就存在马，牛，船之类的东西，并且存在某些功能，例如提供掩体，跨域海洋等等功能，你看看怎么设计，这将会是物品与装备的一部分，因为有的可能是能自己动的生物，而有的是被制作出来，可以被放置在地图某处。」

第一轮追加（掩体的真实语义）：

> 「提供掩体或者说作为一种装备，提供防御或者攻击属性」「也或者提供一种远程攻击的手段，当然也会有增加攻击属性」

第二轮追加（回合经济）：

> 「骑乘时，移动回合肯定是算玩家的，马的属性会被切换，也就不在时间轴内了。等下马以后再重新回到时间轴。」

第三轮追加（能力授予与渲染的两条裁定）：

> 「1.技能冷却跟随载具。2.我同意你的『有效技能 = 已学会的 ∪ 当前载具授予的』」「渲染的话，有的载具可以选择渲染人物，有的不渲染，这样就算覆盖人物了也没问题了」

第四轮追加（配置自由度与渲染开关的进一步收紧）：

> 「我在想，船和马的区别就是属性的不同，以及离开以后马会动船不会。我希望在添加载具作为装备的方面能给足配置的自由度，例如新的东西是否能下水之类的，以及添加什么技能和属性。」「以及是否画出人物或者是否画出载具这方面也给出配置项。」

这些追加把原本模糊的「掩体」「贴图覆盖」「配置自由度」逐步钉死成具体机制，本文档正文按最终裁定写，不重复展示中间被推翻的方案（除非该方案的否决理由本身有复用价值）。

---

## 一、现状核实（写作本文档前已去代码核实）

- **物品/装备系统全部是纯设计**：[物品系统](item-system.md)、[装备栏位与占位掩码](equipment-slots.md)全代码库检索无 `ItemDef`/`ItemStack`/`SlotMask` 匹配。P6 尚未开工。
- **地形通行性是静态查表**：`crates/ll-world/src/terrain.rs` 的 `TerrainTable::blocks_move`/`move_cost` 是按 `ContentIndex` 下标的纯列式查表，注册期一次性 `define`，运行期 `O(1)` 数组访问。`ContentIndex` **是一个全局共享号段**——地形、技能、种族……未来的表面分类全部从同一个 `Interner`/`Registry` 里分配，不是「地形专属的连续编号」（`terrain.rs` 模块文档原话），这一点直接决定了三节 `SurfaceKind` 不能直接拿 `ContentIndex.get()` 当位图下标（会导致位图大小等于整个内容空间）。
- **`resolve_move` 的真实形状**（`crates/ll-sim/src/resolve.rs:409` 起）：撞墙仍消耗 `action_cost(BASE_ACTION_COST, speed)`；可通行时产生 `MoveTo` + `ScheduleNext`；`speed` 全程来自 `effective_speed_from_dexterity(agent.stats.dexterity)`。
- **`resolve_attack` 仍是占位实现，攻防聚合尚未接线**（`crates/ll-sim/src/resolve.rs:495`）：攻击力直接读原始 `BaseStats` 字段，防御恒为 `0`，`Agent.active_stat_modifiers` 完全没有被读取。这不是本文档新发现的缺口，是 [三轴战斗结算](combat-three-axis.md) 已经点名、独立于载具存在的既有缺口。
- **`Agent.active_stat_modifiers: BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>` 已经落地**（`crates/ll-world/src/entity/stats.rs:82`、`agent.rs:207`），已进 `WorldState::hash()`（`state.rs:929`）。**（更正，提交 `883d572`）此前这里记录的是单层 `BTreeMap<AttributeKind, ActiveStatModifier>`、按 `AttributeKind` 键控、后写覆盖先写——`buffs-and-triggers.md` 六节裁定「不同效果能叠加，同效果只刷新时间」后，该记录已过时：现在是按 `(属性, 来源)` 两层键控，外层 `AttributeKind`，内层「来源」的 `ContentIndex`；不同来源各自独立存在、聚合时求和（`resolve_attack` 一类读取路径逐条过滤未过期条目再求和），同一来源再次施加时走 `ActiveStatModifier::merge_same_source`——强度取 `|delta|` 较大者、到期时刻取 `.max()`（不是两段时长相加），两个维度独立比较。** **`AttributeKind` 只有六个定长变体**（`Strength`/`Dexterity`/`Constitution`/`Intelligence`/`Willpower`/`Charisma`），**没有对应「护甲/防御」的变体**——`DerivedStats.armor` 按 [属性系统](attribute-system.md) §七来自 `derive_stats(基础属性, 装备, 状态效果, 负重)`，不是某个 `AttributeKind` 的直接映射。
- **`Agent.unlocked_skills`/`Agent.skill_cooldowns` 已经落地**，均已进 `WorldState::hash()`（`state.rs:922`/`924`）。「能不能放某技能」的真实查询路径只有一处：`resolve_use_skill`（`resolve.rs:684`）。
- **`Timeline::remove(actor: EntityId)`/`Timeline::schedule(actor, at)` 已经落地**（`crates/ll-sim/src/timeline.rs:87`/`72`），`remove` 用 `BinaryHeap::retain`，与插入历史无关，满足约束 C5，且已经是「死亡清理残留条目」的既有用途。
- **`EntityId` 不是跨存储唯一的**（`crates/ll-world/src/entity/id.rs` 模块文档）——二节据此否决「新开一个实体池装船」的方案。
- **稠密位图的既有先例：`ExplorationMemory`（`crates/ll-world/src/exploration.rs`）**——`ZoneExploration { bits: Vec<u64> }`，`local_bit_index` 把「区块内坐标」换算成一个**稠密局部下标**（不是全局 `ContentIndex`），`mark`/`get` 用 `word = index / 64, bit = index % 64` 做位操作，位图按需 `resize`（越界自动扩容）。**三节 `SurfaceKind` 的位集直接照抄这一套**——差异只在「稠密下标换算的对象」从「区块内坐标」换成「表面分类在注册期被分配到的序号」。
- **精灵渲染的既有原语**（`crates/ll-render/src/sprite.rs`）：`Footprint`（占地格数）与 `SpriteSize`/`Pivot`（视觉像素尺寸/锚点）刻意解耦；`footprint_anchor_pixel`/`sprite_draw_position` 是唯一被允许的锚点换算路径；`DrawOrder`（`Layer` + 屏幕 `foot_y` + `entity: u64`）是唯一的绘制顺序键。
- **`mod-lifecycle-and-event-api.md` 已经设计了 `require-content!`/`content-exists?`**，专门解决「引用了一个当前会话没有加载的 mod 内容」这类装载期悄悄放行、运行期才失效的问题——六节直接引用。

---

## 二、核心判断：载具是关系，不是实体类型——采纳，给出精确形状，并钉死一条不变式

### 复核项目所有者的初判

**采纳「骑乘是关系」这个结论**——关系的两端必须落在同一个可寻址空间里才有意义，`EntityId` 不是跨存储唯一的（一节已核实），若骑手与坐骑分别落在两个不同的实体池，`rider.mounted_on: EntityId` 本身就无法告诉你该去哪个池里查。

### 被否决的方案：新开一个 `Arena<Prop>`

否决理由不变：会打破 `Intent::Attack`/`Effect::Damage`/脚本句柄全部隐式假设「一个 `EntityId` 就指向厚层 `Agent`」这条前提，波及面远超「载具」这一个系统。

### 最终形状：一个实体存储，一份可选的关系数据

**马与船都落在既有的 `Arena<Agent>`。** 二者的差异不靠 Rust 类型系统区分，靠内容注册表数据（`MountDef`）区分——[ADR 0021](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) 的正面应用。

```rust
// Agent（新增四个字段，其余不变）
pub struct Agent {
    // …… 既有字段不变
    pub mounted_on: Option<EntityId>,
    pub rider: Option<EntityId>,
    pub mount_profile: Option<ContentIndex>,
    pub suspended_action_offset: Option<i64>,
}
```

### 不变式：代码里不得出现「这是马还是船」的身份判断——项目所有者已把这条写成验收标准

裁定原话：「船和马的区别就是属性的不同，以及离开以后马会动船不会」——**这句话本身就是一条可执行的验收条件**：如果设计是对的，代码里就不该有任何一处写着「若这个坐骑是船……否则（是马）……」这类比较 `NamespacedId`/`ContentIndex` 具体身份的分支。所有行为差异必须能追溯到 `MountDef`（或 `Agent` 自身携带的 `stats`）的某个字段**取值**，不能追溯到「这个实体具体是哪一个」这件事本身。

**逐条核实五条路径**：

| 路径 | 驱动字段 | 是否存在身份分支 |
|---|---|---|
| 通行（三节） | `MountDef.grants_passage`（位集） | 否——查表结果只取决于位集内容，不取决于 `mount_profile` 指向谁 |
| 渲染（五节） | `MountDef.renders_mount`/`renders_rider`（两个布尔） | 否——四种组合都是同一段代码，只是两个布尔各自取值不同 |
| 技能（六节） | `MountDef.granted_skills`（列表） | 否——`skill_source` 只做集合成员判断 |
| 属性（六节） | `MountDef.stat_modifiers`（列表） | 否——纯数据驱动的叠加 |
| 时间轴进出（四节） | `MountDef.autonomous`（布尔） | **原设计有一处遗漏，现已修正，见下** |

**修正的遗漏**：原设计只在「spawn 时要不要把这个实体排入时间轴」这一处消费 `autonomous`，但 `Effect::Dismount` 的重入逻辑（四节②）如果无条件地把坐骑重新 `schedule` 回时间轴，就会让一条 `autonomous == false` 的船在下船后被意外插入队列——这**不是**因为代码里写了「如果是船就……」，而是因为**漏掉了对同一个既有字段的检查**，效果上等同于制造了一个只有船会踩中的分支。修正：`Effect::Mount`/`Effect::Dismount` 的时间轴操作全部包一层 `if mount_def.autonomous { … }`（四节②给出具体代码），这仍然是「读同一个数据字段」，不是身份分支——`autonomous == true` 的船（如果 mod 真造了一条会自己漂流的船）会被正确地排入/移出时间轴，`autonomous == false` 的马（如果 mod 真造了一匹完全不会自己行动的木马）也会被正确地排除在外，行为完全由字段值决定，不由「这个 `ContentIndex` 具体是不是叫 `lostland:horse`」决定。

---

## 三、通行性：有条件的地形穿越，`SurfaceKind` 走内容索引 + 装载期定长位集

### 为什么否决定宽位标志（`SlotMask`/`ActionCapability` 那一套先例在这里不适用）

本文档更早的版本把 `SurfaceKind` 设计成一个 `u8` 定宽位标志（本体占低位、4 位留给 mod），仿照 `SlotMask`/`ActionCapability`——**这条先例在这里不成立，理由是这两个既有场景的可扩展项天然有限**：装备槽位统共 22 个，`ActionCapability` 封闭在移动/攻击/施法/用物品四类，「留几位给 mod」是一个可以预估上限的问题。**地表分类不是**——熔岩、云层、流沙、酸液、深渊、蛛网、沼泽……一个整合包里五个 mod 各加三种就能把 10 个预留位吃光，且**位号依赖装载顺序**：新增一个 mod 会让它之后所有 mod 的位号整体后移，若 `SurfaceKind` 参与任何持久化或内容哈希，装载顺序一变就产生不必要的失效——这条风险是定宽方案本身带不走的。

### 改用的方案：表面种类走 `Registry::intern`，和地形/技能/种族完全一样

```scheme
(register-surface-kind "lostland:water")
```

`register-surface-kind` 是第八个 `register-*` 内容注册函数（见八节），**内部就是一次 `intern` + 一次「登记为已定义」**，与 `register-terrain`/`register-race` 走的是完全相同的路径：`NamespacedId → ContentIndex`，无上限、无冲突（命名空间隔离天然避免撞号）、确定性由既有的 `topo_sort` 保证（mod 加载顺序已经是确定性总序，`ContentIndex` 分配顺序随之确定，不需要为表面分类另设一套顺序保证）。

### 位集：稠密局部下标 + `Vec<u64>`，照抄 `ExplorationMemory` 的既有技法

**不能直接拿 `ContentIndex.get()` 当位图下标**——`ContentIndex` 是全局共享号段（一节已核实），地形、技能、种族都在同一个号段里分配，若位图下标直接等于 `ContentIndex.get()`，位图长度就要撑到「整个内容空间」那么大，绝大多数位永远用不上。

**正确做法：给「表面分类」单开一份稠密计数，与 `ContentIndex` 的全局号段脱钩**——这正是一节已核实的 `ExplorationMemory` 用的手法（`local_bit_index` 把「区块内坐标」换算成稠密局部下标，不是拿世界坐标直接当下标）：

```rust
// crates/ll-world/src/terrain.rs（设计，与 TerrainTable 同一模块）

/// 表面分类注册表：`ContentIndex`（全局号段的一个成员）→ 稠密局部
/// 位下标（0, 1, 2, ……，按 register-surface-kind 调用顺序连续分配，
/// 不留空洞）。这份换算只在注册期发生一次，运行期的 grants_passage
/// 检查只消费换算完的位下标，不重复查这张表。
#[derive(Debug, Default, Clone)]
pub struct SurfaceKindTable {
    /// 按 ContentIndex.get() 下标——是否已注册为表面分类。
    defined: Vec<bool>,
    /// 按 ContentIndex.get() 下标——对应的稠密位下标，未注册时为 None。
    dense_bit: Vec<Option<u32>>,
    next_bit: u32,
}

impl SurfaceKindTable {
    /// 注册期入口：给一个已经 intern 出来的索引分配一个稠密位下标。
    /// 与 `TerrainTable::define` 同一条纪律：重复注册报错，不静默覆盖。
    pub fn define(&mut self, index: ContentIndex) -> Result<u32, SurfaceKindError> { /* … */ }

    /// 查询：这个 ContentIndex 是否已注册为表面分类；若是，返回它的
    /// 稠密位下标——供 TerrainTable::define（六节「明确不阻塞」之外
    /// 的一处新校验）与 MountTable::define（八节）在**各自的注册期**
    /// 解析引用时调用。
    pub fn dense_bit_of(&self, index: ContentIndex) -> Option<u32> { /* … */ }
}

/// 一组表面分类的位集——MountDef.grants_passage 的类型，与
/// `ZoneExploration::bits` 同一个形状：`Vec<u64>`，按需 resize，
/// word = bit_index / 64，bit = bit_index % 64。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceSet(Vec<u64>);

impl SurfaceSet {
    pub fn set(&mut self, dense_bit: u32) {
        let (word, bit) = (dense_bit as usize / 64, dense_bit as usize % 64);
        if word >= self.0.len() { self.0.resize(word + 1, 0); }
        self.0[word] |= 1u64 << bit;
    }
    /// 判定某个具体表面（已换算成稠密位下标）是否在集合里——三节
    /// resolve_move 热路径用的就是这一个方法，O(1)，一次数组访问 +
    /// 一次位与。
    pub fn contains(&self, dense_bit: u32) -> bool {
        let (word, bit) = (dense_bit as usize / 64, dense_bit as usize % 64);
        self.0.get(word).is_some_and(|w| w & (1u64 << bit) != 0)
    }
}
```

**测试仍是 `O(1)`，但没有位数上限**——`Vec<u64>` 按需 `resize`（与 `TerrainTable::define` 遇到更大下标时 `resize` 的既有做法同一手法），五个 mod 各加三种表面只是让某些 `SurfaceSet` 多占几个 `u64` 字，不存在「10 位用完」这类硬上限。

`TerrainDef` 新增一个 `surface: Option<ContentIndex>` 字段（本体地形沿用现有 17 种，绝大多数留 `None`，`lostland:deep_water`/`lostland:shallow_water` 声明为 `Some("lostland:water")`）；`TerrainTable` 在 `define` 时把这个 `ContentIndex` 解析成稠密位下标缓存起来（查 `SurfaceKindTable::dense_bit_of`，查不到即注册失败——六节详述这条校验为什么必须是硬错误），运行期只存查好的稠密位下标，不重复解析。

### `resolve_move` 的新分支

```rust
// crates/ll-sim/src/resolve.rs（设计）
let mover = agent.mounted_on.and_then(|id| world.actors.get(id));
let mover_speed = effective_speed_from_dexterity(
    mover.map_or(agent.stats.dexterity, |m| m.stats.dexterity)
);
let mount_def = mover.and_then(|m| m.mount_profile).and_then(|i| world.mount_table.get(i));

if terrain.blocks_move(&world.terrain_table) {
    let terrain_surface_bit = terrain.surface_dense_bit(&world.terrain_table); // Option<u32>
    let passable = mount_def.zip(terrain_surface_bit)
        .is_some_and(|(def, bit)| def.grants_passage.contains(bit));
    if !passable {
        let cost = action_cost(BASE_ACTION_COST, mover_speed);
        return vec![Effect::ScheduleNext { actor, at: schedule_after(world, cost) }];
    }
    let cost = action_cost(
        mount_def.unwrap().surface_move_cost(terrain_surface_bit.unwrap()),
        mover_speed,
    );
    let mut effects = vec![
        Effect::MoveTo { actor, pos: dest },
        Effect::ScheduleNext { actor, at: schedule_after(world, cost) },
    ];
    if let Some(mount_id) = agent.mounted_on {
        effects.push(Effect::MoveTo { actor: mount_id, pos: dest });
    }
    return effects;
}
// …… 原有可通行分支不变，只是 speed 换成 mover_speed
```

**对未骑乘的绝大多数移动，这条新分支零开销**——`agent.mounted_on` 是 `None` 时 `mover`/`mount_def` 直接短路，普通移动只多一次 `Option` 判断，与位集本身的大小无关。

### 按表面分列的移动耗时：`BTreeMap<ContentIndex, u32>`，不是位集能表达的东西

**位集只能回答「能不能过」，回答不了「多快」**——这是项目所有者明确点出的一处缺口（「船在水上快、在陆上应该很慢，一个数表达不了」）。移动耗时因此**不**跟着位集走稠密下标（没必要——这份表通常只有几个条目，不是需要 `O(1)` 位测试的高频集合运算），改用 `BTreeMap<ContentIndex, u32>`（键是表面分类自己的 `ContentIndex`，不是稠密位下标；`BTreeMap` 而非 `HashMap` 是约束 C5 的既有纪律，`ContentIndex` 已实现 `Ord`）：

```rust
pub struct MountDef {
    // ……
    pub grants_passage: SurfaceSet,
    /// 每个被特批穿越的表面各自的移动代价——键必须是 grants_passage
    /// 里出现过的每一个表面（且仅这些），注册期校验双向自洽（见八节），
    /// 与 TerrainTable::define 现有的 blocks_move/move_cost 自洽校验
    /// 同一条纪律。
    pub surface_move_costs: BTreeMap<ContentIndex, u32>,
}
```

一条船可以声明 `surface_move_costs = { water: 100 }`（在水上飞快）；若船也能勉强上岸（`grants_passage` 额外加一个 `land_shallow` 之类的表面），可以给这个表面单独声明一个很高的代价，`一个数表达不了` 这条限制因此被解除。

### 跨命名空间引用必须配合 `require-content!`——顺带堵一个真实存在的坑

**具体风险**：mod B 的载具声明 `(grants-passage "examplemod:lava")`，但 mod A（`examplemod`，本该定义 `lava` 这个表面）没有被安装或没有被启用。`Registry::intern` 对任意合法命名空间字符串都会照单全收、返回一个索引——**装载期一声不吭**，这个索引对应的表面从未被任何 `register-surface-kind` 定义过（`SurfaceKindTable::dense_bit_of` 查不到），**运行期悄悄失效**：这个坐骑的 `grants_passage` 里那一位永远不会与任何真实存在的地形匹配（因为没有任何地形会声明属于一个从未被定义过的表面），载具作者以为自己造了一辆能过熔岩的车，实际上造了一辆哪儿都过不去的车，且没有任何报错提示。

**这是[扩充给 mod 的 Steel 脚本 API](mod-lifecycle-and-event-api.md) `require-content!`/`content-exists?` 本来就是为它设计的那类坑**——本文档不重新设计这两个原语，只明确一条使用要求：**`register-vehicle` 的 `grants-passage`/`surface-move-costs` 若引用了非本 mod 自身命名空间的表面分类 ID，必须在同一个 mod 里先调用一次 `(require-content! "examplemod:lava")` 声明依赖**。不这样做的后果如上——不是报错，是静默构造出一个部分失效的载具，比报错更难排查。

**这条要求本身不能替代注册期硬校验**——`require-content!` 解决的是「mod A 整个没装」这一类问题（在依赖解析阶段就能发现），但即使 mod A 装了，mod A 也可能因为版本不同而把 `lava` 改了名字或压根没定义（拼写错误一类）。因此**注册期还需要第二层校验**：`MountTable::define`（八节）在解析 `grants-passage`/`surface-move-costs` 里的每一个表面 `ContentIndex` 时，必须调用 `SurfaceKindTable::dense_bit_of` 确认它**确实**被某次 `register-surface-kind` 定义过，查不到就直接拒绝这次 `register-vehicle`（硬错误，不是警告）——与 `TerrainTable::define` 现有的「不自洽就拒绝注册」同一条纪律。两层校验互补：`require-content!` 在依赖解析阶段拦住「整个 mod 缺失」，`MountTable::define` 在内容注册阶段拦住「mod 在但这个具体 ID 没有被正确定义」。

### 性能代价与确定性：结论不变

一档（ADR 0016/0017）：`grants_passage`/`surface_move_costs` 全部是注册期一次性声明，运行期 `O(1)` 位测试/`BTreeMap` 查小表。`SurfaceKindTable`/`TerrainTable`/`MountTable` 全部是内容注册表，**不进 `WorldState::hash()`**——与既有 `TerrainDef`/`RaceDef` 处理方式一致，`MountDef`（含它的 `grants_passage`/`surface_move_costs`）同理不进 `hash()`。真正需要进 `hash()` 的仍然只是二节已给出的四个新 `Agent` 字段（`mounted_on`/`rider`/`suspended_action_offset` 必须进，`mount_profile` 可选，见四节）。

### 继承的一处未验证风险，不在本文档解决

[种族系统](race-system.md) 十二节已经标注「2×2 `footprint` 的碰撞与寻路是否已支持……未经核实」，本文档不重复验证，五节会再次提到。

---

## 四、回合经济：骑乘时谁在行动

### ① 退出/重入时间轴：复用既有的 `Timeline::remove`/`schedule`，且必须用 `autonomous` 守卫

**一节已核实：`Timeline::remove(actor)`/`schedule(actor, at)` 已经落地。** 二节「不变式」一节已经指出并修正了一处遗漏——`Mount`/`Dismount` 的时间轴操作必须全部包一层 `if mount_def.autonomous`：

```rust
// crates/ll-sim/src/apply.rs（设计，Effect::Mount 分支）
if let Some(mount_def) = world.mount_table.get(mount.mount_profile) && mount_def.autonomous {
    let offset = (mount.next_action_at.0 - world.clock.0).max(0);
    mount.suspended_action_offset = Some(offset);
    timeline.remove(mount_id);
}
mount.mounted_on = None; // 坐骑没有"骑乘"字段本身的反向语义，这里指 rider/mounted_on 双写
rider.mounted_on = Some(mount_id);
mount.rider = Some(rider_id);
```

```rust
// Effect::Dismount 分支
rider.mounted_on = None;
mount.rider = None;
if let Some(mount_def) = world.mount_table.get(mount.mount_profile) && mount_def.autonomous {
    let offset = mount.suspended_action_offset.take().unwrap_or(0);
    let new_at = Tick(world.clock.0 + offset);
    mount.next_action_at = new_at;
    timeline.schedule(mount_id, new_at);
}
```

**为什么这是必须的，不是可选的防御性代码**：若 `autonomous == false` 的船在 `Dismount` 时被无条件 `schedule` 回时间轴，它会违反三节/本节反复强调的「船天生不该出现在时间轴里」这条不变式——不是因为代码专门写了「如果是船」，是因为**漏掉了对同一个字段的检查**，效果上等价于制造了一个只有 `autonomous == false` 的实体才会触发的 bug。加上这层守卫后，`autonomous` 在整个骑乘生命周期里只被读取，从未被写入或以任何方式与「这是哪一种坐骑」的身份信息挂钩——它就是一个普通的布尔字段。

### ② 重入 tick：结论不变

`suspended_action_offset` 的计算与消费逻辑、「反复上下马不会累积或凭空产生行动机会」的证明、必须补的回归测试、必须进 `hash()`——均见本文档此前版本已给出的论证，未因本轮追加裁定而改变，此处不重复展开。

### ③ 哪些属性替换、哪些叠加：结论不变，六节给出配置形状

移动速度**替换**（骑乘期间读坐骑的 `stats.dexterity`）；攻击/防御/其余属性加成**叠加**，走六节新给出的通用 `stat_modifiers` 列表。

---

## 五、渲染：两个独立开关，四种组合，第三种组合是白捡的游泳

### 结论：`renders_mount`/`renders_rider` 两个独立布尔，不是一个「谁盖住谁」的枚举

项目所有者进一步要求「是否画出人物或者是否画出载具这方面也给出配置项」——**这不是给已有的 `renders_rider` 添一个镜像字段那么简单，它让整套设计多表达了一类此前没有覆盖到的东西**：

| `renders_mount` | `renders_rider` | 表达的是 |
|---|---|---|
| ✅ | ✅ | 马——人骑在上面，两个精灵都画 |
| ✅ | ❌ | 船——人在船里看不见，只画船 |
| ❌ | ✅ | **游泳、水上行走靴、飞行——载具本身没有贴图，只改规则** |
| ❌ | ❌ | 见下「都不画合不合法」 |

**第三行是白捡的，必须写进文档**：一个 `renders_mount = false`、`grants_passage = {water}`、`surface_move_costs = {water: 较高代价}` 的 `MountDef`，**就是游泳**——不需要为「游泳」单独发明第二套「临时改变通行规则」的机制。这是本设计比最初预想更通用的直接证据：三/四/六节给出的「载具是一份可选的关系数据，不是一种实体类型」这个结论，原来连「有没有一个看得见的坐骑」都不是必要前提——骑乘关系真正的核心是「一份挂在某个 `EntityId` 上的规则集合」，渲染只是这份规则集合恰好可以选择表现或不表现出来的一个维度。**将来若有人问「游泳系统怎么做」，答案就是「注册一个不渲染的 `MountDef`」，不需要另开一份设计文档。**

**一处如实标注的粗糙点**：游泳这类「坐骑就是骑乘者自己」的退化场景，会暴露「移动速度替换」规则（四节③）的一个不够精细之处——替换读的是坐骑自己的 `stats.dexterity`，而这个「游泳 `MountDef`」对应的 `Agent` 是一个独立个体，它的 `dexterity` 是内容作者写死的一个通用值，不会随骑乘者自己的敏捷高低而变化（一个敏捷 20 的角色和一个敏捷 8 的角色游泳速度会被这份设计做成一样快）。这不是架构问题，是数值预算问题——本文档不解决「游泳速度该不该个体化」，如实标注为将来若需要可以在 `Effect::Mount` 时按骑乘者自身属性动态生成/调整这份 `MountDef` 实例（需要额外设计），当前最小形状先接受这个简化。

### ① 「两个都不画」合不合法：允许，注册期给一条 `LoadStatus::Warning`

**结论：允许，不在注册期硬拒绝，但产出警告。**

**为什么不硬拒绝**：`renders_mount = false && renders_rider = false` 描述的是「这段骑乘关系在视觉上完全不可感知，但规则仍然生效」——这不是一个明显无意义的配置，是一类合法的设计意图（例如某种诅咒/附身效果：玩家的移动规则被悄悄改变，但故意不给出任何视觉提示，作为解谜或恐怖气氛的一部分）。硬拒绝会把这类合法用途一并挡在门外，而「合法但罕见」不构成拒绝的理由。

**为什么要警告**：这个组合**更常见的成因是内容作者漏配置**（忘了把两个布尔至少设一个为 `true`），而不是刻意设计——`LoadStatus::Warning` 已经是这套注册体系的既有产出路径（[伤害公式 mod API](damage-formula-mod-api.md)、[职业/技能树/副职/任务系统](class-skill-quest-system.md) 等既有 `register-*` 校验路径已经在用），本文档只是新增一条触发这条既有机制的规则，不新增机制本身。警告文案建议类似「`lostland:mymount`：`renders_mount`/`renders_rider` 均为 `false`——如果不是刻意设计成完全不可见的效果，这很可能是遗漏配置」。

### ② `composite_order`：只在两个开关同时为真时才需要偏离默认值，判据是这次绘制的组合，不是任何一方的身份

五节此前给出的 `DrawOrder` 新增字段 `composite_order: u8`，其存在的唯一理由是「坐骑与骑手共处一格、又要同时绘制」这一种场景（第一行）下的层序竞争。**其余三种组合不需要它，且不应该被它影响**：

- 只画坐骑（第二行）：坐骑的 `composite_order` 取与其余普通实体完全相同的中性默认值——它此刻不需要和任何「同格另一个精灵」竞争层序，没有理由偏离默认值。
- 只画骑手（第三行）：骑手的 `composite_order` 同样取中性默认值，理由相同。
- 都不画（第四行）：无绘制发生，`composite_order` 无意义。

**判据必须是「这一次绘制，坐骑与骑手是否同时被提交」（即 `renders_mount && renders_rider` 同时为真），不是「这个实体是不是一个坐骑」这个身份本身**——这正是二节「不变式」要求的同一条纪律在渲染层的又一次体现：`composite_order` 是不是要偏离默认值，取决于**这一次具体的组合**，不取决于 `mount_profile` 指向的是哪一种 `MountDef`。实现上，这意味着「要不要给坐骑/骑手各自赋一个非默认的 `composite_order`」这个判断，应该写在**渲染层收集这一帧要绘制的精灵列表**那一步（能同时看到 `renders_mount`/`renders_rider` 两个布尔的地方），而不是写死在 `MountDef` 的某个字段里（`composite_order` 的取值不需要、也不应该是注册期声明的一部分）。

### ③ `renders_mount = false` 时，`Footprint` 还生不生效——生效，渲染开关与占地判定完全正交

**结论：`Footprint` 字段本身与 `renders_mount` 完全独立，`renders_mount = false` 不改变 `Footprint` 在（未来落地的）碰撞/占地判定里的行为。**

`renders_mount` 只回答「这一帧要不要提交这个精灵的绘制调用」，`Footprint` 回答的是完全不同的问题——「这个实体在世界里占几格」，两者没有因果关系。**这正是项目所有者点名的、值得单独说清楚的风险**：若把 `renders_mount = false` 误当成「这个坐骑在游戏世界里不存在」，会推出错误的结论「既然不存在，占地也该是零」——但坐骑作为 `Agent` 依然是一个真实占据坐标的实体（二节：马与船都落在 `Arena<Agent>`），`renders_mount` 只是表现层的一个开关，不改变模拟层「这里站着一个东西」这个事实。

**因此内容作者的责任，而不是引擎的特判**：声明一个不可见坐骑时，`Footprint` 必须**如实反映这个坐骑的真实物理占地**——游泳应该声明 `Footprint { width: 1, height: 1 }`（和普通人一样大，因为游泳时"坐骑"就是骑乘者自己的身体）；若填了一个不合理的大 `Footprint` 却选择不渲染，会产生一个玩家在屏幕上完全看不见、但确实存在的大块碰撞区域——这是内容作者配置错误的后果，引擎不需要（也不应该）在 `renders_mount = false` 时自动把 `Footprint` 钳制成某个默认值，那样反而会让「占地数值」这个字段的含义变得依赖另一个字段的取值，破坏了字段之间本该保持的正交性。

**继承的未验证风险，再次提醒**：三节已经指出，`Footprint` 大于 1×1 时碰撞/寻路层是否真的会读取它，目前**未经核实**——本节给出的是「一旦碰撞判定真的读 `Footprint`，`renders_mount` 不应该影响这个判定」这条**字段语义**上的结论，不代表这个判定现在已经存在或已经生效。

### 轴心/层序其余细节：结论不变

两种「实际绘制」场景（第一行两者都画、第二/三行只画一个）都复用既有的 `footprint_anchor_pixel`/`sprite_draw_position`，不需要精灵裁剪或多图合成——见本文档此前版本论证，未因本轮追加而改变。

---

## 六、能力授予与属性配置：通用列表，不是两个写死的系数

### 结论：`attack-modifier`/`defense-modifier` 两个专用字段被否决，改成通用的属性修正列表

项目所有者原话：「我希望在添加载具作为装备的方面能给足配置的自由度……以及添加什么技能和属性」——**两个写死的系数（`attack-modifier`/`defense-modifier`）只能表达两件事，不是「能配置属性」**。一个 mod 想做一辆「增加感知、降低敏捷、提高负重」的马车表达不出来（这三项都不是「攻击」或「防御」）。

```rust
pub struct MountDef {
    // ……
    /// 骑乘期间叠加给骑手的属性修正列表——始终叠加,不支持"替换"
    /// 模式,理由见下「替换/叠加要不要开放给mod」。走既有的
    /// Agent.active_stat_modifiers 通道（一节已核实，提交 `883d572`
    /// 起是按 `(属性, 来源)` 两层键控），Effect::Mount 以这个载具自身
    /// 的 ContentIndex 作为来源键逐项插入，Effect::Dismount 按同一
    /// 来源键逐项移除——不是「覆盖清空」整个属性槽位：两层键控下，
    /// 插入不会覆盖骑手身上其他来源（技能/装备）对同一属性的既有
    /// 修正，移除时也只精确删掉这个载具自己写入的条目，骑手自己的
    /// 修正原样保留。
    pub stat_modifiers: Vec<(AttributeKind, i32)>,
}
```

「增加感知、降低敏捷、提高负重」对照既有 `AttributeKind` 字段文档（一节已引用：`Willpower` 驱动视野半径/感知类效果、`Dexterity` 驱动敏捷本身、`Strength` 驱动负重上限）可以直接表达成 `[(Willpower, 3), (Dexterity, -2), (Strength, 4)]`，不需要为「感知」「负重」这类概念另开专用字段——六个既有 `AttributeKind` 变体的粒度已经够用，这正是项目所有者例子本身给出的证据。

**攻击加成没有丢失，只是不再叫「attack-modifier」**：战马的攻击加成过去写成 `attack-modifier = (Strength, 5)`，现在写成 `stat-modifiers` 列表里的一项 `(strength 5)`——底层机制完全相同（走 `active_stat_modifiers`，最终靠 `resolve_attack` 读 `Strength` 算出攻击力，一节已核实这条读取路径目前还不存在，是既有缺口，非载具专属），只是语法从两个专用字段泛化成了一个通用列表。

**防御加成被诚实地拿掉，不是遗漏**：`defense-modifier` 这个专用字段本身此前就是「先声明、实际不生效」的占位（一节已核实 `AttributeKind` 没有对应「护甲」的变体，`DerivedStats.armor` 走的是 `derive_stats`「状态效果」入参，那条消费逻辑本身不存在）。既然它原本就不真正工作，与其保留一个假装能用的专用字段，不如诚实地不提供——**盾车的防御加成目前无法通过任何字段表达**，这不是本次泛化引入的新限制，是六节此前就已如实标注的既有缺口（`StatBonus` 缺口 5）。将来 `derive_stats`「状态效果」入参补齐消费逻辑时，若决定让 `armor` 也从某个 `AttributeKind`（例如 `Constitution`）派生，盾车的防御加成自然可以复用同一份 `stat_modifiers` 列表表达，不需要另开字段；若决定 `armor` 走独立于 `AttributeKind` 的加成路径，届时再补一个专门字段——这是等那条缺口真正被设计时才能定案的事，本文档不现在猜。

### 「替换/叠加」要不要开放给 mod 配置：不开放，这是引擎规则，理由分两层

**结论：不开放。`stat_modifiers` 列表里的每一项永远是叠加，没有「替换」选项。** 移动速度的「替换」是一条独立于这份列表存在的引擎规则，理由：

1. **移动速度的"替换"根本不属于"属性修正"这个概念范畴**——它回答的是「骑乘时用谁的身体承担移动这件事」（二节「不变式」表格已经把这一条与其余四条并列，是回合经济/物理层面的规则，不是数值加成），不是"给骑手的敏捷加一个负数、凑巧让它等于坐骑敏捷"这种可以用 `delta` 表达的东西——骑手自身的敏捷可能同时被其他 buff 改变，一个固定 `delta` 无法追踪这种动态关系，只有"直接换一个数据源"（替换）能正确表达"这一步完全由坐骑决定"。这条规则对**所有**坐骑一视同仁（不因 `autonomous`/是否渲染而分支），符合二节的不变式，只是它不通过 `stat-modifiers` 表达，是移动结算自己的固定逻辑（三节 `mover_speed` 的计算）。
2. **`stat_modifiers` 建在已经落地、已被技能系统使用的 `ActiveStatModifier` 机制之上**，该机制的既有语义就是"`delta` 叠加"（一节已核实：`ActiveStatModifier { delta, expires_at }`，没有"替换"这个概念）。给这份列表的每一项加一个"替换/叠加"标记，等价于要求给 `ActiveStatModifier` 这个**已经落地、被技能系统在用**的类型新增一个模式字段——这个改动的影响面远超载具本身（会牵动 `resolve_use_skill`/`apply.rs` 里全部消费这个类型的既有代码），而载具目前真正需要"替换"语义的场景只有移动速度**一个**，且这个场景本来就不该塞进这份列表（见上一条）。为了一个不存在的真实需求（没有任何本文档讨论过的效果需要"替换某个非速度属性"）去扩大一个已落地类型的改动面，是不必要的复杂度（YAGNI）。

### 有效技能集与冷却记在载具上：结论不变

`skill_source`/派生并集/冷却按来源分别读写载具自己或骑手自己的 `skill_cooldowns`——均见本文档此前版本论证，未因本轮追加而改变。

### 坐骑被杀死时骑手怎么办、下船安全阀：结论不变

见本文档此前版本论证。

---

## 七、P6 必须先提供什么

在此前版本清单基础上，本轮追加两项：

| # | 必须先有 | 状态 |
|---|---|---|
| 1 | `ItemDef`/`ItemStack` 定义与实例分离本身 | **未落地** |
| 2 | `Owner` 归属枚举 | **未落地** |
| 3 | 「物品变实体」转换路径 | **未落地，本文档发现的最大缺口** |
| 4 | `ItemLocation::Ground` 的 30 日老化清理需对停靠的载具让路 | 依赖第 3 条 |
| 5 | `Agent` 新增 `mounted_on`/`rider`/`mount_profile`/`suspended_action_offset` 四个字段 | **未落地** |
| 6 | `TerrainDef`/`TerrainTable` 新增 `surface: Option<ContentIndex>`（缓存稠密位下标） | **未落地** |
| 7 | `Effect` 新增 `Mount`/`Dismount` | **未落地** |
| 8 | 调度器复用 `Timeline::remove`/`schedule` | **已满足** |
| 9 | `resolve_attack` 学会读 `active_stat_modifiers` | **未落地，非载具专属** |
| 10 | `derive_stats`「状态效果」入参的消费逻辑（防御加成的真正生效路径） | **不阻塞载具攻击加成/技能授予，只阻塞防御加成——且六节已经拿掉了假装能用的 `defense-modifier` 字段，这条依赖现在诚实地表现为"完全无法表达"而不是"字段存在但不生效"** |
| 11 | `SurfaceKindTable`（新增，`Registry::intern` + 稠密位下标分配） | **未落地**，三节 |
| 12 | `MountTable`（新增，`MountDef` 的列式物化，含 `grants_passage`/`surface_move_costs`/`stat_modifiers`/`granted_skills` 的注册期解析与自洽校验） | **未落地**，八节 |

**明确不阻塞载具落地的两项**（不变）：`SlotMask`/`EquipSlot`（装备栏位系统）；技能/属性修正的底层存储与哈希（`unlocked_skills`/`skill_cooldowns`/`active_stat_modifiers` 已经落地）。

---

## 八、mod 可注册性：`register-surface-kind`（第八个）与 `register-vehicle`

### `register-surface-kind`：声明一个表面分类

```scheme
(register-surface-kind "lostland:water")
```

返回 `Result<bool, String>`，与既有全部 `register-*` 同一模式——**不返回稠密位下标或任何数值句柄**，与 `register-terrain` 一致：调用方此后全部通过命名空间字符串 ID 引用这个表面（`register-terrain`/`register-vehicle` 内部各自把字符串解析成 `ContentIndex` 再查 `SurfaceKindTable::dense_bit_of` 拿稠密位下标），mod 脚本层不需要、也不应该直接接触稠密位下标这个纯内部实现细节。

**注册期做两件事**：`intern` 换取 `ContentIndex`；调用 `SurfaceKindTable::define` 分配一个稠密位下标并标记为已定义。重复注册同一个 ID——报错，不静默覆盖（与 `TerrainTable::define` 现有的 `DuplicateDefinition` 同一条纪律）。

**档位：一档。** 三步判据：有自由度（mod 能声明任意新表面）；自由度落在纯数据上（一次性交出一个字符串 ID，换取一个内部位下标，不涉及任何运行期才存在的输入）；调用频率——注册期一次，运行期这份表完全不被再次查询（`grants_passage`/`terrain.surface_dense_bit` 已经在**各自的**注册期把引用解析成位下标缓存好了，`SurfaceKindTable` 本身运行期零访问）。

### `register-vehicle`：签名

```scheme
(register-vehicle "lostland:rowboat"
  3 2                          ;; footprint: 占地宽 高
  24 40                        ;; pivot: 图像内锚点像素 x y（renders-mount=#t 时坐骑自身精灵用它）
  #f                           ;; autonomous?：船=#f，若为生物坐骑则=#t
  #t                           ;; renders-mount?：画不画坐骑自己的精灵
  #f                           ;; renders-rider?：画不画骑手的精灵
  '("lostland:water")          ;; grants-passage：表面分类 ID 列表（本 mod 或已 require-content! 的外部 mod）
  '(("lostland:water" 120))    ;; surface-move-costs：(表面 代价) 对列表，必须覆盖 grants-passage 的每一项
  '(("willpower" 3) ("dexterity" -2) ("strength" 4))  ;; stat-modifiers：始终叠加
  '("lostland:bola_throw"))    ;; granted-skills：本载具授予骑手的技能 ID 列表
```

返回 `Result<bool, String>`。相比此前版本的变化：`grants-passage`/`surface-move-costs` 从「单值/单个数」改成列表；`attack-modifier`/`defense-modifier` 两个专用字段合并成通用的 `stat-modifiers` 列表；新增 `renders-mount?`，与既有的 `renders-rider?` 并列成两个独立开关（五节）。

**档位：一档，理由不变**——`stat_modifiers`/`granted_skills`/`grants_passage`/`surface_move_costs` 全部是注册期一次性交出的纯数据，不依赖任何运行期才存在的输入。

**注册期校验（新增/更新）**：

1. `footprint` 宽高非零。
2. `grants-passage` 里的每一个表面 ID，必须能在 `SurfaceKindTable` 里查到（已被某次 `register-surface-kind` 定义）——查不到即拒绝整个 `register-vehicle`（三节「跨命名空间引用」一节已给出理由）。
3. `surface-move-costs` 的键集合必须与 `grants-passage` 的表面集合**完全一致**（双向自洽：能过的表面必须有定价，定了价的表面必须真的能过）——与 `TerrainTable::define` 现有的 `blocks_move`/`move_cost` 自洽校验同一条纪律。
4. `stat-modifiers` 里的每一项 `AttributeKind` 必须是六个既有变体之一（字符串解析失败即拒绝）。
5. `granted-skills` 里的每个 ID 必须已经通过 `register-skill` 注册。
6. `renders_mount`/`renders_rider` 均为 `false` 时——不拒绝，产出 `LoadStatus::Warning`（五节①）。

### `SurfaceKind` 位分配相关的既有先例引用：更正

**此前版本引用的「`SlotMask`/`ActionCapability` 定宽位标志+高位留mod」先例，三节已经明确否决，不再适用于 `SurfaceKind`**——这里更正一并说明：`SlotMask`/`ActionCapability` 依然是各自领域（装备槽位、行动能力）里正确的选择（这两者的可扩展项确实天然有限），只是**不应该被当成"表面分类要不要用位标志"这个问题的默认答案**——本文档三节最终选择了「内容索引 + 装载期定长位集」，是针对"可扩展项数量没有自然上限"这个特征做出的判断，与前两者的判断标准相同（"可扩展项有没有自然上限"），只是这次的答案不同。

---

## 九、明确排除的范围（不设计过头）

- **多人载具**（马车、多座位船）——「关系」目前是单一 `rider`/`mounted_on` 一对一字段。
- **载具改装/升级**——载具属性完全由 `MountDef`（内容注册）决定，不存在个体差异化。
- **载具驯养/繁殖**——本文档不做，属生物系统本身的问题。
- **目标重定向**（攻击有概率打中坐骑而非骑手）——需要瞄准形状展开阶段的新判定，超出最小形状。
- **载具耐久/维修**——依赖「物品变实体」路径，本文档不做。
- **游泳本身不在此列**——五节已指出，「不渲染的载具」这个组合天然覆盖游泳这类效果，不需要为它单独设计机制，也不需要单独排除它；本文档只是没有为游泳去调具体数值（`surface_move_costs`/`stat_modifiers` 的实际取值属于内容设计范畴）。

---

## 十、开放问题（如实标注，不强行圆）

1. **`resolve_attack` 读取 `active_stat_modifiers` 的具体聚合公式**——本文档只要求它必须发生，不设计聚合的具体形状。
2. **`derive_stats`「状态效果」入参的消费逻辑**（防御加成的真正生效路径，且六节已经不再假装有一个占位字段）——不是载具专属，本文档不解决。
3. **多格实体的碰撞/寻路是否已支持**——三节、五节两处继承自 [种族系统](race-system.md) 十二节的既有未验证项。
4. **水中被迫下船/坐骑死亡后的具体后果**——本文档只给安全阀，不裁定后果。
5. **游泳类"坐骑"的 `dexterity` 该不该按骑乘者个体化**——五节已如实标注为粗糙点，当前接受简化。
6. **`stat-modifiers`/`surface-move-costs` 的具体数值区间、`granted-skills` 数量上限**——内容设计范畴，本文档不定案。
7. **世界范围内载具数量的规模假设**——本文档假设「玩家实际拥有/骑乘的少量载具」，与厚层「数百个」规模假设相符。

---

## 相关文档

- [物品系统](item-system.md) — `ItemDef`/`ItemStack`/`Owner`/`ItemLocation`，七节指出的「物品变实体」缺口
- [装备栏位与占位掩码](equipment-slots.md) — `SlotMask` 定宽位标志，八节说明为什么这个先例不适用于 `SurfaceKind` 但仍适用于其原有领域
- [属性系统](attribute-system.md) — `effective_speed_from_dexterity`（四节坐骑速度直接复用）、`derive_stats`「状态效果」入参（六节防御加成的接线点）、`AttributeKind` 六个变体（六节 `stat_modifiers` 的取值域）
- [三轴战斗结算](combat-three-axis.md) — `resolve_attack` 占位实现现状
- [增益与通用触发器](buffs-and-triggers.md) — `ActiveEffect.expires_at` 绝对到期时刻的先例
- [种族系统](race-system.md) — 体型/`footprint` 十二节的未验证项，三节、五节两处继承
- [行动能力与输入上下文](action-capability-and-input-context.md) — 「调度层不生成 `Intent` 即可让实体完全不行动」的既有先例，四节直接复用；`ActionCapability` 位标志先例，八节说明其边界
- [扩充给 mod 的 Steel 脚本 API](mod-lifecycle-and-event-api.md) — `require-content!`/`content-exists?`，三节「跨命名空间引用」直接依赖
- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) — `EntityId` 寻址厚层的既有假设，二节否决 `Arena<Prop>` 方案时引用
- [0004 — 两层实体存储替代 ECS](../decisions/0004-two-layer-entity-storage.md) — `EntityId` 跨存储不唯一的既有事实
- [0016 — mod 性能分档按声明方式，不按作者身份](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0017 — 声明式分档物化为列式数据](../decisions/0017-tiered-declarations-materialize-columnar.md) — 三节、八节的分档论证
- [0018 — 引擎层与玩法层脚本边界](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) — 八节三步判据
- [0021 — 抽象的理由是有算法要共享，不是看起来该对称](../decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md) — 二节核心论证直接引用
- [0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 三节、四节确定性影响一节的直接依据
- `crates/ll-world/src/terrain.rs`（`TerrainTable`/`TerrainDef`，已落地，三节的扩展对象）
- `crates/ll-world/src/exploration.rs`（`ExplorationMemory`/`ZoneExploration`，已落地，三节 `SurfaceSet` 位集设计直接照抄的既有先例）
- `crates/ll-sim/src/resolve.rs`（`resolve_move`/`resolve_attack`/`resolve_use_skill`，已落地，三、四、六节的扩展对象）
- `crates/ll-sim/src/timeline.rs`（`Timeline::remove`/`schedule`，已落地，四节直接复用）
- `crates/ll-world/src/entity/stats.rs`/`agent.rs`（`active_stat_modifiers`/`unlocked_skills`/`skill_cooldowns`，已落地，六节直接复用）
- `crates/ll-world/src/state.rs`（`WorldState::hash()` 的每-`Agent` 哈希循环，已落地）
- `crates/ll-render/src/sprite.rs`（`Footprint`/`Pivot`/`DrawOrder`，已落地，五节的复用/扩展对象）
- `crates/ll-world/src/entity/id.rs`（`EntityId`，已落地，二节否决方案的直接依据）
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) §5（crate 分层）、§8（时间轴调度器）
