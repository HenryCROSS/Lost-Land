//! 技能注册表 + 前置关系 DAG 校验——「本体即 Mod」在技能系统上的落点
//! （P5-B 任务 3）。
//!
//! # 与 `ClassTable` 同一套模式，但多一步图校验
//!
//! 见 [`crate::class`] 模块文档「照抄 `terrain.rs`/`space_profile.rs`
//! 已验证的模式」与「为什么定义本身直接落在 `ll-mod`」两节——技能的
//! 定义/存储/查询走完全相同的思路。技能比职业多出的是**前置关系**：
//! `SkillDef.prerequisites` 描述一个有向无环图（DAG）的边,「技能树」
//! 这个名字本身要求能表达分支（一个技能可以有多个后续),线性序列
//! 表达不了,见 `knowledge/design/class-skill-quest-system.md` 第二节。
//!
//! # DAG 无环校验：为什么必须在注册期做
//!
//! 若允许循环前置（技能 A 需要技能 B，技能 B 需要技能 A），任何「这个
//! 技能能否解锁」的判定都会陷入死循环，或者产出错误结果（两者互相
//! 依赖,谁都学不到）。[`validate_no_cycles`] 因此是本模块的核心正确性
//! 要求——[`materialize_base_skills`] 在全部本体技能定义完毕后立即
//! 调用它,任何环路都会在加载时就报出来,而不是留到玩家点开技能树时
//! 才表现成异常。
//!
//! # 确定性：拓扑着色遍历顺序不依赖注册顺序（约束 C5）
//!
//! [`crate::topo::topo_sort`] 已经示范过同一类问题的解法：图算法里
//! 「多个候选、选哪个先走」的时刻，一律按某个与输入顺序无关的键排序
//! 决定，不看原始下标。[`validate_no_cycles`] 把「先把已登记的
//! [`ContentIndex`] 按数值升序排好，再从这份排好序的列表出发做白/灰/黑
//! 三色 DFS」这套算法委托给 [`crate::prereq_graph::validate_no_cycles`]
//! （P5-B 任务 6 抽出，`QuestTable` 需要同一套无环校验，见该模块文档）
//! ——即便两次调用之间 `define` 的调用顺序不同（只要最终登记的技能
//! 集合与前置关系相同），报告出来的具体环路也恒定不变。
//!
//! # 与规格 §15 P6 边界的关系
//!
//! `SkillEffect`/`ResourceCost` 的文档已经写明纯数值边界——技能效果
//! 只能是这两个类型能表达的纯数值变体，不得引入任何读取装备槽位的
//! 字段。真实防御来自装备，装备系统（P6）在本批次之后才落地；若某个
//! 技能效果「看起来需要装备信息才能表达」，正确做法是记入待裁定，不是
//! 提前给 `SkillEffect` 加一个装备相关变体。见各自类型的文档
//! （[`ll_sim::skill`]）。
//!
//! # `ResourceCost`/`SkillEffect`/`ResourceKind` 现在直接复用
//! `ll_sim::skill` 的定义，不再本地重复声明（接线批次）
//!
//! 任务 3 实现这三个类型时，`ll-mod` 尚未（且当时也不需要）依赖
//! `ll-sim`——`SkillDef`/`SkillTable` 只是内容注册表，不需要知道结算
//! 层怎么用它们。任务 5 在 `ll-sim` 侧为了让 `resolve_use_skill` 能读到
//! 技能定义，又在 `crates/ll-sim/src/skill.rs` 独立声明了一份结构几乎
//! 相同的 `ResourceCost`/`SkillEffect`（该文件模块文档「本任务选择的
//! 解法：依赖反转」一节详细论证了当时为什么只能这样做——`ll-sim` 不能
//! 依赖 `ll-mod`，两边只能各自声明）。
//!
//! **这次接线改变了前提**：为了让 `SkillTable` 真正实现
//! `ll_sim::skill::SkillCatalog`（把内容注册表接进 `resolve_with_skills`
//! 真实调用得到的结算链路），`ll-mod` 现在必须依赖 `ll-sim`（见本 crate
//! `Cargo.toml` 的接线批次说明）。依赖方向 `ll-world` ← `ll-sim` ←
//! `ll-script` ← `ll-mod`（规格 §5）本就允许这个方向，此前只是「还没有
//! 需要」——一旦允许，继续维持两份结构近似的重复声明就不再有理由：
//! 那会让「技能效果长什么样」有两个可能漂移的真相源（`ll-sim` 那份的
//! 模块文档已经指出过一处真实的字段不对齐：`RestoreResource` 该文件
//! 的版本没有说明恢复的是哪种资源）。本模块因此改为直接
//! `use ll_sim::skill::{ResourceCost, ResourceKind, SkillEffect};`，
//! `ll-sim` 那份定义现在是唯一真相源，`ll-mod` 只是复用者——`ll-sim`
//! 仍然不知道、也不依赖 `ll-mod` 的任何类型，依赖方向没有被打破。

use std::fmt;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
// `pub use`（不是普通 `use`）：`SkillAttrs`/`SkillDef` 的调用方（本体
// 与未来的 mod 注册代码）需要能通过 `ll_mod::skill::ResourceCost`/
// `ll_mod::skill::SkillEffect` 这两个既有路径构造这两个类型的值——
// 这是任务 3 起就有的公开 API 形状，不能因为这次改成复用 `ll-sim` 的
// 定义就让调用方被迫改成直接依赖 `ll-sim`。
pub use ll_sim::skill::{ResourceCost, ResourceKind, SkillCatalog, SkillEffect, SkillRule};
use ll_world::entity::AttributeKind;

/// 单条技能声明：本体与 mod 注册技能时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在技能层面的验收标的——本体的声明与未来 mod 的
/// 声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDef {
    /// 命名空间标识符，例如 `lostland:power_strike`、`yourmod:frostbolt`。
    pub id: NamespacedId,
    /// 所属职业，`None` 表示通用技能——不专属任何职业，任何职业的角色
    /// 都能学。主职与副职共享同一份技能命名空间（P5-4 裁定，见
    /// `knowledge/design/class-skill-quest-system.md` 第三节），这里
    /// 因此只是一个分类/展示字段，不是命名空间隔离的边界。
    pub owning_class: Option<ContentIndex>,
    /// 前置技能，DAG 的边。空列表表示这是一个"起点"技能，不需要任何
    /// 前置即可学习。
    pub prerequisites: Vec<ContentIndex>,
    /// 冷却时长，游戏内 tick 数。
    pub cooldown_ticks: u32,
    /// 消耗的资源类型与数量。
    pub resource_cost: ResourceCost,
    /// 技能效果——纯数值，见 [`SkillEffect`] 文档的 P6 边界。
    pub effect: SkillEffect,
}

/// [`SkillTable::define`] 实际存进列式存储的属性子集——不含 `id`，与
/// [`crate::class::ClassAttrs`] 相对 [`crate::class::ClassDef`] 同一个
/// 理由。**必须公开**：这是 `define` 唯一的参数类型，任何想直接调用
/// `define`（而不是走 [`materialize_base_skills`] 那条便捷路径）的
/// 调用方——包括未来 mod 自己的技能注册函数——都需要能构造这个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillAttrs {
    /// 所属职业。
    pub owning_class: Option<ContentIndex>,
    /// 前置技能列表。
    pub prerequisites: Vec<ContentIndex>,
    /// 冷却时长，tick 数。
    pub cooldown_ticks: u32,
    /// 资源消耗。
    pub resource_cost: ResourceCost,
    /// 技能效果。
    pub effect: SkillEffect,
}

/// 技能注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::class::ClassError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// 前置技能列表里引用了一个当前表从未登记过的索引——不是环，是
    /// 另一类数据错误（引用了压根不存在的技能，或者不小心引用了一个
    /// 属于别的内容类型的索引）。[`validate_no_cycles`] 在做图遍历
    /// 时顺带发现这类悬空引用，一并报出来，不让它被 DFS 静默跳过。
    UnregisteredPrerequisite {
        /// 声明了这条悬空前置的技能。
        skill: ContentIndex,
        /// 被引用但未登记的索引。
        missing: ContentIndex,
    },
    /// 前置关系构成环——附带环路上具体的技能索引（按环路顺序），不是
    /// "检测到环"这种无法定位的笼统提示。
    CyclicPrerequisites(Vec<ContentIndex>),
}

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillError::DuplicateDefinition(index) => {
                write!(f, "技能索引 {} 被重复定义", index.get())
            }
            SkillError::UnregisteredPrerequisite { skill, missing } => write!(
                f,
                "技能索引 {} 声明的前置索引 {} 未在当前技能表中登记",
                skill.get(),
                missing.get()
            ),
            SkillError::CyclicPrerequisites(cycle) => {
                let ids: Vec<String> = cycle.iter().map(|c| c.get().to_string()).collect();
                write!(f, "技能前置关系形成环：{}", ids.join(" -> "))
            }
        }
    }
}

impl std::error::Error for SkillError {}

/// 一次技能查询命中的完整结果，理由同 [`crate::class::ClassView`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillView<'a> {
    /// 所属职业。
    pub owning_class: Option<ContentIndex>,
    /// 前置技能列表。
    pub prerequisites: &'a [ContentIndex],
    /// 冷却时长，tick 数。
    pub cooldown_ticks: u32,
    /// 资源消耗。
    pub resource_cost: ResourceCost,
    /// 技能效果。
    pub effect: SkillEffect,
}

/// 技能属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017），与 [`crate::class::ClassTable`] 同一套道理。
#[derive(Debug, Default, Clone)]
pub struct SkillTable {
    owning_class: Vec<Option<ContentIndex>>,
    prerequisites: Vec<Vec<ContentIndex>>,
    cooldown_ticks: Vec<u32>,
    resource_cost: Vec<ResourceCost>,
    effect: Vec<SkillEffect>,
    defined: Vec<bool>,
    /// 按注册顺序记录已定义的索引。
    ///
    /// [`ContentIndex`] 的内部字段对 `ll-mod` 不可见（P5 之前只有
    /// `ll-core`/`Interner` 能构造它），[`validate_no_cycles`] 需要
    /// 枚举"当前表里全部已定义的索引"来做遍历起点，无法从 `usize`
    /// 下标反推出一个 `ContentIndex`——因此这里额外保留一份真正的
    /// 索引拷贝，而不是尝试从 `defined` 的下标重建。
    defined_ids: Vec<ContentIndex>,
}

impl SkillTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上技能属性。
    ///
    /// **只做「不得重复定义」这一项校验，不在这里校验前置关系**——
    /// 前置可以是"尚未 `define`、但已经 `intern` 过"的索引（与
    /// `TerrainTable` 的 `opens_into` 同一种前向引用模式：`门关闭`
    /// 引用 `门打开` 时，后者可能还没被 `define`，只是已经拿到了
    /// 索引）。真正的图级别校验（无环、前置确实都注册过）在全部技能
    /// 定义完毕后由 [`validate_no_cycles`] 一次性做,不能提前到单条
    /// `define` 调用里,否则会拒绝完全合法的"先声明多个互相指涉的
    /// 技能,再统一校验"这种注册顺序。
    pub fn define(&mut self, index: ContentIndex, attrs: SkillAttrs) -> Result<(), SkillError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.owning_class.resize(new_len, None);
            self.prerequisites.resize(new_len, Vec::new());
            self.cooldown_ticks.resize(new_len, 0);
            self.resource_cost.resize(new_len, ResourceCost::None);
            self.effect
                .resize(new_len, SkillEffect::DealDamage { base: 0 });
        }

        if self.defined[idx] {
            return Err(SkillError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.owning_class[idx] = attrs.owning_class;
        self.prerequisites[idx] = attrs.prerequisites;
        self.cooldown_ticks[idx] = attrs.cooldown_ticks;
        self.resource_cost[idx] = attrs.resource_cost;
        self.effect[idx] = attrs.effect;
        self.defined_ids.push(index);
        Ok(())
    }

    /// 给定的技能索引当前是否已经登记过属性。
    pub fn is_defined(&self, skill: ContentIndex) -> bool {
        self.defined
            .get(skill.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个技能的完整属性，未注册的索引返回 `None`（对齐 ADR
    /// 0015，同 [`crate::class::ClassTable::get`]）。
    pub fn get(&self, skill: ContentIndex) -> Option<SkillView<'_>> {
        if !self.is_defined(skill) {
            return None;
        }
        let idx = skill.get() as usize;
        Some(SkillView {
            owning_class: self.owning_class[idx],
            prerequisites: &self.prerequisites[idx],
            cooldown_ticks: self.cooldown_ticks[idx],
            resource_cost: self.resource_cost[idx],
            effect: self.effect[idx],
        })
    }

    /// 按 [`ContentIndex::get`] 数值升序返回全部已注册技能索引（约束
    /// C5：不依赖内部 `defined_ids` 的原始注册顺序）——供任务 8 的
    /// [`ll_sim::skill_overview::SkillTreeCatalog`] 实现遍历"当前一共
    /// 登记了哪些技能"，理由同 [`crate::quest::QuestTable::defined_indices`]。
    pub fn defined_indices(&self) -> Vec<ContentIndex> {
        let mut ids = self.defined_ids.clone();
        ids.sort_by_key(ContentIndex::get);
        ids
    }
}

/// [`SkillTable`] 对 [`crate::prereq_graph::PrerequisiteGraph`] 的适配
/// ——把「查询前置列表」翻译成通用图算法认得的形状（P5-B 任务 6 从
/// 本模块抽出 [`crate::prereq_graph`] 时新增，见该模块文档「为什么现在
/// 才抽出来」一节）。
impl crate::prereq_graph::PrerequisiteGraph for SkillTable {
    fn is_defined(&self, node: ContentIndex) -> bool {
        SkillTable::is_defined(self, node)
    }

    fn prerequisites(&self, node: ContentIndex) -> &[ContentIndex] {
        self.get(node).map(|view| view.prerequisites).unwrap_or(&[])
    }
}

/// 注册期校验：给定全部已注册技能的前置关系，是否存在环；顺带校验
/// 每条前置是否指向一个当前表里真实登记过的技能。
///
/// # 算法委托给 `prereq_graph`（P5-B 任务 6 起）
///
/// 核心的白/灰/黑三色 DFS 与「起点按 [`ContentIndex::get`] 数值升序
/// 尝试」（约束 C5 的确定性要求）现在都在
/// [`crate::prereq_graph::validate_no_cycles`] 里——任务 6 引入
/// `QuestTable` 之后需要同一套校验，两张表因此共用同一份算法，本函数
/// 只做「适配 + 把通用错误映射回 [`SkillError`]」这一层薄包装，公开
/// 签名、错误变体、错误粒度（报告具体环路而非笼统"存在环"）与之前
/// 完全一致，不影响任何既有调用方或测试。
pub fn validate_no_cycles(skills: &SkillTable) -> Result<(), SkillError> {
    crate::prereq_graph::validate_no_cycles(skills, &skills.defined_ids).map_err(|err| match err {
        crate::prereq_graph::CycleError::UnregisteredPrerequisite { node, missing } => {
            SkillError::UnregisteredPrerequisite {
                skill: node,
                missing,
            }
        }
        crate::prereq_graph::CycleError::Cycle(cycle) => SkillError::CyclicPrerequisites(cycle),
    })
}

/// [`SkillTable`] 对 [`ll_sim::skill::SkillCatalog`] 的实现——「依赖
/// 倒置」在技能系统上真正闭环的一步。
///
/// `ll_sim::skill` 模块文档「本任务选择的解法」一节论证过这条 trait
/// 为什么定义在 `ll-sim`：`resolve_use_skill` 需要读技能定义，但依赖
/// 方向不允许它直接依赖持有 `SkillTable` 的 `ll-mod`。那份文档写完的
/// 时候，`ll-mod` 还没有实现方——本次接线批次补上这个实现：`SkillTable`
/// 现在可以被直接传给 [`ll_sim::resolve::resolve_with_skills`]（或本 crate
/// `Cargo.toml` 描述的批次），让真正由本体/未来 mod 注册出来的技能定义
/// 参与真实结算，而不再是只有测试用的 `FakeCatalog`（`ll-sim/tests/
/// skill_resolve.rs`）在验证这条 trait。
///
/// 实现本身没有任何转换逻辑——`SkillView::{cooldown_ticks,
/// resource_cost, effect}` 与 [`ll_sim::skill::SkillRule`] 的对应字段
/// 现在是完全相同的类型（见本模块文档「现在直接复用」一节），不需要
/// 像本来预想的那样写一层桥接转换函数。
impl SkillCatalog for SkillTable {
    fn skill(&self, skill: ContentIndex) -> Option<SkillRule> {
        self.get(skill).map(|view| SkillRule {
            cooldown_ticks: view.cooldown_ticks,
            resource_cost: view.resource_cost,
            effect: view.effect,
        })
    }
}

/// [`SkillTable`] 对 [`ll_sim::skill_overview::SkillTreeCatalog`] 的
/// 实现（P5-B 任务 8）——给技能树 UI 数据视图（`ll_sim::skill_overview`）
/// 提供"当前一共登记了哪些技能""某个技能的前置是什么"这两项
/// `SkillCatalog` 本身不携带的信息。与上面的 `SkillCatalog` 实现同一个
/// 理由：不需要任何转换逻辑，`SkillView::prerequisites` 与
/// [`Self::defined_indices`] 直接就是 trait 方法要求的形状。
impl ll_sim::skill_overview::SkillTreeCatalog for SkillTable {
    fn all_skills(&self) -> Vec<ContentIndex> {
        self.defined_indices()
    }

    fn prerequisites(&self, skill: ContentIndex) -> Vec<ContentIndex> {
        self.get(skill)
            .map(|view| view.prerequisites.to_vec())
            .unwrap_or_default()
    }
}

/// 本体基础技能在当前注册表里的索引缓存。
///
/// 构成一棵有分支的技能树（验收「树而不是线性序列」这条形状要求，见
/// `knowledge/design/class-skill-quest-system.md` 第二节）：
/// `strike`（起点）解锁 `power_strike`/`brace`/`focus` 三条分支，
/// `combo` 同时要求 `power_strike` 与 `brace` 两个前置（分支之后再
/// 汇聚），演示"一个技能有多个前置"与"一个前置解锁多个后续"两条要求。
#[derive(Debug, Clone, Copy)]
pub struct BaseSkillIds {
    /// 起点技能：基础打击，无前置。
    pub strike: ContentIndex,
    /// `strike` 的分支之一：强力打击，造成更高伤害。
    pub power_strike: ContentIndex,
    /// `strike` 的分支之一：格挡姿态，临时提升体质。
    pub brace: ContentIndex,
    /// `strike` 的分支之一：凝神，恢复法力——刻意声明为通用技能
    /// （`owning_class: None`），演示"不专属任何职业"的技能类别。
    pub focus: ContentIndex,
    /// 汇聚技能：连击，要求 `power_strike` 与 `brace` 两个前置同时
    /// 满足。
    pub combo: ContentIndex,
}

/// 本体技能注册的唯一入口：本体与 mod 共用的注册路径，理由同
/// [`crate::class::materialize_base_classes`]。
///
/// 通过 `intern` 回调解析 `lostland:warrior` 换回它的 [`ContentIndex`]
/// 来设置 `owning_class`——与 `TerrainTable` 的 `opens_into` 同一种
/// 跨内容引用手法（见 `ll_world::terrain` 模块文档）：只靠共用的
/// `intern` 回调按命名空间字符串解析,不直接依赖
/// [`crate::class::BaseClassIds`] 这个具体类型,`class` 与 `skill`
/// 两个模块因此不需要互相硬编码对方的返回结构。
pub fn materialize_base_skills(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseSkillIds, SkillTable), SkillError> {
    let mut table = SkillTable::new();
    let warrior = intern(NamespacedId::parse("lostland:warrior").expect("固定字面量恒合法"));

    let strike = define_skill(
        &mut table,
        intern,
        "lostland:strike",
        Some(warrior),
        Vec::new(),
        0,
        ResourceCost::None,
        SkillEffect::DealDamage { base: 5 },
    )?;
    let power_strike = define_skill(
        &mut table,
        intern,
        "lostland:power_strike",
        Some(warrior),
        vec![strike],
        20,
        ResourceCost::Amount(ResourceKind::Stamina, 10),
        SkillEffect::DealDamage { base: 12 },
    )?;
    let brace = define_skill(
        &mut table,
        intern,
        "lostland:brace",
        Some(warrior),
        vec![strike],
        15,
        ResourceCost::Amount(ResourceKind::Stamina, 5),
        SkillEffect::TemporaryStatModifier {
            attribute: AttributeKind::Constitution,
            amount: 3,
            duration_ticks: 10,
        },
    )?;
    let focus = define_skill(
        &mut table,
        intern,
        "lostland:focus",
        None,
        vec![strike],
        10,
        ResourceCost::None,
        // 恢复法力——`resource` 字段是本次接线补上的（见本模块文档
        // 「现在直接复用」一节）：`ll-sim` 那份定义此前用非结构化注释
        // 表达「恢复的是哪种资源」，编译器管不到，接线时顺手补掉。
        SkillEffect::RestoreResource {
            resource: ResourceKind::Mana,
            base: 8,
        },
    )?;
    let combo = define_skill(
        &mut table,
        intern,
        "lostland:combo",
        Some(warrior),
        vec![power_strike, brace],
        30,
        ResourceCost::Amount(ResourceKind::Stamina, 15),
        SkillEffect::DealDamage { base: 20 },
    )?;

    validate_no_cycles(&table)?;

    Ok((
        BaseSkillIds {
            strike,
            power_strike,
            brace,
            focus,
            combo,
        },
        table,
    ))
}

/// [`materialize_base_skills`] 的内部帮手：把一条声明拆开传入，换取
/// 一次 `intern` + 一次 [`SkillTable::define`]。
#[allow(clippy::too_many_arguments)]
fn define_skill(
    table: &mut SkillTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    owning_class: Option<ContentIndex>,
    prerequisites: Vec<ContentIndex>,
    cooldown_ticks: u32,
    resource_cost: ResourceCost,
    effect: SkillEffect,
) -> Result<ContentIndex, SkillError> {
    let index = intern(NamespacedId::parse(id).expect("本体技能 id 字面量恒合法"));
    table.define(
        index,
        SkillAttrs {
            owning_class,
            prerequisites,
            cooldown_ticks,
            resource_cost,
            effect,
        },
    )?;
    Ok(index)
}

/// 供测试使用：现造一个空 [`Interner`]，注册本体全部基础技能，返回
/// 可用的 `(BaseSkillIds, SkillTable)`。不是生产路径，理由同
/// [`crate::class::base_class_fixture`]。
pub fn base_skill_fixture() -> (BaseSkillIds, SkillTable) {
    let mut interner = Interner::new();
    materialize_base_skills(&mut |id| interner.intern(id))
        .expect("本体技能声明表内部一致（无环、前置均已注册），注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn no_effect_attrs(prerequisites: Vec<ContentIndex>) -> SkillAttrs {
        SkillAttrs {
            owning_class: None,
            prerequisites,
            cooldown_ticks: 0,
            resource_cost: ResourceCost::None,
            effect: SkillEffect::DealDamage { base: 1 },
        }
    }

    #[test]
    fn 合法的分支型技能树注册成功() {
        // 验收"树而不是线性序列"：strike 解锁三条分支，combo 汇聚
        // 其中两条——见 BaseSkillIds 文档。
        // Arrange & Act
        let (ids, table) = base_skill_fixture();

        // Assert
        let combo_view = table.get(ids.combo).expect("combo 已注册");
        assert_eq!(combo_view.prerequisites, &[ids.power_strike, ids.brace]);
    }

    #[test]
    fn 一个前置技能解锁多个后续技能() {
        // Arrange
        let (ids, table) = base_skill_fixture();

        // Act：手动统计以 strike 为前置的技能数量——skill.rs 本身不
        // 提供反向索引（那是 quest 模块 unlocked_by 的职责，技能树
        // 本任务不需要），这里直接检查已知的三个分支各自都把 strike
        // 列为前置。
        let branches = [ids.power_strike, ids.brace, ids.focus];
        let all_reference_strike = branches
            .iter()
            .all(|&branch| table.get(branch).expect("已注册").prerequisites == [ids.strike]);

        // Assert
        assert!(all_reference_strike);
    }

    #[test]
    fn 通用技能不专属任何职业() {
        // Arrange
        let (ids, table) = base_skill_fixture();

        // Act
        let view = table.get(ids.focus).expect("focus 已注册");

        // Assert
        assert_eq!(view.owning_class, None);
    }

    #[test]
    fn 技能前置关系形成环时注册失败() {
        // Arrange：a 需要 b，b 需要 c，c 需要 a——三节点环。
        let mut interner = Interner::new();
        let a = interner.intern(id("yourmod:a"));
        let b = interner.intern(id("yourmod:b"));
        let c = interner.intern(id("yourmod:c"));
        let mut table = SkillTable::new();
        table
            .define(a, no_effect_attrs(vec![b]))
            .expect("a 定义应当成功");
        table
            .define(b, no_effect_attrs(vec![c]))
            .expect("b 定义应当成功");
        table
            .define(c, no_effect_attrs(vec![a]))
            .expect("c 定义应当成功");

        // Act
        let result = validate_no_cycles(&table);

        // Assert
        assert!(matches!(result, Err(SkillError::CyclicPrerequisites(_))));
    }

    #[test]
    fn 环形错误信息包含构成环的具体技能id列表() {
        // Arrange：与上一条同样的三节点环。
        let mut interner = Interner::new();
        let a = interner.intern(id("yourmod:a"));
        let b = interner.intern(id("yourmod:b"));
        let c = interner.intern(id("yourmod:c"));
        let mut table = SkillTable::new();
        table
            .define(a, no_effect_attrs(vec![b]))
            .expect("a 定义应当成功");
        table
            .define(b, no_effect_attrs(vec![c]))
            .expect("b 定义应当成功");
        table
            .define(c, no_effect_attrs(vec![a]))
            .expect("c 定义应当成功");

        // Act
        let result = validate_no_cycles(&table);

        // Assert：不是笼统的"存在环"，而是具体列出构成环的三个索引。
        match result {
            Err(SkillError::CyclicPrerequisites(cycle)) => {
                assert_eq!(cycle.len(), 3);
                assert!(cycle.contains(&a) && cycle.contains(&b) && cycle.contains(&c));
            }
            other => panic!("期望 CyclicPrerequisites，实际是 {other:?}"),
        }
    }

    #[test]
    fn 技能自身引用自己构成一节点环() {
        // Arrange：退化情形——a 的前置是它自己。
        let mut interner = Interner::new();
        let a = interner.intern(id("yourmod:self_ref"));
        let mut table = SkillTable::new();
        table
            .define(a, no_effect_attrs(vec![a]))
            .expect("a 定义应当成功");

        // Act
        let result = validate_no_cycles(&table);

        // Assert
        assert_eq!(result, Err(SkillError::CyclicPrerequisites(vec![a])));
    }

    #[test]
    fn 前置引用未注册的索引时报告悬空引用而非静默通过() {
        // Arrange：a 声明了一个从未 define 过的前置。
        let mut interner = Interner::new();
        let a = interner.intern(id("yourmod:a"));
        let ghost = interner.intern(id("yourmod:ghost"));
        let mut table = SkillTable::new();
        table
            .define(a, no_effect_attrs(vec![ghost]))
            .expect("a 定义应当成功");

        // Act
        let result = validate_no_cycles(&table);

        // Assert
        assert_eq!(
            result,
            Err(SkillError::UnregisteredPrerequisite {
                skill: a,
                missing: ghost
            })
        );
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(id("lostland:strike"));
        let mut table = SkillTable::new();
        table
            .define(index, no_effect_attrs(Vec::new()))
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, no_effect_attrs(Vec::new()));

        // Assert
        assert_eq!(result, Err(SkillError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let never_defined = interner.intern(id("yourmod:never_defined"));
        let table = SkillTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 本体技能与mod注册的自定义技能调用同一个公开define函数完成注册() {
        // 结构等价断言，理由同 crate::class 模块的等价测试：本体技能
        // 与 mod 技能都只是往同一个 Registry::intern 里塞一个
        // NamespacedId，再用完全相同的公开 SkillTable::define 函数
        // 登记属性。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径，不能证明
        // mod 脚本调得到这套 API。真正的证据在 crate::pipeline 的
        // 脚本装载测试与 mods/example_mod/gameplay.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (base_ids, mut table) =
            materialize_base_skills(&mut |id| registry.intern(id)).expect("本体技能声明表内部一致");
        let mod_index = registry.intern(id("yourmod:frostbolt"));
        table
            .define(
                mod_index,
                SkillAttrs {
                    owning_class: None,
                    prerequisites: vec![base_ids.strike],
                    cooldown_ticks: 25,
                    resource_cost: ResourceCost::Amount(ResourceKind::Mana, 12),
                    effect: SkillEffect::DealDamage { base: 15 },
                },
            )
            .expect("mod 技能与本体技能调用同一个公开 define 函数,理应同样成功");

        // Assert：mod 技能确实登记成功,且其前置（本体的 strike）能被
        // 正常查到——证明两类内容共享同一张表、同一套校验。
        let view = table.get(mod_index).expect("mod 技能已通过 define 登记");
        assert_eq!(view.prerequisites, &[base_ids.strike]);
    }

    #[test]
    fn 本体技能表通过完整dag校验() {
        // materialize_base_skills 内部已经调用过 validate_no_cycles
        // （否则 base_skill_fixture 会直接 panic），这里额外单独
        // 调用一次确认结果确实是 Ok，把"内部已校验过"这件事变成一条
        // 显式断言，而不是只依赖 fixture 不 panic 这个间接证据。
        // Arrange
        let (_ids, table) = base_skill_fixture();

        // Act
        let result = validate_no_cycles(&table);

        // Assert
        assert!(result.is_ok());
    }

    /// 接线批次的核心验收：本体技能表（`materialize_base_skills` 真实
    /// 产出的 [`SkillTable`]，不是任何测试专用的假目录）直接喂给
    /// `ll_sim::resolve::resolve_with_skills`，走一遍与真实玩法完全
    /// 相同的 `Intent::UseSkill → resolve → Effect → apply` 链路——
    /// 证明「`SkillTable` 实现 `SkillCatalog`」不只是编译期类型对得上，
    /// 运行期真的产出正确效果、`apply` 落地后 `Agent` 状态确实改变。
    #[test]
    fn 本体技能表接入resolve_with_skills后真实结算出伤害与冷却() {
        // Arrange：一个 1x1 区块的最小世界 + 一个已解锁 strike 的攻击者
        // + 一个待打的目标，两者共用同一个 Registry 与本体技能表。
        let mut registry = Registry::new();
        let (base_ids, table) =
            materialize_base_skills(&mut |id| registry.intern(id)).expect("本体技能声明表内部一致");
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ll_world::zone::ZoneLayout::new(64, zone_count).expect("64 满足全部约束");
        let (terrain_ids, terrain_table) = ll_world::terrain::base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        let mut world = ll_world::state::WorldState::new(
            layout,
            &ll_world::generate::GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件");

        let profession = registry.intern(id("lostland:tester"));
        let race = registry.intern(id("lostland:human"));
        let pos = world.size.wrap(0, 0);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let blank = |unlocked_skills: Vec<ContentIndex>| ll_world::entity::Agent {
            pos,
            stats: ll_world::entity::BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: ll_world::entity::Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: ll_world::entity::Agent::STARTING_MANA,
            stamina: ll_world::entity::Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills,
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        };
        let actor = world.actors.spawn(blank(vec![base_ids.strike]));
        let target = world.actors.spawn(blank(Vec::new()));

        // Act：真实结算链路——不是直接构造 Effect，是从 Intent 出发。
        let effects = ll_sim::resolve::resolve_with_skills(
            &world,
            &ll_sim::intent::Intent::UseSkill {
                actor,
                skill: base_ids.strike,
                target: Some(target),
            },
            &table,
        );
        assert!(
            !effects.is_empty(),
            "已解锁、无冷却、无消耗的 strike 理应产出效果"
        );
        for effect in &effects {
            ll_sim::apply::apply(&mut world, effect);
        }

        // Assert：伤害真的落到了目标身上（strike 的 base 伤害是 5，见
        // materialize_base_skills），冷却也真的写回了施法者。
        let defender = world.actors.get(target).expect("目标应仍存在");
        assert_eq!(
            defender.health,
            ll_world::entity::Agent::STARTING_HEALTH - 5
        );
        let attacker = world.actors.get(actor).expect("攻击者应仍存在");
        assert!(attacker.skill_cooldowns.contains_key(&base_ids.strike));
    }
}
