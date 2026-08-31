//! 存档主体读入后的 `ContentIndex` 重映射：旧索引（存档写出那一刻，
//! `Registry` 的登记顺序）经字符串换成新索引（当前会话 `Registry` 的
//! 登记顺序）。
//!
//! # 为什么这一步不能省
//!
//! 批次 B 的架构判断：`ll-world` 不能反向依赖 `ll-content`（依赖方向，
//! 见 `crate` 文档），因此 `ContentIndex` 在存档主体里直接存裸整数，
//! 头部单独带一张 `content_index_map: Vec<String>`（索引 → 字符串）。
//! 这是二进制格式的标准做法（字符串表 + 索引），但它引入一个必须钉死
//! 的前提：**读档时必须有一趟完整的重映射**。存档写出时的索引分配
//! 只反映「当次会话装载 mod 的顺序」；读档这次会话装载 mod 的顺序不
//! 保证与写出那次相同（mod 顺序调整、新增/移除 mod 都会打乱它）。
//! **漏掉任何一个 `ContentIndex`，它就静默指向错误的内容——不报错、
//! 不崩溃，只是那个 NPC 的职业悄悄变成了别的东西。**
//!
//! # 完整性如何保证：模块内穷尽解构，而非新增 newtype
//!
//! 两种可选方案：
//!
//! 1. **类型层面区分「已重映射」与「未重映射」**（例如给存档主体的
//!    反序列化中间表示引入一个 `RawContentIndex` newtype，与真正对外
//!    使用的 `ContentIndex` 分开，编译器强制每处转换点显式处理）。
//! 2. **模块内穷尽解构**：本模块对 [`WorldState`]/[`Agent`]/[`Goal`]
//!    使用不带 `..` 的穷尽字段解构，任何人给这些结构体新增字段，这里
//!    的模式立刻编译失败（`pattern does not mention field ...`），逼着
//!    下一个改动者显式决定新字段要不要参与重映射。
//!
//! 本模块选**方案 2**，理由是成本：方案 1 需要把 `Agent`/`ThinPopulation`
//! 已经在批次 B 落地并测试过的 `serde` 派生整个换成一层新的中间表示
//! 类型，红灯窗口会波及全仓库已经调用 `Agent {}`/`ThinPopulation::spawn`
//! 的十几处调用点——这正是本计划文档明确要求任务 9 避免的（「不修改
//! 任务 1–8 产出的接口签名」）。方案 2 不需要改一行既有类型定义，只在
//! **本文件**新增的解构点上生效，且它是真实的编译期保证（不是注释里的
//! 约定）：故意删掉下面任意一个字段名都能让 `cargo build` 失败，见本
//! 模块测试「漏掉一个字段时编译失败」一节的说明（该场景本身无法写成
//! 一条会通过的 `#[test]`，因为它的断言对象是编译器,退而求其次的是
//! 「本模块存在」这件事本身+代码 review 时对照本文档的穷尽字段列表）。
//!
//! **代价与局限**：这道防线只保护「顶层容器结构体新增字段」这一类
//! 疏漏（[`WorldState`]/[`Agent`] 本身），不延伸到 [`Goal`]/[`Affiliation`]
//! 这类叶子类型将来新增的 `ContentIndex` 字段（它们的解构点也是穷尽的，
//! 但覆盖的是「今天已知的字段集合」，不是「将来可能新增的字段」的
//! 递归保证）——这与叶子类型改动频率低、且改动时通常连带需要新的
//! 降级语义（需要人工判断该怎么处理,不是机械重映射能自动覆盖的）有关，
//! 如实记录在此，不假装是完全体的类型级证明。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_core::time::Tick;
use ll_mod::registry::Registry;
use ll_world::entity::{ActiveStatModifier, Affiliation, Agent, AttributeKind, Goal, OrgRef};
use ll_world::item::{EquipSlot, GroundItemStack, ItemStack};
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::terrain::TerrainKind;

use crate::content_index_map;
use crate::degrade::{ContentKind, DegradeAction, OwnerContext, decide_degrade_action};
use crate::load_error::LoadError;

/// 一次完整的重映射过程中，按 [`crate::degrade::decide_degrade_action`]
/// 逐条产出的降级决策——交给调用方（`ll-content::save_file`）汇总成
/// [`crate::degrade::LoadOutcome`]。
type DegradeLog = Vec<DegradeAction>;

/// 把 `world` 里全部 `ContentIndex` 字段从存档写出时的旧索引换成当前
/// 会话 `current_registry` 的新索引。
///
/// `content_index_map` 是存档头字段（索引 → 字符串），`placeholder` 是
/// 调用方为 NPC 种族/职业缺失场景准备好的占位索引（可能没有，见
/// [`crate::degrade`] 模块文档「`ContentIndex` 缺占位值的既知债务」）。
///
/// 成功时返回本次重映射过程中产生的全部 [`DegradeAction`]（可能为空）；
/// 失败（[`LoadError::Corrupted`]）仅在两种情形下发生：`content_index_map`
/// 本身有格式错误的条目，或者某个结构性内容（地形/空间层属性，见本
/// 文件 `Remapper::remap_structural` 文档）在当前会话找不到——两者都
/// 意味着存档已经不自洽，不是「缺 mod 可以降级」这一类可挽救的问题。
pub fn remap_world(
    world: &mut WorldState,
    content_index_map: &[String],
    current_registry: &Registry,
    placeholder: Option<ContentIndex>,
) -> Result<DegradeLog, LoadError> {
    let ids = content_index_map::parse_content_index_map(content_index_map)
        .map_err(|err| LoadError::Corrupted(err.to_string()))?;
    let mut remapper = Remapper {
        ids: &ids,
        registry: current_registry,
        placeholder,
        actions: Vec::new(),
    };

    // 穷尽解构：见模块文档「完整性如何保证」——新增字段在这里编译失败。
    let WorldState {
        seed: _,
        // 地形形态参数（世界生成参数落地批次）：四个纯整数，不含任何
        // ContentIndex，也不依赖 mod 加载顺序——与 seed 同一类，不需要
        // 跟着重映射走。
        terrain_shape: _,
        clock: _,
        size: _,
        ref mut terrain,
        ref mut interiors,
        current_interior: _,
        surface_profile: _,
        ref mut population,
        ref mut actors,
        player_entity,
        // exploration（探索记忆）不含任何 ContentIndex——是纯粹的「看
        // 没看过」位图（见 crate::exploration 模块文档），不需要跟着
        // mod 重映射走。
        exploration: _,
        terrain_table: _,
        // 历史事件记录（击杀与死亡记录批次新增）：`HistoricalEvent`
        // 内部的 `KillRecord`/`KillCause` 确实可能携带 `ContentIndex`
        // （`Skill`/`Melee.weapon`/`Environmental` 三个变体）——与
        // `Goal`/`Affiliation` 这类叶子类型同一个既知局限（见模块文档
        // 「代价与局限」一节）：本函数的穷尽解构只保护顶层容器结构体
        // 新增字段这一类疏漏，不递归展开到叶子类型内部未来可能出现的
        // `ContentIndex`。历史记录是"已经发生、不再变化"的既往事实，
        // 缺失 mod 时让它继续携带旧索引（不伪造、不崩溃，与
        // `DegradeAction::Reject` 同一种"诚实保留"取舍）比在这一批改动
        // 里发明一套新的"历史记录专用降级语义"更负责——真正需要按当前
        // 会话内容展示这些记录时，消费方（传说浏览）应自行处理查不到
        // 的情形，如实记录为已知缺口，不在本次变更范围内补齐。
        history: _,
        // WorldId 分配计数器：纯 u32，不依赖 mod 加载顺序，不需要重映射。
        next_world_id: _,
        // 击杀聚合计数（现按决策二数全部击杀，见
        // ll_world::state::WorldState::kill_counts 文档「决策二」一
        // 节；本函数只关心键的形状,不关心计数是怎么累加的）：键本身
        // 就是 ContentIndex（受害者的 creature_kind 或回退到 race），
        // 与 profession/race 同一类"依赖 mod 加载顺序"的内容,必须显式
        // 重映射——否则读档后这份统计会悄悄挂在错误的内容上（模块文档
        // 「为什么这一步不能省」一节点名的正是这类静默错位）。见下方
        // remap_kill_counts。
        ref mut kill_counts,
        // 地面物品（P6 第二批：背包与地面物品）：`stack.def` 指向
        // `ItemDef`，依赖 mod 加载顺序，必须显式重映射——否则读档后
        // 地面上的物品会静默指向错误的内容（模块文档「为什么这一步
        // 不能省」一节点名的正是这类静默错位）。`pos`/`dropped_at` 都
        // 是纯数值，不含 ContentIndex，随 def 一起保留或丢弃。见下方
        // remap_ground_items。
        ref mut ground_items,
        // 已物化据点集合（NPC 生成批次）：元素是 `WorldId`，**不是**
        // `ContentIndex`。`ll_core::ident::WorldId` 模块文档写明它的构造
        // 过程本身稳定、永不复用，不依赖 mod 加载顺序——与本函数上方
        // `next_world_id`、以及 `remap_affiliations` 对 `OrgRef::Instance`
        // 的既有处理同一个理由，因此不需要重映射。
        //
        // 这里的 `_` 是**逐字段确认过不需要重映射**，不是顺手吞掉：本文件
        // 历史上真出过一次 `active_stat_modifiers` 被写成 `field: _` 而在
        // 换 mod 读档时静默清空的事故（见模块文档「完整性如何保证」），
        // 而这一次静默清空的后果会更难察觉——集合被清空之后，玩家换一次
        // mod 读档，全部走过的据点都会**重新生成一批 NPC**，把杀掉的人
        // 原样复活。判据是这个字段的元素类型里根本不含 `ContentIndex`，
        // 编译器会在它某天变成携带索引的类型时让这一行继续通过、因此这条
        // 注释就是那个提醒。
        materialized_settlements: _,
        // 势力表（势力播种批次）——同样是**逐字段确认过不需要重映射**：
        // `Faction` 里除了 `org.def`/`org.authored` 之外全部是 `WorldId`
        // 与 `u32`，而世界生成播种出来的势力那两项**恒为 `None`**（纯
        // 生成，没有 mod 模板、没有 mod 具名定义，见
        // `ll_world::entity::OrgInstance` 字段文档）。
        //
        // **等 mod 真的定义具体势力那天，这一行必须改。** 那时 `org.def`
        // 会携带真实的 `ContentIndex`，`_` 会让它在换一批 mod 装载顺序之后
        // 静默指向另一个模板——正是上面那段注释点名的那类事故。这笔账同时
        // 记在 `OrgInstance` 的类型文档里。
        factions: _,
    } = *world;

    terrain.try_remap_resident_terrain(|kind| -> Result<TerrainKind, LoadError> {
        let new_index = remapper.remap_structural(kind.index(), "地表地形")?;
        Ok(TerrainKind::from_index(new_index))
    })?;

    for interior in interiors.iter_mut() {
        interior.profile = remapper.remap_structural(interior.profile, "Interior 层属性")?;
    }

    population.try_remap_content_indices(|old| {
        remapper.remap_character_attribute(old, OwnerContext::Npc)
    })?;

    for (id, agent) in actors.iter_mut_with_id() {
        let owner = if Some(id) == player_entity {
            OwnerContext::Player
        } else {
            OwnerContext::Npc
        };
        remap_agent(&mut remapper, agent, owner)?;
    }

    remap_kill_counts(&mut remapper, kill_counts)?;
    remap_ground_items(&mut remapper, ground_items)?;

    Ok(remapper.actions)
}

/// 重映射击杀聚合计数（决策二：数全部击杀,见
/// `ll_world::state::WorldState::kill_counts` 文档「决策二」一节）：
/// 键（`ContentIndex`）找不到
/// 当前会话内容时整桶丢弃（[`ContentKind::KillCount`]），与
/// [`remap_skill_cooldowns`] 同一个形状——键是需要重映射/可丢弃的
/// `ContentIndex`，值（计数）本身不含任何 `ContentIndex`，原样搬到
/// 新键下。
///
/// 若两个旧键（理论上不会发生：`content_index_map` 是旧索引到字符串的
/// 一一映射，`Registry::get` 也是字符串到新索引的一一映射）恰好被映射
/// 到同一个新键，两边的计数相加而不是互相覆盖——不假设"不会发生"就
/// 简单覆盖丢数据。
fn remap_kill_counts(
    remapper: &mut Remapper<'_>,
    kill_counts: &mut BTreeMap<ContentIndex, u64>,
) -> Result<(), LoadError> {
    let mut kept = BTreeMap::new();
    for (kind, count) in std::mem::take(kill_counts) {
        if let Some(new_kind) = remapper.remap_droppable(kind, ContentKind::KillCount)? {
            *kept.entry(new_kind).or_insert(0) += count;
        }
    }
    *kill_counts = kept;
    Ok(())
}

/// 逐条 `ContentIndex` 重映射的执行者：持有「旧索引 → 字符串」查表、
/// 当前会话 `Registry`、占位索引，并累积产生的 [`DegradeAction`]。
struct Remapper<'a> {
    ids: &'a [NamespacedId],
    registry: &'a Registry,
    placeholder: Option<ContentIndex>,
    actions: DegradeLog,
}

impl Remapper<'_> {
    /// 旧索引 → 字符串 → 当前会话索引；查不到当前会话索引返回 `Ok(None)`
    /// （不是错误——这是「缺 mod」的正常检测点），旧索引本身超出
    /// `content_index_map` 范围才是错误（存档已经不自洽）。
    fn lookup(&self, old: ContentIndex) -> Result<Option<ContentIndex>, LoadError> {
        let raw = self.ids.get(old.get() as usize).ok_or_else(|| {
            LoadError::Corrupted(format!(
                "ContentIndex {} 超出 content_index_map 范围（长度 {}）",
                old.get(),
                self.ids.len()
            ))
        })?;
        Ok(self.registry.get(raw))
    }

    /// 角色属性类（职业/种族）：找不到时按归属走
    /// [`crate::degrade::decide_degrade_action`] 决定占位或拒绝。
    fn remap_character_attribute(
        &mut self,
        old: ContentIndex,
        owner: OwnerContext,
    ) -> Result<ContentIndex, LoadError> {
        match self.lookup(old)? {
            Some(new) => Ok(new),
            None => {
                let action =
                    decide_degrade_action(ContentKind::CharacterAttribute, owner, self.placeholder);
                self.actions.push(action);
                match action {
                    DegradeAction::FallbackToPlaceholder(idx) => Ok(idx),
                    // Reject：整体转入只读模式，这个字段值不会再被任何
                    // 会推进世界的逻辑消费（见 ReadOnlySave 文档），原样
                    // 保留旧索引已经足够诚实——既不伪造新内容，也不 panic。
                    DegradeAction::Reject => Ok(old),
                    DegradeAction::DropWithWarning => {
                        unreachable!("CharacterAttribute 不会产出 DropWithWarning")
                    }
                }
            }
        }
    }

    /// 可丢弃类（目标类型、归属定义）：找不到时丢弃该条目本身,返回
    /// `None` 告诉调用方从容器里移除，而不是留一个指向错误内容的值。
    fn remap_droppable(
        &mut self,
        old: ContentIndex,
        kind: ContentKind,
    ) -> Result<Option<ContentIndex>, LoadError> {
        match self.lookup(old)? {
            Some(new) => Ok(Some(new)),
            None => {
                let action = decide_degrade_action(kind, OwnerContext::None, None);
                debug_assert!(matches!(action, DegradeAction::DropWithWarning));
                self.actions.push(action);
                Ok(None)
            }
        }
    }

    /// 结构性内容（地形/空间层属性）：找不到直接判定存档损坏。
    ///
    /// 这类内容属于生成期 mod 集合，理应已经被读档流程更早一步的 mod
    /// 内容哈希校验（`crate::load_error::check_mod_content`）挡住——若
    /// 哈希校验通过、这里却仍然查不到，说明出现了哈希校验覆盖不到的
    /// 不一致（例如同一命名空间内单条内容被替换但异或哈希恰好没变，
    /// 见 `ll_mod::registry::Registry::intern` 文档「用异或折叠……代价
    /// 是抗碰撞性弱于……」这条已知取舍）。这类内容也没有一个像
    /// 「玩家角色 vs NPC」那样清晰的降级语义可用（一格地形/一层空间
    /// 属性不像一件物品那样能被「丢弃」），发明一套新的降级规则超出
    /// 本任务范围——诚实报错比假装能安全降级更负责。
    fn remap_structural(
        &mut self,
        old: ContentIndex,
        what: &str,
    ) -> Result<ContentIndex, LoadError> {
        self.lookup(old)?.ok_or_else(|| {
            LoadError::Corrupted(format!(
                "{what}引用的内容（旧索引 {}）在当前会话注册表中找不到——\
                 mod 内容哈希校验本应挡住这种情况，出现意味着存档与当前 mod 集合的不一致比预期更深",
                old.get()
            ))
        })
    }
}

/// 重映射一个 [`Agent`] 的全部 `ContentIndex` 字段。
///
/// 穷尽解构：见模块文档「完整性如何保证」。
fn remap_agent(
    remapper: &mut Remapper<'_>,
    agent: &mut Agent,
    owner: OwnerContext,
) -> Result<(), LoadError> {
    let Agent {
        pos: _,
        stats: _,
        next_action_at: _,
        health: _,
        ref mut affiliations,
        wallet: _,
        ref mut profession,
        ref mut goals,
        // 性别（角色创建批次新增）：一个封闭的 Rust 枚举，**不是内容**
        // （不进 `Registry`、不占 `ContentIndex`，见
        // `ll_world::entity::Gender` 模块文档「为什么不是内容」一节），
        // 因此换一份 mod 集合读档时它的取值不需要、也无从重映射。
        gender: _,
        ref mut race,
        // 资源当前值——纯数值，不携带任何 ContentIndex，不需要重映射。
        mana: _,
        stamina: _,
        // 开放注册资源池（资源池落地批次新增）：键是指向 ResourcePoolDef
        // 的 ContentIndex，必须重映射；值是当前量，纯数值不需要处理，
        // 见 remap_resource_pools 文档。
        ref mut resource_pools,
        // 法术位已消耗数（法术位落地批次新增）：键的前半是指向
        // ResourcePoolDef 的 ContentIndex，必须重映射，理由与
        // resource_pools 完全相同（同一个池身份，只是记录方向相反：
        // 已消耗 vs 还剩多少），见 remap_spent_slots 文档。
        ref mut spent_slots,
        // 休息会话（法术位落地批次新增）：RestState 只有 started_at/
        // target_ticks 两个纯数值字段，不携带任何 ContentIndex，不需要
        // 重映射。
        resting: _,
        // 三个 P5-B 任务 5 新增的 ContentIndex 承载字段：见下方各自的
        // remap_* 帮手。
        ref mut unlocked_skills,
        ref mut skill_cooldowns,
        ref mut subclasses,
        // 「曾经获得过哪些副职」的去重账本（副职发点批次新增）：与上
        // 一行同一种载荷（指向 SubclassTable 的 ContentIndex），因此
        // 走同一条帮手、同一条丢弃语义。这里**不能**写成
        // `subclasses_ever_granted: _`——那正是本文件历史上
        // active_stat_modifiers 出过的那次事故的形态（换 mod 读档时
        // 静默清空），而这一次静默清空的后果是玩家换一次 mod 就能把
        // 全部副职的点数再领一遍，见该字段文档「一处如实标注的边界」。
        ref mut subclasses_ever_granted,
        // 已知配方（配方发现批次新增）：每一条都是指向 RecipeTable 的
        // ContentIndex，依赖 mod 加载顺序，必须显式重映射——见下方
        // remap_known_recipes。这里**不能**写成 `known_recipes: _`：
        // 那正是本文件历史上 active_stat_modifiers 出过的那次事故的
        // 形态（换 mod 读档时静默清空），见模块文档「完整性如何保证」。
        ref mut known_recipes,
        // 已鉴定物品种类（未鉴定物品批次新增）：每一条都是指向 ItemTable
        // 的 ContentIndex，依赖 mod 加载顺序，必须显式重映射——见下方
        // remap_identified_items。这里同样**不能**写成
        // `identified_items: _`：与上面 known_recipes 逐字同理。
        ref mut identified_items,
        // 临时属性修正外层按 AttributeKind（引擎内置的封闭枚举，不是
        // 内容注册表索引）为键，不需要重映射；但内层键是「来源」
        // （buffs-and-triggers.md 六节①，`ContentIndex`）——六节存储
        // 改法之前这个字段完全不携带 ContentIndex，改法落地后必须重
        // 映射，见 remap_active_stat_modifiers。
        ref mut active_stat_modifiers,
        ref mut current_space,
        // mod 状态（ll_world::mod_state）**确实**不需要重映射，但这不是
        // 「懒得处理」——`ModStateValue` 的七个变体逐个查过：Int/Bool/
        // Str 是裸标量；Ref 存的是 `NamespacedId` 的**字符串**形式
        // （`namespace:path`），正是为了不依赖 mod 加载顺序才特意不存
        // ContentIndex（见 ll_world::mod_state::ModStateValue::Ref 文档），
        // 读取时由当前会话重新解析；Entity 携带 EntityId，而 EntityId
        // 是世界内实体地址，不是内容注册表索引；List/Map 只是上述变体
        // 的递归容器，不引入新的索引类型。键的 `(mod_namespace, key)`
        // 两段也都是字符串。全表因此不含任何 ContentIndex。
        //
        // 这段话必须写在这里、而不是指向别处：本文件历史上
        // active_stat_modifiers 那次事故的形态就是 `field: _` 不带论证
        // 地静默丢数据（见模块文档「完整性如何保证」）。若将来
        // ModStateValue 新增了携带 ContentIndex 的变体，这里必须改成
        // 显式重映射。
        mod_state: _,
        // 击杀与死亡记录批次新增的三个字段——逐一显式决定：
        ref mut creature_kind,
        // 出生时刻——纯数值，不携带任何 ContentIndex，不需要重映射。
        spawned_at: _,
        // WorldId 不依赖 mod 加载顺序（`ll_core::ident::WorldId` 模块
        // 文档：构造过程本身稳定，永不复用），与 `remap_affiliations`
        // 对 `OrgRef::Instance` 的既有处理同一个理由，不需要重映射。
        remembered_id: _,
        // 等级与经验系统新增的三个字段（level-and-experience-system.md
        // 六节）：三者均是纯 i32/i64 数值，不携带任何 ContentIndex——
        // 与 health/mana/stamina 同一类「纯数值不需要重映射」字段，显式
        // 列在这里（而不是用 `..` 省略）本身就是穷尽解构要保护的那件事：
        // 让下一个新增字段的人也必须在这里显式决定要不要重映射。
        level: _,
        experience: _,
        xp_to_next_level: _,
        // 未分配的属性点/技能点（升级加点批次新增）：两者都是 u32
        // 计数，类型里不含任何 ContentIndex——与 level/experience 同
        // 一类「纯数值不需要重映射」字段。这里的 `_` 是**逐字段确认过
        // 不需要重映射**，不是顺手吞掉：本文件历史上真出过一次
        // `active_stat_modifiers` 被写成 `field: _` 而在换 mod 读档时
        // 静默清空的事故（见模块文档「完整性如何保证」），穷尽解构 +
        // 逐条注释就是那条防线。
        //
        // 特别地：**技能点不需要跟着 unlocked_skills 一起被回收**。
        // `remap_unlocked_skills` 会丢弃「本次会话查不到的技能」，
        // 但那不意味着当初花掉的技能点应该退回来——退点会让「换一个
        // mod 组合读档再换回来」变成一台点数复制机（丢弃时退点、换回
        // 时技能还在）。存档里的余额原样保留是唯一自洽的选择。
        unspent_attribute_points: _,
        unspent_skill_points: _,
        // 背包（P6 第二批：背包与地面物品）：每一堆的 def 指向
        // ItemDef，依赖 mod 加载顺序，必须显式重映射——见下方
        // remap_inventory。
        ref mut inventory,
        // 装备栏（P6 第三批：装备槽位）：每一堆的 def 同样指向
        // ItemDef，依赖 mod 加载顺序，必须显式重映射——键（EquipSlot）
        // 是引擎内置的位下标常量，不依赖 mod 加载顺序，原样保留，见下方
        // remap_equipment。
        ref mut equipment,
        // 潜行状态（潜行与盗贼被动批次）：纯 bool，不携带任何
        // ContentIndex，不需要重映射——与 health/mana/stamina/level 同
        // 一类「纯数值不需要重映射」字段。显式列在这里（而不是被 `..`
        // 或一个 `_` 通配吞掉）本身就是穷尽解构要保护的那件事，也是本
        // 文件历史上真实出过一次事故的那条防线：`active_stat_modifiers`
        // 曾被写成 `field: _` 形态而在换 mod 读档时静默清空，见模块文档
        // 「完整性如何保证」。这里的 `_` 是**有意的**「确认过不需要重
        // 映射」，不是「顺手吞掉」——判据是这个字段的类型里根本不存在
        // 任何 ContentIndex，编译器会在它某天变成携带索引的类型时让这
        // 一行继续通过、因此这条注释就是那个提醒。
        stealthed: _,
    } = *agent;

    *profession = remapper.remap_character_attribute(*profession, owner)?;
    *race = remapper.remap_character_attribute(*race, owner)?;
    remap_goals(remapper, goals)?;
    remap_affiliations(remapper, affiliations)?;
    remap_space(remapper, current_space)?;
    remap_unlocked_skills(remapper, unlocked_skills)?;
    remap_skill_cooldowns(remapper, skill_cooldowns)?;
    remap_subclasses(remapper, subclasses)?;
    remap_subclasses(remapper, subclasses_ever_granted)?;
    remap_known_recipes(remapper, known_recipes)?;
    remap_identified_items(remapper, identified_items)?;
    remap_active_stat_modifiers(remapper, active_stat_modifiers)?;
    remap_creature_kind(remapper, creature_kind, owner)?;
    remap_resource_pools(remapper, resource_pools)?;
    remap_spent_slots(remapper, spent_slots)?;
    remap_inventory(remapper, inventory)?;
    remap_equipment(remapper, equipment)?;
    Ok(())
}

/// 重映射一个 `Agent` 的开放注册资源池当前值表：键（池索引）找不到
/// 当前会话内容时整条丢弃（[`ContentKind::ResourcePool`]）——理由同
/// [`remap_skill_cooldowns`]：这是「这个池现在还剩多少」的一条记录，
/// 不是实体本体的核心身份，缺一个池的存量不等于「失去自己」。值
/// （当前量）本身不含 `ContentIndex`，跟着键一起丢弃或保留。
fn remap_resource_pools(
    remapper: &mut Remapper<'_>,
    resource_pools: &mut BTreeMap<ContentIndex, i32>,
) -> Result<(), LoadError> {
    let mut kept = BTreeMap::new();
    for (pool, current) in std::mem::take(resource_pools) {
        if let Some(new_pool) = remapper.remap_droppable(pool, ContentKind::ResourcePool)? {
            kept.insert(new_pool, current);
        }
    }
    *resource_pools = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的法术位已消耗数表：键是 `(池索引, 档位)`，池
/// 索引部分找不到当前会话内容时整条丢弃（[`ContentKind::ResourcePool`]，
/// 与 [`remap_resource_pools`] 同一个 `ContentKind`——两者记录的是同一个
/// 池身份，只是方向相反，缺失时的降级语义没有理由不同）。档位（`u8`）
/// 与已消耗数（`u32`）都不含 `ContentIndex`，跟着键一起丢弃或保留。
fn remap_spent_slots(
    remapper: &mut Remapper<'_>,
    spent_slots: &mut BTreeMap<(ContentIndex, u8), u32>,
) -> Result<(), LoadError> {
    let mut kept = BTreeMap::new();
    for ((pool, tier), spent) in std::mem::take(spent_slots) {
        if let Some(new_pool) = remapper.remap_droppable(pool, ContentKind::ResourcePool)? {
            kept.insert((new_pool, tier), spent);
        }
    }
    *spent_slots = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的背包（P6 第二批：背包与地面物品）：每一堆
/// `ItemStack.def` 找不到当前会话内容时整堆丢弃（[`ContentKind::Item`]，
/// 该变体的文档原文就是「背包/地面堆叠里引用的 `ItemDef`」，本函数
/// 与 [`remap_ground_items`] 正是它点名的两个消费者）——理由同
/// [`remap_unlocked_skills`]：这是「持有哪些物品」的一条记录，不是
/// 实体本体的核心身份，缺一件物品不等于「失去自己」。`count`/
/// `durability` 都是纯数值，不含 `ContentIndex`，随 `def` 一起保留或
/// 丢弃。
fn remap_inventory(
    remapper: &mut Remapper<'_>,
    inventory: &mut Vec<ItemStack>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(inventory.len());
    for stack in inventory.drain(..) {
        if let Some(new_def) = remapper.remap_droppable(stack.def, ContentKind::Item)? {
            kept.push(ItemStack {
                def: new_def,
                ..stack
            });
        }
    }
    *inventory = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的装备栏（装备栏位批次，P6 第三批）：与
/// [`remap_inventory`] 同一条判据——每一堆 `ItemStack.def` 找不到当前
/// 会话内容时整槽丢弃（[`ContentKind::Item`]，与背包共用同一个变体，
/// 理由是两者引用的都是"背包/地面堆叠里引用的 `ItemDef`"这同一类
/// 内容,装备栏没有理由用不同的降级语义）——这是「身上穿戴哪些装备」
/// 的一条记录,不是实体本体的核心身份,缺一件装备不等于「失去自己」
/// （角色仍然存在,只是变成裸装）。
///
/// 键（[`EquipSlot`]）是引擎内置的位下标常量，不依赖 mod 加载顺序，
/// 原样保留，不需要重映射——与 `remap_active_stat_modifiers` 外层键
/// （[`AttributeKind`]）「引擎内置的封闭枚举，不携带 `ContentIndex`」
/// 同一条既有判断。
fn remap_equipment(
    remapper: &mut Remapper<'_>,
    equipment: &mut BTreeMap<EquipSlot, ItemStack>,
) -> Result<(), LoadError> {
    let mut kept = BTreeMap::new();
    for (slot, stack) in std::mem::take(equipment) {
        if let Some(new_def) = remapper.remap_droppable(stack.def, ContentKind::Item)? {
            kept.insert(
                slot,
                ItemStack {
                    def: new_def,
                    ..stack
                },
            );
        }
    }
    *equipment = kept;
    Ok(())
}

/// 重映射地面物品（[`WorldState::ground_items`]，P6 第二批；NPC 死亡
/// 掉落批次扩展到容器内容物）：与 [`remap_inventory`] 同一条判据，
/// `def` 找不到当前会话内容时整堆丢弃（[`ContentKind::Item`]）——地面
/// 上一堆物品同样不是任何实体的核心身份，`pos`/`dropped_at` 不含
/// `ContentIndex`，随 `def` 一起保留或丢弃。
///
/// # 容器内容物：逐条独立重映射，不是随容器整体丢弃
///
/// `item.contents`（尸体等容器装着的战利品）是一个 `Vec<ItemStack>`，
/// 每一条都独立可能找不到当前会话内容——与 [`remap_inventory`] 对
/// 背包的处理完全同一套逻辑（本函数复用它）：找不到的那一条单独丢弃，
/// 不会因为容器里某一件已卸载 mod 的战利品就连累容器本身（`stack`，
/// 例如尸体这具"物品"本身）与其余内容物一起消失——尸体是否还存在、
/// 装着什么，是两个独立的问题。
fn remap_ground_items(
    remapper: &mut Remapper<'_>,
    ground_items: &mut Vec<GroundItemStack>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(ground_items.len());
    for mut item in ground_items.drain(..) {
        let Some(new_def) = remapper.remap_droppable(item.stack.def, ContentKind::Item)? else {
            continue;
        };
        remap_inventory(remapper, &mut item.contents)?;
        kept.push(GroundItemStack {
            stack: ItemStack {
                def: new_def,
                ..item.stack
            },
            contents: item.contents,
            ..item
        });
    }
    *ground_items = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的生物类型标记：`Some` 时与 `profession`/`race`
/// 同一条路径（[`Remapper::remap_character_attribute`]）——`creature_kind`
/// 同样是"这个实体是什么"的角色属性，找不到当前会话内容时的降级/
/// 拒绝语义应当与种族/职业一致，不是新的一类。`None`（绝大多数有
/// 种族意义的智慧类人型）原样保留，不触发任何查表。
fn remap_creature_kind(
    remapper: &mut Remapper<'_>,
    creature_kind: &mut Option<ContentIndex>,
    owner: OwnerContext,
) -> Result<(), LoadError> {
    if let Some(kind) = *creature_kind {
        *creature_kind = Some(remapper.remap_character_attribute(kind, owner)?);
    }
    Ok(())
}

/// 重映射一个 `Agent` 的已解锁技能集合：找不到当前会话内容的技能整条
/// 丢弃（[`ContentKind::Skill`]）——理由同 [`remap_goals`]，这是「学过
/// 哪些技能」的记录，不是实体本体的核心身份。
fn remap_unlocked_skills(
    remapper: &mut Remapper<'_>,
    unlocked_skills: &mut Vec<ContentIndex>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(unlocked_skills.len());
    for skill in unlocked_skills.drain(..) {
        if let Some(new_skill) = remapper.remap_droppable(skill, ContentKind::Skill)? {
            kept.push(new_skill);
        }
    }
    *unlocked_skills = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的已知配方列表（配方发现批次）：找不到当前会话
/// 内容的配方**丢弃并警告**（[`ContentKind::Recipe`]）——与
/// [`remap_unlocked_skills`] 同一条判断：知不知道一张图纸是角色持有的
/// 一条记录，不是它的核心身份，缺一条不等于「失去自己」。
///
/// # 丢弃之后不需要「退还」任何东西
///
/// 与 `unspent_skill_points` 那一段相反的情形：学会一条配方**不花费
/// 任何可退还的资源**（读书不消耗书，试做不消耗食材，见
/// `ll_sim::resolve::resolve_read`/`resolve_experiment` 文档），因此
/// 「丢弃时该退什么」这个在技能点那里必须回答的问题在这里根本不存在。
/// 换回原来的 mod 组合时，配方索引重新解析得到、这条记录原样回来。
fn remap_known_recipes(
    remapper: &mut Remapper<'_>,
    known_recipes: &mut Vec<ContentIndex>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(known_recipes.len());
    for recipe in known_recipes.drain(..) {
        if let Some(new_recipe) = remapper.remap_droppable(recipe, ContentKind::Recipe)? {
            kept.push(new_recipe);
        }
    }
    *known_recipes = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的已鉴定物品种类列表（未鉴定物品批次）：找不到
/// 当前会话内容的物品**丢弃并警告**（[`ContentKind::Item`]）——与
/// [`remap_known_recipes`] 同一条判断：认不认得一种东西是角色持有的一
/// 条记录，不是它的核心身份，缺一条不等于「失去自己」。
///
/// # 丢弃之后不需要「退还」任何东西
///
/// 与 [`remap_known_recipes`] 同一条理由，但要注意它在这里**不是**照抄
/// 而是重新成立的：鉴定确实产出过一次经验
/// （`ll_sim::resolve::resolve_identify`），但那份经验早已加进
/// `Agent::experience` 并可能已经换成了等级与点数——它不是一份「寄存在
/// 这条记录里、随记录消失而应当退还」的资源，正如换 mod 读档不会把已经
/// 打怪拿到的经验退回去。换回原来的 mod 组合时，物品索引重新解析得到、
/// 这条记录原样回来，也不会因此再给一次经验（`resolve_identify` 的重复
/// 闸门读的就是这个字段）。
fn remap_identified_items(
    remapper: &mut Remapper<'_>,
    identified_items: &mut Vec<ContentIndex>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(identified_items.len());
    for item in identified_items.drain(..) {
        if let Some(new_item) = remapper.remap_droppable(item, ContentKind::Item)? {
            kept.push(new_item);
        }
    }
    *identified_items = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的技能冷却表：键（技能索引）找不到当前会话内容
/// 时整条丢弃（[`ContentKind::Skill`]，与 [`remap_unlocked_skills`] 同一
/// 判断——冷却到期时刻本身不含 `ContentIndex`，跟着键一起丢弃或保留，
/// 值不需要单独重映射）。
fn remap_skill_cooldowns(
    remapper: &mut Remapper<'_>,
    skill_cooldowns: &mut BTreeMap<ContentIndex, Tick>,
) -> Result<(), LoadError> {
    let mut kept = BTreeMap::new();
    for (skill, until) in std::mem::take(skill_cooldowns) {
        if let Some(new_skill) = remapper.remap_droppable(skill, ContentKind::Skill)? {
            kept.insert(new_skill, until);
        }
    }
    *skill_cooldowns = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的生效中临时属性修正
/// （[`ll_world::entity::Agent::active_stat_modifiers`]，六节存储改法）：
/// 外层键（[`AttributeKind`]）是引擎内置的封闭枚举，不携带
/// `ContentIndex`，原样保留；内层键是「来源」——找不到当前会话内容时
/// 整条丢弃，理由与 [`remap_skill_cooldowns`] 完全相同：目前唯一的
/// 生产者是 `resolve_use_skill`（传入被使用的技能自身的索引），走
/// [`ContentKind::Skill`] 判定丢弃是诚实的现状描述，不是提前假装
/// 「来源」已经有一个专属的 `ContentKind`——`buffs-and-triggers.md`
/// 六节①已经指出未来载具/天赋落地后会有第二、第三个生产者，届时
/// 「来源」不再只可能是技能，这里的判定需要跟着扩展（不在本批次
/// 范围内提前做）。修正本身（`delta`/`expires_at`）不含 `ContentIndex`，
/// 随键一起丢弃或保留，不需要单独重映射。
fn remap_active_stat_modifiers(
    remapper: &mut Remapper<'_>,
    active_stat_modifiers: &mut BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,
) -> Result<(), LoadError> {
    for per_source in active_stat_modifiers.values_mut() {
        let mut kept = BTreeMap::new();
        for (source, modifier) in std::mem::take(per_source) {
            if let Some(new_source) = remapper.remap_droppable(source, ContentKind::Skill)? {
                kept.insert(new_source, modifier);
            }
        }
        *per_source = kept;
    }
    Ok(())
}

/// 重映射一个 `Agent` 的副职集合：找不到当前会话内容的副职整条丢弃
/// （[`ContentKind::Subclass`]）——理由同 [`remap_unlocked_skills`]。
fn remap_subclasses(
    remapper: &mut Remapper<'_>,
    subclasses: &mut Vec<ContentIndex>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(subclasses.len());
    for subclass in subclasses.drain(..) {
        if let Some(new_subclass) = remapper.remap_droppable(subclass, ContentKind::Subclass)? {
            kept.push(new_subclass);
        }
    }
    *subclasses = kept;
    Ok(())
}

/// 重映射 [`Space`] 携带的层属性索引——两个变体都是结构性内容
/// （[`Remapper::remap_structural`]），穷尽 `match`（Rust 对枚举天然
/// 强制穷尽，新增变体会在这里编译失败，不需要额外的解构手法）。
fn remap_space(remapper: &mut Remapper<'_>, space: &mut Space) -> Result<(), LoadError> {
    match space {
        Space::Surface {
            zone: _,
            z: _,
            profile,
        } => {
            *profile = remapper.remap_structural(*profile, "地表层属性")?;
        }
        Space::Interior {
            id: _,
            floor: _,
            anchor: _,
            profile,
        } => {
            *profile = remapper.remap_structural(*profile, "室内层属性")?;
        }
    }
    Ok(())
}

/// 重映射一个 `Agent` 的目标栈：找不到当前会话内容的目标被整条丢弃
/// （[`ContentKind::Goal`]），保留的目标换成新索引。
fn remap_goals(remapper: &mut Remapper<'_>, goals: &mut Vec<Goal>) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(goals.len());
    for goal in goals.drain(..) {
        let Goal {
            kind,
            params,
            progress,
            priority,
        } = goal;
        if let Some(new_kind) = remapper.remap_droppable(kind, ContentKind::Goal)? {
            kept.push(Goal {
                kind: new_kind,
                params,
                progress,
                priority,
            });
        }
    }
    *goals = kept;
    Ok(())
}

/// 重映射一个 `Agent` 的归属列表：`OrgRef::Def`（文化/职业类）找不到
/// 当前会话内容时整条丢弃（[`ContentKind::Affiliation`]），`OrgRef::Instance`
/// 携带的是 [`ll_core::ident::WorldId`]，不依赖 mod 加载顺序，不需要
/// 重映射。
fn remap_affiliations(
    remapper: &mut Remapper<'_>,
    affiliations: &mut Vec<Affiliation>,
) -> Result<(), LoadError> {
    let mut kept = Vec::with_capacity(affiliations.len());
    for affiliation in affiliations.drain(..) {
        let Affiliation {
            kind,
            org,
            standing,
        } = affiliation;
        match org {
            OrgRef::Instance(_) => kept.push(Affiliation {
                kind,
                org,
                standing,
            }),
            OrgRef::Def(index) => {
                if let Some(new_index) =
                    remapper.remap_droppable(index, ContentKind::Affiliation)?
                {
                    kept.push(Affiliation {
                        kind,
                        org: OrgRef::Def(new_index),
                        standing,
                    });
                }
            }
        }
    }
    *affiliations = kept;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_mod::registry::Registry;
    use ll_world::entity::{AffiliationKind, Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::space::Space;
    use ll_world::terrain::materialize_base_terrain;
    use ll_world::zone::ZoneLayout;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 建一个带真实地形数据的测试世界，连同「写出存档那一刻」对应的
    /// `Registry`（其 `snapshot()` 即写入头部的 `content_index_map`）。
    ///
    /// `WorldState::new` 恒会预热出生点周围的邻域（见其文档），因此
    /// 测试世界里的地形从来不是空的——`remap_world` 会真的遍历它，
    /// `content_index_map` 必须覆盖地形用到的全部索引，不能像早期版本
    /// 那样只塞一两条角色属性相关的字符串就假装够用了。
    fn test_world_with_save_registry() -> (WorldState, Registry) {
        let mut registry = Registry::new();
        let (terrain_ids, terrain_table) = materialize_base_terrain(&mut |id| registry.intern(id))
            .expect("本体地形声明表内部一致，注册恒不失败");
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let spawn = layout.tile_size().wrap(0, 0);
        let world = WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");
        (world, registry)
    }

    /// 建一张与 [`test_world_with_save_registry`] 地形内容逐字符串一致
    /// 的「当前会话」注册表——地形部分按同一个声明表函数重新登记（顺序
    /// 恒定，但这不影响测试意图：这些测试的重点是角色属性/目标/归属
    /// 是否按字符串对号，不是地形本身），供各测试在此基础上叠加各自
    /// 需要的角色内容。
    fn current_session_registry_with_terrain() -> Registry {
        let mut registry = Registry::new();
        materialize_base_terrain(&mut |id| registry.intern(id))
            .expect("本体地形声明表内部一致，注册恒不失败");
        registry
    }

    fn bare_agent(pos_zone: ll_world::space::ZoneCoord) -> Agent {
        Agent {
            // 性别：测试夹具/示例里的角色不经角色创建界面，取默认占位值。
            gender: ll_world::entity::Gender::default(),
            pos: TorusSize::new(64, 64).expect("合法尺寸").wrap(1, 1),
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(pos_zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        }
    }

    #[test]
    fn 存在于当前registry的角色属性重映射后指向同一个字符串标识的新索引() {
        // Arrange：存档写出时 registry 的登记顺序与当前会话不同——
        // 「旧索引」与「新索引」因此不相等，重映射必须靠字符串而不是
        // 靠索引数值本身对上号。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let farmer_old = save_registry.intern(id("lostland:farmer"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,让顺序与写出时不同
        let farmer_new = current.intern(id("lostland:farmer"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.profession = farmer_old;
        let player_id = world.actors.spawn(agent);
        world.player_entity = Some(player_id);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(player_id)
                .expect("实体应当仍存在")
                .profession,
            farmer_new
        );
    }

    #[test]
    fn creature_kind有值时按角色属性同一路径重映射() {
        // creature_kind 与 profession/race 走同一条 remap_character_attribute
        // 路径（见 remap_creature_kind 文档），这里独立验证：存档写出时
        // 与当前会话的登记顺序不同,重映射后仍能靠字符串对号找到新索引。
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let goblin_old = save_registry.intern(id("lostland:goblin"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:wolf")); // 抢先登记,打乱顺序
        let goblin_new = current.intern(id("lostland:goblin"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.creature_kind = Some(goblin_old);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .creature_kind,
            Some(goblin_new)
        );
    }

    #[test]
    fn creature_kind为none时重映射不做任何处理() {
        // 绝大多数「有种族意义」的智慧类人型不设置这个字段——None 必须
        // 原样保留,不应该被重映射逻辑意外改写成 Some。
        // Arrange
        let (mut world, save_registry) = test_world_with_save_registry();
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();
        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let agent = bare_agent(zone);
        let entity = world.actors.spawn(agent);

        // Act
        remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .creature_kind,
            None
        );
    }

    #[test]
    fn 漏掉一处索引字段会被本模块的穷尽解构在编译期挡住() {
        // 这条测试本身无法用运行期断言证明"编译期挡住"——它记录的是
        // 一个已经发生过的事实核查：本文件 remap_agent/顶层 WorldState
        // 解构都不带 `..`,曾经尝试临时删掉其中一个字段名（例如
        // `ref mut race`）在本地编译,确认 cargo build 会在这条解构处
        // 报 "pattern does not mention field `race`" 而不是静默通过。
        // 该手工核查步骤记入任务报告,不适合写成自动化测试(它的失败
        // 模式是编译失败,不是断言失败)。这里改为运行期验证一条更弱但
        // 可自动化的性质:一次成功的重映射确实覆盖了 profession 与
        // race 两个字段,而不是只覆盖其中一个。
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let a_old = save_registry.intern(id("lostland:a"));
        let b_old = save_registry.intern(id("lostland:b"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        let a_new = current.intern(id("lostland:a"));
        let b_new = current.intern(id("lostland:b"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.profession = a_old;
        agent.race = b_old;
        let id_ = world.actors.spawn(agent);

        // Act
        remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：两个字段都被真正换过,不是只改了其中一个。
        let restored = world.actors.get(id_).expect("实体应当仍存在");
        assert_eq!(restored.profession, a_new);
        assert_eq!(restored.race, b_new);
    }

    #[test]
    fn 找不到当前会话内容的目标被丢弃且记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished_kind = save_registry.intern(id("lostland:vanished"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        // 当前会话有地形内容,但从未登记过 "lostland:vanished"。
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.goals.push(Goal {
            kind: vanished_kind,
            params: Vec::new(),
            progress: 0,
            priority: 0,
        });
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .goals
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 属性修正的来源存在于当前会话时重映射后指向同一字符串标识的新索引() {
        // buffs-and-triggers.md 六节存储改法：active_stat_modifiers 内层
        // 键（来源）现在携带 ContentIndex，必须像 profession/race 一样
        // 靠字符串标识对号，不能假设存档写出与当前会话的索引数值相同。
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let brace_old = save_registry.intern(id("lostland:brace"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let brace_new = current.intern(id("lostland:brace"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.active_stat_modifiers.insert(
            AttributeKind::Constitution,
            BTreeMap::from([(
                brace_old,
                ActiveStatModifier {
                    delta: 3,
                    expires_at: Tick(80),
                },
            )]),
        );
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：修正的数值（delta/expires_at）原样保留,键从旧索引换成
        // 了与当前会话字符串标识对应的新索引。
        assert!(actions.is_empty());
        let stored = world
            .actors
            .get(entity)
            .expect("实体应当仍存在")
            .active_stat_modifiers
            .get(&AttributeKind::Constitution)
            .and_then(|per_source| per_source.get(&brace_new));
        assert_eq!(
            stored,
            Some(&ActiveStatModifier {
                delta: 3,
                expires_at: Tick(80),
            })
        );
    }

    #[test]
    fn 换mod读档后潜行状态不被静默清空() {
        // 直接针对本文件历史上真实出过的那次事故（`active_stat_modifiers`
        // 曾在穷尽解构里被写成 `field: _` 形态,换 mod 读档时静默清空,
        // 见模块文档「完整性如何保证」）——`Agent::stealthed` 是本批次
        // 新增的字段,它在 `remap_agent` 里同样落在 `stealthed: _,` 这个
        // 形状上（因为它确实不携带任何 ContentIndex）,但「确认过不需要
        // 重映射」与「顺手吞掉」在源码上长得一模一样,只能靠这条测试
        // 把两者区分开：真的被吞掉的话,下面这条断言会变红。
        // Arrange：一个正在潜行的实体，外加一次真实的索引重映射
        // （职业/种族确实换了号，走的不是"什么都没发生"那条捷径）。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        save_registry.intern(id("lostland:filler"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.stealthed = true;
        let entity = world.actors.spawn(agent);

        // Act
        remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world.actors.get(entity).expect("实体应当仍存在").stealthed,
            "潜行状态在重映射后被清空了——remap_agent 的穷尽解构把它吞掉了"
        );
    }

    #[test]
    fn 属性修正的来源在当前会话找不到时整条丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished_source = save_registry.intern(id("lostland:vanished"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        // 当前会话有地形内容,但从未登记过 "lostland:vanished"。
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.active_stat_modifiers.insert(
            AttributeKind::Strength,
            BTreeMap::from([(
                vanished_source,
                ActiveStatModifier {
                    delta: 5,
                    expires_at: Tick(50),
                },
            )]),
        );
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：这一项属性上的来源表变空（不是整个外层键被移除，
        // remap_active_stat_modifiers 只清空内层 map），且记录了警告。
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .active_stat_modifiers
                .get(&AttributeKind::Strength)
                .expect("外层键本身不会被移除")
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 玩家角色属性找不到当前会话内容且无占位时产生reject决策() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished_race = save_registry.intern(id("lostland:vanished_race"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.race = vanished_race;
        let player_id = world.actors.spawn(agent);
        world.player_entity = Some(player_id);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert_eq!(actions, vec![DegradeAction::Reject]);
    }

    #[test]
    fn 地形内容在当前会话找不到时判定为存档损坏() {
        // 结构性内容（地形）没有可用的降级语义,直接报 Corrupted——
        // 见 Remapper::remap_structural 文档。这里构造一个
        // content_index_map,里面的字符串在当前 registry 完全不存在。
        // Arrange：content_index_map 留空——世界里真实存在的地形无论
        // 用到哪个索引都必然超出这个空表的范围,直接命中「存档已经不
        // 自洽」这一类错误。
        let (mut world, _save_registry) = test_world_with_save_registry();
        let old_ids: Vec<String> = Vec::new();
        let current = Registry::new();

        // Act
        let result = remap_world(&mut world, &old_ids, &current, None);

        // Assert
        assert!(matches!(result, Err(LoadError::Corrupted(_))));
    }

    #[test]
    fn 击杀计数按字符串对号重映射到新索引() {
        // Arrange：存档写出时与当前会话的登记顺序不同——重映射必须靠
        // 字符串而不是靠索引数值巧合对上号,与 profession/race 同一条
        // 判据。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let goblin_old = save_registry.intern(id("lostland:goblin"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        world.kill_counts.insert(goblin_old, 47);

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:wolf")); // 抢先登记,打乱顺序
        let goblin_new = current.intern(id("lostland:goblin"));

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        assert_eq!(world.kill_counts.get(&goblin_new), Some(&47));
    }

    #[test]
    fn 击杀计数对应的内容在当前会话找不到时整桶丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        world.kill_counts.insert(vanished, 3);
        let current = current_session_registry_with_terrain();

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(world.kill_counts.is_empty());
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 资源池键按字符串对号重映射到新索引() {
        // Arrange：存档写出时与当前会话的登记顺序不同——与 profession/
        // race 同一条判据,重映射必须靠字符串而不是索引数值巧合对上号。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let pool_old = save_registry.intern(id("lostland:sorcery_points"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let pool_new = current.intern(id("lostland:sorcery_points"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.resource_pools.insert(pool_old, 12);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：键换成了新索引,值（当前量）原样保留。
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .resource_pools
                .get(&pool_new),
            Some(&12)
        );
    }

    #[test]
    fn 资源池键对应的内容在当前会话找不到时整条丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished_pool = save_registry.intern(id("lostland:vanished_pool"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.resource_pools.insert(vanished_pool, 7);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .resource_pools
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 法术位已消耗数键按字符串对号重映射到新索引() {
        // Arrange：与资源池键同一条判据——重映射必须靠字符串而不是索引
        // 数值巧合对上号，见「资源池键按字符串对号重映射到新索引」。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let pool_old = save_registry.intern(id("lostland:wizard_slots"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let pool_new = current.intern(id("lostland:wizard_slots"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.spent_slots.insert((pool_old, 3), 1);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：键的池索引部分换成了新索引,档位与已消耗数原样保留。
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .spent_slots
                .get(&(pool_new, 3)),
            Some(&1)
        );
    }

    #[test]
    fn 法术位已消耗数键对应的内容在当前会话找不到时整条丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished_pool = save_registry.intern(id("lostland:vanished_pool"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.spent_slots.insert((vanished_pool, 1), 2);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .spent_slots
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 归属类型不同的两个测试世界依旧各自独立不互相污染() {
        // AffiliationKind 未在本模块其余测试用到,这里补一条最小覆盖,
        // 确认 remap_affiliations 对 OrgRef::Instance 不做任何重映射
        // （不消费 content_index_map,不产生任何 DegradeAction）。
        // Arrange
        let (mut world, save_registry) = test_world_with_save_registry();
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();
        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.affiliations.push(Affiliation {
            kind: AffiliationKind::Faction,
            org: OrgRef::Instance(ll_core::ident::WorldId::next(&mut 0)),
            standing: 0,
        });
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .affiliations
                .len(),
            1
        );
    }

    #[test]
    fn 背包物品按字符串对号重映射到新索引() {
        // Arrange：存档写出时与当前会话的登记顺序不同——与 profession/
        // race 同一条判据，重映射必须靠字符串而不是索引数值巧合对上号。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let arrow_old = save_registry.intern(id("lostland:arrow"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let arrow_new = current.intern(id("lostland:arrow"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.inventory.push(ItemStack::new(arrow_old, 30));
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：def 换成了新索引,数量原样保留。
        assert!(actions.is_empty());
        assert_eq!(
            world.actors.get(entity).expect("实体应当仍存在").inventory,
            vec![ItemStack::new(arrow_new, 30)]
        );
    }

    #[test]
    fn 背包物品对应的内容在当前会话找不到时整堆丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.inventory.push(ItemStack::new(vanished, 1));
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .inventory
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 装备物品按字符串对号重映射到新索引() {
        // Arrange：与背包物品同一条判据——见「背包物品按字符串对号
        // 重映射到新索引」（装备栏位批次，P6 第三批）。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let sword_old = save_registry.intern(id("lostland:iron_sword"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let sword_new = current.intern(id("lostland:iron_sword"));

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.equipment.insert(
            ll_world::item::EquipSlot::MAIN_HAND,
            ItemStack::with_durability(sword_old, 1, 90),
        );
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：def 换成了新索引,槽位键与耐久原样保留。
        assert!(actions.is_empty());
        assert_eq!(
            world.actors.get(entity).expect("实体应当仍存在").equipment,
            BTreeMap::from([(
                ll_world::item::EquipSlot::MAIN_HAND,
                ItemStack::with_durability(sword_new, 1, 90)
            )])
        );
    }

    #[test]
    fn 装备物品对应的内容在当前会话找不到时整槽丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent
            .equipment
            .insert(ll_world::item::EquipSlot::HEAD, ItemStack::new(vanished, 1));
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .equipment
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 已知配方按字符串对号重映射到新索引() {
        // Arrange：配方发现批次新增的 Agent::known_recipes——与已解锁
        // 技能同一条判据。这条守的是本文件模块文档「完整性如何保证」
        // 记的那次真实事故（active_stat_modifiers 被写成 `field: _`，
        // 换 mod 读档时静默清空）在新字段上的重演：若 remap_agent 的
        // 穷尽解构把 known_recipes 吞成 `_`，本测试会拿到旧索引而红。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let stew_old = save_registry.intern(id("lostland:herb_stew_recipe"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记，打乱顺序
        let stew_new = current.intern(id("lostland:herb_stew_recipe"));
        assert_ne!(
            stew_old, stew_new,
            "两边索引必须真的不同，否则这条测试等于没测"
        );

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.known_recipes.push(stew_old);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .known_recipes,
            vec![stew_new]
        );
    }

    #[test]
    fn 曾经获得过的副职账本按字符串对号重映射到新索引() {
        // Arrange：副职发点批次新增的 Agent::subclasses_ever_granted
        // ——与 subclasses/known_recipes/identified_items 同一条判据、
        // 同一种事故防线，但后果比它们都重：若 remap_agent 的穷尽解构
        // 把它吞成 `subclasses_ever_granted: _`（静默清空），玩家换一
        // 次 mod 读档之后全部副职的点数都能再领一遍——那正是这个字段
        // 被造出来要防的事。本测试刻意让新旧索引不同，吞掉就会红。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let artisan_old = save_registry.intern(id("lostland:artisan"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记，打乱顺序
        let artisan_new = current.intern(id("lostland:artisan"));
        assert_ne!(
            artisan_old, artisan_new,
            "两边索引必须真的不同，否则这条测试等于没测"
        );

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        // **刻意只写账本、不写 subclasses**：这正是「曾经获得过、后来
        // 放弃了」的真实形状，同时保证本测试测的是账本自己那一次
        // remap 调用，而不是碰巧被 subclasses 那一次覆盖到。
        agent.subclasses_ever_granted.push(artisan_old);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        let after = world.actors.get(entity).expect("实体应当仍存在");
        assert_eq!(after.subclasses_ever_granted, vec![artisan_new]);
        assert!(
            after.subclasses.is_empty(),
            "账本与当前持有是两个独立字段，重映射不该把其中一份灌进另一份"
        );
    }

    #[test]
    fn 曾经获得过的副职在当前会话找不到时整条丢弃并记录droppwithwarning() {
        // Arrange：卸掉一个 mod 之后那个副职不存在了——与
        // remap_subclasses 对 subclasses 的处置完全相同（按
        // ContentKind::Subclass 丢弃并警告，不是拒绝整个存档）。
        //
        // 这条丢弃有一个如实标注的后果：那个 mod 装回来之后，它的副职
        // 可以再发一次点。见 Agent::subclasses_ever_granted 文档
        // 「一处如实标注的边界」——代价有界，且与 remap_subclasses 自身
        // 「内容变更不追溯」的既有语义一致。本测试把这条语义钉下来，
        // 免得将来有人以为它是缺陷而顺手改成 Reject。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished_subclass"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.subclasses_ever_granted.push(vanished);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .subclasses_ever_granted
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 已鉴定物品种类按字符串对号重映射到新索引() {
        // Arrange：未鉴定物品批次新增的 Agent::identified_items——与
        // known_recipes 同一条判据、同一种事故防线。若 remap_agent 的
        // 穷尽解构把它吞成 `identified_items: _`，本测试会拿到旧索引
        // 而红（那正是本文件模块文档「完整性如何保证」记的那次真实
        // 事故的形态）。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let pendant_old = save_registry.intern(id("lostland:amber_pendant"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记，打乱顺序
        let pendant_new = current.intern(id("lostland:amber_pendant"));
        assert_ne!(
            pendant_old, pendant_new,
            "两边索引必须真的不同，否则这条测试等于没测"
        );

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.identified_items.push(pendant_old);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(actions.is_empty());
        assert_eq!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .identified_items,
            vec![pendant_new]
        );
    }

    #[test]
    fn 已鉴定物品在当前会话找不到时整条丢弃并记录droppwithwarning() {
        // Arrange：换掉一个 mod 之后那件物品不存在了——按
        // ContentKind::Item 丢弃并警告，不是拒绝整个存档：认不认得一种
        // 东西不是角色的核心身份，理由见 remap_identified_items 文档。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished_trinket"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.identified_items.push(vanished);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .identified_items
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 已知配方对应的内容在当前会话找不到时整条丢弃并记录droppwithwarning() {
        // Arrange：换掉一个 mod 之后那条配方不存在了——按
        // ContentKind::Recipe 无条件丢弃并警告（degrade.rs 的
        // decide_degrade_action），不是拒绝整个存档：知不知道一张图纸
        // 不是角色的核心身份。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished_recipe"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        let zone = world.terrain.layout().tile_to_zone(world.size.wrap(1, 1)).0;
        let mut agent = bare_agent(zone);
        agent.known_recipes.push(vanished);
        let entity = world.actors.spawn(agent);

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(
            world
                .actors
                .get(entity)
                .expect("实体应当仍存在")
                .known_recipes
                .is_empty()
        );
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }

    #[test]
    fn 地面物品按字符串对号重映射到新索引() {
        // Arrange：与背包物品同一条判据——见「背包物品按字符串对号
        // 重映射到新索引」。
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let sword_old = save_registry.intern(id("lostland:iron_sword"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let sword_new = current.intern(id("lostland:iron_sword"));

        let pos = world.size.wrap(3, 4);
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::with_durability(sword_old, 1, 90),
            dropped_at: Tick(50),
            contents: Vec::new(),
            placed: false,
        });

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：def 换成了新索引,位置与丢弃时刻原样保留。
        assert!(actions.is_empty());
        assert_eq!(
            world.ground_items,
            vec![ll_world::item::GroundItemStack {
                pos,
                stack: ItemStack::with_durability(sword_new, 1, 90),
                dropped_at: Tick(50),
                contents: Vec::new(),
                placed: false,
            }]
        );
    }

    #[test]
    fn 尸体内容物按字符串对号重映射到新索引() {
        // NPC 死亡掉落批次：GroundItemStack::contents 里的每一条
        // ItemStack.def 与 stack.def 同样依赖 mod 加载顺序，必须一并
        // 重映射——否则读档后尸体里装的物品会静默指向错误的内容,与
        // 「地面物品按字符串对号重映射到新索引」同一条判据,只是这次
        // 覆盖的是容器内容物这一层。
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let corpse_old = save_registry.intern(id("lostland:goblin"));
        let sword_old = save_registry.intern(id("lostland:iron_sword"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        current.intern(id("lostland:miner")); // 抢先登记,打乱顺序
        let corpse_new = current.intern(id("lostland:goblin"));
        let sword_new = current.intern(id("lostland:iron_sword"));

        let pos = world.size.wrap(3, 4);
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(corpse_old, 1),
            dropped_at: Tick(50),
            contents: vec![ItemStack::new(sword_old, 1)],
            placed: false,
        });

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：容器本身与内容物的 def 都换成了新索引。
        assert!(actions.is_empty());
        assert_eq!(
            world.ground_items,
            vec![ll_world::item::GroundItemStack {
                pos,
                stack: ItemStack::new(corpse_new, 1),
                dropped_at: Tick(50),
                contents: vec![ItemStack::new(sword_new, 1)],
                placed: false,
            }]
        );
    }

    #[test]
    fn 尸体内容物里找不到的物品被单独丢弃而尸体本身保留() {
        // 与背包/地面物品「整堆丢弃」不同：容器内容物是一个列表,列表
        // 内某一条找不到当前会话内容时,只丢弃那一条,容器（尸体）本身
        // 与其余内容物原样保留——不应该因为死者身上一件已卸载 mod 的
        // 物品就让整具尸体连同其余战利品一起消失。
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let corpse_old = save_registry.intern(id("lostland:goblin"));
        let vanished = save_registry.intern(id("lostland:vanished"));
        let sword_old = save_registry.intern(id("lostland:iron_sword"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();

        let mut current = current_session_registry_with_terrain();
        let corpse_new = current.intern(id("lostland:goblin"));
        let sword_new = current.intern(id("lostland:iron_sword"));

        let pos = world.size.wrap(0, 0);
        world.ground_items.push(ll_world::item::GroundItemStack {
            pos,
            stack: ItemStack::new(corpse_old, 1),
            dropped_at: Tick(0),
            contents: vec![ItemStack::new(vanished, 1), ItemStack::new(sword_old, 1)],
            placed: false,
        });

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert：尸体保留,内容物只剩下能重映射的那一条。
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
        assert_eq!(
            world.ground_items,
            vec![ll_world::item::GroundItemStack {
                pos,
                stack: ItemStack::new(corpse_new, 1),
                dropped_at: Tick(0),
                contents: vec![ItemStack::new(sword_new, 1)],
                placed: false,
            }]
        );
    }

    #[test]
    fn 地面物品对应的内容在当前会话找不到时整堆丢弃并记录droppwithwarning() {
        // Arrange
        let (mut world, mut save_registry) = test_world_with_save_registry();
        let vanished = save_registry.intern(id("lostland:vanished"));
        let old_ids: Vec<String> = save_registry
            .snapshot()
            .iter()
            .map(ToString::to_string)
            .collect();
        let current = current_session_registry_with_terrain();

        world.ground_items.push(ll_world::item::GroundItemStack {
            pos: world.size.wrap(0, 0),
            stack: ItemStack::new(vanished, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed: false,
        });

        // Act
        let actions = remap_world(&mut world, &old_ids, &current, None).expect("应当成功");

        // Assert
        assert!(world.ground_items.is_empty());
        assert_eq!(actions, vec![DegradeAction::DropWithWarning]);
    }
}
