# 两级坐标系与离散层重写 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。
> **红灯窗口提醒**：任务 1–10 全部是新增模块/新增类型，不改动任何既有函数的行为，理论上每一步之后 `cargo test --workspace` 都应保持全绿。任务 11（`WorldState.terrain` 换型）是全计划唯一无法避免整仓编译短暂变红的一步——Rust 的全程序类型检查决定了字段换型必须在同一次提交内让全部调用点同时更新完毕，做不到「切一半仍能编译」。纪律是：**红灯只能出现在任务 11 内部的本地开发过程中，绝不能把半改状态提交入库**；任务 11 前后各自保持全绿，是本计划能给出的最强保证。

**目标：** 把 `ll_world::state::WorldState.terrain: ChunkGrid`（整个世界一整张连续瓦片图、一次性生成、整体常驻）替换为 `knowledge/design/coordinate-system-and-layers.md` 定案的两级坐标系模型：地表 `Space::Surface` 按区块（默认 128×128 格）流式生成与常驻、地下城/建筑内部 `Space::Interior` 各自独立的有界局部图、锚定在地表某一格。这项重写必须在 P5 冻结存档格式之前完成结构定案（数值可后调），是本项目至今改动面最大的一次重构。

**架构：** 沿用 C1–C5（`apply` 唯一写入口、时间轴只装朴素数据、随机必经 `DetRng::for_entity`、后台推进必到确定 tick、禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑判断）。**C4 与 C5 在本计划里第一次真正有实际代码可以违反**——此前 C4「尚无相关代码」（见 `docs/architecture/03-invariants.md`），本计划的区块流式加载/LRU 常驻集合正是第一处需要正面回答「后台/常驻集合的变化能不能推到确定 tick」的代码；C5 同理，区块索引若用 `HashMap` 做 O(1) 查找是安全的，但**淘汰顺序、常驻集合快照顺序绝不能依赖哈希桶序**，这是本计划风险最高任务（任务 9）的核心约束。

**技术栈：** `ll-core` + `ll-world`（已有）；不新增 crate、不新增第三方依赖。

**设计依据：** [`knowledge/design/coordinate-system-and-layers.md`](../../../knowledge/design/coordinate-system-and-layers.md)（457 行，本计划的全部结构性决定均来自此文档，正文不复述论证，只标注落点）
**规格：** [`docs/superpowers/specs/2026-08-16-lostland-design.md`](../specs/2026-08-16-lostland-design.md) §4（C1–C5）、§5（crate 分层）、§7.1（本节取代说明）、§15（P5/P7 两行）
**上阶段交接：** [`knowledge/handoff/p4-to-p5.md`](../../../knowledge/handoff/p4-to-p5.md)，尤其「三、坐标系变更会影响存档格式」一节
**架构骨架：** [`docs/architecture/03-invariants.md`](../../architecture/03-invariants.md)（C1–C5 及违反后果）、[`04-torus-topology.md`](../../architecture/04-torus-topology.md)（环面数学的既有教训）
**真实代码基线：** `crates/ll-world/src/{state,chunk,terrain,generate,fov,light,overview}.rs`、`crates/ll-render/src/camera.rs`、`crates/ll-core/src/torus.rs`，HEAD `cdcffb2`，495 个 `#[test]`（交接清单写「496 个测试」，与本次核实的 495 处 `#[test]` 标注计数口径极可能有一位之差——如实记录这个出入，不代为判断哪个数字对，不影响本计划的任务划分）

---

## 全局约束

- **世界状态禁止浮点**；环面坐标只走 `TorusSize`/`TorusPos` 的方法；新的有界局部坐标类型同样只能通过自己的构造函数产出，不得手写距离/位移。
- **`apply` 是唯一写入口**——`Space` 切换（进出 `Interior`）与地表流式加载触发的任何状态变化，都必须走 `Intent → resolve → Effect → apply`，不能在渲染/流式加载代码里直接改 `WorldState`。
- **区块常驻集合与 LRU 淘汰的变化只能由确定性输入驱动**（当前 tick、玩家位置、`Intent` 结算结果），**不得读墙钟/系统时间、不得依赖线程调度顺序**——这是 C4 在本计划里第一次真正需要被遵守，而不是「给未来实现者的红线」。
- **区块索引可以用 `HashMap` 做 O(1) 查找，但淘汰顺序、快照顺序、序列化顺序一律不得依赖其迭代顺序**（C5）——需要顺序的地方用 `BTreeMap`/显式排序键。
- 「私有字段 + 校验构造函数」的类型加 serde 须用 `try_from` 中转（ADR 0011）。
- 依赖方向不得反向：`ll-core` 零依赖，`ll-world` 不得反向依赖 `ll-mod`/`ll-script`（沿用 `terrain.rs` 已验证的「接受注入的解析回调」模式）。
- 文件 200–400 行为宜，800 行上限；提交信息 `<type>: <描述>`，正文讲**为什么**，中文，不得含 AI 署名。

---

## 关键设计判断（本计划在设计文档留白处做出的实现判断，非设计文档裁定）

设计文档定了「是什么」，以下是把它落成代码需要回答的「怎么做」，属于本计划的实现判断，评审时可推翻：

1. **区块内部存储直接复用既有 `ChunkGrid`，不新建存储类型。** 设计文档十节裁定「区块 = 4×4 存储块」（甲案），核实 `ChunkGrid`（`chunk.rs`）本身就是「按 `CHUNK_SIZE=32` 分块存储的环面网格」，且它的构造校验（`world >= 43×25`）在区块默认 128×128 下天然满足。**一个区块-层 = 一个 `ChunkGrid` 实例，`TorusSize` 取区块边长**——`ChunkGrid` 类型本身不需要改一行，甲案的「4×4 存储块对齐」由 `ChunkGrid` 内部的 `CHUNK_SIZE` 自动满足。这比重新设计一个存储类型风险小得多，也让 `chunk.rs` 现有的全部测试保持有效（见任务 9）。
2. **`Interior` 的楼层地图不能复用 `ChunkGrid`**，理由是设计文档六节已核实的「`TorusSize::wrap` 在有界场景下视野半径接近地图尺寸一半时会隔着两端相互看见」——需要一个真正不环绕的姊妹类型（任务 5 的 `BoundedGrid`）。
3. **`SurfaceStore`（区块流式存储）与 `Interior` 楼层共享同一个 256 常驻上限与同一套 LRU 时钟**，不是两个独立预算。依据：设计文档五节「常驻集合的构成」第 2 条明确把「当前所在建筑/地下城的全部层」算进同一个 256 预算。实现上拆成「统一的淘汰时钟与计数器」+「Surface 用 `ChunkGrid`、Interior 用 `BoundedGrid` 的具体存储各自独立」两层，而不是两套互相不知道对方的 LRU（否则总常驻量会超出 256 这个设计意图）。
4. **`TorusPos` 补一个基于 `(x, y)` 字典序的 `Ord`/`PartialOrd`**，纯用于区块索引/常驻集合快照需要一个确定性排序键的场景（C5 要求），不赋予任何游戏逻辑含义——这是任务 1 的一部分，不单独立项。

---

### 任务 1：`ll-core` 坐标地基扩展——有界局部坐标 + `TorusPos` 补 `Ord`

**Files:** `crates/ll-core/src/bounded.rs`（新）、`crates/ll-core/src/torus.rs`（追加 derive，不改逻辑）

纯新增，不改变任何既有函数行为。

**Interfaces Produces（概念形状，字段/方法名可在实现时调整）：**
```rust
// torus.rs：追加 Ord/PartialOrd 派生，供区块索引类需要稳定排序键的场景使用（C5）。
#[derive(..., PartialOrd, Ord)]
pub struct TorusPos { x: i32, y: i32 }

// bounded.rs：非环绕的有界局部坐标，供 Interior 使用。
pub struct BoundedSize { width: u32, height: u32 }
pub struct BoundedPos { x: i32, y: i32 }
impl BoundedSize {
    pub fn new(width: u32, height: u32) -> Option<Self>;
    /// 越界返回 None——与 TorusSize::wrap 恒成功不同，这里没有「绕回」
    /// 可以兜底，越界就是越界。
    pub fn try_pos(&self, x: i32, y: i32) -> Option<BoundedPos>;
    pub fn delta(&self, from: BoundedPos, to: BoundedPos) -> (i32, i32); // 不求最短路径，就是原始位移
    pub fn squared_euclidean(&self, a: BoundedPos, b: BoundedPos) -> u64;
    pub fn chebyshev(&self, a: BoundedPos, b: BoundedPos) -> u32;
}
```

- [ ] **TDD 循环**：
  - `越界坐标返回 None 而非绕回`
  - `合法坐标往返一致`
  - `两点位移不做环绕最短路径计算，就是原始差值`
  - `TorusPos 按 (x, y) 字典序排序`（新增 `Ord`，确认与既有 `PartialEq`/`Hash` 不冲突）
- [ ] **提交**（`ll-core` 现有全部测试必须保持通过，本任务不改任何既有函数）

---

### 任务 2：`ll-world::space`——`Space`/`ZoneCoord`/`SpaceId` 类型

**Files:** `crates/ll-world/src/space.rs`（新）
**依赖：** 无新依赖（`SpaceId` 复用 `ll_core::ident::WorldId`，P4 已交付）

落地设计文档四节的 `Space` 枚举形状，纯新增类型，不接入 `WorldState`。

**Interfaces Produces：**
```rust
/// 区块坐标：与世界瓦片坐标（TorusPos）同一个类型，喂给区块粒度的
/// TorusSize 即得——不新增坐标类型，只是语义上的另一种叫法。
pub type ZoneCoord = ll_core::torus::TorusPos;
pub type SpaceId = ll_core::ident::WorldId;

pub enum Space {
    Surface { zone: ZoneCoord, z: i8, profile: ContentIndex },
    Interior { id: SpaceId, floor: i16, anchor: ll_core::torus::TorusPos, profile: ContentIndex },
}
```

- [ ] **TDD 循环**：
  - `Surface 与 Interior 变体的 profile 字段类型一致，可以用同一个查表函数`
  - `Interior 的 floor 允许负数`
  - 一个说明性 doctest/注释：`Surface.z 当前恒为 0`（不作为运行时断言，只是防止后来者误用）
- [ ] **提交**

---

### 任务 3：`SpaceProfile` 走注册表——仿 `TerrainDef`/`TerrainTable` 模式

**Files:** `crates/ll-world/src/space_profile.rs`（新）
**依赖：** 任务 2（形状对齐，代码上无强依赖）

落地设计文档七节的 `SpaceProfile` 形状，**照抄 `terrain.rs` 已验证的模式**：私有字段 + `SpaceProfileTable::define` 注册期校验 + `materialize_base_space_profiles(intern: &mut dyn FnMut(...))` 本体注册入口 + `base_space_profile_fixture()` 测试夹具。本体至少注册「地表」「洞窟」「地下城」「建筑内部」四种基础 profile。

**Interfaces Produces：**
```rust
pub struct SpaceProfile {
    pub id: NamespacedId,
    pub ambient_light_floor: i32,
    pub exposed_to_sky: bool,
    pub base_temperature: i32,
    pub diggable: bool,
    pub buildable: bool,
    pub reverb_tag: Option<NamespacedId>,
}
pub struct SpaceProfileTable { /* 列式存储，同 TerrainTable 的结构 */ }
pub fn materialize_base_space_profiles(intern: &mut dyn FnMut(NamespacedId) -> ContentIndex)
    -> Result<(BaseSpaceProfileIds, SpaceProfileTable), SpaceProfileError>;
pub fn base_space_profile_fixture() -> (BaseSpaceProfileIds, SpaceProfileTable);
```

- [ ] **TDD 循环**（对照 `terrain.rs` 现有测试逐条抄一遍同构版本）：
  - `地表 profile 的 exposed_to_sky 为真`
  - `地下城 profile 的 exposed_to_sky 为假`
  - `重复定义同一个索引返回错误而非静默覆盖`（与 `TerrainTable` 同一条纪律）
  - `本体 profile 与假想 mod profile 走同一条 intern 路径`（本体即 Mod 检验，最小形式）
- [ ] **提交**

---

### 任务 4：光照与 `SpaceProfile` 的调用方组合

**Files:** `crates/ll-world/src/light.rs`（**不改**——设计文档七节明确「不改一行」）、新增调用方函数，建议放 `crates/ll-world/src/space_profile.rs` 或 `fov.rs` 附近

**依赖：** 任务 3

只新增一个纯函数，`light.rs` 本身零改动，其现有全部测试不受影响。

**Interfaces Produces：**
```rust
/// 消费光照的调用方必须经过这个函数，不能对 Interior 直接调用
/// ambient_light(tick)——否则地下城正午会满光照。
pub fn effective_ambient_light(profile: &SpaceProfile, tick: Tick) -> LightLevel {
    if profile.exposed_to_sky {
        ll_world::light::ambient_light(tick)
    } else {
        LightLevel(profile.ambient_light_floor)
    }
}
```

- [ ] **TDD 循环**：
  - `露天 profile 的有效光照随时钟变化`
  - `地下 profile 的有效光照恒为地板值，不随时钟变化`
- [ ] **提交**

---

### 任务 5：`BoundedGrid`——`Interior` 用的有界局部地形存储

**Files:** `crates/ll-world/src/bounded_grid.rs`（新）
**依赖：** 任务 1（`BoundedSize`/`BoundedPos`）

**与 `ChunkGrid` 平行但不环绕**——`new`/`terrain_at`/`set_terrain` 接口形状一致，内部不做取模折返，越界坐标是 `Option`/`Err`，不是「绕到另一头」。是否内部也按存储块分块，本任务自行判断（`Interior` 楼层通常不大，可以先用单一 `Vec` 实现，不强求对齐 `CHUNK_SIZE`——设计文档十节末尾已明确「这条对齐关系不适用于 Interior」）。

**Interfaces Produces：**
```rust
pub struct BoundedGrid { /* 单一 Vec<TerrainKind>，按 BoundedSize 定长 */ }
impl BoundedGrid {
    pub fn new(size: BoundedSize, fill: TerrainKind) -> Self;
    pub fn size(&self) -> BoundedSize;
    pub fn terrain_at(&self, pos: BoundedPos) -> TerrainKind;
    pub fn set_terrain(&mut self, pos: BoundedPos, kind: TerrainKind);
}
```

- [ ] **TDD 循环**：
  - `写入后可读回同一地形`
  - `越界坐标不 panic`（构造 `BoundedPos` 本身已经在 `try_pos` 那一步拒绝越界，这里补一条「不存在绕回语义」的说明性测试，与 `ChunkGrid` 的「环面绕回坐标指向同一格」测试形成对照，防止有人误以为行为一致）
- [ ] **提交**

---

### 任务 6：`compute_fov` 泛化——兼容环面与有界两种网格

**Files:** `crates/ll-world/src/fov.rs`
**依赖：** 任务 5

这是设计文档六节核实过「算法零改动，但输入类型需要新变体」的落点。**建议做法**：给 `compute_fov` 依赖的「世界」部分抽一个最小 trait（例如提供 `terrain_at` 与一个「按偏移求出目标坐标，越界时返回 `None`」的方法），`ChunkGrid`+`TorusSize` 与 `BoundedGrid`+`BoundedSize` 各自实现一份。**越界处理是这里真正的算法级改动**（不是设计文档说的「零改动」那部分，是「需要新变体」那部分的具体内容）：环面场景 `wrap` 恒成功，有界场景越界必须让扫描把该方向视为「无格可看」而不是绕接缝——这需要 `scan_row_in_sector` 内部把 `ctx.world.wrap(...)` 换成一个可能返回 `None` 的调用，`None` 时跳过该格（既不标记可见也不参与遮挡）。**具体 trait 形状留给实现者决定**，以上是概念方向。

**必须保持向后兼容**：`ChunkGrid` 场景下算法产出的可见集合必须与改造前逐格相同——这是本任务最重要的回归验证点，`tests/fov_blackbox.rs` 的属性测试与既有单元测试全部必须原样通过，不得修改其期望值。

**Interfaces Produces（概念形状）：**
```rust
pub trait SightGrid {
    type Pos: Copy;
    fn terrain_at(&self, pos: Self::Pos) -> TerrainKind;
    /// 从 origin 出发按 (dx, dy) 偏移求坐标；有界网格越界返回 None。
    fn offset(&self, origin: Self::Pos, dx: i32, dy: i32) -> Option<Self::Pos>;
    fn squared_euclidean(&self, a: Self::Pos, b: Self::Pos) -> u64;
}
pub fn compute_fov<G: SightGrid>(grid: &G, table: &TerrainTable, origin: G::Pos, radius: u32) -> VisibleSet<G::Pos>;
```

- [ ] **TDD 循环**：
  - **既有全部 `fov.rs` 单元测试与 `tests/fov_blackbox.rs` 属性测试原样通过，零改动期望值**（回归红线）
  - `有界网格中越过边界的方向被视为不可见，不绕接缝`
  - `有界网格中视野半径超过地图一角时不 panic、不产出越界坐标`
  - `环面与有界两种网格用同一份 scan_octant/scan_row_in_sector 逻辑`（可以是代码审查项而非运行时断言：确认没有为两种场景各写一份阴影投射算法）
- [ ] **提交**

---

### 任务 7：`Camera` 泛化——`Interior` 场景可用的相机变体

**Files:** `crates/ll-render/src/camera.rs`
**依赖：** 任务 5

与任务 6 同源的另一半。`world_to_screen`/`visible_tiles` 的屏幕换算算法不变，但需要一个基于 `BoundedSize`/`BoundedPos` 的等价「世界」上下文。**`Surface` 场景的 `Camera` 不改一行**（设计文档六节已核实），本任务只新增一个 `Interior` 专用相机类型或让 `Camera` 对世界上下文做同样的 trait 抽象。

**Interfaces Produces（概念形状，可与任务 6 共用同一个 trait，也可独立，留给实现者判断）：**
```rust
pub struct BoundedCamera { pub center: BoundedPos, pub world: BoundedSize }
impl BoundedCamera {
    pub fn world_to_screen(&self, pos: BoundedPos) -> (i32, i32);
    pub fn visible_tiles(&self) -> Vec<BoundedPos>;
}
```

- [ ] **TDD 循环**：
  - **既有全部 `camera.rs` 单元测试原样通过**（回归红线）
  - `有界相机的相机中心落在视口正中`（对照既有同名测试的有界版本）
  - `有界相机在世界边缘不产出环绕坐标`（Interior 地图边缘就是边缘，不应该有「贴着西边看见东边」的效果）
- [ ] **提交**

---

### 任务 8：区块坐标换算 + 窗口化地形生成入口

**Files:** `crates/ll-world/src/zone.rs`（新）、`crates/ll-world/src/generate.rs`（新增函数，`terrain_at_coord`/`build_noise` 不改一行）

**依赖：** 任务 1（`TorusPos` 的 `Ord`，用于区块坐标排序场景）

落地设计文档五节「核心机制：全局连续噪声场的窗口采样」——`terrain_at_coord`/`build_noise` 已经是可独立复用的两步（模块文档原话），本任务只加一层「对某个区块窗口调用它们、写入一个 `ChunkGrid`」的入口，不改这两个函数。

**Interfaces Produces：**
```rust
/// 区块布局配置：区块边长（默认 128）+ 世界区块数（默认 48×32）。
/// 两者都是可配置数值，不是结构约束（见设计文档十二节）。
pub struct ZoneLayout { pub zone_span: u32, pub zone_count: TorusSize }
impl ZoneLayout {
    pub fn default_config() -> Self;
    /// 世界瓦片总尺寸 = zone_span * zone_count，供需要瓦片级 TorusSize 的
    /// 调用方（如 minimap）派生使用，不单独存一份。
    pub fn tile_size(&self) -> TorusSize;
    pub fn tile_to_zone(&self, pos: TorusPos) -> (ZoneCoord, /* 区块内局部坐标 */ TorusPos);
}

/// 只生成一个区块窗口的地形，不遍历整个世界——generate_terrain 的
/// 窗口化版本，复用同一个 noise 源与同一套阈值逻辑。
pub fn generate_zone_window(
    noise: &TileableNoise,
    params: &GenParams,
    layout: &ZoneLayout,
    zone: ZoneCoord,
    terrain_ids: &BaseTerrainIds,
) -> Result<ChunkGrid, WorldError>;
```

- [ ] **TDD 循环**：
  - `瓦片坐标到区块坐标的换算与区块内局部坐标的换算互为逆运算`
  - `相邻区块窗口生成的地形在共享边界上与整图生成结果一致`（复用 `generate.rs` 已有的「东西接缝两侧地形一致」测试思路，改为比较「区块窗口生成」与「全图生成」在同一坐标处的结果，这是本任务最重要的正确性回归——它直接验证设计文档五节「窗口化调用不需要改这个函数一行」这条论断在区块粒度上真的成立）
  - `区块边长不是 CELL_SIZE 或 CHUNK_SIZE 整数倍时构造 ZoneLayout 失败`
- [ ] **提交**

---

### 任务 9：`SurfaceStore`——区块流式加载与常驻 LRU

**Files:** `crates/ll-world/src/surface_store.rs`（新）
**依赖：** 任务 8

**本计划风险最高的任务，理由见「风险登记」一节。**

落地设计文档五节「常驻集合与 LRU」。核心结构：`HashMap<ZoneCoord, ChunkGrid>` 做 O(1) 查找（安全用法，见 C5），配一个**独立于哈希桶序的确定性淘汰时钟**——建议 `BTreeMap<(LogicalTick, ZoneCoord), ()>` 或等价的「按 `(最近访问 tick, 区块坐标)` 排序」结构，`ZoneCoord` 借任务 1 新增的 `Ord` 在并列 tick 时打破平局，保证淘汰顺序在任何平台/任何进程都可复现。**这是关键设计判断 3 提到的「统一预算」的具体落点**：本任务产出的淘汰时钟/计数器接口需要设计成可被任务 10（`Interior`）共用同一个 256 上限，而不是自己独占一份。

**对外接口形状与 `ChunkGrid` 保持一致**（`terrain_at`/`set_terrain`），这是关键设计判断 1 的直接目的：任务 11 换型时，调用点的方法名不用改，只有构造方式变。

**Interfaces Produces：**
```rust
pub struct SurfaceStore {
    layout: ZoneLayout,
    resident: HashMap<ZoneCoord, ChunkGrid>,
    /// 淘汰时钟：谁最近被访问过，见上文——具体结构留给实现者，
    /// 必须满足「相同输入序列在任何平台产出相同淘汰顺序」。
    recency: /* BTreeMap 或等价结构 */,
    resident_cap: usize, // 默认 256，可配置
}
impl SurfaceStore {
    pub fn new(layout: ZoneLayout, resident_cap: usize) -> Self;
    /// 读取给定瓦片坐标的地形；若所属区块未常驻，按需生成并计入常驻集合，
    /// 超出上限时淘汰最久未访问的一个。这是流式加载的唯一入口。
    pub fn terrain_at(&mut self, noise: &TileableNoise, params: &GenParams,
                       terrain_ids: &BaseTerrainIds, pos: TorusPos, at_tick: Tick) -> TerrainKind;
    /// 写入给定瓦片坐标的地形。前置条件：该坐标所属区块必须已经常驻
    /// （调用方只应该对当前正在模拟/渲染的区块调用，这类区块按定义
    /// 已经常驻）——未常驻时的行为（panic 还是隐式加载）由实现者决定，
    /// 但必须显式选择并写文档，不能不声不响两种都做。
    pub fn set_terrain(&mut self, pos: TorusPos, kind: TerrainKind);
    /// 当前常驻的区块坐标集合，按 Ord 排序返回（供 hash()/序列化使用，
    /// 见任务 11），不暴露内部 HashMap 的原始迭代顺序。
    pub fn resident_zones(&self) -> Vec<ZoneCoord>;
}
```

- [ ] **TDD 循环**（这是全计划测试密度要求最高的任务）：
  - `读取未常驻区块的坐标会触发按需生成`
  - `常驻区块数超过上限时淘汰最久未访问的一个`
  - `刚被访问过的区块不会被淘汰`（LRU 基本性质）
  - `相同的访问序列在两次独立运行中产出相同的淘汰顺序`（**确定性核心断言，C4/C5 的直接体现**——用固定的访问序列跑两遍，断言两次的 `resident_zones()` 快照逐位相同）
  - `并列访问 tick 的两个区块淘汰顺序由区块坐标的 Ord 打破平局，不依赖 HashMap 迭代顺序`（构造一个人为让两个区块「同一 tick 被访问」的场景，反复运行确认淘汰的总是同一个）
  - `窗口化生成的结果与任务 8 的区块窗口生成函数一致`（`SurfaceStore` 不能自己另外实现一套生成逻辑）
  - `写入已常驻区块的地形能被读回`
- [ ] **提交**（建议按「常驻集合与淘汰机制」「按需生成接线」拆成两次提交，即便在同一个任务里完成，降低单次 diff 的排查难度）

---

### 任务 10：`Interior` 存储与锚点

**Files:** `crates/ll-world/src/interior.rs`（新）
**依赖：** 任务 2（`SpaceId`/`Space::Interior` 形状）、任务 5（`BoundedGrid`）、任务 9（共享常驻预算，见关键设计判断 3）

落地设计文档六节「稀疏性：拆成两条」与四节「锚定关系：单一真相源」。**`anchor` 只存在 `Interior` 实例自己身上，反向索引（世界格子 → 入口列表）是派生视图，不是第二份存储**——这条纪律必须在类型设计上体现（反向索引的构造函数只能从 `Interior` 集合现算，没有独立的 `set` 方法）。

**Interfaces Produces：**
```rust
pub struct Interior {
    pub id: SpaceId,
    pub anchor: TorusPos,
    pub profile: ContentIndex,
    /// 稀疏：一栋楼可能只有 {0, 1, 2, -1} 四个 floor。
    floors: HashMap<i16, BoundedGrid>,
}
pub struct InteriorTable {
    /// 权威数据：按 SpaceId 索引。
    interiors: HashMap<SpaceId, Interior>,
}
impl InteriorTable {
    pub fn insert(&mut self, interior: Interior);
    pub fn get(&self, id: SpaceId) -> Option<&Interior>;
    /// 派生视图：现算，不缓存（若未来需要缓存，更新规则必须单向——
    /// anchor 变了就重建对应条目，见设计文档四节）。返回结果按 SpaceId
    /// 排序，不依赖内部 HashMap 迭代顺序（C5）。
    pub fn entries_at(&self, pos: TorusPos) -> Vec<SpaceId>;
}
```

- [ ] **TDD 循环**：
  - `锚点相同的两个 Interior 都能被反向查询找到`
  - `反向查询结果按 SpaceId 排序，多次调用顺序稳定`（C5 直接体现）
  - `Interior 的楼层可以是不连续的整数集合`（例如只有 `{0, 2, -1}`，中间的 1 不存在也不报错）
  - `不存在的 SpaceId 查询返回 None 而非 panic`
- [ ] **提交**

---

### 任务 11：`WorldState.terrain` 迁移到 `SurfaceStore`

**Files:** `crates/ll-world/src/state.rs`（重写）、`crates/ll-sim/src/{apply,resolve}.rs`（调用点小改）、`crates/ll-world/src/overview.rs`（`minimap` 小改）、`crates/ll-world/tests/{determinism,fov_blackbox}.rs`（黄金基准重生成/构造方式小改）、`crates/ll-sim/tests/replay.rs`（黄金基准重生成）、三个既有验收 demo（`ll-world/examples/p2_acceptance`、`ll-sim/examples/p3_acceptance`、`ll-ui/examples/p4_acceptance`）

**依赖：** 任务 9（`SurfaceStore` 必须已经独立测试通过）

**本计划改动面最大的任务，必须一次性完成，不能拆成能各自独立提交的子任务**（详见文档顶部「红灯窗口提醒」）。

**具体改动清单**：

1. `WorldState.terrain` 字段类型从 `ChunkGrid` 换成 `SurfaceStore`；`WorldState.size` 的角色需要重新定位——是继续存整个世界的瓦片级 `TorusSize`（由 `ZoneLayout` 派生并交叉校验，参照 ADR 0011「案例三」既有模式），还是直接存 `ZoneLayout` 本身、`size` 变成派生方法，两种做法都能满足「默认派生，只存偏差」的项目原则，具体选哪个留给实现者判断。
2. `WorldState::new` 签名必须新增 `ZoneLayout`（或等价参数），不再一次性调用 `generate_terrain` 生成整张图——初始时常驻集合可以为空，或者按设计文档五节「常驻集合的构成」预热玩家出生点周围的邻域，具体策略本任务自行决定并写清楚理由。
3. `WorldState::hash()` 不能再遍历「整个世界的每一格」（多数区块不常驻，根本没有具体瓦片数据可读）。**改为遍历 `SurfaceStore::resident_zones()` 返回的已排序区块坐标集合**，逐区块逐格混入哈希。**这意味着黄金基准数值必然改变，但测试的断言结构（同一操作序列产生同一哈希、不同种子产生不同哈希）基本保留**——不是推倒重来，是同一份测试逻辑换一套输入构造方式和一批新基准数。
4. `ChunkGridData`/`Serialize`/`Deserialize for ChunkGrid` 那段手写序列化代码（`state.rs` 现有 `ChunkGridData` 结构）需要对应换成 `SurfaceStore` 的序列化：**本阶段只要求完整可序列化、往返一致**，不要求做「未修改区块可以靠种子重新生成、不必写入存档」这类优化——那是存档格式冻结时（P5）的空间，本任务不能把这个可能性堵死（`SurfaceStore` 的字段设计上应该能区分「这个区块是否被玩法修改过」，供 P5 判断是否值得做差量存储，但差量存储本身不在本任务范围）。
5. `ll-sim::apply::apply` 里 `Effect::SetTerrain` 分支（`world.terrain.set_terrain(pos, kind)`）与 `ll-sim::resolve` 里全部 `world.terrain.terrain_at(...)` 调用点——**方法名不变**（关键设计判断 1 的直接收益），但 `terrain_at` 现在需要 `&mut self`（流式加载可能触发按需生成，见任务 9 的 `SurfaceStore::terrain_at` 签名），若 `resolve` 因此从 `&WorldState` 变成需要 `&mut` 访问地形，这与「`resolve` 是纯函数」（C1）会产生直接冲突——**这是本任务必须正面解决的架构问题，不能绕过**：建议的方向是 `resolve` 通过一个只读的「区块必然已经常驻」的保证来获得一个不触发按需生成的只读 `terrain_at` 变体（因为 `resolve` 只应该查询当前 `Space` 及其可见范围内的地形，这些按定义都已经常驻），真正的按需加载触发点应该收窄到 `apply`（写入口）或专门的「推进流式加载」步骤，不应该藏在 `resolve` 的只读查询路径里。**这个设计决定的具体落点，留给实现者在动手前先写一版方案说明，不预先拍板**，但必须写清楚，不能悄悄让 `resolve` 变成非纯函数。
6. `overview::minimap` 的 `terrain_at` 调用同理需要考虑可变性问题（当前签名 `world: &WorldState`）。
7. 三个既有验收 demo 的世界构造代码（`WorldState::new(...)`）需要同步更新参数；这三个 demo 的**玩法断言不需要改**（走哪条路、能不能开门这些逻辑不变），只是构造调用的参数变了——这是纯粹的连带修复，不单独立项。

**测试迁移策略（本任务的核心交付物之一）：**

| 现有测试 | 处理方式 |
|---|---|
| `chunk.rs`/`terrain.rs`/`noise.rs`/`light.rs` 内嵌测试 | **完全不改**——这些类型本身不变 |
| `fov.rs`/`camera.rs`（含泛化后的有界分支） | **任务 6/7 已处理，本任务不再涉及** |
| `apply.rs`/`resolve.rs` 内嵌测试（约 40+ 个） | **构造方式小改**（`WorldState::new` 参数变化），断言逻辑保留 |
| `tests/determinism.rs`、`tests/replay.rs` | **黄金基准数值必须重新生成一次**，断言结构保留（见上文第 3 点）。**重新生成基准值时必须先人工核对新哈希确实反映了正确的新逻辑，不能例行公事跑一遍取新值就填进去**——这正是这两个文件顶部反复强调的「绝不允许测试挂了就把期望值改成实际值」，本任务是这条纪律第一次被真正考验 |
| `overview.rs` 的 `minimap` 测试 | **小改**（可变性签名变化），断言逻辑保留；`continent_map` 测试留给任务 13 |
| P2/P3/P4 三个验收 demo | **构造调用连带修复**，随本任务一次性更新，不阻塞到任务 15 |

- [ ] **提交前必须通过的检查**：`cargo check --workspace` 无错误、`cargo test --workspace` 全绿、`cargo clippy --workspace` 无新增警告——这三条在本任务提交前必须逐一确认，任何一条不过都不应该提交。
- [ ] **提交**（`refactor:`，正文说明黄金基准为什么必须变、变之前做了什么核对）

---

### 任务 12：`Space` 接入 `WorldState`/`Agent`——进出 `Interior` 的 Intent/Effect

**Files:** `crates/ll-world/src/entity/agent.rs`（新增字段）、`crates/ll-sim/src/{intent,effect,resolve,apply}.rs`
**依赖：** 任务 10、任务 11

`Agent` 需要一个「当前所在 `Space`」字段（默认 `Surface`）。进出 `Interior` 走完整的 `Intent → resolve → Effect → apply` 链路，不能在渲染/输入层直接改这个字段（C1）。

**Interfaces Produces（概念形状）：**
```rust
// Agent 新增字段
pub current_space: Space,

// intent.rs 新增
pub enum Intent { /* 既有变体 */ EnterSpace { actor: EntityId, target: SpaceId }, ExitSpace { actor: EntityId } }

// effect.rs 新增
pub enum Effect { /* 既有变体 */ ChangeSpace { actor: EntityId, space: Space } }

// resolve.rs 新增判断：目标格是否有可进入的 Interior 入口
// （借任务 10 的 InteriorTable::entries_at 查询），够不着入口时不产出 EnterSpace 对应的 Effect。
```

- [ ] **TDD 循环**：
  - `站在有 Interior 入口的格子上触发进入意图，产出 ChangeSpace Effect`
  - `站在没有入口的格子上触发进入意图，不产生任何空间切换`
  - `进入 Interior 后 Agent.pos 不变，只有 current_space 变化`（对应设计文档「内部移动不改变世界地图坐标」）
  - `退出 Interior 后 Agent.pos 恢复为 Interior 的 anchor`
  - `WorldState::hash() 纳入 current_space 的变化`（否则空间切换这类会被结算改动的字段游离在确定性回归测试之外，重演 `hash()` 文档里「早期版本只混入地形」的同一类缺口）
- [ ] **提交**

---

### 任务 13：`continent_map` 新数据源 + `minimap` 改接线

**Files:** `crates/ll-world/src/overview.rs`
**依赖：** 任务 11

`continent_map` **不能**为了画一张概览图就把全部区块的完整地形都生成出来（那正是流式生成要避免的事）。需要一份独立的、世界创建时一次性生成的粗粒度场（类似「种族分布场」，按区块而非瓦片分辨率），专供 `continent_map` 使用。`minimap` 只需要把 `terrain_at` 的调用改成经过 `SurfaceStore`（任务 11 已处理签名变化），窗口逻辑本身不变。

**Interfaces Produces（概念形状）：**
```rust
/// 世界创建时一次性生成的粗粒度地形场，按区块分辨率，
/// 与精细的逐瓦片地形是两份不同粒度的数据。
pub struct ContinentField { /* Vec<TerrainKind>，长度 = zone_count.width * zone_count.height */ }
pub fn generate_continent_field(layout: &ZoneLayout, noise: &TileableNoise, params: &GenParams,
                                  terrain_ids: &BaseTerrainIds) -> ContinentField;
pub fn continent_map(field: &ContinentField, layout: &ZoneLayout, downsample: u32) -> Vec<OverviewCell>;
```

- [ ] **TDD 循环**：
  - **既有 `minimap` 测试原样通过**（只是构造签名变化）
  - `continent_map 不触发任何区块的按需生成`（断言调用前后 SurfaceStore 的常驻集合不变——这是本任务最重要的正确性验证，直接对应「不能为了画概览图就实体化全部区块」这条约束）
  - `continent_map 只展示 Surface，不展示任何 Interior`（沿用「不做往下看」的裁定）
- [ ] **提交**

---

### 任务 14：主循环/相机的流式滚动与按层渲染接线

**Files:** 待裁定——当前仓库还没有 `ll-app`（规格 §5：主循环组装层，仍未建立），本任务的落点取决于当前谁在驱动渲染循环，建议先核实 `ll-render`/`ll-sim` 现有 demo 的主循环代码放在哪（三个既有 `examples/p{2,3,4}_acceptance/main.rs`），本任务大概率要落在类似的 demo 级主循环代码里，而不是一个尚不存在的 `ll-app`

**依赖：** 任务 6、任务 7（FOV/Camera 泛化）、任务 12（`Space` 切换落地）

把「玩家跨区块边界时相机平滑滚动、看不到过渡」「只渲染当前 `Space`」这两条设计裁定接到实际渲染循环：

- 相机跟随玩家 `Agent.pos`，`world_to_screen`/`visible_tiles` 不变（设计文档六节已核实：地表场景 `Camera` 不用改）；跨区块边界时，`SurfaceStore` 的按需生成在后台/同步触发，只要生成速度跟得上玩家移动速度，视觉上就不会有接缝或空白——这条性质依赖任务 9 的常驻邻域缓冲（默认 5×5）设计,本任务不需要重新论证,只需要接线。
- 渲染主循环需要按 `Agent.current_space` 判断喂给 FOV/相机的是环面版本还是有界版本——这是一处真实的分支：`Space::Surface` 走 `SurfaceStore`/`Camera`/`compute_fov<ChunkGrid>`,`Space::Interior` 走 `InteriorTable`/`BoundedCamera`/`compute_fov<BoundedGrid>`。

- [ ] **必须实测**（不是单元测试能覆盖的，见任务 15 验收 demo）：连续按方向键跨越至少一个区块边界，肉眼确认无接缝、无卡顿式加载等待。
- [ ] **提交**

---

### 任务 15：验收 Demo

**Files:** 建议 `crates/ll-world/examples/p5_coordinate_acceptance/`（若任务 14 的落点核实后发现更合适的 crate，可调整路径）

必须展示（对应用户提出的四条最低要求）：

1. **跨区块边界时地表无缝**——连续按方向键走过至少一个区块边界，截图/录屏证据，肉眼确认无接缝、无地形突变、无加载卡顿。
2. **世界地图标记跟着更新**——`continent_map`/`minimap` 显示的玩家位置随移动更新，且 `continent_map` 的调用不触发额外区块生成（复用任务 13 的断言）。
3. **能进出一个 Interior 且只渲染当前层**——demo 世界放置至少一个 `Interior` 入口，玩家走进去、渲染切换为该 `Interior` 楼层的画面（地表不可见），走出来、渲染恢复地表画面且玩家仍在原来的锚点位置。
4. **层属性生效**——`Interior` 的 `exposed_to_sky = false`，demo 里对比进入前后的有效环境光照（`effective_ambient_light`，任务 4）数值/画面亮度，证明地下环境光趋近 `ambient_light_floor` 而不是随昼夜变化。

**必须实测，如实报告哪些验证了、哪些没有**——沿用 P3/P4 交接清单反复强调的纪律：单元测试各自绿不代表连线通。**特别要检查**：这次是继「`resolve` 从不读敏捷」「玩家死亡后主循环空转」「熔岩地板被水隔断」「加载界面文字换行错位」之后第五次验收 demo，前四次全部抓出了单元测试测不出的连线缺陷——本次两个最可能重演同一模式的地方是（a）任务 11「`resolve` 需要地形只读访问但地形又是流式加载」那处架构决定是否真的没有让 `resolve` 变成非纯函数，（b）任务 14 的常驻邻域缓冲在真实按键速度下是否真的跟得上,而不是单元测试里人为控制的访问序列。

- [ ] **提交**

---

## 自查

### 完整调用链（P1/P3 教训要求的一节）

```
玩家按方向键
  → Intent::Move { actor, direction }                              ← ll-sim::intent（既有）
  → resolve(world, intent) 读取当前 Space 下的地形                    ← ll-sim::resolve（任务 11 改造：地形查询改走
                                                                        SurfaceStore/BoundedGrid，但保持只读纯函数）
  → Effect::MoveTo { actor, pos }                                   ← ll-sim::effect（既有）
  → apply(world, effect) 写 Agent.pos                                ← ll-sim::apply（唯一写入口，不变）
  → 新位置所属区块坐标变化（tile_to_zone）                             ← ll-world::zone::ZoneLayout（任务 8）
  → 该区块尚未常驻 → SurfaceStore 按需生成（复用区块窗口生成）           ← ll-world::surface_store（任务 9）+
                                                                        generate_zone_window（任务 8）
  → 相机跟随 Agent.pos，world_to_screen 用 TorusSize::delta 求最短位移  ← ll-render::camera（不变，设计文档六节已核实）
  → 玩家看不到过渡（区块边界在地表不可见，常驻邻域缓冲已提前生成）        ← 任务 9 的 5×5 邻域 + 任务 14 的接线
  → 世界地图上的标记跟着更新（минимap/continent_map 现算现出）          ← ll-world::overview（任务 13）
  → 走到楼梯/门这类携带 Interior 入口的格子                            ← InteriorTable::entries_at（任务 10）
  → Intent::EnterSpace → resolve 查询该格是否有入口 → Effect::ChangeSpace ← ll-sim::intent/resolve/effect（任务 12）
  → apply 写 Agent.current_space                                     ← ll-sim::apply（任务 12）
  → 渲染主循环按 current_space 切到 BoundedGrid/BoundedCamera 分支      ← 任务 14
  → FOV 按新 Space 重算（compute_fov<BoundedGrid>，不跨层）             ← ll-world::fov（任务 6）
  → 环境光照查 SpaceProfile.exposed_to_sky，Interior 恒为地板值         ← effective_ambient_light（任务 4）
```

**每一环都指出了负责的任务与接口。** 与 P4 计划的自查一样，唯一的软连接是任务 11「`resolve` 如何在流式加载的地形上保持纯函数」这一步的具体实现方式未预先拍板——但这不是断链，是明确标注了需要实现者在动手前先写方案说明的一个具体决策点，不是「设计了全部模块却没有一条路径能通到窗口」那种事后才发现的断裂。

### 规格覆盖

| 规格/设计文档要求 | 对应任务 |
|---|---|
| §7.1 本节取代说明：区块流式加载替换整图常驻 | 任务 8、9、11 |
| 设计文档四节：`Space` 统一接口 | 任务 2 |
| 设计文档四节：锚定关系单一真相源 | 任务 10 |
| 设计文档五节：区块大小、常驻集合、LRU | 任务 8、9 |
| 设计文档六节：`Interior` 离散层、不做真三维 | 任务 5、10 |
| 设计文档六节：`compute_fov`/`Camera` 需要有界姊妹类型 | 任务 6、7 |
| 设计文档七节：`SpaceProfile` 与光照连锁 | 任务 3、4 |
| 设计文档八节：LOD 两个维度 | 未在本计划实现（见下方「有意留给后续阶段的缺口」） |
| 设计文档九节：只渲染当前层，不做「往下看」 | 任务 12、14、15 |
| 设计文档十节：区块 = 4×4 存储块（甲案） | 关键设计判断 1（直接复用 `ChunkGrid`） |
| 设计文档十三/十四节：既有工作影响范围 | 全部任务对照执行 |
| §15 P5 行：坐标系必须先于存档格式定形 | 本计划整体存在的理由 |

### 有意留给后续阶段的缺口

- **`Interior` 真正的生成器**（洞穴算法、房间走廊算法、建筑定义）——本计划只交付存储与接口形状，`InteriorTable::insert` 需要调用方已经生成好一个 `Interior` 实例，本计划的验收 demo 用手工构造的固定布局，不实现任何生成算法。真正的生成器是 P7 世界生成器的工作（设计文档六节「生成器与内容注册表的接口」）。
- **世界历史生成的聚落/势力播种**——属 P7，本计划的区块模型是它的前提，不是它本身。
- **探索记忆/战争迷雾**——`OverviewCell::explored` 恒为真的债务不在本计划范围，留给 P5 存档格式批次（`overview.rs` 模块文档已有记录）。
- **气候周期性条带、地形光照透过率**——两项历史遗留、跨越三轮交接清单仍未认领的规格条目，本计划同样不认领，留给 P7/P9。
- **点光源（火把等）的局部光照**——设计文档七节明确「本文档不展开」，本计划同样不做。
- **LOD 维度二（同一个 `Surface` 内部按区块做前景/背景推进）**——设计文档八节的这部分依赖智能体经济系统（P9）才有意义，本计划只交付它需要复用的常驻区块-层集合本身（任务 9），不实现推进逻辑。
- **丙案（取消存储块层）**——本计划严格按甲案执行（关键设计判断 1），不在本计划内评估或实施丙案，即便设计文档十节指出这次改动是合并成本最低的窗口，见下方「待裁定」。
- **`SurfaceStore` 的差量存储优化**（未修改区块靠种子重新生成、不写入存档）——任务 11 的序列化设计要求「不堵死这条路」，但不在本计划实现，留给 P5 冻结存档格式时评估。

---

## 与 `#[serde(skip)]` 债务的关系

`WorldState` 现有三处 `#[serde(skip)]` 债务——`population`（`ThinPopulation`）、`actors`（`Arena<Agent>`）、`terrain_table`（`TerrainTable`，P4 新增）——**均不属于本计划范围，明确留给 P5 冻结存档格式那一批**，理由：

1. 三处债务的共同根因是「依赖 mod 加载顺序/注册表上下文的类型无法脱离该上下文反序列化」，这是**内容注册表与存档格式如何交叉校验**的问题，与本计划要解决的**地形本身怎么存储/流式加载**是两个不同维度的问题——修好流式加载不会让 `ContentIndex` 变得可持久化，也不需要等它变得可持久化。
2. 交接清单（`p4-to-p5.md`）已经明确把这三处与 `ContentIndex` 映射层、mod 集合双记录、脚本状态存储列为「P5 存档格式的一部分，应同批落地」——本计划提前动手会打乱那批统一设计的时机,且本计划自己的任务 11 已经因为「区块流式存储怎么序列化」引入了一个新的、性质相似但范围不同的序列化问题（`SurfaceStore` 本身怎么序列化），硬把旧的三处债务也塞进来会让任务 11 的范围失控。
3. 本计划任务 11 新增的 `SurfaceStore`/`Interior` 序列化**不是旧债务的第四个实例**——旧三处的模式是「有值但无法序列化，只能 skip」，本计划的新字段从一开始就要做到完整可序列化往返（哪怕效率不是最优），不引入新的 `#[serde(skip)]`。

**顺手能做的部分**：任务 11 换型 `WorldState.terrain` 时，会重新审视 `state.rs` 现有的 `try_from` 交叉校验模式（`size`/`terrain` 尺寸一致性），这份经验直接可供 P5 处理三处旧债务时参考，但不代为解决——评审时不应期待任务 11 顺带修好 `population`/`actors`/`terrain_table`。

---

## 待裁定

以下事项是阅读设计文档与代码时发现的、本计划不代为裁定的分叉：

### 1. 丙案（取消存储块层）是否要在本次窗口一并做掉

设计文档十节原文：「这个窗口正在关闭——`WorldState.terrain` 本来就要为两级坐标系重写，此刻合并丙案的增量成本最低；等更多代码依赖 `ChunkGrid` 之后再改会更贵。」本计划的关键设计判断 1 选择了**不做**丙案（继续用 `ChunkGrid` 作为区块内部存储），理由是甲案改动面更小、风险更可控，且设计文档自己也把丙案标注为「有条件推迟」而非「立即执行」。但设计文档这句话本身就是在提醒「现在不做，以后更贵」——这是一个真实的取舍，本计划给出的是保守选项，若项目所有者认为现在正是窗口、值得多担一点风险换后续更省，应该在评审时提出，本计划可以调整任务 9 的范围把丙案并进去。

### 2. 世界区块数默认值（48×32）未经项目所有者最终确认

设计文档十一节原文：「项目所有者尚未给定最终数字。」本计划任务 8 的 `ZoneLayout::default_config()` 会先用设计文档给出的默认值（区块 128×128、世界区块数 48×32）起草，**这是数值不是结构**，可以在实现后任意调整而不影响任务划分本身，但落地前最好有一次明确确认，避免默认值被误当成已拍板的最终数字。

### 3. 常驻预算是否真的要「Surface 与 Interior 共享同一个 256 上限」

关键设计判断 3 是本计划基于设计文档五节一句话（「25（邻域）＋ 少量 Interior 层 ＋ LRU 补足」）做出的推断，设计文档本身没有给出这个共享预算的具体接口形状，也没有明确回答「如果玩家进入一个巨大的地下城（远超几层）,是否应该允许它独占超过 256 的预算，还是必须和 Surface 邻域抢同一份预算、导致玩家离开地下城很远的地方地表区块被过早淘汰」这类边界情形。本计划任务 9/10 按「共享同一份预算」的方向设计,但这条边界情形的取舍**更接近需要项目所有者拍板的产品判断**（「地下城内部体验优先」还是「地表流式加载优先」),列在这里供裁定,不影响任务能否开工——任务 9 可以先按共享预算实现,后续按裁定结果调整分配策略。

---

## 收尾必做：反向核对规格

按项目纪律，本计划执行完毕收尾时必须反向核对一次规格与设计文档——不是查实现是否满足设计文档，而是查设计文档是否已被实现淘汰（例如：任务 9 的常驻集合具体实现方式是否让设计文档五节某些论证性文字变得不再准确、任务 11 换型后 `state.rs` 的交叉校验模式是否需要在架构骨架文档 `docs/architecture/03-invariants.md`/`07-determinism.md` 里补一节「C4/C5 第一次有真实代码需要遵守」的说明）。这条纪律本计划自己在正文里已经提前用了一次（本文档「架构」一节明确写下「C4/C5 在本计划里第一次真正有实际代码可以违反」），收尾时应当核实这句话在实现完成后依然成立，而不是假设它会自动成立。
