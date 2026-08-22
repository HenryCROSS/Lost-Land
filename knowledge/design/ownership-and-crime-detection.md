# 物品归属（`Owner`）与犯罪判定系统

**冻结于** 2026-08-21。核对提交 `fa361a8`（`main` 分支）。**并发声明**：写作本文档时工作区里有另一路并行改动（`crates/ll-mod/src/item.rs`、`crates/ll-mod/src/script_item_api.rs`、`crates/ll-sim/src/{item,resolve}.rs` 及三个测试文件，均处于未提交的修改状态，给攻击接线武器引用）——本文档不触碰、不依赖这些改动的具体内容，只依赖它们改动前就已经落地的公共接口（`ItemStack`/`GroundItemStack`/`resolve_pick_up` 等），若这些接口的具体行数在本文档冻结后发生位移，不影响本文档的结论。

**落地状态**：纯设计，`crates/` 中无任何 `Owner`、盗窃判定、销赃、犯罪记录的对应类型。已核实的现状：

- `Owner` 的最小形状目前只记在 `ll_world::item` 模块文档（`crates/ll-world/src/item.rs` 第 28–56 行）「`Owner` 本批次仍然不落地」一节，与 [物品系统](item-system.md) 三节原文一致——五变体 `Unowned`/`Player`/`Npc(EntityId)`/`Faction(ContentIndex)`/`Shop(EntityId)`，无任何消费者。
- `ItemStack`（`def`/`count`/`durability` 三字段，`#[derive(Copy)]`）与 `GroundItemStack`（`pos`/`stack`/`dropped_at: Tick`，同样 `Copy`）均已落地，均不含 `owner` 字段。
- `resolve_pick_up`（`crates/ll-sim/src/resolve.rs:1265` 起）对拾取**没有任何权限检查**——找到 `actor` 脚下第一堆地面物品就直接产出 `RemoveGroundItem` + `MergeIntoInventory` 两个 `Effect`，不问这堆物品原来是谁的。这是本文档「盗窃判定现在完全不存在」这句话的直接代码证据。
- `can_merge`（`crates/ll-world/src/item.rs:508`）目前只比较 `def`/`durability`；`merge_stacks`/`split_stack` 都用 `..a`/`..stack` 结构更新语法构造返回值——这意味着给 `ItemStack` 新增字段后，这两个函数**不需要改一行代码**就会自动继承新字段，唯一必须手动追加比较的只有 `can_merge`（`item-system.md` 二节原文与模块文档都已经预告了这一点，本文档只是把它兑现到 `Owner` 这个具体字段上）。
- `HistoricalEvent`/`KillRecord`（`crates/ll-world/src/history.rs`）已落地，是本文档设计犯罪记录的直接先例，见五节。
- `Affiliation`/`OrgRef`（`crates/ll-world/src/entity/affiliation.rs`）已落地（P3）：`AffiliationKind::Faction` 对应 `OrgRef::Instance(WorldId)`——势力是**世界生成期间产出的实例**，不是 mod 装载期确定的类型。这与 `item-system.md` 原文 `Owner::Faction(ContentIndex)` 的类型选择**不一致**，见一节 1.2。
- `Agent::remembered_id: Option<WorldId>`（`crates/ll-world/src/entity/agent.rs:355`）与 `WorldState::remembered_id_of_or_assign`（`crates/ll-world/src/state.rs:803`）已落地——字段文档明确列出的懒分配触发时机是一个**开放列表**（"出生进历史家族族谱、被玩家收为随从、成为任务发布者、死于一场被记录的击杀……"），当前唯一真实调用点是死亡路径，但机制本身通用，见一节 1.2 与七节场景一。
- **空间查询现状复核——2026-08-21 二次复核，结论修正**：初版核实过 `resolve_attack`/伤害公式求值器拿不到"以目标为中心查附近同阵营实体"这一条特定输入（`trait-system.md` 五节 6、217 行，盗贼偷袭要的"目标旁边有没有我的盟友"），以及 `script-entity-handles-and-batch-queries.md` 状态行核实的批量查询原语（`filter-within-distance` 等）仍是纯设计——**这两条依然成立，未被推翻**。但初版把这两条**过度泛化**成了"没有任何路径能问'我旁边有没有人'"，这句话经复核**不成立**：`compute_fov`（`crates/ll-world/src/fov.rs:250`）的原点自始至终是参数，不是玩家专属；`Arena::iter_with_id`（`crates/ll-world/src/entity/arena.rs:144`）能连 `EntityId` 一起遍历 `world.actors`；`crates/ll-sim/src/apply.rs:246` 已经示范了 `compute_fov(&SurfaceWindow::new(&world.terrain), &world.terrain_table, origin, radius)` 这个调用形状能在 `ll-sim`（`resolve`/`apply` 所在 crate）内部直接使用，不依赖 `ll-mod` 的任何注册表。**修正结论：目击可以表达——遍历 `world.actors`、以每个 NPC 自己的位置算一次 FOV、检查肇事者是否落在其中即可**，见二节新 2.3–2.7。
- 依赖但未落地的邻接系统：`society-and-affiliation.md`（势力/文化 P9，`CultureDef` 未落地）、任何交易/对话/悬赏系统（不存在）。

---

## 一、`Owner` 的形状

### 1.1 五变体够不够

支撑七节两个验收场景（偷剑、捡尸体上的无主物）只需要 `Unowned`/`Npc` 两个变体；`Player`/`Faction`/`Shop` 是未来场景（随从装备归属、商店库存、四节合法转移）需要的。**结论：骨架够用，不砍变体**——五变体已经是 `item-system.md` 定型过的形状，未来系统（商店、势力经济）都会对齐它，本文档没有权限单方面砍掉别的系统将来要用的变体。但两处引用类型需要修正：

### 1.2 修正一：`Npc(EntityId)` → `Npc(WorldId)`

`EntityId` 依赖 `Arena` 的世代号（`crates/ll-world/src/entity/arena.rs`），实体被 `despawn` 后世代号递增，旧 `EntityId` 立即失效——`Arena::get` 返回 `None`（这条机制已被 `script-entity-handles-and-batch-queries.md` 三节 1 核实并复用为句柄防伪造的地基）。私产的归属若存 `EntityId`，NPC 一旦死亡，其名下所有物品的 `Owner` 就会指向一个查不到值的悬空引用——但物品不该因为主人死亡就变得"归属不可判断"（继承、随从叛离决策、犯罪记录追溯都需要在主人死后仍能读出"这原本是谁的"）。

`KillRecord::killer`/`victim` 已经用 `WorldId` 而不是 `EntityId` 解决过同一个问题（见 `history.rs` 模块文档「为什么落在 `ll-world`」一节引用的判据）：`WorldId` "故意要指向一个已经不存在的东西，且必须永远解析成功"（`ll-core/src/ident.rs:209` 附近注释原文）。`Owner::Npc` 需要的正是这个性质，与 `EntityId` "故意让引用失效"的设计目标互斥。**结论：`Owner::Npc(WorldId)`。**

依赖：给一个活着的 NPC 的物品打上 `Owner::Npc(WorldId)`，这个 NPC 必须先有 `remembered_id`。现状是**唯一真实调用点是死亡路径**，但 `remembered_id_of_or_assign` 本身不专属于死亡——字段文档把"值得被记住"列成一个开放清单（出生/收随从/发任务/死亡……），"物品被赋予具体 `Owner::Npc` 时"完全可以成为这份清单的新增第五项：**不需要新机制，只需要犯罪系统真正落地时，在"给 NPC 的私产打归属"那一步调用一次既有的 `remembered_id_of_or_assign`**。这是一处需要落地时接线的依赖，不是阻塞性缺口——如实标注，不回避。

### 1.3 修正二：`Faction(ContentIndex)` → `Faction(WorldId)`

`item-system.md` 写作于 2026-08-17，当时 `Affiliation`/`OrgRef` 还没有落地，`ContentIndex` 是当时唯一能拿到手的"势力引用"类型。现在 `OrgRef` 已经把六类归属正式分成两条轨道：`Def(ContentIndex)`（文化、职业——mod 装载期确定的类型）与 `Instance(WorldId)`（势力、宗教、行会、家族——世界生成期间产出的具体个体）。`Faction` 明确落在 `Instance` 一侧（`affiliation.rs` 原文：「势力：领土、法律、税收、兵役。世界生成期间造出来的实例」）。若 `Owner::Faction` 继续用 `ContentIndex`，会出现"物品的势力归属"和"角色对势力的归属"（`Affiliation.org`）指向同一个东西却用两套不相容的引用类型——将来接线时必然要在两者之间做一次容易出错的手工换算。**结论：`Owner::Faction(WorldId)`，与 `OrgRef::Instance` 直接对齐，不新造一套换算。**

依赖：势力实例本身（世界生成产出的具体 `WorldId`）要到 P9 世界生成落地才存在——本条修正只是把类型选对，不代表 `Owner::Faction` 现在就能被赋值,如实标注见五节 5.2。

### 1.4 `Shop(EntityId)` 暂不改动

商店库存到底是"具名 NPC 摆摊"（`Owner::Npc` 就够用，`Shop` 变体可以不存在）还是"独立于任何角色的建筑设施"（需要一个本文档权限之外的 `StructureKind`/`StructureId` 类型，`society-and-affiliation.md` 未落地），这个判断属于经济/社会文档的地盘，本文档不代其发言。`Shop(EntityId)` **保留原样，标注为待定**——它甚至可能不需要独立存在，等真正的商店系统裁定。

### 1.5 无主物

`Owner::Unowned`——野外掉落、怪物尸体、任何"没有人对这堆东西主张所有权"的地面物品。`GroundItemStack` 目前没有 `owner` 字段；落地时新增该字段，默认值应为 `Unowned`（现有代码里构造 `GroundItemStack` 的每一处——`Effect::RemoveGroundItem`/`Drop` 结算——隐含的语义都是"这堆东西现在没有主张归属的机制"，加字段只是把这个隐含语义显式化，不改变任何现有行为）。

### 1.6 `Owner` 存哪；对 `can_merge`/存档的影响

**存在 `ItemStack` 上，不单独开一张表。**

被否决方案：**`Owner` 存进一张独立的 `HashMap<某种物品实例 ID, Owner>`**。否决理由——`ItemStack` 现在没有实例级别的稳定 ID（一堆物品拆分/合并后"这是不是同一份实例"这个问题本身就没有明确答案，`item-system.md` 从未定义过"物品实例 ID"这个概念）；没有 ID 就没有键，这张表根本无法维护。更根本的问题：`Owner` 是"这堆东西现在归谁"，`can_merge` 已经用同样的模式处理了 `durability`——**两堆同一种物品若实例状态不同就不该合并**，`Owner` 不同显然是最直观的一种"不同"，跟 `durability` 存在同一个结构体上是同一条既有纪律的自然延伸,不是新发明。

具体改动（只是形状，不写实现代码）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    pub def: ContentIndex,
    pub count: u32,
    pub durability: Option<i32>,
    pub owner: Owner,               // 新增
    pub stolen_marker: Option<StolenMarker>,  // 新增，见三节
}
```

- `ItemStack` 仍然是 `Copy`：`Owner` 的候选形状（`Unowned`/`Player`/`Npc(WorldId)`/`Faction(WorldId)`/`Shop(EntityId)`）全部由 `Copy` 类型组成（`WorldId`/`EntityId`/`ContentIndex` 均已 `#[derive(Copy)]`），`StolenMarker`（见三节，`{ original_owner: Owner, stolen_at: Tick }`）同理全部由 `Copy` 字段组成——不破坏 `ItemStack` 现在"小而 `Copy`"这条性质。
- **`can_merge` 必须追加两行**：`a.owner == b.owner && a.stolen_marker == b.stolen_marker`——`item-system.md` 二节原文早已点名"以后给 `ItemStack` 加了『绑定角色』字段，只要补进这个比较，堆叠逻辑就自动正确"，这里兑现的正是这句话。`stolen_marker` 也要比较，理由见三节：两堆偷来的东西若计时不同却被允许合并，会让其中一堆的计时被悄悄抹掉。
- **`merge_stacks`/`split_stack` 不需要改**——`..a`/`..stack` 结构更新语法自动带上新字段，已在开头「落地状态」核实过。
- **存档增量**：`Owner` 是一个小 `enum`（判别式 + 至多一个 `u32`），`StolenMarker` 是 `Owner + Tick`（`i64`）——每个 `ItemStack` 增加约 12–20 字节，量级与新增 `durability: Option<i32>` 时同一个数量级，不构成存档体积问题。

---

## 二、盗窃判定发生在哪一步

### 2.1 挂载点：`resolve_pick_up`

「直接拿」= `Intent::PickUp`，唯一的挂载点就是 `resolve_pick_up`（`crates/ll-sim/src/resolve.rs:1265`）。现状：`find(|item| item.pos == agent.pos)` 找到脚下第一堆就直接拾取，`Owner` 落地后要在这里插入一次比较——`ground.stack.owner` 是否等于"这次拾取算合法"的判据（`Unowned`，或 `Player` 且 `actor == world.player_entity`，或 `Npc(id)` 且 `id` 等于 `actor` 自己的 `remembered_id`）。不满足则这次拾取在**归属意义上**是盗窃——但"归属意义上是盗窃"和"会被追究"是两件不同的事，见 2.2。

### 2.2 「有主人但主人不在场」vs「被目击」——两者依然分开判定

现实里，拿走一件有主之物本身就构成盗窃（既遂），不需要有人看见——目击只影响会不会被追究。项目所有者的原话「需要经过一段时间自动销赃」也隐含了这个前提：**销赃的对象是"已经既遂但没人立刻发现"的盗窃**，若目击是入罪的必要条件，"没被目击的盗窃"根本不会进入犯罪记录，销赃这个概念也就无从谈起。**结论不变：拾取判定与目击判定分开——拿了有主之物即记一条犯罪事件（既遂），"是否被目击"只决定这条记录带不带目击者名单，不决定"算不算犯罪"。** 与初版的区别只在于：初版认为目击判定做不到，只能恒 `false`；本次复核确认目击判定可以真正算出来，见 2.3–2.6。

### 2.3 目击算法：两段式（粗过滤 + FOV）

**核实结论：目击可以表达，用项目所有者提出的思路——遍历厚层实体、以每个 NPC 自己的位置算一次 FOV、看肇事者在不在里面。**

```rust
// 纯设计示意，不写实现代码
//
// 只读——resolve 阶段只能拿到 &WorldState，不能给未具名目击者懒分配
// remembered_id（那需要 &mut self）。本函数只产出"哪些实体看见了"这
// 件事实（EntityId 足够，resolve 阶段本就持有），真正把目击者翻译成
// 永久的 WorldId 留给 apply 侧的 record_crime（与 record_kill 现在
// 调用 remembered_id_of_or_assign 是同一处分工：resolve 判断"发生了
// 什么"，apply 才允许"写世界"，约束 C1）。
fn witnessed_by(world: &WorldState, culprit_pos: TorusPos, at: Tick) -> Vec<EntityId> {
    let mut witnesses = Vec::new();
    for (id, agent) in world.actors.iter_with_id() {          // 见 2.4「顺序」
        if agent.pos == culprit_pos {
            continue; // 肇事者自己不算目击者
        }
        let sight = sight_radius_at(NPC_BASE_SIGHT_RADIUS, ambient_light(at)); // 2.6
        if world.terrain.layout().chebyshev(agent.pos, culprit_pos) > sight {
            continue; // 第一段：粗距离过滤，见 2.5
        }
        let visible = compute_fov(
            &SurfaceWindow::new(&world.terrain),
            &world.terrain_table,
            agent.pos,
            sight,
        );
        if visible.contains(culprit_pos) {                    // 第二段：真正的 FOV
            witnesses.push(id);
        }
    }
    witnesses
}
```

`apply` 侧对应的 `WorldState::record_crime`（形状见五节）拿到这份 `Vec<EntityId>` 后，对每一个调用一次既有的 `remembered_id_of_or_assign`——与 `record_kill` 现在对 `killer`/`victim` 做的事完全同构，"目击一次犯罪"顺理成章地成为 `remembered_id_of_or_assign` 那份开放触发清单的第六项（前五项见一节 1.2）。

- **第一段（粗过滤）**：`TorusSize::chebyshev`（`crates/ll-core/src/torus.rs:184`）已落地，`O(1)` 每候选者——`compute_fov` 自己返回的可见格本就满足 `chebyshev(origin, pos) <= radius`（fov.rs 文档「扫描进深的安全上限」一节已证），所以"候选者到肇事地点的切比雪夫距离超过候选者自己的视野半径"时，真正跑一遍阴影投射也不可能得出"看得见"，可以安全跳过。
- **第二段（真正的 FOV）**：只对幸存者调用 `compute_fov`，成员测试用 `VisibleSet::contains`（`crates/ll-world/src/fov.rs:207`）。
- **候选者从哪来**：`world.actors`（`Arena<Agent>`，厚层）——`crate::entity::ThinPopulation` 模块文档原文「薄层容器：列式（SoA）人口，容纳数十万到数百万背景 NPC」已经核实：数量级巨大的背景人口根本不在 `actors` 里，`iter_with_id` 遍历的候选池天然只是"当前被具体模拟"的前景实体，规模远小于世界总人口。

### 2.4 C5 边界：只做成员测试，不遍历 `VisibleSet` 做决策；目击者名单顺序

`VisibleSet` 内部是 `HashSet<P>`（`crates/ll-world/src/fov.rs:203`），`iter()` 的文档原文明确写着「遍历所有可见格，不保证顺序」。**目击判定只调用 `contains`（成员测试）**——这不依赖迭代顺序，是安全的；**本设计不遍历 `VisibleSet` 做任何决策**，如果未来有人想"列出这个 NPC 视野里的全部实体"而不是"肇事者在不在视野里"，那才会触及 `iter()` 的顺序问题，届时需要额外排序（例如按 `TorusPos` 或 `EntityId` 排序）才能满足 C5（`docs/architecture/03-invariants.md` 137 行：「禁止让 `HashMap`/`HashSet` 的迭代顺序参与逻辑判断」）——本设计不需要这一步，如实标注边界。

**目击者名单的收集顺序**：`witnesses` 这个 `Vec<WorldId>` 的顺序**就是 `Arena::iter_with_id` 的顺序**——`Arena::slots: Vec<Slot<T>>`（`crates/ll-world/src/entity/arena.rs:34`），`iter_with_id` 是 `self.slots.iter().enumerate().filter_map(..)`，按 `Vec` 下标升序遍历，全程不经过任何 `HashMap`/`HashSet`。**这个顺序是确定的，满足 C5**，与 `resident_zones`/`topo_sort` 等既有先例同一类判据。

### 2.5 性能：两段式可行；非法操作频率够低

**两段式核实可行**：粗过滤是 `O(1)`/候选者的整数比较，真正的阴影投射（`O(radius²)`，半径 12 大概几百格，`fov.rs` 模块文档未见 `criterion` 基准——`crates/ll-world/benches/` 目前只有 `terrain.rs` 一个基准文件，没有 FOV 专属基准，本文档给不出实测 µs 数字，如实标注）只跑在幸存者身上。

**频率核实**：`Intent::PickUp` 是事件驱动，不是每 tick 每实体的轮询——拾取判定只在玩家/NPC 真正发出一次拾取意图时触发一次，不会随世界人口或帧率线性增长。更有力的既有先例：`crates/ll-sim/src/apply.rs:246` 显示，**玩家每走一步**（`Effect::MarkExplored`，见 `resolve.rs` 「为什么只有玩家移动才追加 `MarkExplored`」一节）就无条件跑一次同样量级的 `compute_fov`（半径 12）——移动的频率远高于拾取有主之物这种边缘操作。**若"每步一次 FOV"这个开销这个代码库已经在承受，"每次拾取对幸存者集合各算一次 FOV"的开销级别不会更差，且幸存者集合经过粗过滤后通常远小于全体前景实体数。** 该文档同一节也解释了为什么 `MarkExplored` 只给玩家一个人算、不给每个 NPC 都算——这正是本文档"先粗过滤缩小候选池"这条设计要避免的同一类爆炸，两处判断方向一致，不矛盾。

### 2.6 视野半径用谁的——光照会影响，暗视目前不影响

`ambient_light(tick)`/`sight_radius_at(base_radius, light)`（均在 `crates/ll-world/src/light.rs:118`/`45`）都是**只依赖 `Tick` 的纯函数**，不需要 `SpaceProfile` 注册表，可以在 `ll-sim` 内部直接调用——这与 `resolve.rs` 里 `EXPLORATION_SIGHT_RADIUS`（固定值，不接光照）是**不同的判断**：那处固定半径的理由是"记不记得某处地形与此刻多暗是两件事"（玩法语义选择），不是"技术上做不到"，其文档原文明确写着真正随光照变化的换算（`effective_ambient_light`）此刻只在 `ll-game` 的 demo 层现算，因为那条换算需要 `SpaceProfile` 表（室内/室外的额外调光），`ll-sim` 依赖顺序上拿不到——**但目击判定不需要 `SpaceProfile` 这一层，只需要纯时间驱动的 `ambient_light`，这一条在 `ll-sim` 内部可以直接调用，不受这条依赖限制**。

**结论：「晚上偷东西更不容易被发现」在这条纯时间驱动的链路上成立**——光照越低，`sight_radius_at` 算出的候选者视野半径越小，2.3 的粗过滤与 FOV 两段都会更容易把候选者判定为"看不见"，越是深夜候选者能命中肇事地点的概率越低（不会低于观察者的夜间下限——未声明暗视时是 `DEFAULT_NIGHT_SIGHT_RADIUS = 4`，声明了暗视的种族是它自己声明的格数，可高可低）。

> **本节 2.6 的结论已被后续批次推翻两次，按时间顺序：**
> ①「暗视接线批次」补上了 `effective_light_for_race`，公式真的开始执行；
> ②「暗视语义改版批次」发现那个形态在本作光照量纲下**永远不可能生效**
> （矮人声明 4，午夜环境光 100），把暗视整体改成**夜间视野格数下限**
> （`RaceDef.darkvision_cells`），并把 `effective_light_for_race` 连同
> 「有效光照」这个概念一并删除。**现状是暗视真的在生效**：本体矮人夜里
> 7 格、精灵 6 格、人类 4 格，端到端证据见
> `crates/ll-mod/tests/base_mod_darkvision.rs`。下面这段保留原文作为
> 当时的核实记录，其结论已不再成立。

**但种族暗视不成立，需要更正项目所有者原话里的这半句**：核实结论——`RaceDef.darkvision_floor` 字段本身已落地（`crates/ll-mod/src/race.rs:107`，参与哈希与测试），字段文档写明了意图公式「`effective_light = max(实际光照, darkvision_floor)`」，但**全代码库检索确认没有任何函数名为 `effective_light`、也没有任何地方真正执行这条 `max` 运算**——`darkvision_floor` 是一个"字段就位、消费者未接线"的既有模式（与 `history.rs` 的 `VictimState.poisoned` 恒 `false` 同一类），不是"暗视已经在生效"。**若要让暗视种族在夜间维持更大的目击半径，需要新增一步：候选者的视野半径改用 `sight_radius_at(base, LightLevel(ambient_light(at).0.max(darkvision_floor)))`，这一步现在不存在，本文档只指出接线点，不假装它已经工作。**

### 2.7 判定入口的可扩展形状——不写死 `is_theft`

项目所有者原话「玩家进行了 xxx 非法操作」暗示盗窃不是唯一一种。**判定入口不应该是一个只服务盗窃的布尔函数**，而应该分成两个职责独立的部分：

1. **各自的 `resolve_*` 判断"这是不是一次非法操作"，产出一个 `CrimeKind`**（五节给出形状）——`resolve_pick_up` 判断"拿了不属于自己的物品"，产出 `CrimeKind::Theft { .. }`；未来 `resolve_open_door` 可以判断"这扇门锁着且不是你的"产出破门变体；`resolve_attack` 可以判断"目标是非战斗 NPC"产出袭击平民变体；某种区域判定可以产出闯入禁区变体——**这些变体现在都不设计，只是指出它们将来都能落进同一个枚举**。
2. **一对通用帮手不关心具体是哪种非法操作**：`witnessed_by`（2.3，只需要肇事地点与时刻）+ `record_crime`（五节，只需要肇事者、地点、`CrimeKind`、目击者名单）。任何新增的 `CrimeKind` 变体都直接复用这两个帮手，不需要为每种非法操作各写一套目击逻辑。

这与 `KillCause`（`history.rs`）现在 7 个变体、当前只有 2 个变体真正被构造是同一个既有模式——枚举形状按未来需要预先留好扩展位，具体消费方逐个落地时各自追加变体，不是本文档现在就要设计"破门"的判定细节。

---

## 三、销赃计时

### 三个候选起点其实只有一个

「偷的那一刻」与「进背包那一刻」这两个候选点在当前引擎结构下**没有区别**：`resolve_pick_up` 一次结算原子地产出 `RemoveGroundItem` + `MergeIntoInventory` 两个 `Effect`，两者在同一个 `Intent::PickUp` 的同一次 `resolve` 调用里一起产出，对应同一个 `world.clock`（同一个 `Tick`）。这个引擎没有"物品先进入某种中间态（例如手里但还没入包），再决定要不要收下"这种分步流程——**没有第三个候选点需要排除，两个候选点本身就是同一个时刻**。

### 3.1 计时存哪

存在 `ItemStack.stolen_marker: Option<StolenMarker>` 上（一节 1.6 已给出字段位置）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StolenMarker {
    /// 犯罪发生前，这堆物品原本的 Owner——销赃后需要知道"从谁那里
    /// 偷来的"才能追认给正确的势力/角色，也是犯罪记录（五节）的
    /// killer/victim 对应字段。
    pub original_owner: Owner,
    /// 犯罪发生的时刻——销赃计时的起点，见下文「防重置」。
    pub stolen_at: Tick,
}
```

`Some(marker)` 表示"这堆物品当前处于赃物状态，尚未洗白"；`None` 表示"清白"（从来没被偷过，或已经洗白完毕）。

### 3.2 洗白后变成什么 `Owner`

**变成当前持有者的归属**，不是无主，也不是恒定"变成小偷的"——若持有者已经把赃物转手给别人（3.4 讨论转手不重置计时），洗白那一刻该认的是"现在手里拿着它的人"，不是"最初动手偷的人"。具体：`stolen_marker.take()` 清空的同一时刻，把 `owner` 改写成：

- 若这堆物品在玩家背包里 → `Owner::Player`
- 若在某个具名 NPC 背包里 → `Owner::Npc(那个 NPC 的 remembered_id)`（这个 NPC 若还没有 `remembered_id`，此刻懒分配一个——与一节 1.2 「物品被赋予具体归属时」触发懒分配是同一处接线，销赃是这条触发路径的第二个具体调用点）
- 若这堆物品已经又被丢在地上（尚未被任何人捡起） → 洗白后维持 `Unowned`（没有"现在的持有者"可以归属给谁，回到无主状态是唯一自洽的选择）

### 3.3 防重置漏洞——正面处理

任务要求正面处理"丢在地上再捡起来能不能重置/加速计时"。**规则：`stolen_at` 只在 `stolen_marker` 从 `None` 变成 `Some` 的那一刻写入一次，此后任何转手（拾取/丢弃/再拾取）都必须原样保留已有的 `StolenMarker`，不得覆写。**

逐场景核对：

| 场景 | `resolve_pick_up`/`resolve_drop` 该做什么 | 是否影响计时 |
|---|---|---|
| 从有主地面堆首次拾取（`ground.stack.owner` 与拾取者不符，且 `stolen_marker` 为 `None`） | 写入 `stolen_marker = Some(StolenMarker { original_owner: ground.stack.owner, stolen_at: world.clock })`，`owner` 保持不变（**保持原 `owner` 不变，不是立刻改写成小偷的**——归属仍然是原主人的，只是多了一个"处于赃物状态"的标记，直到销赃那一刻才真正变更 `owner`，这样犯罪记录五节要读的"原主人是谁"随时可从 `owner` 本身读出，不需要额外查历史事件) | 起点，仅此一次 |
| 赃物被丢在地上（`resolve_drop`） | `GroundItemStack.stack` 直接是那个 `ItemStack`（结构体整体搬移），`owner`/`stolen_marker` 原样带过去 | **不受影响**——`resolve_drop` 不触碰这两个字段 |
| 赃物在地上被同一个小偷重新捡起 | `ground.stack.owner` 仍是原主人，`stolen_marker` 已经是 `Some`——判定逻辑读到 `stolen_marker.is_some()` 时**跳过重新赋值**，直接沿用已有的 `stolen_at` | **不受影响** |
| 赃物在地上被第二个人（不是原主人、不是最初的小偷）捡起 | `ground.stack.owner` 仍是原主人，对第二个人而言这同样是一次盗窃（既遂）——但 `stolen_marker` 已存在，同上规则**不重置** `stolen_at`；犯罪记录（五节）额外追加一条以第二个人为主体的记录,但计时不受影响 | **不受影响** |
| 赃物被合并进另一堆同种物品 | `can_merge` 现在要求 `owner`/`stolen_marker` 均相等（一节 1.6）——两堆 `stolen_at` 不同的赃物**不能合并**，不存在"合并抹掉较早时间戳"的路径；只有 `stolen_at` 恰好相同（同一次拾取产生的两个物理实例，理论上不会发生，`resolve_pick_up` 一次只处理一堆）的堆才谈得上合并，此时合并不改变任何时间戳 | 天然不可利用 |
| 赃物被拆分（`split_stack`） | `..stack` 结构更新语法，两个子堆各自完整继承 `owner`/`stolen_marker` | **不受影响** |
| 达到销赃阈值 | 见 3.2，`owner` 改写、`stolen_marker` 置 `None` | 终点 |

**这条规则本身足以堵住"丢地上再捡起来"这一类利用**——因为判定逻辑的分支条件是「`stolen_marker` 是否已经是 `Some`」而不是「这次拾取的 `owner` 是否非法」，赋值只发生一次，后续任何转手都只是"原样携带一个已经存在的标记"，没有第二个写入点可以被利用来重置或提前时间戳。至于「反过来能加速洗白」的方向——本设计里唯一能改变 `stolen_marker`/`owner` 的路径只有「首次拾取写入」和「计时到期清空」两处，没有第三条路径可以缩短 `stolen_at` 到 `now` 之间的间隔,因此也没有加速空间。**唯一需要未来实现者留意的开放约束**：四节的合法转移（赠送/购买/任务）一旦落地，其 `resolve` 必须校验"转出方是否等于该堆当前的 `owner`"——若允许转出方是小偷本人（`owner` 仍是原主人、`stolen_marker` 为 `Some`）把赃物"卖给"自己控制的另一个角色从而瞬间清空标记,那就是绕开销赃计时的作弊路径。这条校验属于四节合法转移 `resolve` 自身的职责,本文档只把它记录成一条必须满足的前置约束,不在此处实现。

### 3.4 被否决的方案

**计时存 `GroundItemStack.dropped_at`（复用现有字段）**——否决：`dropped_at` 的语义是"这堆物品最后一次被放到地上的时刻"，用于老化清理（30 天）；赃物在被丢弃前很可能已经在背包里放了很久，`dropped_at` 根本不会在那段时间存在（物品在背包里没有 `GroundItemStack`）。复用会把"最后一次落地"和"最初被偷"两个不同的时刻混为一谈，且赃物一旦进背包这个时间戳直接丢失（背包用 `Vec<ItemStack>`，不含 `GroundItemStack`）。

**计时存在 `Agent` 上（例如 `Agent.recent_thefts: Vec<(ContentIndex, Tick)>`）**——否决：物品本身可能转手（3.3 已论证转手不重置计时），若计时跟着"当前持有者"这个 `Agent` 走，转手瞬间就会丢失原始时间戳（新持有者的 `Agent` 上没有这条记录），复现"转手能重置计时"这个已经被明确要求堵住的漏洞。计时必须跟着物品实例走，只能存在 `ItemStack` 自己身上。

---

## 四、合法转移

赠送、购买、任务这三种改变 `Owner` 的合法途径，都需要对应的 `Intent`/机制，**当前均不存在**：没有交易系统（无价格结算、无货币扣减接线，见 `agent-goals-and-economy.md` 状态行）、没有对话系统（赠送需要一个"NPC 决定要不要给你"的交互载体）、没有任务系统里"发放物品奖励"这一步的具体实现（`class-skill-quest-system.md` 的 `QuestNodeDef` 已落地类型定义,但奖励发放的 `resolve`/`Effect` 未落地）。**如实标注：三者现在都是空中楼阁，接口形状可以先给,调用方不存在。**

### 接口形状

**只需要一个 `Effect`，不需要三个**——赠送/购买/任务发放物品在"改变 `Owner`"这个动作本身上完全同构（都是"把一堆物品的 `owner` 从 A 改成 B，且清空 `stolen_marker`"），区别只在"谁触发的、有没有对价"，那部分逻辑属于各自系统（交易的价格结算、任务的完成判定），不属于归属转移本身：

```rust
// 追加到既有的 Effect 枚举（ll_sim::effect），不新增变体，
// 而是给已有的 Effect::MergeIntoInventory /
// Effect::RemoveGroundItem 之外再加一个：
Effect::TransferOwnership {
    stack_def: ContentIndex,
    new_owner: Owner,
}
```

`apply` 侧对应的 `WorldState` 方法形状：

```rust
pub fn transfer_item_ownership(&mut self, /* 定位到具体哪一堆的参数 */, new_owner: Owner) {
    // 找到目标 ItemStack，owner = new_owner，stolen_marker = None
}
```

**谁将来会调用它**（均未落地，只标注挂载点）：

- **赠送**：未来的对话系统/交互系统在"NPC 同意给予"分支产出的 `resolve`，或未来的 `Intent::Give`（当前 `Intent` 枚举没有这个变体）。
- **购买**：未来的交易系统（`agent-goals-and-economy.md` 的"行会中介贸易与定价"是最接近的既有设计，但该文档明确"物品与装备的字段定义"不归它管，交易的 `resolve` 需要在扣减货币的同时调用本节的转移逻辑，两个系统目前都没有落地这一步）。
- **任务**：`QuestNodeDef` 完成判定之后，"发放物品奖励"这一步的 `resolve`/`Effect` 序列——当前 `class-skill-quest-system.md` 只定义了任务节点结构本身,没有奖励发放的具体机制。

**给未来实现者的一条约束**（已在 3.3 末尾提过，此处正式记录）：三种合法转移的 `resolve` 都必须校验"发起转移的一方是否确实是该堆物品当前的 `owner`"（`Owner::Unowned` 的物品谁都能转移，因为没有人的权益受损）——不满足则这次转移本身不合法，不该产出 `Effect::TransferOwnership`。这条校验现在没有代码可写（三个系统都不存在），但必须写在这里，作为它们各自落地时的前置要求，否则四节和三节的防漏洞论证就有一个共同的缺口。

---

## 五、犯罪系统本身

### 5.1 犯罪记录是历史事件的特化

**结论：是，理由与 `kill-and-death-events.md` 一节论证完全同构。** 玩家偷了村民的剑、NPC 偷了另一个 NPC 的东西、悬赏任务因为一次盗窃而触发——这是同一件事（谁、对谁、做了什么、在哪、什么时候）在不同后果规模下的实例，区别只在"值不值得被记住"，不在"记录的形状该是什么"。若单开一张"犯罪日志"，会立刻复现 `kill-and-death-events.md` 已经否决过的 `BattleLog` 那类问题：下游消费者（未来的通缉系统、NPC 态度调整、传说浏览）要么被迫适配两套数据源（历史事件查"谁杀了谁"、犯罪日志查"谁偷了谁"，"这个 NPC 有没有前科"这类跨类型查询要同时扫两张表），要么两张表之间出现"一处更新漏了同步"的静默不一致——与 [0010](../decisions/0010-single-source-of-truth-for-daylight.md)/[0014](../decisions/0014-season-pure-function-derivation.md) 记录的教训是同一个根因。

`HistoricalEventKind` 因此新增一个变体——**这一版按二节 2.7 的可扩展要求，把它设计成 `Crime(CrimeRecord)` 而不是专属盗窃的 `Theft(TheftRecord)`**，具体是哪种非法操作走内层的 `CrimeKind`：

```rust
pub enum HistoricalEventKind {
    Kill(KillRecord),
    Crime(CrimeRecord),   // 新增
}

pub struct CrimeRecord {
    /// 肇事者——None 表示尚未"具名"（懒分配失败，理论上不该发生：
    /// resolve_pick_up 阶段应当已经确保 actor 存在）。
    pub perpetrator: Option<WorldId>,
    /// 目击者名单——二节 2.3 算出的结果，经 apply 侧翻译成 WorldId
    /// （见二节 2.3 末尾）。**不额外存一个 `witnessed: bool`**——
    /// "是否被目击"就是 `!witnesses.is_empty()`，与本节下文"是否犯罪
    /// 是查询不是存储字段"同一条 0009 纪律，避免两个字段互相漂移。
    pub witnesses: Vec<WorldId>,
    /// 具体是哪一种非法操作，见下。
    pub kind: CrimeKind,
}

/// 非法操作的具体种类——本批次只交付 `Theft`，见二节 2.7「判定入口
/// 的可扩展形状」：破门、袭击平民、闯入禁区等未来变体直接追加进这个
/// 枚举，不需要改目击算法（二节 2.3）或 `record_crime`（下）一行代码
/// ——与 `KillCause` 现在 7 个变体、当前只有 2 个真正被构造是同一个
/// 既有模式。
pub enum CrimeKind {
    Theft {
        /// 原本的归属——Npc/Faction 场景下是被偷者，Unowned 场景下
        /// 这条记录根本不会产生（见下）。
        victim: Owner,
        item_def: ContentIndex,
        count: u32,
    },
}
```

`apply` 侧对应的 `WorldState::record_crime` 形状（与 `record_kill` 同构，`crates/ll-world/src/state.rs:841` 附近）：

```rust
pub fn record_crime(
    &mut self,
    at: Tick,
    location: TorusPos,
    perpetrator: EntityId,
    witnesses: &[EntityId],   // 二节 2.3 的 witnessed_by 只读产出的候选
    kind: CrimeKind,
) -> Option<WorldId> {
    let perpetrator_id = self.remembered_id_of_or_assign(perpetrator);
    let witness_ids: Vec<WorldId> = witnesses
        .iter()
        .filter_map(|&w| self.remembered_id_of_or_assign(w))
        .collect();
    // push HistoricalEvent { kind: Crime(CrimeRecord { .. }), .. }，
    // 与 record_kill 同一套结构
}
```

**`Owner::Unowned` 的拾取根本不产出这个变体**——`resolve_pick_up` 判定"是否构成盗窃"本身就是这条记录存不存在的前提,七节场景二会逐步演示这一点。

**"是否犯罪"是查询，不是存储字段**——`WorldState` 不需要一个 `is_criminal: bool` 字段挂在 `Agent` 上,而是在需要时扫描 `history` 里 `HistoricalEventKind::Crime` 且 `perpetrator == 某 WorldId` 的条目（可选按时间窗口过滤）。这与 [0009](../decisions/0009-derive-by-default-store-only-deviation.md) "默认派生、只存偏差"是同一条既有纪律的直接应用——犯罪状态是从不可变的历史日志派生出来的只读视图，不是需要额外维护、可能与日志本身产生不一致的第二份真相。

### 5.2 管辖区——依赖 P9，如实标注

「在 A 城偷东西，B 城知不知道」这个问题的答案取决于"A 城和 B 城之间有没有信息渠道/是否同属一个势力"——这需要**领土归属**（哪片地属于哪个势力）与**势力间关系**（`Affiliation.standing` 已经落地这个概念，但势力实例本身要 P9 世界生成才存在）。当前没有任何代码能回答"某个坐标属于哪个势力"这个问题——`society-and-affiliation.md` 状态行已核实"关系派生基线、`Kinship`……均未落地"，`CultureDef` 未落地。**结论：管辖区无法实现,如实标注为将来扩展。** 当前唯一可行的降级方案是"犯罪记录全局可查，不按地理范围过滤"——`CrimeRecord` 本身不需要单独携带位置，`HistoricalEvent.location`（信封自带字段）已经够用，未来势力/领土系统落地后，按地点反查所属势力、再决定"这个势力知不知道"是一个纯粹的**查询期**过滤逻辑，不需要改动 `CrimeRecord` 本身的形状。

### 5.3 后果——只给挂载点

通缉、赏金、NPC 态度——这些系统均不存在（悬赏结构未落地、NPC 态度/关系记忆偏移未落地，`kill-and-death-events.md` 状态行已核实"关系记忆偏移的存储结构本身……未落地，本文档只指出触发点"，本文档面对的是同一个上游缺口）。**挂载点：`Effect::RecordHistoricalEvent(HistoricalEventKind::Crime(..))` 产出的那一刻,是未来任何后果系统应该订阅的事件源**——目击者名单（`CrimeRecord.witnesses`）非空时尤其该是通缉/NPC 态度系统的第一个消费入口,但两者本身仍不存在,本文档不代它们设计——与"三轴战斗结算的命中判定……是未来驱动动画播放的 `Effect` 来源"（README 索引第十六份文档描述）同一个模式：本文档只交付事件本身,不交付任何消费它的下游系统。

---

## 六、mod 可配置

| 项目 | 档位 | 理由 |
|---|---|---|
| **拾取有主物即构成盗窃（既遂）这条判定规则本身** | 引擎规则,不可配置 | 这是整套系统的核心不变式,不是内容参数——若允许 mod 关掉它,`Owner` 字段本身就失去意义 |
| **`can_merge` 追加 `owner`/`stolen_marker` 比较** | 引擎规则,不可配置 | 数据完整性约束,与 `durability` 的比较同一类,不属于"内容" |
| **某件具体物品是否允许公共取用、不算偷**（例如任务给的物品、公共设施里的展示品） | **可配置,一档**,复用现有 `ItemDef.tags` 机制,不新开注册函数 | 见下文 |
| **销赃时长** | **可配置,运行期参数**,不进注册表 | 见下文，与 `DEFAULT_GROUND_ITEM_MAX_AGE_TICKS` 同一先例 |
| **NPC 目击半径基准值（`NPC_BASE_SIGHT_RADIUS`）** | **可配置,运行期参数**,不进注册表——与销赃时长同一先例，见下文 | 二节 2.3/2.6 |
| **暗视对目击半径的加成** | ~~不存在,无法配置——`darkvision_floor` 字段已落地但零消费者~~ **已被推翻**：暗视现已改成 `RaceDef.darkvision_cells`（夜间视野格数下限）并真的在生效，见二节 2.6 顶部的更正框；NPC 目击半径这一路要不要跟着读它，仍是一个未接线的独立问题 | 二节 2.6 |
| **后果（通缉/赏金/态度）** | 将来扩展,连挂载系统本身都未落地,现在不给档位 | 五节 5.3 |

### 「公共取用」——复用现有 `tags`，不新增注册函数

`ItemDef.tags: Vec<ContentIndex>` 已经落地（`item-system.md` 一节表格），是"武器/消耗品/任务物品"这类内容标签的既有通道。给某件物品打上一个 `lostland:public_use` 标签，`resolve_pick_up` 的盗窃判定读到这个标签就直接放行（不比较 `owner`）——**这条能力完全不需要任何新的 `register-*` 函数**,内容作者用已有的 `register-item-*` 追加机制（若 `tags` 目前只能在 `register-item` 六参数里设置,那就是六参数里已有的能力,本文档不清楚 `tags` 具体走哪条注册路径,但可以肯定它不需要为"犯罪判定"这一件事单开新函数——这是本条规则的核心结论,不依赖具体走哪条已有路径）。

若未来发现 `tags` 不够用（例如需要一个只服务盗窃判定的独立布尔位,而不是借用通用标签语义），按 `register-item-equip-mask`/`register-item-stat-bonus`/`register-item-durability` 已经立下的先例（`crates/ll-mod/src/script_item_api.rs:80-87`）新增一个**追加声明**函数,不改 `register-item` 现有六参数：

```
(register-item-theft-exempt id)
```

一档（`ADR 0016` 分类）：静态布尔值,注册期直接物化成一张按 `ContentIndex` 索引的 `BitVec`（`ADR 0017`"按属性分列，不按内容分结构"),运行期 `resolve_pick_up` 一次数组访问,零脚本调用。**不改任何既有 `register-*` 的参数个数**——这是一个全新的独立函数,与 `register-item-equip-mask` 的追加模式完全同构。

### 销赃时长——援引既有先例，不新开注册表

`cleanup_aged_ground_items`（`crates/ll-world/src/state.rs:656`）已经给出了一个几乎一模一样的先例：30 天的老化阈值**不是**写死的常量,也**不是**塞进一张新造的注册表,而是一个运行期参数,只提供一个 `DEFAULT_..._TICKS` 常量给不需要自定义的调用方当默认值——理由原文：「当前代码库还没有一张『游戏规则配置表』……真正的 mod 可声明覆盖需要那张表先存在，不在本批次范围内提前搭建一套只服务这一个数字的注册表（YAGNI）」。

销赃时长是同一类数字（一个全局标量，不按内容/物品各自配置），**按同一条判断，直接复用这条先例**：

```rust
pub const DEFAULT_LAUNDER_TICKS: i64 = /* 若干游戏日 * TICKS_PER_DAY，具体天数留给项目所有者拍板 */;

pub fn launder_stolen_items(&mut self, threshold_ticks: i64) -> usize { /* 见三节 3.2 */ }
```

`threshold_ticks` 是运行期参数，mod/未来的"游戏规则配置层"只需要算出一个新数字传进来，不需要引擎方法跟着改一行代码——**不为这一个数字单独造注册表，与老化阈值同一条 YAGNI 判断**，等真正的"游戏规则配置表"这个更大的基础设施存在时（当前不存在，`cleanup_aged_ground_items` 文档已经如实标注过这一点），销赃时长自然会成为那张表里的一行，不需要现在提前搭一套只服务它自己的机制。

### `NPC_BASE_SIGHT_RADIUS`——同一先例的第二个数字

二节 2.3/2.6 的目击算法需要一个"NPC 基准视野半径"（喂给 `sight_radius_at` 再叠加光照缩放）。这不是内容——它不按物品/角色各自配置，是一个全局标量——**按与销赃时长完全相同的判断，同样不新开注册表**：

```rust
pub const DEFAULT_NPC_BASE_SIGHT_RADIUS: u32 = 12; // 与玩家渲染层 BASE_SIGHT_RADIUS 同一量级,具体数值留给项目所有者拍板

fn witnessed_by(world: &WorldState, culprit_pos: TorusPos, at: Tick, base_radius: u32) -> Vec<EntityId> { /* 见二节 2.3 */ }
```

`base_radius` 运行期传入，`DEFAULT_NPC_BASE_SIGHT_RADIUS` 只是不需要自定义时的默认值——与销赃阈值、地面物品老化阈值同一条既有纪律，不重复论证。**种族级别的差异化（暗视加成、种族基准视野不同）不在这条参数化范围内**——那需要把 NPC 目击半径这一路改用 `ll_sim::vision::sight_radius_for_race`（玩家渲染那一路已经在用；暗视本身已经生效，见二节 2.6 顶部的更正框），本节的参数化只解决"这一个全局数字该怎么给"，不解决"暗视要不要生效"。

---

## 七、场景走查

### 场景一：从村民家里偷一把剑，若干天后销赃完成

1. **前置**：村民 NPC（`EntityId` 为 `V`）的背包里有一把剑 `ItemStack { def: iron_sword, count: 1, durability: Some(100), owner: Owner::Npc(W_v), stolen_marker: None }`——`W_v` 是这个村民的 `remembered_id`，在给他的私产打归属时懒分配（一节 1.2）。
2. 玩家角色（`actor`，`world.player_entity == Some(actor)`）执行 `Intent::PickUp`。**注**：本场景假设剑已经从村民背包被丢在地上（`GroundItemStack { pos, stack, dropped_at }`）——当前引擎的 `resolve_pick_up` 只能捡地面物品，"从别人背包里直接偷"这个动作本身（不经过地面这一步）需要一个新的 `Intent`（例如 `Intent::Steal { target: EntityId }`），当前不存在，本场景走的是"剑已经在地上"这条现有路径能表达的部分。
3. `resolve_pick_up` 读到 `ground.stack.owner == Owner::Npc(W_v)`，`actor` 的归属判据（`Owner::Player`）与之不符 → 判定为盗窃（既遂）。产出效果序列：`RemoveGroundItem` + `MergeIntoInventory`（原有两个）+ 新增一步——把即将合并进背包的 `ItemStack` 的 `stolen_marker` 设为 `Some(StolenMarker { original_owner: Owner::Npc(W_v), stolen_at: world.clock })`（`owner` 字段本身**不变**，仍是 `Owner::Npc(W_v)`，见三节 3.3 表格第一行）。
4. **目击判定**（二节 2.3）：`resolve_pick_up` 同一次结算里调用 `witnessed_by(world, actor.pos, world.clock)`，对 `world.actors` 做两段式扫描。两种走向都要交代：
   - **若村民 V 本人此刻就站在自己家里**（`V` 是厚层实体，`agent.pos` 与犯罪地点的切比雪夫距离落在其视野半径内，且 FOV 真的能看到该坐标）→ `witnessed_by` 返回 `[V 的 EntityId]`。
   - **若这间屋子此刻无人**（`V` 已外出，或 `V` 只是薄层背景人口、根本不在 `world.actors` 里）→ 返回空列表。
   - **无论哪种走向，第 3 步的既遂判定与 `stolen_marker` 计时都照常发生**——二节 2.2 的结论没有变：目击只决定 `CrimeRecord.witnesses` 是否非空，不决定"算不算犯罪"。
5. `Effect::RecordHistoricalEvent(HistoricalEventKind::Crime(CrimeRecord { perpetrator: Some(player_remembered_id), witnesses, kind: CrimeKind::Theft { victim: Owner::Npc(W_v), item_def: iron_sword, count: 1 } }))`——`witnesses` 是第 4 步算出的结果（经 `apply` 侧 `record_crime` 把 `EntityId` 翻译成 `WorldId`，可能是空 `Vec`）。
6. 玩家背包里现在有一把 `owner` 仍标记为 `Owner::Npc(W_v)`、带 `stolen_marker` 的剑——**账面上这把剑依然不是玩家的**，只是玩家实际持有它。历史日志里多了一条可查询的 `Crime` 记录（不论是否被目击）。
7. 世界时钟推进（`advance`），期间玩家可以把剑丢在地上再捡起（`stolen_marker` 原样携带,不重置，三节 3.3）、可以带着它到处走。
8. 某次惰性追赶/系统性检查调用 `launder_stolen_items(DEFAULT_LAUNDER_TICKS)`（六节）：`now - stolen_at >= threshold_ticks` 成立 → 这把剑的 `owner` 改写为 `Owner::Player`，`stolen_marker` 清空为 `None`。
9. 销赃完成：这把剑现在是玩家账面上合法的财产,再次执行任何"这是谁的"判定（例如未来的合法转移四节）都会得到 `Owner::Player`,不再关联最初的盗窃。**这条销赃动作本身不产出新的历史事件**（`HistoricalEventKind` 没有新增"洗白"变体）——`Crime` 记录本身不可变、永久留在日志里（五节 5.1 已论证"是否犯罪"是查询而非存储），销赃只改变当下的 `Owner`,不改写历史,这正是"洗白"这个词准确的含义：物品变得合法,不代表"曾经偷过、曾经被人看见"这些事从未发生过。

### 场景二：从怪物尸体上捡东西——无主，不算犯罪

1. 一只怪物被击杀，`resolve_attack`/`apply` 结算完成后产出若干 `GroundItemStack`（掉落物）——这些堆在生成时 `owner` 字段应为 `Owner::Unowned`（一节 1.5，默认值）,`stolen_marker` 为 `None`。
2. 玩家角色执行 `Intent::PickUp`，`resolve_pick_up` 读到 `ground.stack.owner == Owner::Unowned`。
3. 判定逻辑：`Unowned` 不触发盗窃分支（二节 2.1 已给出判据：`Unowned` 恒放行）——直接走现有的两个 `Effect`（`RemoveGroundItem` + `MergeIntoInventory`），**不追加 `stolen_marker`，不调用 `witnessed_by`，不产出 `HistoricalEventKind::Crime`**（即使这一刻真有 NPC 站在旁边看着，也不构成犯罪——目击判定只在第 3 步已经认定"这是非法操作"之后才会触发，`Unowned` 从不触发这一步，二节 2.7 的判定入口分工在此处生效）。
4. 玩家背包里的战利品 `owner` 仍是 `Owner::Unowned`——这堆物品此刻"谁都不欠"，未来若玩家把它送给别人（四节合法转移），转移校验（"发起方是否是当前 `owner`"）对 `Unowned` 恒放行，因为没有人的权益因为这次转移受损。

---

## 八、现在能做的 vs 等什么

**现在能做的**（本文档冻结后，若要往前推进一步，不需要等待任何外部系统）：

- 给 `ItemStack` 加 `owner: Owner`/`stolen_marker: Option<StolenMarker>` 两个字段，`can_merge` 追加对应比较——一节 1.6，机械改动，`merge_stacks`/`split_stack` 免改。
- `GroundItemStack` 的构造点补上 `owner: Owner::Unowned` 默认值——一节 1.5。
- `resolve_pick_up` 插入归属判定分支（`Unowned`/`Player` 自己/`Npc` 自己放行，否则记 `stolen_marker`）——二节 2.1、三节 3.3。
- `witnessed_by` 两段式目击算法本身（`chebyshev` 粗过滤 + `compute_fov` 成员测试，全部基于已落地的 `Arena::iter_with_id`/`fov.rs`/`ll_world::light`）——二节 2.3–2.6，**这是本轮修订新确认可以现在做的部分**，不再是"等空间查询"。
- `HistoricalEventKind::Crime(CrimeRecord{ kind: CrimeKind, .. })` 变体本身与 `record_crime`（对齐 `record_kill` 的既有模式，内部对每个目击者调用既有的 `remembered_id_of_or_assign`）——五节 5.1。
- `launder_stolen_items`/`DEFAULT_LAUNDER_TICKS`、`NPC_BASE_SIGHT_RADIUS`/`DEFAULT_NPC_BASE_SIGHT_RADIUS` 两个运行期参数（对齐 `cleanup_aged_ground_items` 的既有模式，不进注册表）——六节。
- `register-item-theft-exempt` 一档注册函数——六节，不改任何既有 `register-*` 参数个数。

**等什么**（本文档不能替它们做决定，只给出接口形状/依赖标注）：

| 缺口 | 阻塞了什么 | 状态 |
|---|---|---|
| **"从别人背包直接偷"这个 `Intent`**（当前只能偷地面物品，场景一走的是"剑已经在地上"这条降级路径） | 更完整的盗窃场景（直接从 NPC 背包/尸体容器行窃） | 不存在,`Intent` 需要新变体 |
| ~~空间查询（以某坐标为中心查附近实体）~~——**本轮修订已核实解除**：目击判定用 `compute_fov`（原点是参数）+ `Arena::iter_with_id` 现在就能表达，见二节 2.3。仍然真实存在、未解除的是两条更窄的缺口： | | |
| ├ `resolve_attack` 拿不到"以目标为中心查同阵营盟友"这一条特定输入（盗贼偷袭需要的能力，非本文档范围） | 天赋系统的偷袭条件（`trait-system.md`），与本文档的目击判定是两回事 | `trait-system.md` 已核实,仍未解除 |
| ├ 室内/建筑物的额外调光（`effective_ambient_light`/`SpaceProfile`）在 `ll-sim` 里拿不到，目击半径只按纯时间光照算，不区分"在室内还是室外" | 二节 2.6「室内更暗」这层细节表达不出来,只有昼夜这一层能表达 | `resolve.rs` 已核实同一处依赖缺口 |
| ├ ~~`RaceDef.darkvision_floor` 字段已落地但零消费者~~ **已被推翻两次**：先接线，再改语义成 `darkvision_cells`（夜间视野格数下限） | 暗视种族"晚上依然能看见"这半句玩法结论**现已成立**（矮人 7 格 / 精灵 6 格 / 人类 4 格） | 二节 2.6 顶部更正框；`crates/ll-mod/tests/base_mod_darkvision.rs` |
| **交易系统** | 合法转移之"购买"（四节） | 不存在,无价格结算/货币接线 |
| **对话/交互系统** | 合法转移之"赠送" | 不存在 |
| **任务奖励发放的 `resolve`/`Effect`** | 合法转移之"任务" | `QuestNodeDef` 已落地但奖励发放机制未落地 |
| **世界生成 / 势力实例（P9）** | `Owner::Faction` 真正能被赋值、犯罪的"管辖区"（五节 5.2） | `society-and-affiliation.md` 状态行已核实 |
| **通缉/赏金/NPC 态度系统** | 犯罪记录产生任何即时后果（五节 5.3） | 均不存在,只给挂载点 `RecordHistoricalEvent` |
| **商店系统裁定** | `Owner::Shop` 的最终引用类型（一节 1.4） | 不代经济/社会文档发言,标注待定 |

---

**未采纳的更大范围方案汇总**（正文已分别论证，此处只做索引）：独立犯罪日志表（五节 5.1，理由同 `BattleLog` 否决先例）；`Owner` 存进独立映射表而非 `ItemStack`（一节 1.6）；计时存 `dropped_at`/`Agent` 而非 `ItemStack.stolen_marker`（三节 3.4）；`Owner::Npc`/`Owner::Faction` 沿用原 `EntityId`/`ContentIndex`（一节 1.2、1.3）；为「公共取用」新开注册表而不是复用 `tags`/追加声明模式（六节）。
