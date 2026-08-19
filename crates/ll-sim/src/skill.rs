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
//! **代价是真实的重复**：`SkillEffect`/`ResourceCost` 在两个 crate 里
//! 各存在一份结构相同（或结构上更明确，见 [`SkillEffect::RestoreResource`]
//! 文档）的声明，未来把 `ll-mod::skill::SkillTable` 真正接到
//! `resolve_use_skill`（游戏内容加载管线，超出本计划范围）的那次改动，
//! 需要一层显式的转换函数（`ll_mod::skill::SkillEffect` → 本模块
//! `SkillEffect`）架在两者之间——这是「已知缺口，记录不硬做」的部分：
//! 交叉引用/跨表桥接问题已经计划在统一的后续阶段处理（同 `SkillDef.owning_class`
//! 是否指向真实 `ClassDef` 无法在注册期跨表校验的先例，见
//! `crates/ll-mod/src/skill.rs` 模块文档），本任务不提前造一条不完整
//! 的桥接代码。
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCost {
    /// 不消耗任何资源——纯冷却限制的技能。
    None,
    /// 消耗给定种类与数量的资源。
    Amount(ResourceKind, u32),
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
/// 文档）内部用来处理 `Intent::UseSkill` 的默认实现——本计划范围内还
/// 没有任何生产代码持有真正的技能注册表并接到 `resolve`（那是游戏内容
/// 加载管线的职责，见本模块文档「代价是真实的重复」一节），因此
/// `resolve` 对 `UseSkill` 意图的默认行为就是「任何技能都查不到」，
/// 与「资源不足」「技能未解锁」走的是同一条「静默不产出效果」的既有
/// 纪律（本函数不是特殊路径）。真正想让技能结算生效的调用方应改用
/// [`crate::resolve::resolve_with_skills`]，传入一个真正实现了
/// [`SkillCatalog`] 的目录。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSkills;

impl SkillCatalog for NoSkills {
    fn skill(&self, _skill: ContentIndex) -> Option<SkillRule> {
        None
    }
}
