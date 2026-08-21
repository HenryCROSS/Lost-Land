//! 技能定义在 `resolve` 侧需要的最小只读视图（P5-B 任务 5）。
//!
//! # 为什么这里重新声明了一遍 `SkillEffect`/`ResourceCost`
//!
//! `crates/ll-mod/src/skill.rs`（P5-B 任务 3）已经定义过形状几乎相同的
//! `SkillEffect`/`ResourceCost`——**这不是遗漏，是依赖方向逼出来的**。
//! 规格 §5 的依赖顺序是 `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`：
//! `ll-mod` 在最上游，可以依赖 `ll-sim`（间接经由 `ll-script`），但反过
//! 来 `ll-sim` **不能**依赖 `ll-mod`——`cargo tree -p ll-mod` 已经确认
//! `ll-mod → ll-script → ll-sim → ll-world` 这条链真实存在，若 `ll-sim`
//! 再依赖 `ll-mod` 就是一个环。
//!
//! `ll_mod::skill::SkillDef`/`SkillTable` 是 `ClassDef`/`SkillDef` 这类
//! **内容注册表**的家（任务 2/3 的模块文档已经说明：它们不依赖任何
//! 「世界空间」概念，因此直接落在 `ll-mod`，不像地形那样拆成两处）。
//! 但 `Intent::UseSkill` 的结算恰恰需要在 `ll-sim::resolve` 里读到「这个
//! 技能的冷却/资源消耗/效果是什么」——`resolve` 拿不到定义在下游 crate
//! 的类型，这是这次实现踩到的一处真实的 ADR 0016 接口面缺口：**任务
//! 2/3 把技能定义放在 `ll-mod`，是「本体即 Mod」检验通过的正确选择
//! （定义本身不该有本体特权），但这个选择让 `resolve` 这一层完全没有
//! 官方支持的方式去读它**——不是「本体能做 mod 不能做」的那种特权洞，
//! 是「谁都做不到」的一处架构缺口。
//!
//! # 本任务选择的解法：依赖反转，`ll-sim` 定义接口，`ll-mod` 实现
//!
//! [`SkillCatalog`] 这个 trait 定义在本 crate（`resolve` 所在的层），
//! `resolve_use_skill` 只依赖这个 trait，不依赖任何
//! 具体存储形状。真正持有 `SkillTable` 的一方（`ll-mod`，或未来串起
//! 加载管线的更上游代码）在自己那一层为 `SkillTable` 实现
//! `SkillCatalog`，把结算需要的字段通过 [`SkillRule`] 这个独立类型
//! 交出来——`ll-world::fov::SightGrid` 已经示范过同一种「trait 收敛
//! 差异面，算法/结算逻辑只依赖 trait」的手法（这里的差异面是「技能
//! 定义存在哪个 crate」而不是「网格是环面还是有界」，但解法同构）。
//! 依赖方向没有被打破：`ll-mod` 实现一个 `ll-sim` 定义的 trait，是
//! 「下游为上游的接口提供实现」，不是「上游依赖下游的类型」。
//!
//! **接线批次的更新（原「代价是真实的重复」一节，如实标注现状）**：
//! 上面这段论证写于任务 5，当时 `ll-mod` 还不允许依赖 `ll-sim`（那时
//! 没有任何生产代码需要这个方向），`SkillEffect`/`ResourceCost` 因此在
//! 两个 crate 里各存在一份结构相同（或结构上更明确，见
//! [`SkillEffect::RestoreResource`] 文档）的独立声明。接线批次把
//! `ll-sim` 提升为 `ll-mod` 的生产依赖（见 `crates/ll-mod/Cargo.toml`
//! 的接线批次说明）之后，这个限制不再成立——`crates/ll-mod/src/skill.rs`
//! 现在直接 `use` 本模块的 `ResourceCost`/`ResourceKind`/`SkillEffect`，
//! 不再维护一份会漂移的副本；`ll_mod::skill::SkillTable` 也已经实现了
//! 下面的 [`SkillCatalog`]，不需要任何转换函数。依赖方向没有变化：
//! `ll-sim` 仍然不知道、也不依赖 `ll-mod` 的任何类型，只是 `ll-mod`
//! 现在选择复用本模块的定义而不是重新声明一份。
//!
//! # 为什么不是改 `resolve` 的签名去接收 `ll-mod` 的具体类型
//!
//! 曾经考虑过反过来：让 `ll-sim` 依赖 `ll-mod`。这在 Rust 里会直接编译
//! 失败（循环依赖），不是风格选择。也考虑过把 `SkillDef`/`SkillEffect`
//! 从 `ll-mod` 挪回 `ll-world`（像地形那样「定义在下游、封装在上游」）
//! ——**否决**：那会推翻任务 2/3 已经评审通过、已提交的设计判断（`class.rs`/
//! `skill.rs` 模块文档明确论证过职业/技能不依赖世界空间、不需要那层
//! 拆分），且任务 5 的文件范围本就不包含 `ll-mod`，贸然改动上一批
//! 已经冻结的接口不是本任务的职责。

use ll_core::ident::ContentIndex;
use ll_world::entity::AttributeKind;

/// 技能消耗/恢复的资源种类。
///
/// 与 `Agent::mana`/`Agent::stamina`（P5-B 任务 5 新增，见其字段文档）
/// 一一对应——本类型只是「指的是哪一项」的标签，具体数值仍然存在
/// `Agent` 上。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResourceKind {
    /// 法力。
    Mana,
    /// 耐力。
    Stamina,
}

/// 技能消耗的资源类型与数量——形状对齐 `ll_mod::skill::ResourceCost`
/// （见本模块文档「为什么这里重新声明了一遍」）。
///
/// # 为什么新增 `PoolAmount`，不是就地把 `Amount` 的载荷从
/// `ResourceKind` 改成 `ContentIndex`（资源池落地批次）
///
/// `resource-pools-and-rest.md` 二节的最终设计确实是把 `Amount` 的
/// 载荷从封闭的 `ResourceKind` 换成开放的 `ContentIndex`——但
/// `Amount(ResourceKind, u32)` 不是一个孤立的类型：`register-skill`
/// 既有的 `resource-kind`/`resource-amount`（`"none"`/`"mana"`/
/// `"stamina"`）两个参数、真实 mod 内容
/// （`mods/example_mod/gameplay.scm` 的 `frostbolt`）、以及围绕它们的
/// 全部既有测试都建立在这个封闭形状之上。就地改型会把一条已经稳定、
/// 已经被真实内容使用的脚本 API 变成破坏性变更,且与本批次要解决的
/// 问题（"落地一族新的开放资源池"）无关——[`PoolAmount`](Self::PoolAmount)
/// 是新增的第三个变体,`Amount`/[`ResourceKind`] 原样保留,服务既有
/// `"mana"`/`"stamina"` 两个内置资源种类;`PoolAmount` 服务经
/// `register-resource-pool`（`ll_mod::resource_pool`）注册的开放池。
/// 两条通道刻意并存，不是过渡态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCost {
    /// 不消耗任何资源——纯冷却限制的技能。
    None,
    /// 消耗给定种类与数量的资源——`ResourceKind` 是封闭枚举，服务既有
    /// `register-skill` 内置的 `"mana"`/`"stamina"` 两种资源，见本类型
    /// 文档「为什么新增 `PoolAmount`」一节。
    Amount(ResourceKind, u32),
    /// 消耗给定标量资源池当前值 `amount` 点（`resource-pools-and-rest.md`
    /// 二节）——`ContentIndex` 指向经 `register-resource-pool` 注册的
    /// `ResourcePoolDef`，容量走 `crate::resource_pool::effective_scalar_capacity`
    /// 现算，不足则技能静默不产出效果，见 `crate::resolve::resolve_use_skill`
    /// 「门四」文档。
    PoolAmount(ContentIndex, u32),
    /// 消耗给定法术位池、`min_tier` 档或更高档的一个槽位
    /// （`resource-pools-and-rest.md` 二节「从最低阶开始取」）——
    /// `ContentIndex` 指向经 `register-resource-pool` 注册的
    /// `ResourcePoolShape::TieredSlots` 池。消耗算法本质与
    /// `PoolAmount` 不同（在有序档位里找空位，而不是减去固定数量），
    /// 因此是独立变体,不是 `PoolAmount` 换个载荷就能表达——见
    /// `crate::resource_pool` 模块文档「二节」原文引用的 ADR 0021。
    /// 单向可兑换：高阶位能放低阶法术，低阶位不能放高阶法术——从
    /// `min_tier` 起往上找第一个「上限 > 已消耗数」的档位占用，找不到
    /// 则技能静默不产出效果，见 `crate::resolve::resolve_use_skill`
    /// 「门四」文档。不支持玩家手选具体档位，理由见
    /// `resource-pools-and-rest.md` 二节原文。
    SlotTier(ContentIndex, u8),
    /// 血代价：直接扣 `amount` 点 `Agent::health`，绕开减伤/抗性
    /// （`resource-pools-and-rest.md` 五节）——**刻意不复用
    /// [`SkillEffect::DealDamage`]**：`Effect::SpendBloodCost` 是独立
    /// 效果，不查 `damage_after_defense`/`resistance_multiplier`/
    /// `active_stat_modifiers`，也因此天然不触发任何键在
    /// `Effect::Damage` 上的触发器（例如"受伤反击"）。不设最低血量
    /// 兜底——扣到零或以下时施法者会被自己的法术杀死，见
    /// `crate::resolve::resolve_use_skill` 「血代价」一节。
    Blood(u32),
}

/// 技能效果——**只能是纯数值**，不得引入任何读取装备槽位的字段，见
/// `crates/ll-mod/src/skill.rs` 模块文档「与规格 §15 P6 边界的关系」
/// 一节，本类型延续同一条边界。
///
/// # 与 `ll_mod::skill::SkillEffect` 的一处刻意差异
///
/// `ll-mod` 那份声明的 `RestoreResource { base: i32 }` 没有说明恢复的
/// 是哪一种资源——本体夹具（`base_skill_fixture` 的 `focus` 技能）靠
/// 代码注释「恢复法力」这种非结构化方式表达，编译器管不到。既然本模块
/// 是独立声明（见上），这里顺手把这个歧义补掉：显式带上 `resource`
/// 字段。未来桥接 `ll-mod` 那份定义时，这份差异需要在转换函数里显式
/// 决定「怎么补上这一个字段」（多半是让 `ll-mod` 那份定义同步补上，而
/// 不是在桥接层瞎猜）——记入本模块文档，不假装两份定义已经完全对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillEffect {
    /// 造成伤害，基础值，不经任何装备/穿透加成。
    DealDamage {
        /// 基础伤害值。
        base: i32,
    },
    /// 恢复资源，基础值。
    RestoreResource {
        /// 恢复的资源种类。
        resource: ResourceKind,
        /// 基础恢复值。
        base: i32,
    },
    /// 临时属性修正：`duration_ticks` 个 tick 内，`attribute` 项的有效
    /// 值增减 `amount`——落到 [`crate::effect::Effect::ApplyStatModifier`]，
    /// 见该变体文档。
    TemporaryStatModifier {
        /// 受影响的主属性。
        attribute: AttributeKind,
        /// 增减量，可为负。
        amount: i32,
        /// 持续的 tick 数。
        duration_ticks: u32,
    },
}

/// `resolve_use_skill` 需要的一条技能定义的完整只读视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillRule {
    /// 冷却时长，tick 数。
    pub cooldown_ticks: u32,
    /// 资源消耗。
    pub resource_cost: ResourceCost,
    /// 技能效果。
    pub effect: SkillEffect,
}

/// `resolve_use_skill` 依赖的最小「技能定义来源」接口——把结算算法本身
/// 与「技能定义具体存在哪个 crate、用什么容器存」解耦，见本模块文档
/// 「本任务选择的解法」一节完整论证。
///
/// # 为什么用 trait + 动态分发，不是泛型
///
/// 与 [`ll_world::fov::SightGrid`]（逐格热路径，选择泛型静态分发）不同，
/// 技能查询是「每次 `Intent::UseSkill` 结算一次」的低频调用（一场战斗
/// 里一个实体一回合最多用一次技能，不是逐格/逐帧路径），`dyn
/// SkillCatalog` 的一次虚调用开销可以忽略不计，换来的是
/// [`crate::resolve::resolve`] 不需要为每一种技能目录实现单态化出一份
/// 拷贝，调用方（测试、未来的生产接线）也不需要在泛型参数上纠结。
pub trait SkillCatalog {
    /// 查询一条技能定义；未注册的索引返回 `None`（对齐 ADR 0015 的解析
    /// 纪律：查不到就是查不到，`resolve_use_skill` 据此把「技能不存在」
    /// 与「技能存在但条件不满足」同等对待——两者都不产出任何效果，见
    /// `resolve_use_skill` 文档）。
    fn skill(&self, skill: ContentIndex) -> Option<SkillRule>;
}

/// 空技能目录：查询任何索引恒返回 `None`。
///
/// 是 [`crate::resolve::resolve`]（不接收技能目录参数的既有入口，见其
/// 文档）内部用来处理 `Intent::UseSkill` 的默认实现——调用方没有技能
/// 内容（例如尚未装载任何 mod、或明确不需要技能结算的调用场景）时的
/// 保底行为，`resolve` 对 `UseSkill` 意图的默认行为就是「任何技能都
/// 查不到」，与「资源不足」「技能未解锁」走的是同一条「静默不产出
/// 效果」的既有纪律（本函数不是特殊路径）。真正想让技能结算生效的
/// 调用方应改用 [`crate::resolve::resolve_with_skills`]（或
/// `resolve_with_skills_and_quests`），传入一个真正实现了
/// [`SkillCatalog`] 的目录——`ll_mod::skill::SkillTable`（接线批次）
/// 现在就是这样一个真正的实现，不再只有测试用的假目录。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSkills;

impl SkillCatalog for NoSkills {
    fn skill(&self, _skill: ContentIndex) -> Option<SkillRule> {
        None
    }
}
