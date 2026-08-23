//! 网状任务图注册表 + 任务进度持久化——「本体即 Mod」在任务系统上的
//! 落点（P5-B 任务 6/7）。
//!
//! # 与 `SkillTable` 同一套模式，同一套图校验
//!
//! 见 [`crate::skill`] 模块文档「与 `ClassTable` 同一套模式，但多一步
//! 图校验」一节——任务节点的定义/存储/查询走完全相同的思路：私有字段 +
//! `QuestTable::define` 注册期校验。任务节点比技能多出的同样
//! 是**前置关系**，但语义更贴近"任务"本身：`QuestNodeDef.prerequisites`
//! 描述的是"完成哪些任务节点之后，这个任务节点才能开始"，图结构本身与
//! 技能树完全同构（都是 DAG），因此无环校验直接复用
//! [`crate::prereq_graph`]（见该模块文档「为什么现在才抽出来」一节），
//! 不重新写一份 DFS。
//!
//! # 网状而非树状——单一真相源，`unlocked_by` 是派生视图
//!
//! `QuestNodeDef` 只存"我需要哪些前置任务"，不存"我完成后解锁哪些
//! 后续任务"——后者由 [`unlocked_by`] 现算，与
//! `knowledge/design/class-skill-quest-system.md` 第四节「单一真相源」
//! 一致（同一条纪律在两级坐标系重写批次的 `Interior.anchor`/反向索引
//! 已经用过）。"网状"体现在两处都允许多对多：一个任务节点可以有多个
//! 前置（多条完成路径汇聚），一个前置任务节点完成后也可以同时解锁多个
//! 后续节点（分支）——`mods/lostland/quests.json5` 的四条本体任务同时
//! 演示这两点，
//! 见 [`BaseQuestIds`] 文档。
//!
//! # 完成条件分档（ADR 0018 三档分级在任务系统上的落点）
//!
//! [`QuestCondition`] 只有两个变体——一档（[`QuestCondition::KillCount`]，
//! 声明式纯数据）与三档（[`QuestCondition::Script`]，脚本回调标识符）。
//! **本批次不做二档**（受限公式）：任务完成条件在当前已知需求下要么是
//! 简单计数（一档够用）要么是复杂逻辑（需要三档），中间的"公式"这一档
//! 没有明确用例，按 YAGNI 推迟——若后续发现真实需要，应该是一个独立的
//! 扩展任务，不是本批次顺手加的分支。
//!
//! **本批次只交付条件的注册/查询**，不交付"某个具体任务节点的条件当前
//! 是否已经满足"这个判定逻辑本身（一档需要读取"该实体击杀过多少个某类
//! 敌人"这类计数器，三档需要真正调用脚本——两者都需要串起
//! `ll-sim`/`ll-script` 的运行期管线，超出本批次范围，与
//! `crates/ll-sim/src/skill.rs` 模块文档「本任务选择的解法」一节列出的
//! 「已知缺口，记录不硬做」同一条纪律）。
//!
//! # 任务进度持久化：脚本状态存储，不是 `Agent` 字段（关键设计判断 2）
//!
//! "这个实体完成了哪些任务节点"走 P5-A 任务 8 交付的脚本状态存储的
//! **每实体存储**，不是 `Agent` 的直接字段——理由（计划原文）：任务
//! 内容高度依赖 mod 扩展，不是每回合都要读的高频状态；脚本状态存储的
//! 命名空间隔离天然适合"任务是 mod 定义的内容，进度应该按 mod 命名
//! 空间隔离"这条需求。见 [`mark_quest_completed`]/[`is_quest_completed`]
//! 文档。
//!
//! # 写入必须经 `apply`（裁定 P5-1 / ADR 0023）
//!
//! [`mark_quest_completed`] 只产出一条 `ModStateWrite`，**不直接
//! 改任何 `WorldState`**——脚本状态就是 `WorldState` 的一部分（挂在
//! `Agent::mod_state`），写它就是改世界，必须经
//! `ll_sim::effect::Effect::SetModState → apply` 这条唯一写入口
//! （约束 C1），否则"同一串 Intent 重放"复现不出任务进度。
//!
//! # 跨表引用：`QuestCondition::KillCount.target_kind` 无法在注册期校验
//!
//! 与 `SkillDef.owning_class` 是否指向真实 `ClassDef` 同一类已知缺口
//! （见 [`crate::skill`] 模块文档「与规格 §15 P6 边界的关系」一节的
//! 姊妹缺口）：`target_kind: ContentIndex` 意在指向"某种敌人类型"，
//! 但当前代码库还没有任何"敌人/生物类型"注册表存在，`QuestTable` 因此
//! 完全没有办法校验这个索引"指向的东西是否真实存在、是不是真的是一个
//! 敌人类型"——不是遗漏，是这张表在当前批次根本够不到那张不存在的表。
//! 记录在此，不提前为一张不存在的表造校验逻辑；待敌人类型注册表在
//! 未来某批次落地后，应该并入计划里"统一的交叉引用校验阶段"一并处理。
//! **接线批次的补充**：`ll_sim::resolve::resolve_with_skills_and_quests`
//! 现在确实会结算 `KillCount`，选择了 `Agent::race` 作为这个不存在的
//! "敌人类型"注册表的临时替代——见 [`ll_sim::quest`] 模块文档「击杀
//! 计数」一节，那里如实记录了这条简化。
//!
//! # 任务进度基础操作搬到了 `ll-sim`（接线批次）
//!
//! `quest_progress_key`/`mark_quest_completed`/`is_quest_completed`
//! 三个函数曾经定义在本文件——接线批次把它们下沉到
//! [`ll_sim::quest`]，因为 `resolve` 结算击杀时需要直接调用它们
//! （产出击杀达标后的任务完成写入），而依赖方向不允许 `ll-sim`
//! 反过来依赖本 crate。本模块下方 `pub use` 重新导出，保持既有调用点
//! （包括本文件自己的测试）不需要改名，理由与完整论证见
//! [`ll_sim::quest`] 模块文档「为什么搬到这里」一节。
//!
//! [`RegisteredQuests`] 是本批次新增的另一半：把 [`QuestTable`] 与
//! 负责反查 `ContentIndex → NamespacedId` 的 [`crate::registry::Registry`]
//! 绑在一起，实现 [`ll_sim::quest::QuestCatalog`]——这是「依赖倒置」
//! 在任务系统上真正闭环的一步，见其类型文档。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::base_contract::{BaseContractError, BaseContractResolver};
use crate::registry::Registry;
use ll_sim::quest::{QuestCatalog, QuestKillRule};
pub use ll_sim::quest::{is_quest_completed, mark_quest_completed, quest_progress_key};

/// 任务完成条件。**只有一档、三档，不做二档**——见模块文档「完成条件
/// 分档」一节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestCondition {
    /// 一档：声明式，纯数据——击杀 `count` 个 `target_kind` 类型的敌人。
    ///
    /// `target_kind` 指向"敌人类型"这个概念上的内容索引——当前代码库
    /// 尚无实际的敌人类型注册表，本变体只声明这份静态数据，不校验
    /// `target_kind` 是否真实存在，见模块文档「跨表引用」一节。
    KillCount {
        /// 目标敌人类型。
        target_kind: ContentIndex,
        /// 需要击杀的数量。
        count: u32,
    },
    /// 三档：脚本回调判定是否完成——处理"拜访某个 NPC 并说出特定台词"
    /// 这类无法穷举成数据的条件。
    ///
    /// 存的是回调标识符（`NamespacedId`），不是函数指针本身——与
    /// `SkillDef.effect` 一样，注册期只存"引用"，真正在运行期调用这个
    /// 回调是判定管线（超出本批次范围）的职责，本批次只保证这个变体
    /// 能被正确注册、查询、参与无环校验。
    Script(NamespacedId),
}

/// 单条任务节点声明：本体与 mod 注册任务时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在任务层面的验收标的——本体的声明与未来 mod 的
/// 声明除了 `id` 里的命名空间字符串不同之外，不存在任何结构性差异。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestNodeDef {
    /// 命名空间标识符，例如 `lostland:main_quest_1`、`yourmod:side_quest`。
    pub id: NamespacedId,
    /// 前置任务节点，DAG 的边，单一真相源——"完成后解锁了哪些后续任务"
    /// 是 [`unlocked_by`] 现算出的派生视图，不是另一份存储（见模块
    /// 文档「网状而非树状」一节）。空列表表示这是一个不需要任何前置即
    /// 可开始的"起点"任务。
    pub prerequisites: Vec<ContentIndex>,
    /// 完成条件，见 [`QuestCondition`] 文档。
    pub condition: QuestCondition,
}

/// [`QuestTable::define`] 实际存进列式存储的属性子集——不含 `id`，
/// 理由同 [`crate::skill::SkillAttrs`]。**必须公开**：这是 `define`
/// 唯一的参数类型，任何想直接调用 `define`（而不是走
/// 脚本 `register-quest` 那条路径）的调用方——包括未来 mod
/// 自己的任务注册函数——都需要能构造这个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestAttrs {
    /// 前置任务节点列表。
    pub prerequisites: Vec<ContentIndex>,
    /// 完成条件。
    pub condition: QuestCondition,
}

/// 任务注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::skill::SkillError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// 前置任务列表里引用了一个当前表从未登记过的索引，理由同
    /// [`crate::skill::SkillError::UnregisteredPrerequisite`]。
    UnregisteredPrerequisite {
        /// 声明了这条悬空前置的任务节点。
        quest: ContentIndex,
        /// 被引用但未登记的索引。
        missing: ContentIndex,
    },
    /// 前置关系构成环——附带环路上具体的任务节点索引（按环路顺序）。
    CyclicPrerequisites(Vec<ContentIndex>),
}

impl QuestError {
    /// 这条错误牵涉到的全部内容索引，理由与用法见
    /// [`crate::skill::SkillError::involved_indices`]。
    pub fn involved_indices(&self) -> Vec<ContentIndex> {
        match self {
            QuestError::DuplicateDefinition(index) => vec![*index],
            QuestError::UnregisteredPrerequisite { quest, missing } => vec![*quest, *missing],
            QuestError::CyclicPrerequisites(cycle) => cycle.clone(),
        }
    }
}

impl fmt::Display for QuestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuestError::DuplicateDefinition(index) => {
                write!(f, "任务索引 {} 被重复定义", index.get())
            }
            QuestError::UnregisteredPrerequisite { quest, missing } => write!(
                f,
                "任务索引 {} 声明的前置索引 {} 未在当前任务表中登记",
                quest.get(),
                missing.get()
            ),
            QuestError::CyclicPrerequisites(cycle) => {
                let ids: Vec<String> = cycle.iter().map(|c| c.get().to_string()).collect();
                write!(f, "任务前置关系形成环：{}", ids.join(" -> "))
            }
        }
    }
}

impl std::error::Error for QuestError {}

/// 一次任务查询命中的完整结果，理由同 [`crate::skill::SkillView`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestView<'a> {
    /// 前置任务节点列表。
    pub prerequisites: &'a [ContentIndex],
    /// 完成条件——`QuestCondition` 携带 `NamespacedId`（三档变体），
    /// 不是 `Copy`（与 `SkillView::effect` 直接返回值的写法不同），
    /// 这里按引用交出，避免每次查询都克隆一份。
    pub condition: &'a QuestCondition,
}

/// 任务节点属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分
/// 结构（ADR 0017），与 [`crate::skill::SkillTable`] 同一套道理。
#[derive(Debug, Default, Clone)]
pub struct QuestTable {
    prerequisites: Vec<Vec<ContentIndex>>,
    condition: Vec<QuestCondition>,
    defined: Vec<bool>,
    /// 按注册顺序记录已定义的索引，理由同
    /// [`crate::skill::SkillTable`] 同名字段文档。
    defined_ids: Vec<ContentIndex>,
}

impl QuestTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上任务属性。
    ///
    /// 只做「不得重复定义」这一项校验，不在这里校验前置关系——理由同
    /// [`crate::skill::SkillTable::define`]：前置可以是"尚未 `define`、
    /// 但已经 `intern` 过"的索引，真正的图级别校验在全部任务定义完毕
    /// 后由 [`validate_no_cycles`] 一次性做。
    pub fn define(&mut self, index: ContentIndex, attrs: QuestAttrs) -> Result<(), QuestError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.prerequisites.resize(new_len, Vec::new());
            // QuestCondition 没有语义上的默认值——用一个 count 为 0 的
            // KillCount 占位，与 TerrainTable::move_cost 扩容时填 0
            // 同一个理由：未定义的槽位永远被 `defined` 位图挡住，不会
            // 被外部查询实际读到。
            self.condition.resize(
                new_len,
                QuestCondition::KillCount {
                    target_kind: ContentIndex::default(),
                    count: 0,
                },
            );
        }

        if self.defined[idx] {
            return Err(QuestError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.prerequisites[idx] = attrs.prerequisites;
        self.condition[idx] = attrs.condition;
        self.defined_ids.push(index);
        Ok(())
    }

    /// 给定的任务索引当前是否已经登记过属性。
    pub fn is_defined(&self, quest: ContentIndex) -> bool {
        self.defined
            .get(quest.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个任务节点的完整属性，未注册的索引返回 `None`（对齐 ADR
    /// 0015，同 [`crate::skill::SkillTable::get`]）。
    pub fn get(&self, quest: ContentIndex) -> Option<QuestView<'_>> {
        if !self.is_defined(quest) {
            return None;
        }
        let idx = quest.get() as usize;
        Some(QuestView {
            prerequisites: &self.prerequisites[idx],
            condition: &self.condition[idx],
        })
    }
}

/// [`QuestTable`] 对 [`crate::prereq_graph::PrerequisiteGraph`] 的适配
/// ——与 [`crate::skill::SkillTable`] 的同名实现同一个理由，两张表因此
/// 共用 [`crate::prereq_graph::validate_no_cycles`] 同一份 DFS。
impl crate::prereq_graph::PrerequisiteGraph for QuestTable {
    fn is_defined(&self, node: ContentIndex) -> bool {
        QuestTable::is_defined(self, node)
    }

    fn prerequisites(&self, node: ContentIndex) -> &[ContentIndex] {
        self.get(node).map(|view| view.prerequisites).unwrap_or(&[])
    }
}

/// 注册期校验：给定全部已注册任务的前置关系，是否存在环；顺带校验
/// 每条前置是否指向一个当前表里真实登记过的任务节点。算法委托给
/// [`crate::prereq_graph::validate_no_cycles`]，理由见
/// [`crate::skill::validate_no_cycles`] 文档「算法委托给
/// `prereq_graph`」一节——两张表共用同一份三色 DFS，本函数只做「适配 +
/// 把通用错误映射回 [`QuestError`]」。
pub fn validate_no_cycles(quests: &QuestTable) -> Result<(), QuestError> {
    crate::prereq_graph::validate_no_cycles(quests, &quests.defined_ids).map_err(|err| match err {
        crate::prereq_graph::CycleError::UnregisteredPrerequisite { node, missing } => {
            QuestError::UnregisteredPrerequisite {
                quest: node,
                missing,
            }
        }
        crate::prereq_graph::CycleError::Cycle(cycle) => QuestError::CyclicPrerequisites(cycle),
    })
}

/// 派生视图：给定一个已完成的任务节点集合，返回因此解锁的后续节点。
///
/// **不是存储，是纯函数**——见模块文档「网状而非树状」一节：单一真相
/// 源是 `prerequisites`（谁需要谁），"解锁了谁"每次调用现算，不缓存、
/// 不随时间变化产出不同结果（只要 `table`/`completed` 不变）。
///
/// 一个节点"因此解锁"的判定：不在 `completed` 集合里（已完成的节点
/// 不需要再"解锁"一次），且它的全部前置都落在 `completed` 集合内——
/// 空前置列表天然满足"全部前置都落在集合内"（`Iterator::all` 对空
/// 迭代器恒真），因此起点任务（无前置）即便 `completed` 为空也总是
/// 出现在结果里，这正是"不需要任何前置即可开始"的字面含义。
///
/// 返回列表按 [`ContentIndex::get`] 数值升序排列（约束 C5）——不依赖
/// `table` 内部 `defined_ids` 的注册顺序，保证同一个已完成集合无论
/// `table` 是以什么顺序注册出来的，结果都逐位一致。
pub fn unlocked_by(table: &QuestTable, completed: &[ContentIndex]) -> Vec<ContentIndex> {
    use std::collections::BTreeSet;

    let completed_set: BTreeSet<ContentIndex> = completed.iter().copied().collect();
    let mut order = table.defined_ids.clone();
    order.sort_by_key(ContentIndex::get);

    let mut unlocked = Vec::new();
    for node in order {
        if completed_set.contains(&node) {
            continue;
        }
        let view = table
            .get(node)
            .expect("defined_ids 里的索引必然已通过 define 注册");
        if view.prerequisites.iter().all(|p| completed_set.contains(p)) {
            unlocked.push(node);
        }
    }
    unlocked
}

impl QuestTable {
    /// 按 [`ContentIndex::get`] 数值升序返回全部已注册任务节点索引——
    /// 约束 C5，不依赖内部 `defined_ids` 的原始注册顺序，理由同
    /// [`unlocked_by`] 文档「返回列表按……升序排列」一节。供
    /// [`RegisteredQuests`] 与任务 8 的
    /// `crate::quest_overview::build_quest_log_view` 遍历"当前一共登记
    /// 了哪些任务节点"。
    pub fn defined_indices(&self) -> Vec<ContentIndex> {
        let mut ids = self.defined_ids.clone();
        ids.sort_by_key(ContentIndex::get);
        ids
    }
}

/// 把 [`QuestTable`] 与负责反查 `ContentIndex → NamespacedId` 的
/// [`crate::registry::Registry`] 绑在一起，实现
/// [`ll_sim::quest::QuestCatalog`]——依赖倒置在任务系统上的落点。
///
/// # 为什么比 [`crate::skill::SkillTable`] 直接实现 `SkillCatalog`
/// 多绕一层
///
/// 技能只读冷却/资源/效果这些纯数值，不需要知道自己的
/// `NamespacedId`——`SkillTable` 因此可以自己单独实现
/// `ll_sim::skill::SkillCatalog`。任务完成写入
/// （[`mark_quest_completed`]）却按 `NamespacedId` 的命名空间隔离存储
/// （见其文档），`QuestTable` 自身不存这个字符串（`QuestView` 只有
/// `prerequisites`/`condition`，见其文档）——单靠 `QuestTable` 拿不出
/// 一条可以直接喂给 `mark_quest_completed` 的规则,必须额外持有一份
/// 能做反查的 `Registry`，两者缺一都无法独立完成这件事。
pub struct RegisteredQuests<'a> {
    /// 已注册的任务表。
    pub table: &'a QuestTable,
    /// 负责 `ContentIndex → NamespacedId` 反查的注册表——真实生产环境
    /// 应该是加载管线持有的那一份 [`crate::registry::Registry`]，必须
    /// 与 `table` 出自同一次加载会话（否则反查会返回错误的标识符或
    /// `None`）。
    pub registry: &'a crate::registry::Registry,
}

impl QuestCatalog for RegisteredQuests<'_> {
    fn kill_count_quests(&self) -> Vec<QuestKillRule> {
        let mut rules = Vec::new();
        for index in self.table.defined_indices() {
            let Some(view) = self.table.get(index) else {
                continue;
            };
            let QuestCondition::KillCount { target_kind, count } = view.condition else {
                continue;
            };
            // 索引反查失败（传入的 registry 与 table 不是同一次加载
            // 产出）时静默跳过——与 ADR 0015「查不到就是查不到」同一条
            // 纪律，不 panic。
            let Some(quest_id) = self.registry.resolve(index) else {
                continue;
            };
            // 前置任务同样需要反查成 NamespacedId——见 QuestKillRule::
            // prerequisites 文档「这不是可选的装饰字段」一节：任何一个
            // 前置反查失败都整条规则跳过（保守处理，把"无法验证前置
            // 是否满足"当成"前置未满足"，不是当成"没有前置"）。
            let mut prerequisites = Vec::with_capacity(view.prerequisites.len());
            let mut prerequisites_resolved = true;
            for prerequisite in view.prerequisites {
                match self.registry.resolve(*prerequisite) {
                    Some(prerequisite_id) => prerequisites.push(prerequisite_id.clone()),
                    None => {
                        prerequisites_resolved = false;
                        break;
                    }
                }
            }
            if !prerequisites_resolved {
                continue;
            }
            rules.push(QuestKillRule {
                quest: quest_id.clone(),
                target_kind: *target_kind,
                required_count: *count,
                prerequisites,
            });
        }
        rules
    }
}

/// 本体基础任务节点在当前注册表里的索引缓存——**句柄，不是内容**。
///
/// 四条任务的字段值已经搬进 `mods/lostland/quests.json5`，本结构体只
/// 保住使用点的编译期安全，填充由 [`resolve_base_quests`] 在装载完成后
/// 按 id 逐字段解析完成，理由完整见 [`crate::class::BaseClassIds`] 与
/// [`crate::base_contract`] 两处文档。
///
/// 这四条构成一个**网状**（不是树状）的最小示例：`main_quest_1`
/// （起点）同时解锁 `branch_a`/`branch_b` 两条分支（一个前置解锁多个
/// 后续，验收"网"而不是"线性序列"）；`finale` 要求 `branch_a` 与
/// `branch_b` 两个前置同时满足才能开始（两条分支在此汇聚，验收"一个
/// 任务可以有多个前置"——这一点单靠"树"结构表达不了，节点只有一个父
/// 节点是树的定义性质，`finale` 有两个父节点，图因此不是树）。同时
/// 演示两档完成条件：`main_quest_1`/`branch_a`/`finale` 用一档
/// （`KillCount`），`branch_b` 用三档（`Script`）。
#[derive(Debug, Clone, Copy)]
pub struct BaseQuestIds {
    /// 起点任务：无前置。
    pub main_quest_1: ContentIndex,
    /// `main_quest_1` 的分支之一：一档完成条件。
    pub branch_a: ContentIndex,
    /// `main_quest_1` 的分支之一：三档完成条件（脚本回调）。
    pub branch_b: ContentIndex,
    /// 汇聚任务：要求 `branch_a` 与 `branch_b` 两个前置同时满足。
    pub finale: ContentIndex,
}

/// 本体四条基础任务的 id 字面量——[`resolve_base_quests`] 的契约清单，
/// 理由同 [`crate::class`] 的 `BASE_CLASS_IDS`。
const BASE_QUEST_IDS: [(&str, &str); 4] = [
    ("BaseQuestIds::main_quest_1", "lostland:main_quest_1"),
    ("BaseQuestIds::branch_a", "lostland:branch_a"),
    ("BaseQuestIds::branch_b", "lostland:branch_b"),
    ("BaseQuestIds::finale", "lostland:finale"),
];

/// 装载完成后解析本体任务契约：按 id 逐字段填充 [`BaseQuestIds`]，
/// 缺任何一条就整批失败。取代原先的 `materialize_base_quests`/
/// `base_quest_fixture`，理由同 [`crate::class::resolve_base_classes`]。
///
/// 与 [`crate::skill::resolve_base_skills`] 同一条纪律：环检查不在这里
/// 跑，它是**整张表**的性质，属于装载管线（见该函数文档
/// 「这里**不**跑 `validate_no_cycles`」一节）。
pub fn resolve_base_quests(
    registry: &Registry,
    table: &QuestTable,
) -> Result<BaseQuestIds, BaseContractError> {
    let mut resolver = BaseContractResolver::new("本体任务", registry);
    let mut resolved = BASE_QUEST_IDS
        .iter()
        .map(|(field, id)| resolver.require(field, id, |index| table.is_defined(index)));
    let main_quest_1 = resolved.next().expect("BASE_QUEST_IDS 恒有四条");
    let branch_a = resolved.next().expect("BASE_QUEST_IDS 恒有四条");
    let branch_b = resolved.next().expect("BASE_QUEST_IDS 恒有四条");
    let finale = resolved.next().expect("BASE_QUEST_IDS 恒有四条");
    drop(resolved);
    resolver.finish()?;

    Ok(BaseQuestIds {
        main_quest_1,
        branch_a,
        branch_b,
        finale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_contract::MissingReason;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn kill_count_attrs(prerequisites: Vec<ContentIndex>) -> QuestAttrs {
        QuestAttrs {
            prerequisites,
            condition: QuestCondition::KillCount {
                target_kind: ContentIndex::default(),
                count: 1,
            },
        }
    }

    /// 一张现造的、与本体内容无关的网状任务图。
    ///
    /// 本模块的单元测试验的是 [`QuestTable`]/[`unlocked_by`]/
    /// [`validate_no_cycles`] 这套**机制**，不是「本体有哪几条任务」
    /// ——后者的定义已经搬进 `mods/lostland/quests.json5`，由
    /// `crates/ll-mod/tests/base_mod_class_skill_quest.rs` 端到端逐字段
    /// 核对。这里用 `testmod:` 现造一张同形的图（`root` 解锁两条分支，
    /// `merge` 汇聚它们，其中一条分支用 `Script` 档条件），理由同
    /// [`crate::race`] 的 `sample_table`。
    struct SampleGraph {
        registry: Registry,
        table: QuestTable,
        root: ContentIndex,
        branch_a: ContentIndex,
        branch_b: ContentIndex,
        merge: ContentIndex,
    }

    fn sample_graph() -> SampleGraph {
        let mut registry = Registry::new();
        let mut table = QuestTable::new();
        let goblin = registry.intern(id("testmod:goblin"));

        let define = |registry: &mut Registry,
                      table: &mut QuestTable,
                      raw: &str,
                      prerequisites: Vec<ContentIndex>,
                      condition: QuestCondition| {
            let index = registry.intern(id(raw));
            table
                .define(
                    index,
                    QuestAttrs {
                        prerequisites,
                        condition,
                    },
                )
                .expect("首次定义应当成功");
            index
        };

        let root = define(
            &mut registry,
            &mut table,
            "testmod:root",
            Vec::new(),
            QuestCondition::KillCount {
                target_kind: goblin,
                count: 3,
            },
        );
        let branch_a = define(
            &mut registry,
            &mut table,
            "testmod:branch_a",
            vec![root],
            QuestCondition::KillCount {
                target_kind: goblin,
                count: 5,
            },
        );
        let branch_b = define(
            &mut registry,
            &mut table,
            "testmod:branch_b",
            vec![root],
            QuestCondition::Script(id("testmod:branch_b_condition")),
        );
        let merge = define(
            &mut registry,
            &mut table,
            "testmod:merge",
            vec![branch_a, branch_b],
            QuestCondition::KillCount {
                target_kind: goblin,
                count: 1,
            },
        );

        SampleGraph {
            registry,
            table,
            root,
            branch_a,
            branch_b,
            merge,
        }
    }

    /// 把 [`BASE_QUEST_IDS`] 四条全部注册进一张表，字段值填测试占位
    /// 值——[`resolve_base_quests`] 成功路径的最小前置。
    fn registry_with_all_base_quests() -> (Registry, QuestTable) {
        let mut registry = Registry::new();
        let mut table = QuestTable::new();
        for (_, raw) in BASE_QUEST_IDS {
            let index = registry.intern(id(raw));
            table
                .define(index, kill_count_attrs(Vec::new()))
                .expect("首次定义应当成功");
        }
        (registry, table)
    }

    #[test]
    fn 网状结构一个前置任务解锁多个后续任务() {
        // Arrange
        let graph = sample_graph();

        // Act
        let branches = [graph.branch_a, graph.branch_b];
        let all_reference_root = branches
            .iter()
            .all(|&branch| graph.table.get(branch).expect("已注册").prerequisites == [graph.root]);

        // Assert
        assert!(all_reference_root);
    }

    #[test]
    fn 网状结构一个任务节点要求多个前置同时满足() {
        // Arrange
        let graph = sample_graph();

        // Act
        let view = graph.table.get(graph.merge).expect("merge 已注册");

        // Assert：两个前置都在,证明这不是一棵树（树里每个节点只有一个
        // 父节点)。
        assert_eq!(view.prerequisites, &[graph.branch_a, graph.branch_b]);
    }

    #[test]
    fn 脚本回调型条件与击杀计数型条件走同一条define路径() {
        // 边界：本测试只证明 `QuestCondition::Script(id)` 这个数据值
        // 能与 `KillCount` 走同一条 `Registry::intern`/`define` 路径
        // 注册，不运行任何脚本、也不证明"脚本回调"这四个字所暗示的
        // 东西——`QuestCondition::Script` 目前只是一个携带命名空间 ID
        // 的数据标签。真正的脚本可达证据在 `crate::pipeline` 的脚本
        // 装载测试与 `mods/lostland/quests.json5`。
        // Arrange
        let graph = sample_graph();

        // Act
        let view = graph.table.get(graph.branch_b).expect("branch_b 已注册");

        // Assert
        assert_eq!(
            view.condition,
            &QuestCondition::Script(id("testmod:branch_b_condition"))
        );
    }

    #[test]
    fn unlocked_by对给定已完成集合返回正确的后续节点() {
        // Arrange
        let graph = sample_graph();

        // Act：只完成 root——两条分支都应解锁,merge 还不该解锁（它还
        // 需要 branch_a/branch_b 都完成)。
        let unlocked = unlocked_by(&graph.table, &[graph.root]);

        // Assert
        assert_eq!(unlocked, vec![graph.branch_a, graph.branch_b]);
    }

    #[test]
    fn unlocked_by在两条分支都完成后解锁汇聚任务() {
        // Arrange
        let graph = sample_graph();

        // Act
        let unlocked = unlocked_by(&graph.table, &[graph.root, graph.branch_a, graph.branch_b]);

        // Assert
        assert_eq!(unlocked, vec![graph.merge]);
    }

    #[test]
    fn unlocked_by不是存储两次调用产出相同结果() {
        // 验收"纯函数性质"：同一个 table/completed，多次调用不应该因为
        // 内部状态变化而产出不同结果（本函数根本不持有任何可变状态)。
        // Arrange
        let graph = sample_graph();
        let completed = [graph.root];

        // Act
        let first = unlocked_by(&graph.table, &completed);
        let second = unlocked_by(&graph.table, &completed);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn unlocked_by对空的已完成集合只返回无前置的起点任务() {
        // Arrange
        let graph = sample_graph();

        // Act
        let unlocked = unlocked_by(&graph.table, &[]);

        // Assert
        assert_eq!(unlocked, vec![graph.root]);
    }

    #[test]
    fn 任务前置关系形成环时注册失败() {
        // Arrange：a 需要 b，b 需要 a——二节点环。
        let mut registry = Registry::new();
        let a = registry.intern(id("yourmod:a"));
        let b = registry.intern(id("yourmod:b"));
        let mut table = QuestTable::new();
        table
            .define(a, kill_count_attrs(vec![b]))
            .expect("a 定义应当成功");
        table
            .define(b, kill_count_attrs(vec![a]))
            .expect("b 定义应当成功");

        // Act
        let result = validate_no_cycles(&table);

        // Assert
        match result {
            Err(QuestError::CyclicPrerequisites(cycle)) => {
                assert_eq!(cycle.len(), 2);
                assert!(cycle.contains(&a) && cycle.contains(&b));
            }
            other => panic!("期望 CyclicPrerequisites，实际是 {other:?}"),
        }
    }

    #[test]
    fn 前置引用未注册的索引时报告悬空引用而非静默通过() {
        // Arrange
        let mut registry = Registry::new();
        let a = registry.intern(id("yourmod:a"));
        let ghost = registry.intern(id("yourmod:ghost"));
        let mut table = QuestTable::new();
        table
            .define(a, kill_count_attrs(vec![ghost]))
            .expect("a 定义应当成功");

        // Act
        let result = validate_no_cycles(&table);

        // Assert
        assert_eq!(
            result,
            Err(QuestError::UnregisteredPrerequisite {
                quest: a,
                missing: ghost
            })
        );
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // Arrange
        let mut registry = Registry::new();
        let index = registry.intern(id("testmod:root"));
        let mut table = QuestTable::new();
        table
            .define(index, kill_count_attrs(Vec::new()))
            .expect("首次定义应当成功");

        // Act
        let result = table.define(index, kill_count_attrs(Vec::new()));

        // Assert
        assert_eq!(result, Err(QuestError::DuplicateDefinition(index)));
    }

    #[test]
    fn 未注册的内容索引查询返回none() {
        // Arrange
        let mut registry = Registry::new();
        let never_defined = registry.intern(id("yourmod:never_defined"));
        let table = QuestTable::new();

        // Act
        let result = table.get(never_defined);

        // Assert
        assert_eq!(result, None);
    }

    #[test]
    fn 后注册的mod任务可以把先注册的任务当作前置() {
        // 结构等价断言：本体任务与 mod 任务共享同一张表、同一套校验，
        // 没有任何一条只对本体开放的旁路——本体任务现在也走
        // `mods/lostland/quests.json5` 的 `register-quest`。
        // Arrange
        let mut graph = sample_graph();

        // Act
        let mod_index = graph.registry.intern(id("yourmod:side_quest"));
        graph
            .table
            .define(
                mod_index,
                QuestAttrs {
                    prerequisites: vec![graph.root],
                    condition: QuestCondition::KillCount {
                        target_kind: ContentIndex::default(),
                        count: 2,
                    },
                },
            )
            .expect("mod 任务与先注册的任务调用同一个公开 define 函数,理应同样成功");

        // Assert
        let view = graph
            .table
            .get(mod_index)
            .expect("mod 任务已通过 define 登记");
        assert_eq!(view.prerequisites, &[graph.root]);
    }

    #[test]
    fn 网状任务图通过完整dag校验() {
        // Arrange
        let graph = sample_graph();

        // Act
        let result = validate_no_cycles(&graph.table);

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    fn quest_progress_key对不同任务产出不同的键() {
        // Arrange
        let quest_a = id("lostland:main_quest_1");
        let quest_b = id("lostland:branch_a");

        // Act & Assert
        assert_ne!(quest_progress_key(&quest_a), quest_progress_key(&quest_b));
    }

    #[test]
    fn 四条本体任务都在时契约解析成功且返回真实索引() {
        // Arrange
        let (registry, table) = registry_with_all_base_quests();

        // Act
        let ids = resolve_base_quests(&registry, &table).expect("四条都在，解析应当成功");

        // Assert
        assert_eq!(
            registry.resolve(ids.main_quest_1).map(|id| id.to_string()),
            Some("lostland:main_quest_1".to_string())
        );
        assert_eq!(
            registry.resolve(ids.finale).map(|id| id.to_string()),
            Some("lostland:finale".to_string())
        );
    }

    #[test]
    fn 本体任务一条都没注册时契约解析一次列出全部四条() {
        // Arrange
        let registry = Registry::new();
        let table = QuestTable::new();

        // Act
        let error = resolve_base_quests(&registry, &table).expect_err("空注册表必须解析失败");

        // Assert
        assert_eq!(error.contract, "本体任务");
        assert_eq!(error.required, 4);
        assert_eq!(error.missing.len(), 4);
    }

    #[test]
    fn 任务id只被intern没被define时契约解析报notdefined() {
        // Arrange
        let mut registry = Registry::new();
        for (_, raw) in BASE_QUEST_IDS {
            registry.intern(id(raw));
        }
        let table = QuestTable::new();

        // Act
        let error =
            resolve_base_quests(&registry, &table).expect_err("只 intern 未 define 必须失败");

        // Assert
        assert!(
            error
                .missing
                .iter()
                .all(|entry| entry.reason == MissingReason::NotDefined)
        );
    }

    /// P5-B 任务 7：任务进度持久化——脚本状态存储接线的测试。
    mod progress_persistence {
        use super::*;
        use ll_sim::apply::apply;
        use ll_sim::effect::Effect;
        use ll_world::entity::{Agent, BaseStats};
        use ll_world::generate::GenParams;
        use ll_world::space::Space;
        use ll_world::state::WorldState;
        use ll_world::terrain::base_terrain_fixture;
        use ll_world::zone::ZoneLayout;
        use std::collections::BTreeMap;

        /// 测试用最小世界——理由同 `ll_sim::apply` 测试模块的同名帮手：
        /// 只需要满足 `WorldState::new` 的前置条件，具体地形/尺寸细节
        /// 不影响本模块要验证的行为（任务进度只挂在 `Agent` 上）。
        fn test_world() -> WorldState {
            let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
            let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
            let (terrain_ids, terrain_table) = base_terrain_fixture();
            let spawn = layout.tile_size().wrap(0, 0);
            WorldState::new(
                layout,
                &GenParams::default(),
                &terrain_ids,
                terrain_table,
                spawn,
            )
            .expect("测试布局满足全部构造前置条件")
        }

        /// 一份内部自洽的空白 `Agent`——字段列表需要与
        /// `ll_world::entity::Agent` 保持同步，理由同
        /// `ll_sim::apply` 测试模块的同名帮手。
        fn blank_agent(world: &WorldState) -> Agent {
            let mut registry = Registry::new();
            let profession = registry.intern(id("testmod:tester"));
            let race = registry.intern(id("testmod:human"));
            let pos = world.size.wrap(0, 0);
            let (zone, _) = world.terrain.layout().tile_to_zone(pos);
            Agent {
                pos,
                stats: BaseStats::BASELINE,
                next_action_at: ll_core::time::Tick(0),
                health: Agent::STARTING_HEALTH,
                affiliations: Vec::new(),
                wallet: 0,
                profession,
                goals: Vec::new(),
                race,
                mana: Agent::STARTING_MANA,
                stamina: Agent::STARTING_STAMINA,
                resource_pools: std::collections::BTreeMap::new(),
                spent_slots: std::collections::BTreeMap::new(),
                inventory: Vec::new(),
                equipment: std::collections::BTreeMap::new(),
                resting: None,
                unlocked_skills: Vec::new(),
                known_recipes: Vec::new(),
                skill_cooldowns: BTreeMap::new(),
                subclasses: Vec::new(),
                active_stat_modifiers: BTreeMap::new(),
                current_space: Space::surface(zone, ContentIndex::default()),
                mod_state: BTreeMap::new(),
                creature_kind: None,
                spawned_at: ll_core::time::Tick(0),
                remembered_id: None,
                level: ll_world::entity::Agent::STARTING_LEVEL,
                experience: 0,
                xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
                unspent_attribute_points: 0,
                unspent_skill_points: 0,
                stealthed: false,
            }
        }

        #[test]
        fn 任务进度写入后可以在同一会话内读回() {
            // Arrange
            let mut world = test_world();
            let actor = world.actors.spawn(blank_agent(&world));
            let quest = id("lostland:main_quest_1");

            // Act：mark_quest_completed 只产出数据，真正落盘经
            // Effect::SetModState -> apply（裁定 P5-1）。
            let write = mark_quest_completed(actor, &quest);
            apply(
                &mut world,
                &Effect::SetModState {
                    writes: vec![write],
                },
            );

            // Assert
            let agent = world.actors.get(actor).expect("刚生成的实体必然存在");
            assert!(is_quest_completed(agent, &quest));
        }

        #[test]
        fn 未标记完成的任务查询为未完成() {
            // Arrange：没有任何写入——防御性测试，确认"没写过"与"写过"
            // 在读取侧确实是两种不同的可观察结果，不是哨兵值恰好等于
            // "已完成"这种巧合。
            let world = test_world();
            let agent = blank_agent(&world);
            let quest = id("lostland:main_quest_1");

            // Act & Assert
            assert!(!is_quest_completed(&agent, &quest));
        }

        #[test]
        fn 任务进度经worldstate序列化往返后保持一致() {
            // Arrange
            let mut world = test_world();
            let actor = world.actors.spawn(blank_agent(&world));
            let quest = id("lostland:main_quest_1");
            apply(
                &mut world,
                &Effect::SetModState {
                    writes: vec![mark_quest_completed(actor, &quest)],
                },
            );

            // Act：完整序列化往返，模拟存档读写（P5-A 任务 9 的
            // `ll-content` 管线尚未接线时的替代验证方式，见实施计划
            // 任务 7「必须验证」一节）。
            let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
            let reloaded: WorldState = serde_json::from_slice(&encoded).expect("往返不应失败");

            // Assert
            let agent = reloaded.actors.get(actor).expect("往返后实体仍然存在");
            assert!(is_quest_completed(agent, &quest));
        }

        #[test]
        fn 不同mod命名空间的任务进度互不干扰() {
            // Arrange：两个不同命名空间、但路径部分相同的任务节点——
            // 若命名空间隔离出了问题（例如键里漏掉了命名空间），两者会
            // 在存储里互相覆盖。
            let mut world = test_world();
            let actor = world.actors.spawn(blank_agent(&world));
            let quest_a = id("moda:shared_path");
            let quest_b = id("modb:shared_path");

            // Act：只标记 quest_a 完成。
            apply(
                &mut world,
                &Effect::SetModState {
                    writes: vec![mark_quest_completed(actor, &quest_a)],
                },
            );

            // Assert：quest_a 已完成，quest_b（不同命名空间）不受影响。
            let agent = world.actors.get(actor).expect("刚生成的实体必然存在");
            assert!(is_quest_completed(agent, &quest_a));
            assert!(!is_quest_completed(agent, &quest_b));
        }
    }
}
