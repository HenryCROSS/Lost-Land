//! 厚层实体：被真正模拟的 `Agent`。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, WorldId};
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::{Deserialize, Serialize};

use crate::item::{EquipSlot, ItemStack};
use crate::script_state::ScriptValue;
use crate::space::Space;

use super::{ActiveStatModifier, Affiliation, AttributeKind, BaseStats, Goal};

/// 一段正在进行的休息会话（`knowledge/design/resource-pools-and-rest.md`
/// 七、八节）——`Agent::resting` 唯一的载荷类型。
///
/// # 为什么不区分短休/长休
///
/// 设计文档八节裁定「不设两档，只有一种休息动作」：`target_ticks` 由
/// 发起时的 `Intent::Rest` 自带,力度差异（回满/回固定量）已经在
/// `RegenRule::OnRest` 这一层给出,不需要在动作本身再叠一层分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestState {
    /// 这段休息开始时的世界时刻。
    pub started_at: Tick,
    /// 目标持续的 tick 数——`world.clock + 本次行动耗时 >= started_at +
    /// target_ticks` 时视为「正常完成」，判定逻辑在 `ll-sim` 侧的
    /// `resolve_wait`（本 crate 不依赖 `ll-sim`，这里只声明数据形状）。
    pub target_ticks: u32,
}

/// 厚层实体：数百个，有界，被真正模拟。
///
/// 行式排布（AoS）是刻意的：数量少、按实体随机访问、一次要读它的全部
/// 字段，行式排布比列式更优——与薄层 [`crate::entity::ThinPopulation`]
/// 用列式排布不是不一致，是各自匹配访问模式（见该类型文档）。
///
/// # 可派生 `serde`（P5 批次 B 补齐，偿还两处历史债务）
///
/// 曾经不派生 `serde`：`pos` 是 [`TorusPos`]、`profession`/`race` 是
/// [`ContentIndex`]，两者当时在 `ll-core` 里都没有可直接使用的序列化
/// 实现。两处障碍现在都已解除：
///
/// 1. `TorusPos` 已在两级坐标系重写批次补上不依赖世界尺寸上下文的直接
///    `Serialize`/`Deserialize`（`ll-core/src/torus.rs`）——它本身没有
///    需要跨字段校验的不变式（任意一对 `(x, y)` 只要落在构造它的
///    `TorusSize` 环内就是合法值，反序列化只做结构转换）。
/// 2. `ContentIndex` 同样直接派生（[0015](../../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)）：
///    「结构合法」与「已注册」是两件事，前者无上下文、可以直接派生；
///    后者依赖当前会话加载了哪些 mod，是一次独立的解析，不能也不该
///    塞进 serde 的 `try_from`——`ll_core::ident` 模块文档「为什么可以
///    直接派生」一节详述了这条判断。反序列化出的 `ContentIndex` 是否
///    对应当前会话里一条真实注册的内容，由拿到注册表之后的调用方
///    （存档主体读写管线，见任务 9）显式解析，解析失败即是「缺失 mod」
///    的检测点——不是本类型自身要负责的事。
///
/// 因此 `Agent` 现在可以直接派生：全部字段要么是纯整数/字符串组合
/// （`health`/`wallet`/`Vec<Affiliation>`/`Vec<Goal>`），要么是
/// 已经各自补齐直接派生的复合类型（`TorusPos`/`BaseStats`/
/// `ContentIndex`/`Space`）。
///
/// # 为什么 `health` 是 `Agent` 的字段，而不是 `WorldState` 的旁挂表
///
/// 生命值和 `pos`/`stats`/`wallet` 一样，是逐实体状态，天然属于
/// `Agent`——厚层行式存储本就是为「一次要读某个实体的全部字段」这种
/// 访问模式而设计的（见本类型文档开篇）。曾经有一版把它做成
/// `WorldState` 上的 `BTreeMap<EntityId, i32>` 旁挂表，图的是不改
/// `Agent` 既有布局；但这引出两个问题：
///
/// 1. 实体状态从此有两个存放地（`Agent` 本体 + 旁挂表），后续任何新增
///    字段都要面对「这个该放哪」的选择，维护者必须记住这条历史包袱。
/// 2. 旁挂表不受 [`super::Arena`] 的世代号管辖：实体被 `despawn` 之后，
///    `Arena` 里对应的槽位立刻变成 `Vacant`，但旁挂表里的条目不会跟着
///    消失，除非另外手动清理——一旦某次 `Effect` 忘了同步删除，旁挂表
///    就会攒下一批指向不存在实体的孤儿记录。这不是假设的风险：这正是
///    上一批实现里 `WorldState::health` 字段被发现的真实缺陷（该字段
///    存档可见但 `Arena` 的生死不参与序列化，往返后孤儿记录会一直
///    累积）。把生命值做成 `Agent` 的字段后，这类记录随 `Agent` 一起
///    被 `Arena::despawn` 整体收走，物理上不可能出现孤儿。
///
/// **如果日后又想把它挪回 `WorldState` 的旁挂表**——不要。以上两条
/// 理由没有过期；旁挂表能做的事，`Agent` 字段都能做，且不引入孤儿
/// 记录的风险。
///
/// # 为什么只存「当前值」，不存「上限」
///
/// 生命上限由体质（`stats.constitution`）驱动，是衍生属性（见
/// `knowledge/design/attribute-system.md` 「衍生属性绝不进存档」
/// 一节）：完整的 `derive_stats` 公式属于战斗结算批次，本批次不提前
/// 设计它。若现在就把上限存成字段，`stats` 一旦变化（升级、装备、
/// buff）就必须记得同步这个字段，否则两者不同步——这正是该原则要
/// 防的缺陷类别。因此这里只留「当前生命值」这一个存档需要的字段；
/// 上限留到 `derive_stats` 落地时按 `stats` 现算，不占字段、不需要
/// 同步。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    /// 环面世界坐标。
    pub pos: TorusPos,
    /// 六项主属性 + 幸运——幸运并入 `AttributeKind` 批次之前，幸运是
    /// `Agent` 上的独立字段（`Agent::luck`），现已并入 [`BaseStats`]，
    /// 见其字段 [`BaseStats::luck`] 文档「为什么现在并入 `BaseStats`」
    /// 一节：并入后幸运才能被 [`Self::active_stat_modifiers`]（临时
    /// 效果）与装备静态加成（`ll_sim::item::StatBonus`）影响。
    pub stats: BaseStats,
    /// 下次行动的世界时刻，时间轴排序依据。
    pub next_action_at: Tick,
    /// 当前生命值。可以为零或负——是否算「死亡」是规则判断（`resolve`
    /// 的职责），`apply` 只管照 `Effect::Damage` 的数字做加减，不在这
    /// 个字段本身设下限。没有独立的「上限」字段，见本类型文档「为什么
    /// 只存当前值」一节。
    pub health: i32,

    // ↓ 以下六个字段 P3 可以留空，但字段必须现在就有——见
    // `knowledge/design/society-and-affiliation.md` 第五节与
    // `knowledge/design/agent-goals-and-economy.md` 第九节：存档格式在
    // P5 冻结，P3 加是零成本，P8 加要写迁移链。
    /// 归属列表（势力/宗教/行会/文化/家族/职业）。
    pub affiliations: Vec<Affiliation>,
    /// 钱包，最小货币单位。厚层直接存值，不像薄层那样走公式。
    pub wallet: i64,
    /// 当前主职业，指向注册表。
    pub profession: ContentIndex,
    /// 目标栈。
    pub goals: Vec<Goal>,
    /// 种族，指向注册表。
    ///
    /// 与 `profession` 同样的模式：种族是内容（mod 可以注册新种族），
    /// 因此用 [`ContentIndex`] 而不是一个封闭的本体枚举——`fireball`
    /// 那类命名冲突问题（见 `ll_core::ident` 模块文档）在种族上同样
    /// 存在，不该单开一套不经注册表的表示法。
    ///
    /// 未来将驱动的东西：基础属性的种族修正（矮人体质高、精灵敏捷高
    /// 这类刻板但好用的差异化）、种族专属的可穿装备槽位（见
    /// `knowledge/design/equipment-slots.md`）、随从招募与关系系统里
    /// 「同族/异族」这层筛选、以及部分剧情/势力对话分支的解锁条件。
    /// P3 阶段不消费这个字段，只建布局。
    pub race: ContentIndex,
    /// 当前所在的空间（任务 12：两级坐标系重写）。
    ///
    /// 默认（新生成的实体）是 `Space::Surface`——地表是唯一不需要「先
    /// 存在一个具体实例」就能站上去的空间，`Interior` 必须先经由
    /// `InteriorTable` 插入一个实例才谈得上「进入哪一个」。
    ///
    /// # 为什么不是「`pos` 之外的第二份位置真相源」
    ///
    /// `pos: TorusPos` 恒是这个实体在世界地图上的坐标，不因为进入
    /// `Interior` 而改变（设计文档「内部移动不改变世界地图坐标」，见
    /// [`crate::interior`] 模块文档）——`Interior` 内部的移动是另一个
    /// 坐标系（[`ll_core::bounded::BoundedPos`]），本批次不引入对应的
    /// 「楼层内位置」字段，见 `ll-sim::resolve` 模块文档「`Interior`
    /// 内部移动的范围边界」一节：本批次只接线进出，不接线内部漫游。
    /// `current_space` 因此只回答「这个实体此刻应该用哪一套地形/FOV/
    /// 相机」，不重复记录位置。
    ///
    /// # 唯一写入口仍是 `apply`（C1）
    ///
    /// 与 `pos` 一样，这个字段只能通过
    /// `ll_sim::apply::apply` 对 `Effect::ChangeSpace` 的响应写入,不得
    /// 在渲染/输入层直接赋值。
    pub current_space: Space,
    /// 当前法力值。P5-B 任务 5 新增——`attribute-system.md`「六、次级
    /// 属性」把法力/耐力列为「尚未落地」的次级属性，本字段只补上技能
    /// 资源消耗判定（`ll_sim::resolve` 的 `Intent::UseSkill` 分支）需要
    /// 的「当前值」这一半，不是完整的次级属性系统（上限、随体质/智力
    /// 衍生的公式仍然属于后续批次，与 [`Self::health`] 「只存当前值，
    /// 不存上限」同一条纪律，见该字段文档）。
    pub mana: i32,
    /// 当前耐力值。理由与 [`Self::mana`] 完全对称——见其文档。
    pub stamina: i32,
    /// 开放注册的资源池当前值（法力池/未来的「气」「怒气」……），键是
    /// 指向 `ResourcePoolDef`（`knowledge/design/resource-pools-and-rest.md`
    /// 二节）的 [`ContentIndex`]，值是绝对量（不是偏差）——与
    /// [`Self::mana`]/[`Self::stamina`] 同属「只存当前值」的既有纪律，
    /// 但走开放注册表而不是封闭枚举，mod 可以声明任意新的标量池而不
    /// 需要引擎新增字段。**不是 `mana`/`stamina` 的替代**：两条通道
    /// 刻意并存（见 `ll_sim::skill::ResourceCost` 文档「为什么新增
    /// `PoolAmount` 而不是就地改造 `Amount`」一节）——`mana`/`stamina`
    /// 服务既有 `register-skill` 的 `"mana"`/`"stamina"` 内置资源种类，
    /// 本字段服务经 `register-resource-pool` 注册的开放池。
    ///
    /// **容量（上限）不存在这里**：容量由天赋按等级现算
    /// （`TraitDef.granted_resource_pools`/`CapacityFormula`），本字段
    /// 只记「当前离零还差多少」这个真实偏差，与 `health`「只存当前值
    /// 不存上限」是同一条纪律的又一次复用。查不到某个池的键时，当前
    /// 值视为 `0`（从未获得过、或从未变动过）——不需要在角色获得一个
    /// 新天赋的那一刻就急着写入一条 `0` 值占位。
    ///
    /// `BTreeMap` 不是 `HashMap`：约束 C5，`WorldState::hash()` 需要
    /// 按确定顺序遍历这个字段。
    pub resource_pools: BTreeMap<ContentIndex, i32>,
    /// 法术位（`ResourcePoolShape::TieredSlots`）各档已消耗数——键是
    /// `(池索引, 档位)`，档位从 1 起（`resource-pools-and-rest.md` 二节
    /// 「索引 0 = 第 1 档」是 `CapacityValue::Tiered` 内部 `Vec` 的约定，
    /// 本字段与查询接口一律用 1 起的档位号，两者不是同一个数轴，见
    /// `ll_sim::resource_pool::effective_slot_tier_capacity` 文档）。
    ///
    /// 与 [`Self::resource_pools`] 同一条「只存偏差，不存上限」纪律，但
    /// 偏差的方向相反：标量池存「当前还剩多少」，本字段存「已经花掉
    /// 多少」——法术位的消耗算法是「在有序档位里找空位」，天然需要知道
    /// 「这一档已经占了几个」而不是「这一档还剩几个」，容量（上限）
    /// 同样由天赋按等级现算，不存这里。查不到某个 `(池, 档位)` 键时，
    /// 已消耗数视为 `0`（从未消耗过）。
    ///
    /// `BTreeMap` 不是 `HashMap`：约束 C5，`WorldState::hash()` 需要按
    /// 确定顺序遍历这个字段——`(ContentIndex, u8)` 元组已实现 `Ord`
    /// （字典序：先比池索引，再比档位），键序天然确定。
    ///
    /// **序列化走 [`spent_slots_serde`]**——理由同
    /// [`Self::script_state`]（JSON 等文本格式要求 map 键是字符串，元组
    /// 键不满足，见 `crate::script_state::serde_map` 模块文档）。
    #[serde(with = "spent_slots_serde")]
    pub spent_slots: BTreeMap<(ContentIndex, u8), u32>,
    /// 正在进行的休息会话，`None` 表示当前未在休息
    /// （`knowledge/design/resource-pools-and-rest.md` 七、八节）。
    ///
    /// # 为什么只存「何时开始、目标时长」，不存「已经过了多久」
    ///
    /// 与 [`Self::skill_cooldowns`] 同一条惰性判定纪律：`resolve_wait`
    /// 每次结算时现比较 `world.clock + 本次行动耗时` 与
    /// `started_at.0 + target_ticks`，不需要每个 tick 主动递减一个「剩余
    /// 时长」字段——这也是八节「刷恢复漏洞」防线一成立的前提：恢复只在
    /// 这个比较判定为「已到达」的那一刻整批产出，不存在一个可以被读取
    /// 并按比例结算的中间进度值。
    pub resting: Option<RestState>,
    /// 已解锁的技能集合。P5-B 任务 5 新增，关键设计判断 2 的落点：
    /// 「解锁与否」是几乎每次技能结算都要查询的高频状态，直接归引擎层
    /// 字段，不经脚本状态存储的跨界调用开销（见
    /// `docs/superpowers/plans/2026-08-19-p5-gameplay-systems.md` 关键
    /// 设计判断 2 的完整论证）。
    ///
    /// `Vec` 不是 `BTreeSet`：查询模式是「这个 `ContentIndex` 在不在
    /// 里面」（`contains`，`O(n)`，n 是玩家已学技能数，量级不超过几十），
    /// 不需要有序遍历或去重的额外开销；写入路径（学习新技能）本身应当
    /// 保证不重复插入，不依赖容器自身去重——与 `Agent::goals`/
    /// `Agent::affiliations` 同样用 `Vec` 的理由一致。
    ///
    /// **写入路径（升级加点批次落地）**：唯一的写入口是
    /// `ll_sim::apply` 处理 `Effect::LearnSkill` 的那一条分支（约束
    /// C1），效果由 `ll_sim::resolve` 结算 `Intent::LearnSkill` 产出，
    /// 而「不重复插入」这条上文提到的责任正落在那里的第 3 道闸门
    /// （已经学过就整条静默失败，不再扣点也不再插入）。此前这个字段
    /// 只有读取者（`resolve_use_skill` 的解锁闸门、
    /// `ll_sim::skill_overview` 的技能树视图），写入路径是一处真实
    /// 缺口。
    pub unlocked_skills: Vec<ContentIndex>,
    /// 已知配方集合（配方发现批次）——`Intent::Craft` 对**声明了需要
    /// 发现**的配方（`ll_sim::craft::RecipeRule::requires_discovery`）
    /// 多加的那道闸门读的就是这个字段。
    ///
    /// # 为什么是独立字段，不复用 [`Self::unlocked_skills`]
    ///
    /// `knowledge/design/food-and-cooking-system.md` 五节已经逐条核过
    /// 这个选项并否掉了：`ContentIndex` 是贯穿全部内容类型的同一个全局
    /// 号段，机制上确实装得下一个指向配方表的索引，但**语义上是一次
    /// 静默的概念污染**——「已解锁的技能」这个名字与它现有的全部消费点
    /// （`resolve_use_skill` 的解锁闸门、`ll_sim::skill_overview` 的
    /// 技能树视图、`ll_content::remap::remap_unlocked_skills` 按
    /// `ContentKind::Skill` 做的存档降级）都假设里面装的是技能。往里塞
    /// 配方索引会让这三处各自多背一层「这条到底是技能还是配方」的判断，
    /// 而 `remap` 那一处根本无从判断——它只有一个索引，没有类型标签，
    /// 换 mod 读档时会把配方当成「查不到的技能」丢掉。独立字段没有这些
    /// 问题，代价只是一个 `Vec`。
    ///
    /// # `Vec` 不是 `BTreeSet`
    ///
    /// 与 [`Self::unlocked_skills`] 逐字同理：查询模式是「这个
    /// `ContentIndex` 在不在里面」（`contains`，`O(n)`，n 是玩家已知
    /// 配方数，量级不超过几十），不需要有序遍历；「不重复插入」由写入
    /// 路径自己保证（`ll_sim::apply` 处理
    /// `ll_sim::effect::Effect::LearnRecipe` 时先查 `contains`），不
    /// 依赖容器去重。
    ///
    /// # 唯一写入口是 `apply`（约束 C1 / ADR 0023）
    ///
    /// 与 [`Self::unlocked_skills`]/[`Self::subclasses`] 同一条纪律：
    /// 这个字段属 `WorldState`，不属 [`Self::script_state`]，因此写入
    /// 必须走一个独立的 `Effect` 变体（`Effect::LearnRecipe`）经
    /// `ll_sim::apply::apply` 落地，不能塞进 `Effect::SetScriptState`。
    /// 产出这条效果的两条发现路径见
    /// `ll_sim::resolve::resolve_read`（读书）与
    /// `ll_sim::resolve::resolve_experiment`（试做）。
    pub known_recipes: Vec<ContentIndex>,
    /// 各技能的冷却到期时刻——**到期时刻，不是「剩余时长」**（关键设计
    /// 判断 4 的惰性到期判定：存一个会随时间流逝而变得过时的「还剩多少」
    /// 需要每帧主动递减维护，存「到期于哪一刻」则只需要在真正查询「能不
    /// 能放技能」那一刻，把这个值与 `WorldState::clock` 比大小——两者
    /// 语义等价，后者不需要任何每帧维护逻辑）。
    ///
    /// `BTreeMap` 不是 `HashMap`——约束 C5：禁止 `HashMap`/`HashSet` 的
    /// 迭代顺序参与任何逻辑判断，`WorldState::hash` 需要按确定顺序遍历
    /// 这个字段（见其文档）。
    ///
    /// **有意留给后续阶段的缺口**：某个技能一旦进入过冷却，条目会一直
    /// 留在这个 `BTreeMap` 里（哪怕已经过期很久、这个技能再也没被使用
    /// 过）——本任务不主动清理，理由同惰性判定本身：清理是一次额外的
    /// 遍历成本，而「查询」路径已经能正确处理过期条目（判断 `tick <
    /// clock` 即可，不需要条目本身消失）。若未来这个字段的条目数成为
    /// 实际问题（例如某个 mod 允许一个实体学习成百上千个技能），届时
    /// 再补一次定期清理，不在本任务范围内提前做。
    pub skill_cooldowns: BTreeMap<ContentIndex, Tick>,
    /// 已持有的副职集合。P5-B 任务 5 新增，裁定 P5-4（见
    /// `knowledge/design/class-skill-quest-system.md` 第三节）：副职与
    /// 主职（[`Self::profession`]）共享同一份技能 `ContentIndex` 命名
    /// 空间，`subclasses` 本身只记录「这个实体持有哪些副职类型」，不
    /// 需要为每个副职单独维护一段隔离的技能号段。
    ///
    /// `Vec` 不是单个 `Option<ContentIndex>`：设计文档裁定允许同时持有
    /// 「至少一个」副职（复数），不是恰好一个——具体上限（若有）留给
    /// 后续内容设计批次决定，本字段的容器形状本身不设上限。
    pub subclasses: Vec<ContentIndex>,
    /// 正在生效的临时属性修正，外层按 [`AttributeKind`] 索引（匹配
    /// `effective_attribute` 的真实访问模式：一次查询要问的始终是
    /// 「这一项属性现在有哪些修正在生效」），内层按「来源」——施加这条
    /// 修正的那份内容定义自己的 [`ContentIndex`]（目前唯一的生产者是
    /// `resolve_use_skill`，传入被使用的技能索引；`buffs-and-triggers.md`
    /// 六节①已指出未来载具/天赋落地后会有第二、第三个生产者，键的类型
    /// 不需要为此改变）索引。
    ///
    /// # 同源刷新、异源叠加
    ///
    /// 项目所有者裁定「不同效果能叠加，同效果只刷新时间」——键的第二层
    /// 就是这句话里「效果」的准确定义：`(属性, 来源)` 相同视为「同一
    /// 效果再次施加」，走 [`ActiveStatModifier::merge_same_source`]；
    /// 键不同（不同属性，或同一属性但不同来源）各自独立存在、互不覆盖，
    /// 聚合时逐条过滤未过期条目再求和（见 `crates/ll-sim/src/resolve.rs`
    /// 的 `effective_attribute`）。完整论证见 `buffs-and-triggers.md`
    /// 六节；本字段的形状是该节裁定的直接落地，不是本文档重新设计的。
    ///
    /// 两层都是 `BTreeMap` 不是 `HashMap`——约束 C5：外层按
    /// [`AttributeKind`]（已实现 `Ord`）排序，内层按 [`ContentIndex`]
    /// （已实现 `Ord`）排序，不涉及任何 `HashMap`/`HashSet` 迭代顺序。
    pub active_stat_modifiers: BTreeMap<AttributeKind, BTreeMap<ContentIndex, ActiveStatModifier>>,
    /// 每实体脚本状态：依附于这个具体实体的脚本存储（NPC 当前在追的
    /// 目标、某个技能的冷却计时……），键是 `(mod_namespace, key)`，见
    /// `knowledge/design/script-state-storage.md` 二、四节。
    ///
    /// # 为什么随 `Agent` 走，不开一张 `WorldState` 旁挂表
    ///
    /// 与本类型文档「为什么 `health` 是 `Agent` 的字段」一节同一条
    /// 理由：`Agent` 死亡时 `Arena::despawn` 会把整个槽位连同这个字段
    /// 一起收走，物理上不会产生孤儿记录——若做成旁挂表，`Effect::Kill`
    /// 一旦忘了同步清理，就会重演 `WorldState::health` 曾经的孤儿记录
    /// 缺陷（见该节历史记录）。设计文档六、7 节讨论的「孤儿状态」专指
    /// **mod 被卸载**这一种情况（数据仍应保留），不是「实体死亡」这
    /// 一种（数据理应随实体一起消失）——两者是完全不同的生命周期，
    /// 这里选用 `Agent` 字段而非旁挂表，保证的正是后一种「实体死亡
    /// 即回收」不需要额外代码维护。
    ///
    /// `BTreeMap` 不是 `HashMap`：约束 C5 禁止 `HashMap`/`HashSet` 的
    /// 迭代顺序参与逻辑判断，序列化/摘要遍历需要确定顺序，见设计文档
    /// 五、1 节。**序列化走 [`crate::script_state::serde_map`]**——理由
    /// 见该模块文档（JSON 等文本格式要求 map 键是字符串，元组键不
    /// 满足）。
    #[serde(with = "crate::script_state::serde_map")]
    pub script_state: BTreeMap<(String, String), ScriptValue>,
    /// 生物类型，用于击杀匹配与死因统计分类
    /// （`knowledge/design/kill-and-death-events.md` 六节）。
    ///
    /// `None` 时退回 [`Self::race`]——绝大多数"有种族意义"的智慧类
    /// 人型（玩家、NPC）不需要设置这个字段，只有专门的"怪物"内容需要。
    /// 独立于 `race` 存在的理由：`race` 原本设计给玩家角色种族，用它
    /// 兼职"敌人类型"会让"击杀 3 个哥布林"与"击杀 3 个哥布林种族的
    /// 玩家角色"共用同一个索引——这正是 `crates/ll-mod/src/quest.rs`
    /// 模块文档「跨表引用」一节如实记录的既有简化，本字段是消除它的
    /// 落点（匹配规则本身留给消费方——本批次只新增字段，不改动既有的
    /// `KillCount` 匹配逻辑，避免在同一批改动里牵连任务系统）。
    pub creature_kind: Option<ContentIndex>,
    /// 出生/升格为厚层实体的世界时刻——供死亡记录里"存活时长"一类
    /// 未来统计使用，也是"这个实体何时开始存在"这一问题唯一的权威
    /// 答案（薄层不追踪逐个体的时间，见 [`crate::entity::ThinPopulation`]
    /// 模块文档）。
    pub spawned_at: Tick,
    /// 若这个实体已经"值得被记住"（出生进历史家族族谱、被玩家收为
    /// 随从、成为任务发布者、死于一场被记录的击杀……），这里是它的
    /// 永久标识；否则为 `None`。
    ///
    /// # 懒分配，不是每个 `Agent` 出生时的必填项
    ///
    /// `knowledge/design/identity-and-ids.md`「类型/实例分离」定案表
    /// 只把 `WorldId` 分配给「势力、家族、聚落、宗教团体、历史事件」，
    /// 没有覆盖「历史人物/具名 NPC」这一类个体——
    /// `kill-and-death-events.md` 五节指出了这个缺口，本字段是消费端
    /// （"判断一个实体是否具名"）需要的落点，正式归属留给该文档未来
    /// 修订。若给每个 `Agent` 出生时都发一个 `WorldId`，"被记住"这个
    /// 本该几乎零成本的轴就要背上"必须分配全局唯一递增 ID"的负担，
    /// 与背景 NPC 零存储现算的设计前提冲突（见同一节论证）——因此只在
    /// 首次"值得被记住"的那一刻才赋值，见
    /// [`crate::state::WorldState::remembered_id_of_or_assign`]。
    pub remembered_id: Option<WorldId>,
    /// 角色总等级（`knowledge/design/level-and-experience-system.md`
    /// 二节）——单一整数，不拆分职业等级/技能等级，见该文档「什么东西
    /// 有等级」一节的完整论证：当前项目只有一个主职（`profession` 是
    /// 单值）、技能解锁走离散 DAG（不是连续练级量），两种「分拆等级」
    /// 的既有范式都不成立。
    pub level: i32,
    /// 当前等级内已经累积的经验值——**不是**终身累计总量，是「距离
    /// 下一级还差多少」这个进度条的分子，升级时随 [`Self::xp_to_next_level`]
    /// 一起扣减归零重算，见 `ll_sim::apply` 模块 `GrantExperience` 分支
    /// 的文档。
    pub experience: i64,
    /// 升到下一级所需的经验总量——**缓存值，不是现算**，见设计文档
    /// 「为什么 `xp_to_next_level` 必须缓存」一节：驱动它的
    /// `XpCurveDef` 是递推公式（下一级门槛依赖上一级门槛的求值结果），
    /// 现算需要从 1 级重放整条递推链（`O(当前等级)`），缓存后只需要在
    /// 真正升级的那一刻增量重算一次（`O(1)`）。与 [`Self::health`]/
    /// [`Self::mana`] 「只存当前值不存上限」的既有纪律形式相反，但是
    /// 同一条「不做不必要的重复计算」纪律在不同数学结构（纯函数 vs
    /// 递推）下的正确应用，不是破例——详见该字段的完整论证。
    pub xp_to_next_level: i64,
    /// 尚未分配的属性点——升级时授予，由**玩家自己**决定加到哪一项
    /// （项目所有者裁定原文：「升级获得属性点技能点，然后就自己加点」）。
    ///
    /// # 为什么必须存，而不是从等级现算
    ///
    /// 「等级 × 每级点数 − 已经加出去的点数」这个现算式需要「已经加
    /// 出去多少点」这个量，而它本身不可恢复：加出去的点已经并进
    /// [`Self::stats`]，与种族修正烘焙进去的那部分（见
    /// `ll_sim::character::bake_race_stat_modifiers`）不可区分。因此
    /// 未分配余额是**真正的偏离量**（[ADR 0009](../../../../knowledge/decisions/0009-derive-by-default-store-only-deviation.md)
    /// 意义上的），必须存。
    ///
    /// # 为什么加点直接改 `stats`，不是第四路修正来源
    ///
    /// 属性修正当前有三路：种族 `stat_modifiers`（**烘焙**进 `stats`，
    /// 不是运行期第三方）、装备 `StatBonus`、限时
    /// [`Self::active_stat_modifiers`]（后两路在
    /// `ll_sim::resolve::derive_stats` 里逐次叠加）。玩家加的点属于
    /// 第一类而不是后两类：它**永久、无来源、不会过期、不会因为脱下
    /// 什么而消失**，把它记成一条「来源是谁？」答不上来的修正，只会
    /// 让 `derive_stats` 多一条永远不过期的特殊分支。直接落进
    /// `stats` 与种族烘焙同路，`derive_stats` 一个字都不用改（它本就
    /// 以 `base` 为起点再叠加另外两路），也因此天然享有
    /// [`BaseStats::HARD_CAP`] 只约束基础值、装备可以突破这条设计。
    pub unspent_attribute_points: u32,
    /// 尚未分配的技能点——与 [`Self::unspent_attribute_points`] 同一
    /// 次升级同时授予，花在「学会一个前置已满足的新技能」上，见
    /// `ll_sim::resolve` 的 `Intent::LearnSkill` 结算。
    ///
    /// # 技能点与「学会技能」的关系
    ///
    /// 技能点是**闸门**，不是技能本身的等级：本项目的技能是离散 DAG
    /// （`knowledge/design/level-and-experience-system.md` 二节「为什么
    /// 不是技能各自等级」已经裁定技能不连续练级），一个技能只有「学
    /// 会」与「没学会」两种状态（[`Self::unlocked_skills`] 是个集合，
    /// 不是等级表）。因此一点技能点买断一个技能，学会之后这个技能不
    /// 再消耗任何点数——不存在「往同一个技能里继续投点」这回事。
    pub unspent_skill_points: u32,
    /// 背包（P6 第二批：背包与地面物品）——`item-system.md` 四节
    /// `ItemLocation::Inventory { holder, slot }` 在本批次的落地：`holder`
    /// 就是这个 `Agent` 自身（因此不需要在 `ItemStack` 上重复记
    /// 一份持有者），`slot` 尚未落地（22 槽位与 `SlotMask` 是第三批的
    /// 工作，见项目任务书「本批次范围」一节），本批次的背包是一个无
    /// 序容器，不区分槽位。
    ///
    /// # 为什么是 `Agent` 的字段，不是 `WorldState` 的旁挂表
    ///
    /// 与 [`Self::health`] 「为什么是 `Agent` 字段」同一条理由（见其
    /// 文档）：背包是逐实体状态，`Agent` despawn 时应当随实体一起被
    /// `Arena` 整体收走，不产生指向不存在实体的孤儿库存记录。
    ///
    /// `Vec` 不是 `BTreeMap<u16, ItemStack>`（尽管设计文档的 `slot`
    /// 字段暗示了槽位索引）：本批次没有槽位概念（见上），查询模式是
    /// 「背包里有没有这个 `def`」（`iter().find`，`O(n)`，n 是背包物品
    /// 种类数，量级不超过几十），与 [`Self::unlocked_skills`] 同样用
    /// `Vec` 而非有序容器的理由一致。
    pub inventory: Vec<ItemStack>,
    /// 装备栏（P6 第三批：装备槽位）——落地
    /// `knowledge/design/equipment-slots.md`，`item-system.md` 四节
    /// `ItemLocation::Equipped { holder, slot }` 在本批次的落地：`holder`
    /// 就是这个 `Agent` 自身（与 [`Self::inventory`] 「`holder` 不需要
    /// 在 `ItemStack` 上重复记一份」同一条理由）。
    ///
    /// # 为什么以锚点槽位为键，不是逐槽复制存一份
    ///
    /// 一件横跨多槽的物品（双手武器占 `MAIN_HAND`+`OFF_HAND`）**只存
    /// 一份**，键取它自身 `equip_mask` 的最低位（[`crate::item::SlotMask::anchor_slot`]，
    /// 例如双手武器的锚点是 `MAIN_HAND`）——被否决的替代方案是在
    /// `MAIN_HAND`/`OFF_HAND` 两个键下各存一份指向同一堆物品的副本：
    /// 那会制造一个必须手动维持一致的不变式（卸下时若只删掉其中一个
    /// 键，就会残留一把"半卸下"的幽灵武器,悬空指向一件已经不完整的
    /// 装备状态)。锚点键方案physical上排除了这类不一致——不存在"部分
    /// 卸下"的中间状态,一次移除操作对应整件物品的完整卸下。代价是
    /// "查询某个具体槽位当前是否被占用"不能只做一次哈希表查找,需要
    /// 遍历全部已装备条目、结合各自的 `equip_mask` 判断是否覆盖目标
    /// 槽位（`ll_sim::resolve::resolve_equip`/`resolve_unequip` 的查找
    /// 逻辑），但装备栏条目数量的量级（≤22）让这次遍历的成本可以忽略。
    ///
    /// # 为什么是 `BTreeMap`，不是 `Vec`（与 `Self::inventory` 不同）
    ///
    /// 背包用 `Vec` 是因为查询模式是"有没有这个 `def`"（无序,`O(n)`
    /// 足够）；装备栏的查询模式是"这个槽位现在是谁"（键是
    /// [`crate::item::EquipSlot`]，与 `Self::skill_cooldowns`/
    /// `Self::resource_pools` 同一类"按键查值"场景），且写入
    /// `WorldState::hash()`（ADR 0022）时需要确定性的迭代顺序——
    /// `BTreeMap` 按键的自然顺序遍历,不涉及 `HashMap`/`HashSet` 的
    /// 迭代顺序不确定性（约束 C5），与 `Self::skill_cooldowns` 同一条
    /// 既有纪律。
    pub equipment: BTreeMap<EquipSlot, ItemStack>,
    /// 是否正处于潜行状态（潜行与盗贼被动批次）——项目所有者裁定
    /// 「潜行需要时可切换状态的」，本字段就是那个「状态」，由
    /// `ll_sim::intent::Intent::ToggleStealth` 经
    /// `ll_sim::effect::Effect::SetStealth` → `apply` 翻转（约束 C1：
    /// 唯一写入口仍是 `apply`，与 [`Self::pos`]/[`Self::current_space`]
    /// 同一条纪律）。
    ///
    /// # 为什么是世界状态，不是渲染层的一个显示标志
    ///
    /// 它真的改变结算：潜行时移动开销上升
    /// （`ll_sim::resolve::STEALTH_MOVE_COST_PERMILLE`）、潜行中发起的
    /// 攻击直通偷袭判定（`ll_sim::resolve` 的「偷袭接线」一节）、
    /// 卫兵盘查的触发率下降（`mods/example_mod/behavior.scm` 经
    /// `actor-stealthed?` 读取本字段）。三条都是「同一个世界从这一刻起
    /// 会走出不同的未来」，按 ADR 0022 必须进 `WorldState::hash()`、
    /// 必须进存档——与 [`Self::resting`]「正在进行的休息会话」同一类
    /// 逐实体的可切换状态。
    ///
    /// # 为什么是 `bool`，不是一个带载荷的结构体
    ///
    /// 对比 [`Self::resting`]：休息必须记住「何时开始、目标多久」才能
    /// 判定完成，潜行没有任何随时间推进的进度——它只有「开着」和「关着」
    /// 两种，破除是一次显式事件（攻击）而不是一次到期比较。提前塞进
    /// 「潜行开始于哪一刻」这类字段会立刻变成没有读者的死数据（ADR
    /// 0021 同一条判据：抽象/字段要有真实消费者驱动）。
    ///
    /// # 破除条件：攻击破除，受伤不破除
    ///
    /// 完整论证见 `ll_sim::resolve::resolve_attack` 文档「潜行破除」
    /// 一节——一句话版本：本批次的潜行**不是隐身**（卫兵照常看得见你，
    /// 见 `ll_script::api::actor` 模块文档），它只影响「要不要把你当回
    /// 事」这次判定；自己动手打人是当事人自己做的一次公开动作，理应
    /// 破除，被别人打中则不是当事人能选择的事，让任意第三方（含范围
    /// 伤害/陷阱）都能无代价剥掉一个角色的潜行，是一条所有者没有要求、
    /// 且当前没有任何反制设计（重新潜行的代价/冷却）配套的规则。
    pub stealthed: bool,
}

/// [`Agent::spent_slots`] 的自定义 serde 编码：把 `(ContentIndex, u8)`
/// 元组键的 map 序列化成有序条目列表——理由与手法同
/// `crate::script_state::serde_map`（JSON 等文本格式的 map 键必须是
/// 字符串，元组键不满足；同一个模式在本 crate 内的第二次出现，不需要
/// 抽成共享泛型工具：两处的键/值具体类型不同，各自十几行代码，提前
/// 泛型化没有真实的第三个调用点驱动，见「ADR 0021：抽象要有真实可
/// 共享的算法支撑」同一条判断）。
mod spent_slots_serde {
    use std::collections::BTreeMap;

    use ll_core::ident::ContentIndex;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// 把 map 序列化成有序条目列表。
    pub fn serialize<S>(
        map: &BTreeMap<(ContentIndex, u8), u32>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries: Vec<(&(ContentIndex, u8), &u32)> = map.iter().collect();
        entries.serialize(serializer)
    }

    /// 从有序条目列表重建 map。
    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<(ContentIndex, u8), u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries: Vec<((ContentIndex, u8), u32)> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}

impl Agent {
    /// 生成/升格新实体时的占位起始生命值。
    ///
    /// 真正的生命上限应由 `stats.constitution` 经 `derive_stats` 算出
    /// （见本类型文档「为什么只存当前值」一节），但那个公式属于战斗
    /// 结算批次，本批次不提前设计。新实体总要有一个非零起点——不然
    /// 刚生成就是「零血」，第一下判定就会触发死亡——因此这里给一个
    /// 明确标记为占位的常量，供 [`crate::entity::ThinPopulation::promote`]
    /// 等构造路径统一使用，公式落地后只需替换这一处。
    pub const STARTING_HEALTH: i32 = 100;
    /// 生成/升格新实体时的占位起始法力值，理由同 [`Self::STARTING_HEALTH`]
    /// ——真正的上限应由智力经衍生公式算出，公式落地前需要一个非零
    /// 占位，供 [`crate::entity::ThinPopulation::promote`] 等构造路径
    /// 统一使用。
    pub const STARTING_MANA: i32 = 50;
    /// 生成/升格新实体时的占位起始耐力值，理由同 [`Self::STARTING_MANA`]。
    pub const STARTING_STAMINA: i32 = 50;
    /// 生成/升格新实体时的起始等级——角色总是从 1 级开始，不是占位值
    /// （不存在需要替换成别的公式的「真正的起始等级」）。
    pub const STARTING_LEVEL: i32 = 1;
    /// 生成/升格新实体时 1→2 级所需经验的占位值。真正的值应取自这个
    /// 实体所属职业/种族绑定的 `XpCurveDef::base_requirement`
    /// （`knowledge/design/level-and-experience-system.md` 三节「为什么
    /// 需要种子值」）——但 `ll-world`/`ThinPopulation::promote` 这一层
    /// 没有内容注册表可查（同一条理由见 [`Self::STARTING_HEALTH`] 文档），
    /// 因此和生命/法力/耐力一样先给一个非零占位，供 `ll-sim`/`ll-mod`
    /// 接线批次在真正生成角色时用查表结果覆盖。
    pub const STARTING_XP_TO_NEXT_LEVEL: i64 = 100;
    /// 每升一级授予的属性点数。
    ///
    /// 常量而不是一条可注册的曲线：项目所有者的裁定只说「升级获得
    /// 属性点技能点」，没有要求每级给的数量本身可变；给一个当前没有
    /// 任何内容作者要求过的量提前开注册位，正是 [ADR 0021](../../../../knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)
    /// 与 YAGNI 同时点名的那种投机式抽象。真有 mod 需要改它时，改法
    /// 与 `xp_curve` 一样是新增一条注册函数，不需要先在这里预留形状。
    pub const ATTRIBUTE_POINTS_PER_LEVEL: u32 = 2;
    /// 每升一级授予的技能点数，理由同 [`Self::ATTRIBUTE_POINTS_PER_LEVEL`]。
    ///
    /// 比属性点少：技能是一次性买断的离散解锁（见
    /// [`Self::unspent_skill_points`] 文档），一级一点让「这一级学哪
    /// 一个」成为一次真正的取舍；属性点一级两点则让玩家能同时兼顾
    /// 一条主线与一条副线，不至于每一级都被迫二选一。
    pub const SKILL_POINTS_PER_LEVEL: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{AffiliationKind, OrgRef};
    use crate::space::Space;
    use ll_core::ident::{Interner, NamespacedId, WorldId};

    /// 构造一个全字段都非默认值的 `Agent`——包括非空的 `affiliations`/
    /// `goals`，确保往返测试真的会经过 `Vec` 分支，而不是恰好因为空
    /// `Vec` 序列化恒等于自身而掩盖真正的编解码缺陷。
    fn fully_populated_agent() -> Agent {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:blacksmith").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:dwarf").expect("合法标识符"));
        let culture =
            interner.intern(NamespacedId::parse("lostland:mountain").expect("合法标识符"));
        let goal_kind = interner.intern(NamespacedId::parse("lostland:trade").expect("合法标识符"));
        let strike = interner.intern(NamespacedId::parse("lostland:strike").expect("合法标识符"));
        let power_strike =
            interner.intern(NamespacedId::parse("lostland:power_strike").expect("合法标识符"));
        let ranger_subclass =
            interner.intern(NamespacedId::parse("lostland:ranger_subclass").expect("合法标识符"));
        let roast_recipe =
            interner.intern(NamespacedId::parse("lostland:roast_meat_recipe").expect("合法标识符"));
        let mut world_id_counter = 7u32;
        let zone = ll_core::torus::TorusSize::new(48, 32)
            .expect("48x32 是合法尺寸")
            .wrap(3, 5);

        Agent {
            pos: ll_core::torus::TorusSize::new(64, 64)
                .expect("64x64 是合法尺寸")
                .wrap(10, 20),
            stats: BaseStats {
                strength: 14,
                dexterity: 12,
                constitution: 16,
                intelligence: 8,
                willpower: 11,
                charisma: 9,
                luck: 9,
            },
            next_action_at: Tick(42),
            health: 77,
            affiliations: vec![Affiliation {
                kind: AffiliationKind::Culture,
                org: OrgRef::Def(culture),
                standing: 250,
            }],
            wallet: 12345,
            profession,
            goals: vec![Goal {
                kind: goal_kind,
                params: vec![1, -2, 3],
                progress: 500,
                priority: 3,
            }],
            race,
            current_space: Space::Interior {
                id: WorldId::next(&mut world_id_counter),
                floor: -2,
                anchor: zone,
                profile: ContentIndex::default(),
            },
            mana: 33,
            stamina: 44,
            resource_pools: BTreeMap::from([(strike, 12)]),
            spent_slots: BTreeMap::from([((strike, 3), 2)]),
            resting: Some(RestState {
                started_at: Tick(30),
                target_ticks: 480,
            }),
            unlocked_skills: vec![strike, power_strike],
            // 非空——见本函数文档：新增字段若在这里取默认值（空
            // `Vec`），序列化往返测试对它等于没测（空 `Vec` 序列化恒
            // 等于自身）。
            known_recipes: vec![roast_recipe],
            skill_cooldowns: BTreeMap::from([(power_strike, Tick(120))]),
            subclasses: vec![ranger_subclass],
            active_stat_modifiers: BTreeMap::from([(
                AttributeKind::Constitution,
                BTreeMap::from([(
                    strike,
                    ActiveStatModifier {
                        delta: 3,
                        expires_at: Tick(150),
                    },
                )]),
            )]),
            script_state: BTreeMap::from([(
                ("lostland".to_string(), "cooldown".to_string()),
                ScriptValue::Int(5),
            )]),
            creature_kind: Some(race),
            spawned_at: Tick(10),
            remembered_id: Some(WorldId::next(&mut world_id_counter)),
            level: 5,
            experience: 250,
            xp_to_next_level: 800,
            // 非默认值（0）——见本函数文档：新增字段若在这里取默认
            // 值，序列化往返测试对它等于没测。
            unspent_attribute_points: 3,
            unspent_skill_points: 2,
            // 非默认值——本帮手的全部意义就是「每个字段都不是默认
            // 值」，否则往返测试会因为「恰好等于默认值」而掩盖真正的
            // 编解码缺陷（见本函数文档）。
            stealthed: true,
            // 非空背包——见 agent序列化往返后全部字段逐一相等 文档：
            // 空 Vec 序列化恒等于自身,不足以验证真正的编解码,必须让
            // 往返测试真的经过至少一条 ItemStack。
            inventory: vec![ItemStack::with_durability(strike, 3, 80)],
            // 非空装备栏——同一条理由（P6 第三批），且键用的是
            // EquipSlot（新类型）而非 ContentIndex，往返测试必须真的
            // 覆盖它的编解码,不能只靠空 BTreeMap 混过去。
            equipment: BTreeMap::from([(
                EquipSlot::MAIN_HAND,
                ItemStack::with_durability(strike, 1, 100),
            )]),
        }
    }

    #[test]
    fn agent序列化往返后全部字段逐一相等() {
        // Arrange
        let original = fully_populated_agent();

        // Act
        let encoded = serde_json::to_string(&original).expect("全部字段均已可派生序列化");
        let decoded: Agent = serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert：Agent 派生了 PartialEq，逐字段比较（含 current_space）
        // 由这一个断言覆盖，不需要逐个字段单独断言。
        assert_eq!(decoded, original);
    }

    #[test]
    fn agent可以同时持有一个主职与多个副职() {
        // P5-B 任务 4 的 TDD 清单要求这条断言（「同一个 Agent 可以持有
        // 一个主职与至少一个副职」），但字段本身定义在 Agent（本任务）
        // ——这里补上，见 `subclasses` 字段文档。
        // Arrange
        let agent = fully_populated_agent();

        // Act & Assert：主职（profession）与副职集合（subclasses）是两个
        // 互不重叠的字段，后者当前恰好持有一个副职，容器形状本身允许
        // 多于一个。
        assert_eq!(agent.subclasses.len(), 1);
    }
}
