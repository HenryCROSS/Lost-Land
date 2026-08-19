# P5-A：存档格式冻结与身份收尾 实施计划

> **给执行者：** 必须配合 `superpowers:subagent-driven-development` 逐任务实施。步骤使用 `- [ ]` 复选框追踪。
> **与 P5-B 的关系**：本计划是 P5 两份计划中的第一份（见下方「为什么拆成两份」）。P5-B（`2026-08-19-p5-gameplay-systems.md`，职业/技能树/副职/网状任务）依赖本计划任务 3（`Agent` 补齐 serde）与任务 8（脚本状态存储）落地后才能开工其中涉及技能冷却持久化的部分——具体依赖关系见 P5-B 文档顶部。
> **红灯窗口提醒**：本计划多数任务是新增字段/新增类型，理论上可以保持全绿；但任务 3（`Agent`/`ThinPopulation` 补齐 serde）与任务 9（存档主体格式真正接入 `WorldState` 读写）各自涉及“同一批调用点必须同时更新完毕才能编译通过”的换型，与坐标系重写计划任务 11 是同一种性质。红灯只能出现在这两个任务内部的本地开发过程中，绝不能把半改状态提交入库；任务前后各自保持全绿。测试迁移策略见文档末尾专节。

**目标：** 把规格 §11.2 定义的存档格式从「裸 `serde` 往返」升级为真正可用的玩家存档：`ContentIndex` 映射层真正接入存档头、三处 `#[serde(skip)]` 债务偿清、生成期/当前 mod 集合真正双记录、缺失 mod 按内容类型分级降级、schema 版本与 mod 版本分离报错、脚本状态存储落地、双模式存档与不可逆降级、世界身份（种子+尺寸+生成期 mod 集合）在开局建档前确定。这是 P5 的核心交付物，玩法系统（P5-B）建在它之上。

**架构：** 沿用 C1–C5。本计划第一次让 **C1**（`apply` 唯一写入口）在“脚本状态写入”这件事上被真正考验——`state-set!`/`entity-state-set!` 是否绕过 `apply` 直接改 `WorldState` 是本计划任务 8 的核心正确性要求（脚本状态存储设计文档 8.2 节已经论证这条边界，但那是设计，本计划才是把它变成会被测试钉住的代码）。**C3**（确定性随机）与本计划关系不大（不新增随机源）。**C5**（禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑）在存档头写出（mod 列表、ID 映射表）、脚本状态存储（`BTreeMap`）两处都是硬性要求，见各自任务。

**技术栈：** 新建 `ll-content` crate（规格 §5 已经预留，此前未建立）；`ll-mod`（`Registry`/`mod_set` 已有接口真正接线）；`ll-world`（`WorldState`/`Agent`/`ThinPopulation` 补齐 serde）；`ll-script`（新增 `api/state.rs`）。新增第三方依赖：`postcard`、`lz4_flex`（规格 §11.2 已指定，需在 `cargo-deny` 许可证扫描下核实为 MIT/Apache-2.0 兼容，任务 1 的一部分）。

**设计依据：**
- [`knowledge/design/identity-and-ids.md`](../../../knowledge/design/identity-and-ids.md) 六「存档与 mod 集合」
- [`knowledge/design/script-state-storage.md`](../../../knowledge/design/script-state-storage.md)（全文，本计划任务 8 的直接依据）
- [`knowledge/design/coordinate-system-and-layers.md`](../../../knowledge/design/coordinate-system-and-layers.md)（已落地，本计划继承其成果，不重复设计）
- 规格 §4（C1–C5）、§5（crate 分层，`ll-content` 归属）、§11.2（存档格式）、§14.2–14.3（属性测试/模糊测试对存档的具体要求）、§15 P5 行
- **上阶段交接**：[`knowledge/handoff/p4-to-p5.md`](../../../knowledge/handoff/p4-to-p5.md)，尤其二、三、五节
- **ADR**：0009（默认派生只存偏差）、0010（单一真相源）、0011（`try_from` 交叉校验模式）、0013（裁定须及时提升为 ADR）、0015（注册校验是解析不是不变式）、0016/0017（分档与列式物化）、0019（被禁能力需替代品）、0020（脚本内部浮点，边界类型把关）
- **真实代码基线**：`crates/ll-world/src/state.rs`、`crates/ll-world/src/entity/{agent,thin,arena}.rs`、`crates/ll-core/src/{ident,torus}.rs`、`crates/ll-mod/src/{registry,mod_set,pipeline}.rs`、`crates/ll-script/src/{host,api/query}.rs`、`crates/ll-world/src/noise.rs`，HEAD `0b3244e`，597 个 `#[test]`

---

## 为什么拆成两份计划文档

用户要求“若判断应当拆成两份，说明理由并照做”。判断：**应当拆**。理由：

1. **任务类型完全不同**：本计划（存档）几乎全是“把已有的类型层面占位（`Registry::snapshot`/`GenerationModSet`/脚本状态存储设计）真正接线进读写路径”，属于基础设施收尾；P5-B（玩法）几乎全是“从规格一句话（‘职业、技能树、副职、网状任务’）出发设计全新的内容类型与注册表”，属于内容系统新建。两者所需的判断力与验收标准形状不同（前者验收是“往返一致/降级正确”，后者验收是“职业可用、技能树可推进、任务可完成”）。
2. **依赖方向单向**：P5-B 的任务 3（技能冷却持久化）与任务 5（任务进度持久化）需要本计划任务 3（`Agent` 补齐 serde）与任务 8（脚本状态存储）已经落地——反过来不成立。单向依赖适合拆成两份、后一份在前一份收尾评审通过后再启动，而不是拆成可以任意交错的并行任务组。
3. **规模本身已经达到独立成计人们的量级**：本计划单独已有 13 个任务，覆盖面（新建 crate、三处历史债务、mod 集合双记录、脚本状态存储、双模式存档、fuzz、E2E 脚手架）不亚于坐标系重写计划；硬塞进同一份文档会让任务列表超过 25 项，审阅与追踪成本过高。
4. **红灯窗口互不相关**：本计划的红灯窗口在 `Agent` 换型与存档读写接线；P5-B 的红灯窗口在职业/技能字段接入 `Agent`（如果需要新增字段）。混在一起会让“测试迁移策略”一节的表格失去意义（读者分不清哪次变红对应哪个任务）。

**两份计划共享的部分**：本文档顶部的“全局约束”“架构”“真实代码基线”对 P5-B 同样成立，P5-B 文档不重复展开，只在需要处交叉引用。

---

## 全局约束

- **世界状态禁止浮点**（`WorldState` 本身；脚本内部可用浮点，见 ADR 0020，但跨过 `register_fn` 边界落进 `WorldState`/脚本状态存储的值一律整数——本计划任务 8 的 `ScriptValue` 类型设计直接体现这条边界）。
- **`apply` 是唯一写入口**——脚本状态写入（`state-set!`/`entity-state-set!`）必须走这条路径写穿到 `WorldState`，不能在 `ll-script` 内部另开一条写路径。
- **注册校验是解析，不是不变式**（ADR 0015）——`ContentIndex` 类字段的 serde 恢复只做结构转换，不在 `try_from` 里校验“这个索引当前是否已注册”；那是拿到注册表之后调用方的职责（`terrain_table` 重新灌入校验已经是这个模式的先例，本计划任务 5 延续同一模式）。
- **禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑判断**（C5）——存档头 mod 列表、ID 映射表、脚本状态存储容器，一律用 `Vec`（已排序）或 `BTreeMap`。
- 「私有字段 + 校验构造函数」的类型加 serde 须用 `try_from` 中转（ADR 0011），**但** `ContentIndex`/`TorusPos` 这类“无上下文不变式”的类型直接派生（ADR 0015 已经把这条界线钉死，`ll-core` 现有代码是先例，不要在存档头类型上重新发明一套 `try_from` 包装）。
- 依赖方向不得反向：新建的 `ll-content` 位于 `ll-mod` 之后、`ll-ui` 之前（规格 §5 依赖顺序），不得被 `ll-world`/`ll-sim` 反向依赖。
- 文件 200–400 行为宜，800 行上限；提交信息 `<type>: <描述>`，正文讲**为什么**，中文，不得含 AI 署名。
- **worktree 隔离**：任务 1（新建 `ll-content`）与任何后续可能并行的任务，必须遵守 `p4-to-p5.md` 六、1 节记录的教训——新建 crate 的任务必须在独立 worktree 完成，不能与其他任务共享工作树同时进行 `Cargo.toml` 编辑。

---

## 关键设计判断（本计划在设计文档留白处做出的实现判断，非设计文档裁定）

1. **`ll-content` 是本计划的新增 crate，承接规格 §5 给它的职责**（数据表加载、存档序列化与迁移）——存档头/主体的读写逻辑、schema 迁移函数链、双模式降级标记都落在这里，不塞进 `ll-world`（`ll-world` 只负责“这个类型本身能不能序列化”，不负责“存档文件长什么样、怎么落盘”）。`ll-content` 依赖 `ll-mod`（读 `Registry`/`ModSet`）、`ll-world`（读 `WorldState`），不被两者反向依赖。
2. **`ContentIndex` 映射表接入存档头，走 `Registry::snapshot()`/`rebuild_from()` 现成接口**——本计划不重新设计这层，只做“真正调用它们、把结果放进存档头 JSON 的哪个字段”这件接线工作。
3. **`GenerationModSet` 的绑定时机是“世界创建”（`WorldState::new` 被调用的那一刻），不是等待 P7 完整历史生成器。** `mod_set.rs` 现有注释写“留给 P6 世界生成器”——这处引用在 [2026-08-18 规格修订] 插入新 P6「物品与装备」之后已经过期，真正的世界生成器现在排在 P7；若照单全收，P5 就无法真正绑定生成期 mod 集合，只能继续留占位。本计划的判断是：**“世界生成”这件事的语义不是“完整的历史生成器跑完”，而是“`WorldState::new` 产出了一个具体世界”**——P5 当前的世界创建方式（区块流式 + 出生点邻域预热，尚无历史生成）已经满足“种子 + mod 集合 → 确定的世界”这条性质，`GenerationModSet` 应该在这一刻封存，不需要等 P7。P7 落地真正的历史生成器时，若发现绑定时机需要调整（例如聚落播种本身也读取 mod 定义、也应该算进“生成期”），那是 P7 计划的职责，不影响本计划现在把绑定时机定在 `WorldState::new`。**这处发现（`mod_set.rs` 注释因规格阶段顺移而过期）本身也是待裁定/新债务的一部分，见文档末尾。**
4. **脚本状态存储的“每实体扩展数据”真相源放在 `Agent` 新增字段，不是独立的 `WorldState` 旁挂表**——直接沿用 `agent.rs` 模块文档“为什么 `health` 是 `Agent` 的字段而不是旁挂表”一节已经论证过的理由（旁挂表不受 `Arena` 世代号管辖，会积累孤儿记录）；脚本状态存储设计文档 2.1 节“每实体扩展数据……随实体生死自动回收”这句话，只有存成 `Agent` 字段才能真正成立。
5. **`Agent` 补齐 serde 时，`profession`/`race`（`ContentIndex`）与 `goals`（含 `ContentIndex`）一律直接派生，不额外包一层 `try_from`**——依据关键判断 2 与 ADR 0015：这批字段的“是否已注册”校验发生在存档整体读入之后（`WorldState` 反序列化完成、拿到当前会话 `Registry` 之后）的一次显式扫描，不是每个字段各自在 `Deserialize` 内部校验。这与 `terrain_table` 现有“读档后默认为空、显式重新灌入”模式是同一条思路的推广，只是这次不是“空表”而是“索引可能悬空，需要显式扫描标记”。
6. **只读模式（缺失玩家角色种族/职业时）实现为存档读入的一个独立枚举结果，不是抛异常**——`LoadOutcome::Playable(WorldState) | ReadOnly(ReadOnlySave) | Rejected(LoadError)`，具体见任务 6。这不是设计文档裁定的具体类型形状（`identity-and-ids.md` 只给了“建议”），是本计划的实现判断，评审时可推翻。

---

### 任务 1：新建 `ll-content` crate + 存档头类型骨架 + schema 迁移框架

**Files:** `crates/ll-content/{Cargo.toml,src/lib.rs,src/header.rs,src/migration.rs}`（新）、workspace `Cargo.toml` 成员声明
**依赖：** 无（本计划的地基任务）
**worktree 隔离：** 是——新建 crate，必须独立 worktree（全局约束已述）。

落地规格 §11.2「头部：明文 JSON。schema 版本、存档时间、角色名、当前区域、游玩时长、启用 mod 列表、ID 映射表」。本任务只搭骨架与迁移框架的机制本身，不接入真实 `WorldState`（那是任务 9）。

**Interfaces Produces（概念形状）：**
```rust
// header.rs
#[derive(Serialize, Deserialize)]
pub struct SaveHeader {
    pub schema_version: u32,
    pub saved_at: /* Unix 时间戳或等价，不用 chrono 之外的重依赖，具体类型留给实现者核实现有依赖 */ i64,
    pub character_name: String,
    pub current_region: String, // 人类可读，供存档列表界面展示，不是 ContentIndex
    pub playtime_ticks: i64,
    /// 生成期 mod 集合快照——见任务 4，本任务只留字段占位。
    pub generation_mods: Vec<ModHeaderEntry>,
    /// 当前 mod 集合快照——同上。
    pub current_mods: Vec<ModHeaderEntry>,
    /// ContentIndex ↔ 字符串映射表，来自 Registry::snapshot()——见任务 2。
    pub content_index_map: Vec<String>, // 按 ContentIndex 顺序排列的 NamespacedId 字符串形式
    /// 世界身份三要素之二（种子在别处或这里，具体归属见任务 4）。
    pub world_size: (u32, u32),
    pub mode: SaveMode, // 见任务 10
}

#[derive(Serialize, Deserialize)]
pub struct ModHeaderEntry {
    pub namespace: String,
    pub version: String,
    pub content_hash: u64,
}

// migration.rs
pub trait Migration {
    fn from_version(&self) -> u32;
    fn to_version(&self) -> u32;
    /// 对存档主体的原始字节做版本升级——具体签名（是否需要先反序列化成
    /// 中间表示）留给实现者判断，取决于 postcard 的版本兼容策略。
    fn migrate(&self, body: Vec<u8>) -> Result<Vec<u8>, MigrationError>;
}

pub struct MigrationChain { /* 按 from_version 排序的 Migration 列表 */ }
impl MigrationChain {
    pub fn apply(&self, from: u32, body: Vec<u8>) -> Result<Vec<u8>, MigrationError>;
}
```

**关于 `postcard`/`lz4_flex` 依赖引入**：本任务需要在 `Cargo.toml` 声明这两个依赖，并核实 `cargo-deny` 许可证扫描通过（两者均为 MIT/Apache-2.0，规格 §11.2 已经指名要用，理论上不会有许可证冲突，但按项目纪律仍须实际跑一次 `cargo deny check` 确认，不能假设“规格写了就一定过”）。

- [ ] **TDD 循环**：
  - `SaveHeader 可以序列化为可读的 JSON 字符串`
  - `MigrationChain 对相邻版本能找到迁移路径`
  - `MigrationChain 对不存在的迁移路径返回明确错误，不 panic`
  - `跳级迁移（v1 → v3，中间有 v2）能正确串联两步迁移函数`
- [ ] **提交**（`cargo deny check` 必须通过，作为本任务提交前检查的一部分）

---

### 任务 2：`Registry::snapshot()`/`rebuild_from()` 真正接入存档头

**Files:** `crates/ll-content/src/content_index_map.rs`（新）
**依赖：** 任务 1

`ll-mod::Registry` 的 `snapshot()`/`rebuild_from()` 目前只是内存里的往返接口（`p4-to-p5.md` 已明确标注）。本任务把它们接进 `SaveHeader::content_index_map` 字段的真实读写路径。

**Interfaces Produces：**
```rust
/// 存档时调用：把当前会话的 Registry 状态编码进存档头字段。
pub fn snapshot_for_header(registry: &Registry) -> Vec<String> {
    registry.snapshot().iter().map(|id| id.to_string()).collect()
}

/// 读档时调用：从存档头字段重建一个 Registry，且校验重建后的顺序与
/// 快照顺序一一对应（这条不变式 Registry::rebuild_from 内部已经保证，
/// 这里的校验是防御性的，确认字符串解析没有中途失败导致错位）。
pub fn rebuild_from_header(entries: &[String]) -> Result<Registry, ContentIndexMapError> {
    let ids = entries.iter()
        .map(|s| NamespacedId::parse(s).map_err(|_| ContentIndexMapError::MalformedId(s.clone())))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Registry::rebuild_from(&ids))
}
```

**必须正面回答的问题（P4 报告已标注为未接线的部分）**：读档失败时（`content_index_map` 里的某个字符串解析失败，或 `NamespacedId::parse` 报错）如何对接规格 §10.4「缺失 mod 不得崩溃」——**答案不是在这里处理**：这个模块只负责“字符串 ↔ Registry”的机械转换，`ContentIndexMapError` 应该是一个明确的错误类型，向上传播给任务 6（缺失 mod 降级策略）的调用方决定如何降级，本模块不做任何降级判断。

- [ ] **TDD 循环**：
  - `snapshot_for_header 与 rebuild_from_header 往返后 Registry 的字符串 ↔ 索引映射逐条一致`
  - `content_index_map 含有非法格式字符串时返回 ContentIndexMapError 而非 panic`
  - `空的 content_index_map（全新存档场景不适用，但边界应处理）重建出空 Registry`
- [ ] **提交**

---

### 任务 3：`Agent` 与 `ThinPopulation` 补齐 serde——偿还两处历史债务

**Files:** `crates/ll-world/src/entity/agent.rs`、`crates/ll-world/src/entity/thin.rs`、`crates/ll-world/src/state.rs`（去掉 `actors`/`population` 的 `#[serde(skip)]`）
**依赖：** 无直接依赖，但建议在任务 1/2 之后做（便于任务 5 紧接着设计“读档后索引悬空扫描”时已经有存档头基础设施可以引用）。

**本任务范围比 `p4-to-p5.md` 记录的债务更窄——这是通读代码后发现的真实简化，见文档末尾「新债务」第一条。** `TorusPos` 在坐标系重写批次中已经补上了不依赖上下文的直接 `Serialize`/`Deserialize`（`ll-core/src/torus.rs`，同 `ContentIndex` 一样走 ADR 0015 的“无上下文不变式，直接派生”模式）——`Agent::pos` 字段本身已经没有序列化障碍。真正卡住 `Agent`/`ThinPopulation` 的**只剩 `ContentIndex` 字段**（`profession`/`race`，及 `Goal.kind`），不再需要世界尺寸上下文。`agent.rs`/`thin.rs` 现有的模块文档仍然写着“`pos` 是 `TorusPos`……两者在 `ll-core` 里都没有……可直接使用的序列化实现”——**这句话已经过期，本任务需要同时修正这段文档**，不能只加 derive 不改注释,否则会误导下一个读者以为 `TorusPos` 仍然是障碍。

**具体改动**：
1. `Agent`（及其字段 `BaseStats`/`Affiliation`/`Goal` 等）逐个核实是否已可派生 `Serialize`/`Deserialize`——多数应该是纯整数/字符串组合，直接派生；`ContentIndex` 字段直接派生（关键设计判断 5）。
2. `ThinPopulation` 的列式存储（`profession: Vec<ContentIndex>` 等）同理，逐列核实。
3. `Arena<Agent>` 已经有通用的 `Serialize`/`Deserialize impl<T: Serialize/Deserialize>`（已核实，`arena.rs` 现有代码），`Agent` 满足约束后 `Arena<Agent>` 自动可用，不需要改 `arena.rs`。
4. `WorldState::actors`/`WorldState::population` 去掉 `#[serde(skip)]`，纳入 `WorldStateRepr`/`TryFrom` 交叉校验链（若需要额外校验——例如 `Arena` 内部槽位一致性——核实是否已有，没有则补一条）。

**这是本计划唯一需要“同一批调用点必须同时更新完毕才能编译”的换型任务**——`Agent`/`ThinPopulation` 一旦要求全部字段可派生，任何目前手动构造 `Agent`/`ThinPopulation` 且用到不可派生类型的调用点都要跟着改，范围包括三个既有验收 demo 与 `ll-sim`/`ll-world` 测试夹具。红灯只能出现在本任务内部开发过程中。

- [ ] **TDD 循环**：
  - `Agent 序列化往返后全部字段逐一相等`（含 `current_space`）
  - `ThinPopulation 序列化往返后各列长度与内容一致`
  - `WorldState 序列化往返后 actors/population 不再是空的默认值，而是往返前的真实内容`（这条断言此前不可能写,因为字段被 skip；现在必须补上，直接对应本任务存在的理由）
  - **既有 `WorldState` 序列化相关测试原样通过或按签名变化做最小调整**——`try_from` 交叉校验的既有测试不应该因为本任务而改变断言意图
- [ ] **提交前检查**：`cargo check --workspace`、`cargo test --workspace`、`cargo clippy --workspace` 三项全过
- [ ] **提交**（`refactor:`，正文说明 `TorusPos` 障碍已经在坐标系重写批次解除、本任务只需要处理 `ContentIndex`，并同步更正 `agent.rs`/`thin.rs` 过期的模块文档）

---

### 任务 4：世界身份三要素 + `GenerationModSet` 真正绑定 + 推荐预设

**Files:** `crates/ll-content/src/world_identity.rs`（新）、`crates/ll-mod/src/mod_set.rs`（更正过期注释，见关键设计判断 3）、`crates/ll-world/tests/noise_presets.rs`（新，或并入现有 `noise.rs` 测试模块）
**依赖：** 任务 2（`GenerationModSet` 需要 `Registry` 快照能力）

落地“世界身份 = 种子 + 尺寸 + 生成期 mod 集合”（本次会话新增需求）与 `identity-and-ids.md` 六、③「生成期 mod 集合 ≠ 当前 mod 集合」。

**具体改动**：
1. `GenerationModSet::capture(registry: &Registry, manifests: &[ModManifest]) -> GenerationModSet`——与 `CurrentModSet::derive_from` 同构，但语义上只应该在“新建世界”那一刻被调用一次，调用点是 demo/未来主循环里 `WorldState::new` 之前的建档流程,不是每次读档都重新计算（生成期集合写入后永久不变,读档时应该直接从存档头读回,不是重新 derive）。
2. 更正 `mod_set.rs` 模块文档“留给 P6 世界生成器”这句过期引用（规格顺移后世界生成器现在是 P7），按关键设计判断 3 改为“绑定时机是 `WorldState::new`，不等待 P7 历史生成器”。
3. 世界尺寸选择：本计划**不**设计开局 UI（`ll-ui` 完整控件库在 P7），只交付“给定一个 `ZoneLayout` 候选列表，返回是否安全（不触发噪声退化）”的纯函数校验 + 一份推荐预设表（小/中/大，含至少一个正方形与一个矩形预设），供未来 P7 UI 直接引用。
4. **推荐预设表**：核实 `noise.rs` 现有 `safe_coarse_scale` 修复（提交 `8e4387a`）覆盖范围——已核实该修复是通用算法级修复（候选值命中周期时减半），不是逐尺寸特判，因此**不存在“预设踩雷”这个狭义问题**；本任务的推荐预设表价值在于（a）给出实际会被玩家选中的具体候选值（避免开局界面提供任意尺寸导致极端值如 16×16 或 8192×8192 引发别的问题——过小起伏不够、过大常驻内存/生成耗时不合理）、（b）为每个预设补一条“产出地形多样性不低于 N 种”的回归断言，比现有“不退化成单点常数”测试更强（现有测试断言“不退化”，本任务断言“确实多样”，两者不是同一件事——不退化不等于多样，只是排除了最坏情形）。

**Interfaces Produces（概念形状）：**
```rust
pub struct SizePreset {
    pub label: &'static str, // "小陆地" / "标准" / "广阔" 等，供 UI 展示，非最终文案
    pub zone_span: u32,
    pub zone_count: (u32, u32),
}
pub const RECOMMENDED_PRESETS: &[SizePreset] = &[ /* 至少 3 档，含设计文档十一节 48x32 默认 */ ];

pub fn validate_size_choice(zone_span: u32, zone_count: (u32, u32)) -> Result<ZoneLayout, WorldError>;
```

- [ ] **TDD 循环**：
  - `GenerationModSet 一旦封存后与后续 CurrentModSet 的变化无关`（构造两者后修改一个不影响另一个,类型上已经隔离，这里补运行期断言）
  - `每个推荐预设产出的地形多样性不低于阈值`（对每个预设跑几个固定种子，统计出现的 TerrainKind 种类数）
  - `每个推荐预设满足 ZoneLayout 现有构造约束`（不引入新的对齐规则,复用已有校验）
  - `validate_size_choice 对不满足 CELL_SIZE 整除约束的尺寸返回错误`
- [ ] **提交**

---

### 任务 5：三处 `#[serde(skip)]` 之三——`terrain_table` 读档校验点

**Files:** `crates/ll-world/src/state.rs`、`crates/ll-world/src/terrain.rs`（若需要补一个校验函数）
**依赖：** 任务 2（读档后需要重建的 `Registry` 作为“重新灌入”的依据）

`terrain_table` 与 `population`/`actors` 是不同性质的债务——它**不需要参与序列化**（本身就是当前会话注册期产物，见 `state.rs` 字段文档），需要的是 **Task 8 报告明确建议的显式校验点**：读档后必须有一处代码断言“`terrain_table` 已经被替换为非空表”，否则拒绝进入游戏,而不是带着空表安静运行。

**Interfaces Produces：**
```rust
impl WorldState {
    /// 读档后置校验：terrain_table 是否已经被调用方重新灌入。
    /// 这不是构造时自动完成的——terrain_table 依赖当前会话的 mod 加载
    /// 结果，WorldState::try_from（存档反序列化）本身没有能力单独产出
    /// 一张正确的表,必须由调用方在拿到当前会话 TerrainTable 后显式调用
    /// 本方法确认。
    pub fn assert_terrain_table_loaded(&self) -> Result<(), WorldError> {
        if self.terrain_table.is_empty() {
            return Err(WorldError::TerrainTableNotReloaded);
        }
        Ok(())
    }
}
```

**同时**：本任务是任务 9（存档主体读写流程）里必须调用这个校验点的地方,本任务只负责产出这个方法本身,真正在读档流程里调用它是任务 9 的“完整调用链”里的一环——见文档末尾自查表。`surface_profile` 字段（同类已知限制,见 `state.rs` 字段文档）是否也需要类似校验点,一并核实（大概率需要,理由相同：也是依赖当前会话 `ContentIndex` 的占位字段）。

- [ ] **TDD 循环**：
  - `terrain_table 为空时 assert 返回错误`
  - `terrain_table 非空时 assert 返回成功`
  - `surface_profile 是否需要同类校验——核实后要么补一条同构方法,要么写清楚为什么不需要（例如它只在 ExitSpace 触发时才被读取,不像 terrain_table 那样每次地形查询都依赖,校验时机可以推迟到 Intent::ExitSpace 开放之前而不是读档后立即校验）`
- [ ] **提交**

---

### 任务 6：缺失 mod 降级策略——按内容类型分级 + 只读模式

**Files:** `crates/ll-content/src/degrade.rs`（新）
**依赖：** 任务 2、任务 3（需要 `Agent`/`ThinPopulation` 已可序列化,才能真正在“读档后逐条检查”这一步操作它们）

落地 `identity-and-ids.md` 六、②的表格：物品丢弃提示、NPC 种族降级占位、**玩家角色种族不可降级**、外加只读模式。

**Interfaces Produces（概念形状）：**
```rust
pub enum DegradeAction {
    DropWithWarning,       // 物品类：丢弃并记录警告
    FallbackToPlaceholder(ContentIndex), // NPC 种族/职业类：降级为占位
    Reject,                // 玩家角色种族/职业类：不允许降级
}

/// 按内容类型与"这条记录是否属于玩家角色"决定降级动作。
/// 具体规则表（哪些字段套用哪种策略）是本任务的核心交付物。
pub fn decide_degrade_action(content_kind: ContentKind, owner: OwnerContext) -> DegradeAction;

pub enum LoadOutcome {
    /// 完全正常，可以继续游玩。
    Playable(WorldState),
    /// 撞上 Reject 类降级，但存档本身没有损坏——只读模式：
    /// 可以查看/导出,不能推进 tick。
    ReadOnly(ReadOnlySave),
    /// 存档本身损坏或缺失的内容超出可挽救范围。
    Rejected(LoadError),
}
pub struct ReadOnlySave { /* 持有 WorldState 但不暴露任何会推进世界的方法 */ }
```

**必须正面处理的边界**：“玩家角色”如何界定——`WorldState` 目前没有“哪个 `EntityId` 是玩家”这个显式字段（核实：现有三个验收 demo 都是在应用层自己记住哪个实体是玩家,不在 `WorldState` 里）。本任务需要核实这一点并决定：要么本任务顺带给 `WorldState` 补一个 `player_entity: Option<EntityId>` 字段（若发现这是缺失的必要前提，需要说明为什么这属于本任务的必要范围而不是越界），要么找到现有的等价机制。**这是本任务开工前必须先核实清楚的一个真实开放问题，不能假设"玩家是谁"这件事已经有答案。**

- [ ] **TDD 循环**：
  - `物品类内容缺失时决策为丢弃并警告`
  - `NPC 种族缺失时决策为降级占位`
  - `玩家角色种族缺失时决策为拒绝降级`
  - `拒绝降级触发只读模式而非直接报错拒绝打开`
  - `只读模式下无法调用任何会推进 tick 或写入 Effect 的方法`（编译期或运行期保证,具体选哪种留给实现者,但必须选一种并测试它）
- [ ] **提交**

---

### 任务 7：schema 版本与 mod 版本分离报错

**Files:** `crates/ll-content/src/load_error.rs`（新或并入 `degrade.rs`）
**依赖：** 任务 1（`MigrationChain`）、任务 6（`LoadError` 类型）

落地 `identity-and-ids.md` 六、④：两种失败必须用词分开、指向不同修复动作。

**Interfaces Produces：**
```rust
pub enum LoadError {
    /// schema 版本高于当前游戏能处理的最新版本——需要更新游戏本体。
    SchemaTooNew { save_version: u32, max_supported: u32 },
    /// schema 迁移链找不到路径（不应该发生,除非迁移链本身有缺口）。
    SchemaMigrationGap { from: u32 },
    /// mod 内容不兼容：版本号或内容哈希与生成期记录不一致。
    ModContentMismatch { namespace: String, expected_hash: u64, actual_hash: Option<u64> },
    /// 存档文件本身损坏（截断/篡改），与上面两类都无关。
    Corrupted(String),
}
```

**用户可见文案**（不是本任务的最终交付物,但接口设计必须支持——`LoadError` 的各变体应该能各自映射到不同的提示文案,不能塌缩成一句“存档版本不兼容”）：本任务只交付类型与判定逻辑,文案本地化（Fluent `.ftl`）留给 P7 UI 落地时接线,这里只保证判定逻辑区分得够细,不会在这一步就把两种原因合并。

- [ ] **TDD 循环**：
  - `schema 版本高于当前支持的最新版本时返回 SchemaTooNew`
  - `mod 内容哈希与生成期记录不一致时返回 ModContentMismatch,即便版本号相同`
  - `schema 版本正常但 mod 内容不兼容时,不会被误判为 SchemaTooNew`（这是本任务存在的核心理由——两种失败不能互相掩盖）
- [ ] **提交**

---

### 任务 8：脚本状态存储——值类型、存储位置、命名空间隔离

**Files:** `crates/ll-world/src/state.rs`（`WorldState` 新增全局存储字段）、`crates/ll-world/src/entity/agent.rs`（新增每实体存储字段）、`crates/ll-script/src/api/state.rs`（新）
**依赖：** 任务 3（`Agent`/`Arena<Agent>` 已可序列化，脚本状态存储设计文档 3.3 节明确记录这条依赖前提）

落地 `script-state-storage.md` 二、三、四节。**这是本计划里除任务 3 之外唯一涉及 `Agent` 结构变化的任务，建议紧接任务 3 之后开工，避免两次分别触碰 `Agent` 引入不必要的合并冲突。**

**Interfaces Produces（概念形状，设计文档已给出完整值类型定义，此处不重复，只列本任务新增的存储与 API 落点）：**
```rust
// state.rs 新增字段
pub global_script_state: BTreeMap<(String, String), ScriptValue>, // (mod_namespace, key)

// agent.rs 新增字段
pub script_state: BTreeMap<(String, String), ScriptValue>, // (mod_namespace, key)，随 Agent 生死自动回收

// api/state.rs
pub fn register(engine: &mut ScriptEngine) {
    // state-set! / state-get! / entity-state-set! / entity-state-get! / state-get-foreign
    // 每个函数的 mod_namespace 由宿主注册时的执行上下文固化,不是脚本参数
    // ——与 script_terrain_api.rs 的既有模式同构（模块文档已引用）。
}
```

**必须验证的边界**（设计文档 3.3 节「依赖前提」已经预警的过渡期现象）：脚本存的 `EntityId` 在读档后若指向的实体已经不在（世代号不符），`entity-state-get!` 应返回哨兵值而不是 panic——这条行为依赖 `ScriptEntityHandle` 既有的世代号失效检测（`script-entity-handles-and-batch-queries.md` 3.4 节），本任务复用而非重新实现。

**写入路径必须经过 `apply`（C1 的直接考验）**：`state-set!`/`entity-state-set!` 在脚本调用时如何做到“最终落进 `WorldState`”而不违反“脚本只能产出 `Effect`”这条既有纪律，是本任务必须正面回答的架构问题——**两个可能方向**：(a) 这两个函数是引擎层 API（`ll-script`），允许直接写穿 `WorldState`（脚本状态设计文档 8.2 节的原话是“直接写穿,没有中间层”，暗示这类 API 本身就是被认可的例外，类比 `apply` 本身的地位），(b) 或者应该产出一个新的 `Effect::SetScriptState`/`Effect::SetEntityScriptState` 变体，经 `apply` 写入，与其余状态修改路径完全统一。**本计划不预先拍板选哪个方向**——设计文档 8.2 节字面写的是方向 (a)（“没有中间层”），但方向 (a) 与规格 §4 C1「`apply` 是全局唯一能修改世界的地方」字面冲突（脚本状态是 `WorldState` 的一部分，若脚本能直接写它,C1 就不再是“唯一”）。**这处冲突记入文档末尾「待裁定」，实现者动手前必须先解决这个矛盾，不能假装两者都成立。**

- [ ] **TDD 循环**：
  - `全局存储写入后同一 mod 可以读回`
  - `跨 mod 默认读取失败，state-get-foreign 可以显式跨读`
  - `每实体存储随实体销毁而消失（不产生孤儿）`
  - `实体已死亡时 entity-state-get! 返回哨兵值而非 panic`
  - `state-set! 不能写入调用者命名空间之外的键`（类型层面已经保证,这里补运行时断言确认没有逃逸路径）
  - `写入超过配额时返回失败哨兵值并产生 ScriptDiagnostic::Warning`（配额见任务 8 附属，或独立成任务 8b——见下方拆分说明）
- [ ] **提交前必须解决**：上文「写入路径必须经过 apply」的架构矛盾——写清楚选择哪个方向、为什么，不能留白
- [ ] **提交**

---

### 任务 8b：脚本状态存储——配额、孤儿保留、VM 强制重建

**Files:** `crates/ll-script/src/api/state.rs`（延续任务 8）、`crates/ll-script/src/host.rs`（VM 重建触发点）
**依赖：** 任务 8

**拆分理由**：任务 8 的核心是“状态存哪、怎么隔离命名空间、怎么读写”，任务 8b 是“存多少、存坏了怎么办、什么时候清空重来”——两者都属于脚本状态存储设计文档的范围,但关注点不同（前者是接口形状，后者是运行期纪律），拆开便于分别验收，也便于风险登记单独标注（VM 强制重建的代价评估依赖 ADR 0012 的实测数字，任务 8 本身不需要这份评估）。

落地设计文档六（有界性）、七（孤儿状态）、九（VM 强制重建）。

**Interfaces Produces（概念形状）：**
```rust
pub const PER_MOD_QUOTA_BYTES: usize = 256 * 1024;
pub const PER_MOD_ENTITY_QUOTA_BYTES: usize = 4 * 1024;

// host.rs 或调用主循环处：读档完成后的强制重建钩子
pub fn rebuild_all_engines_after_load(/* 参数留给实现者,需要遍历全部已加载 mod 重新 load_source */);
```

- [ ] **TDD 循环**：
  - `单 mod 写入超过 256KB 累计配额时后续写入被拒绝`
  - `单（mod × 实体）写入超过 4KB 时被拒绝，不影响该 mod 对其他实体的配额`
  - `配额判定是加载期静态划分，不受其他 mod 实际用量影响`（构造两个 mod，一个写满，断言另一个不受影响——直接对应设计文档 6.1 节的确定性论证）
  - `mod 被移除后其脚本状态在读档时原样保留，不被清除`
  - `读档完成后强制重建全部 ScriptEngine 实例`（可以通过一个可观测的标记——例如重建计数器——断言重建确实发生，而不是断言“行为看起来正常”这种弱验证）
- [ ] **提交**

---

### 任务 9：存档主体格式——postcard + lz4_flex + 完整读写管线

**Files:** `crates/ll-content/src/save_file.rs`（新）
**依赖：** 任务 1、2、3、4、5、7（本任务是前面多数任务的汇合点，是本计划改动面最大的任务）

**本任务是本计划唯一必须一次性完成、不能拆成能各自独立提交的子任务的一步**（与坐标系重写计划任务 11 同一性质：读写管线要把已经各自独立测试通过的组件真正串成一条完整路径，中间任何一环缺失都无法运行）。

**具体串联顺序**（这是本任务的核心交付物,也是文档末尾自查表的直接依据）：

1. **存档写出**：`Registry::snapshot()`（任务 2）→ 写入 `SaveHeader.content_index_map`；`GenerationModSet`/`CurrentModSet`（任务 4）→ 写入 header 两个字段；`WorldState`（任务 3 已可序列化）→ `postcard::to_allocvec` → `lz4_flex` 压缩 → 写入文件主体；header 单独用 `serde_json` 写在文件前部或独立文件（具体“一个文件两段 vs 两个文件”留给实现者判断，规格没有强制要求物理布局，只要求“存档列表界面仅读头部”这条性质成立，即读取头部不能触发主体解压）。
2. **存档读入**：先读 header（`serde_json`，不触碰主体）→ 判定 `schema_version`（任务 7）→ 若需要迁移，跑 `MigrationChain`（任务 1）→ 解压 + `postcard` 反序列化主体 → `rebuild_from_header`（任务 2）重建 `Registry` → 逐条比对 `CurrentModSet` 与 header 的 `generation_mods`/`current_mods`（任务 6/7 判定缺失/不兼容）→ 按 `DegradeAction`（任务 6）逐类处理 → `assert_terrain_table_loaded`（任务 5）→ 产出 `LoadOutcome`（任务 6）。

**Interfaces Produces（概念形状）：**
```rust
pub fn save_to_file(path: &Path, header: &SaveHeader, world: &WorldState) -> Result<(), SaveError>;
pub fn load_from_header_only(path: &Path) -> Result<SaveHeader, LoadError>; // 存档列表界面用
pub fn load_full(path: &Path, current_registry: &Registry, current_manifests: &[ModManifest]) -> LoadOutcome;
```

**测试迁移策略（本任务的核心交付物之一）**：

| 现有测试 | 处理方式 |
|---|---|
| `state.rs`/`registry.rs`/`mod_set.rs` 内嵌测试 | **不改**——组件级正确性已由各自任务验证 |
| 任务 1–8 各自新增的单元/集成测试 | **不改**——本任务不重新验证组件内部逻辑，只验证串联 |
| **序列化往返属性测试**（规格 §14.2「`deserialize(serialize(w)) == w`，对任意随机生成的 `WorldState`」） | **本任务首次能真正落地**——此前 `actors`/`population` 被 skip，这条属性测试即便写了也测不出什么（skip 字段两边都是默认值，恒等）；现在必须补上真正覆盖非默认 `WorldState` 的 `proptest` 用例，这是规格 §14.2 表格里唯一一条此前无法兑现、本任务补齐的属性测试 |
| 三个既有验收 demo | **不受影响**——demo 目前不经过 `ll-content` 的存档读写路径，本任务不强制它们接入（那是任务 13 验收 demo 的职责） |

- [ ] **提交前必须通过的检查**：`cargo check --workspace`、`cargo test --workspace`、`cargo clippy --workspace` 全过；新增的存档往返 proptest 至少跑够默认迭代次数不失败
- [ ] **提交**（`feat:`，正文说明这是把前八个任务的组件真正串成一条可用路径）

---

### 任务 10：双模式存档——模式2/模式3 与单向降级

**Files:** `crates/ll-content/src/mode.rs`（新）
**依赖：** 任务 1（`SaveHeader.mode` 字段）、任务 9（读写管线）

落地规格决策 10：模式2（纯永久死亡，仅保留断点续玩存档）、模式3（自由读档，多存档位），**模式2 → 模式3 单向降级，不可逆，降级动作写入头部并永久标记**。

**Interfaces Produces：**
```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum SaveMode {
    Permadeath,
    /// 携带一个标记：是否曾经从 Permadeath 降级而来——即便当前就是
    /// FreeSave，这个标记也不会被移除,永久记录“这局游戏曾经降级过”。
    FreeSave { downgraded_from_permadeath: bool },
}

impl SaveMode {
    /// 唯一允许的模式变化路径。Permadeath → FreeSave 合法且返回新值；
    /// 任何其他方向（包括 FreeSave 试图“升回” Permadeath）返回 None。
    pub fn downgrade(self) -> Option<SaveMode>;
}
```

**模式2 下的存档管理**：只保留断点续玩存档——意味着任务 9 的 `save_to_file` 在 `Permadeath` 模式下应该覆盖同一个存档位而不是允许多存档位；`FreeSave` 模式允许多存档位。这条差异是存档管理 UI 的职责边界，本任务只交付判定逻辑（“当前模式允许几个存档位”），不交付 UI。

- [ ] **TDD 循环**：
  - `Permadeath 可以降级为 FreeSave`
  - `FreeSave 无法升级回 Permadeath`
  - `降级后的 FreeSave 标记 downgraded_from_permadeath 为真，且该标记在后续任何操作中都不会被清除`
  - `降级动作发生后重新读档,标记依然存在`（往返测试,不能只测内存中的一次调用）
- [ ] **提交**

---

### 任务 11：存档反序列化 fuzz target

**Files:** `crates/ll-content/fuzz/fuzz_targets/save_load.rs`（新，`cargo-fuzz` 结构）
**依赖：** 任务 9

落地 `p4-to-p5.md` 五、3 的建议——“P5 的存档反序列化是 L5 模糊测试新的最高优先级目标”。规格 §14.3 判定标准：任何输入都不得 panic、不得 OOM、不得无限循环，只允许返回 `Err`。

**目标函数**：对任意字节序列调用 `load_full`（或其分层的 header-only/body 两段各自 fuzz，具体拆分留给实现者判断哪种覆盖率更高），断言不 panic。

- [ ] 搭建 `cargo-fuzz` 基础设施（若 workspace 尚无任何 fuzz target，需要确认 `cargo-fuzz` 工具链可用，这是环境依赖而非代码依赖）
- [ ] **跑一轮短时间 fuzz（几分钟量级）确认没有立即崩溃**，作为提交前的最低验证——完整的持续 fuzz 是夜间任务（规格 §14.7 已规定不阻断单次合并），本任务只负责“target 存在且能跑”
- [ ] **提交**

---

### 任务 12：L6 端到端测试脚手架起步

**Files:** `crates/ll-content/tests/e2e_save_cycle.rs`（新，或视实际驱动主循环的位置调整 crate）
**依赖：** 任务 9、10

落地 `p4-to-p5.md` 五、1 的建议——三轮交接清单反复提议但一直未真正起步的 L6 端到端层。本任务**不要求覆盖全部玩法**，只要求交付“存档 → 修改 mod 列表 → 读档 → 断言降级正确”这一条完整链路的自动化测试，作为 L6 层第一个真正的用例，为后续阶段（含 P5-B）复用这套脚手架。

**必须使用程序化驱动，不得使用 SendKeys 或任何合成键盘事件**（裁定 CS-7，见文档末尾验收 demo 一节的详细说明）——L6 测试本身就应该走 `Intent → resolve → apply` 这条与真实按键完全相同的路径,不涉及任何窗口系统,不存在“前台窗口归属”问题,这条纪律对本任务是自动满足的,不需要额外小心。

- [ ] **TDD 循环**：
  - `构造一个含若干实体与地形改动的 WorldState → 存档 → 原样读档 → 世界哈希一致`
  - `存档后卸载一个曾贡献内容的 mod → 读档 → 相关实体降级而非崩溃`
  - `模式2 存档降级为模式3 后 → 存档 → 读档 → 模式仍是 FreeSave 且标记为真`
- [ ] **提交**

---

### 任务 13：验收 Demo

**Files:** 建议 `crates/ll-content/examples/p5_save_acceptance/`（若实现过程中发现更合适的落点，可调整）
**依赖：** 任务 1–12 全部

必须展示（对应用户提出的三条最低要求）：

1. **存档 → 读档后世界逐位一致**——用 `WorldState::hash()`（坐标系重写批次已交付，含实体/空间状态）在存档前后比对,而不只是“肉眼看起来一样”。
2. **缺失 mod 时按类型正确降级且不崩溃**——demo 场景至少构造三种缺失：一个物品类内容缺失（丢弃提示）、一个 NPC 种族缺失（降级占位）、玩家角色种族缺失（触发只读模式,而非崩溃或静默丢数据）。
3. **模式2 → 模式3 单向降级生效且不可逆**——demo 里执行一次降级,尝试“升级回”模式2 应被拒绝,且降级标记在存档往返后依然存在。

**必须实测，如实报告哪些验证了、哪些没有**——沿用既有纪律，这是第六次验收 demo，前五次全部抓出了单元测试测不出的连线缺陷。**本次最可能重演同一模式的地方**：(a) 任务 8「脚本状态写入是否真的经过 `apply`」这处待裁定的架构决定,若最终选择了“直接写穿”的方向,demo 应该专门验证一次“脚本状态变化是否被 `WorldState::hash()` 正确捕捉”——如果没有,说明这条写入路径游离在确定性回归测试之外,重演 `hash()` 文档记录过的“早期版本只混入地形”同一类缺口；(b) 任务 6「谁是玩家角色」这个开放问题的最终实现是否真的能在“玩家角色种族缺失”这个具体场景下被正确识别,而不是误把某个 NPC 当成玩家来判定只读模式。

**裁定 CS-7（本次会话新增纪律）**：demo 的按键验收不得使用 `SendKeys` 或任何合成键盘事件盲注——若本次验收 demo 需要展示实际按键触发存档/读档（例如“玩家按 F5 存档”这类交互），必须先确认前台窗口归属；确认不了就改用程序化驱动（走与真实按键完全相同的 `Intent → resolve → apply` 路径，任务 12 的脚手架已经是这个模式，本 demo 应该直接复用而不是重新引入按键合成）。

- [ ] **提交**

---

## 自查

### 完整调用链（P1/P3/坐标系重写计划要求的一节）

```
玩家在开局界面选择世界尺寸                                         ← ll-content::world_identity（任务 4）
  → validate_size_choice 校验，拒绝会退化的尺寸                     ← ll-content::world_identity（任务 4）
  → WorldState::new 创建世界（种子+尺寸确定）                        ← ll-world::state（既有，坐标系重写批次）
  → GenerationModSet::capture 在此刻封存                            ← ll-content::world_identity（任务 4）
  → 玩家游玩，产生 Intent → resolve → Effect → apply                ← ll-sim（既有）
  → 脚本技能/AI 产出 Intent，脚本状态经 state-set!/entity-state-set! ← ll-script::api::state（任务 8）
    写入 WorldState（写入路径是否真经过 apply——待裁定）
  → 玩家触发存档                                                    ← ll-content::save_file（任务 9）
    → Registry::snapshot() 写入 header.content_index_map            ← ll-content::content_index_map（任务 2）
    → GenerationModSet/CurrentModSet 写入 header 两个字段            ← ll-content::world_identity（任务 4）
    → WorldState（含 actors/population/脚本状态）序列化为主体         ← ll-world::state（任务 3、8）
    → postcard + lz4_flex 编码落盘                                  ← ll-content::save_file（任务 9）
  → 退出游戏，玩家卸载/更换一个 mod                                  ← （玩家操作，不在本计划范围内）
  → 玩家读档                                                        ← ll-content::save_file（任务 9）
    → 先读 header（不解压主体）                                     ← ll-content::save_file（任务 9）
    → schema 版本判定，需要则跑迁移链                                ← ll-content::migration（任务 1）
    → mod 内容哈希比对，区分「schema 问题」与「mod 问题」            ← ll-content::load_error（任务 7）
    → 解压+反序列化主体                                             ← ll-content::save_file（任务 9）
    → Registry::rebuild_from 重建注册表                             ← ll-content::content_index_map（任务 2）
    → 逐类内容按 DegradeAction 处理缺失                              ← ll-content::degrade（任务 6）
    → terrain_table 显式重新灌入并校验                               ← ll-world::state::assert_terrain_table_loaded（任务 5）
    → 产出 LoadOutcome：Playable / ReadOnly / Rejected               ← ll-content::degrade（任务 6）
    → 若 Playable：世界与存档前逐位一致（hash 比对）                 ← 验收 demo（任务 13）
    → 若 ReadOnly：只读模式生效，无法推进 tick                       ← ll-content::degrade（任务 6）
  → 玩家在模式2 存档中选择降级为模式3                                ← ll-content::mode（任务 10）
    → 降级标记写入 header 并永久保留                                 ← ll-content::mode（任务 10）
```

**每一环都指出了负责的任务与接口。** 唯一的软连接是任务 8「脚本状态写入是否真经过 `apply`」——这不是断链，是明确标注了需要实现者在动手前先解决的一个真实架构矛盾（设计文档字面表述与规格 C1 字面表述冲突），已经列入下方「待裁定」。

### 测试迁移策略总览（红灯窗口在哪，能否每步保持全绿）

| 任务 | 是否可能变红 | 说明 |
|---|---|---|
| 1、2、4、5、6、7、10、11、12、13 | **否，理论上全程可保持全绿** | 纯新增类型/新增 crate/新增测试，不改动既有函数签名或行为 |
| 3（`Agent`/`ThinPopulation` 补齐 serde） | **是，本计划唯一确定会有红灯窗口的任务** | 一旦 `Agent` 要求全部字段可派生，任何现有构造 `Agent` 的调用点（含三个验收 demo、`ll-sim`/`ll-world` 现有测试夹具）都要同步更新；红灯只能出现在本任务内部本地开发过程中，提交前必须 `cargo test --workspace` 全绿 |
| 8（脚本状态存储接口） | **视任务 8 的架构裁定而定** | 若最终选择“新增 `Effect` 变体经 `apply` 写入”的方向，会牵连 `ll-sim::effect`/`apply` 现有的穷尽匹配（`match effect { ... }` 需要新增分支），这类改动通常局部、可控，不太可能引发大范围红灯，但仍建议按任务 3 同等谨慎对待 |
| 9（存档主体读写管线） | **否，是汇合点但不改动被汇合的组件本身** | 只是把已经各自测试通过的函数串起来调用，不修改任务 1–8 产出的接口签名（若串联过程中发现签名不匹配，属于任务 1–8 某处设计有缺陷，应该回头修那个任务而不是在任务 9 里临时打补丁） |

**结论**：本计划与坐标系重写计划的红灯纪律相同——多数任务可保持全绿，任务 3 是唯一确定的红灯窗口，任务 8 视裁定结果可能有一次局部、可控的连带修改。

---

## 有意留给后续阶段的缺口

- **开局界面本身**（选尺寸、选双模式、选存档位）——`ll-ui` 完整控件库在 P7，本计划只交付这些选择背后的判定逻辑（`validate_size_choice`/`SaveMode`），不交付任何界面。
- **存档列表界面的具体渲染**——本计划保证“仅读头部”这条性质成立（`load_from_header_only`），不交付列表 UI 本身。
- **错误文案本地化**——`LoadError` 各变体的用户可见文案，留给 P7 UI 落地时用 Fluent 接线。
- **脚本状态存储是否接入 `WorldState::hash()`**——设计文档十、2 明确标注不强制要求，本计划任务 8 同样不强制，若实现者认为需要（例如为了让脚本状态变化也能被确定性回归测试捕捉），可以在任务 8 内顺手做，但不是验收必需项。
- **触发器/增益系统（`buffs-and-triggers.md`）**——该设计文档自己标注实现阶段依赖 P6 装备落地，与本计划无关，明确不在本计划范围。
- **`ll-econsim` 空跑压测**——规格 §14.4 的经济收敛测试依赖智能体经济（P9），本计划不涉及。

---

## 待裁定

以下事项是阅读设计文档与代码时发现的、本计划不代为裁定的分叉：

### 1. 脚本状态写入是否经过 `apply`（任务 8 核心矛盾）

`script-state-storage.md` 8.2 节字面写“直接写穿，没有中间层”；规格 §4 C1 字面写“`apply` 是全局唯一能修改世界的地方”。两者按字面无法同时成立——若脚本状态是 `WorldState` 的一部分（设计文档明确如此定位），“直接写穿”就意味着存在一条不经过 `apply` 的写入路径，与 C1 的“唯一”矛盾。**本计划任务 8 列出了两个可能方向（脚本层例外 vs 新增 `Effect` 变体），不代为选择**，这是需要项目所有者拍板的架构问题，因为它影响的不只是本任务，还包括未来任何“脚本能否绕过 `apply` 写状态”的先例（P5-B 的技能冷却持久化若复用脚本状态存储，同样继承这条裁定的后果）。

### 2. “谁是玩家角色”目前没有显式记录点

任务 6 的降级策略需要区分“这条记录属于玩家角色”还是“属于某个 NPC”，但核实后 `WorldState` 没有 `player_entity` 字段，三个既有验收 demo 都是在应用层自己记住。**是否应该给 `WorldState` 补一个官方的 `player_entity: Option<EntityId>` 字段**，是本计划任务 6 开工前需要先确认的问题——若确认需要，这个字段该由哪个任务补（任务 3 换型 `Agent`/`Arena` 时顺带，还是任务 6 单独补），本计划不预先指定，留给实现者在任务 6 开工前与项目所有者确认后再动手。

### 3. `mod_set.rs` 过期注释的更正是否应该单独提交

关键设计判断 3 已经指出 `mod_set.rs` 现有注释“留给 P6 世界生成器”因规格顺移已过期。本计划把更正这条注释放进任务 4，但这本质是一处独立于本计划主线的文档纠错——**若项目所有者认为这类纠错应该单独走一次“反向核对规格”式的收尾评审而不是夹在功能任务里顺手改**，可以把这部分从任务 4 拆出来。本计划的判断是夹带修改成本更低（同一个文件、同一批读者），但不强求。

---

## 收尾必做：反向核对规格

按项目纪律，本计划执行完毕收尾时必须反向核对一次规格与设计文档——尤其核对 `identity-and-ids.md`「落地状态：纯设计，尚无代码」这句话在 `WorldId` 已经随坐标系重写批次落地之后是否仍然准确（本计划通读代码时已经发现这句话目前是**过期的**：`WorldId`/`Interner` 均已在 `ll-core/src/ident.rs` 落地，见文档末尾「新债务」）；核对 `script-state-storage.md` 是否需要根据任务 8 的最终架构裁定（待裁定 1）反过来修订自己 8.2 节的表述；核对本计划任务 9 完成后，`p4-to-p5.md` 记录的“三处 `#[serde(skip)]` 待还”是否可以在下一轮交接清单里标记为已还清。

---

## 本次通读发现、交接清单未提及的债务（如实记录，不代为裁定）

1. **`identity-and-ids.md` 头部「落地状态：纯设计，尚无代码。……`WorldId`、`OrgInstance` 均未落地」已经过期**——`crates/ll-core/src/ident.rs` 核实 `WorldId` 已经完整落地（含 `next`/`get`，三条单元测试），是坐标系重写批次为 `SpaceId` 需要而顺带交付的。`OrgInstance` 确实仍未落地（`identity-and-ids.md` 关于势力/家族的部分仍是纯设计），但文档开头这句话笼统地把两者都标记为未落地，会误导下一个读者。**建议**：P5 收尾评审时更正这处表述，不需要单独立项，顺手改。
2. **`Agent::pos`（`TorusPos`）序列化障碍已经在坐标系重写批次解除，但 `agent.rs`/`thin.rs` 的模块文档没有跟着更新**——本计划任务 3 已经安排更正，这里额外记录一句：这类“代码已经解决但文档没跟上”的情形，与 `mod_set.rs` 那处过期注释是同一类问题的第二个独立实例，两处都是规格/设计随实现推进后，散落在各模块文档里的“已知限制”描述没有被回头巡检。**若 P5 或后续阶段有余力，值得做一次全项目范围的“模块文档里的‘尚未实现/不可持久化’描述是否仍然准确”专项巡检**，而不是每次靠通读撞见一处改一处。
3. **`Registry::content_hash_of` 未区分“命名空间从未贡献内容”与“贡献了内容但哈希查询失败”**——`mod_set.rs` 现有注释自己标注了这条：“`0` 表示该 mod 本次装载未贡献任何内容（或注册表里确实查不到——两者当前不做区分，属于本任务未处理的边界）”。本计划任务 4/7 依赖 `content_hash_of` 判断 mod 内容是否变化,这处未区分的边界若不处理，可能导致“mod 确实没贡献任何内容”与“查询本身出了问题”被误判为同一种情况。**建议**：任务 7（schema/mod 版本分离报错）开工时一并核实这条边界是否需要收紧，若需要，`content_hash_of` 应该改为返回三态（`None` = 从未贡献 / `Some(0)` = 贡献了但哈希恰好是 0 / 需要新增变体表示"查询失败"，若这种情况确实存在的话）——具体是否需要，留给任务 7 实现者核实后判断，本计划不预先断言这是个真问题还是虚惊一场。
