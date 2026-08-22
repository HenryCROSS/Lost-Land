//! 装载后内容校验：把「内容搬进脚本之后丢掉的那批编译期检查」补回来
//! 的第二层与第三层。
//!
//! # 与 [`crate::base_contract`] 的分层关系
//!
//! 本体内容的**定义**迁进 `mods/lostland/*.scm` 之后，Rust 一次丢掉了
//! 好几种此前免费拿到的保证。第一层已经由 [`crate::base_contract`]
//! 补回：「Rust 代码点名引用的那几条本体内容此刻在不在」。但那一层只
//! 看**句柄结构体点名的那几条 id**，看不见内容自身的形状——一条种族
//! 定义把 `owning_class` 指向一个不存在的职业、或者某个字段全仓库
//! 没有任何一条内容给它赋过非默认值，契约解析全都照样通过。
//!
//! 本模块补的是这两件事：
//!
//! - **引用完整性**（[`ReferenceViolation`]）：每一个跨表的
//!   `ContentIndex` 字段都必须指向一条**真的被对应内容表定义过**的
//!   条目。这一条对**全部**已装载内容生效（本体的与第三方 mod 的一视
//!   同仁），是装载管线的**硬失败**条件——一个把 `damage_formula` 指向
//!   拼错 id 的 mod，与其让它安静地退化成"查不到就当没有"，不如在装载
//!   那一刻点名报出来。
//! - **字段覆盖**（[`UncoveredField`]）：每个内容表声明的每个字段，
//!   都必须**至少被一条已装载内容设成非默认值**。这一条只对
//!   [`ContentAuditPolicy::namespace`] 指定的那个命名空间（生产环境是
//!   本体自己的 `lostland`）生效，理由见下面「字段覆盖为什么只看本体
//!   命名空间」一节。
//!
//! # 与 `scripts/ci/check_field_consumers.py` 的分工：两头都要堵
//!
//! 那个脚本查的是「**Rust 决策层里有没有人读**这个字段」，抓的是
//! `Agent.luck`/`RaceDef.darkvision_floor` 这类"声明了、存了、哈希了、
//! 有往返测试，却没有任何游戏逻辑消费它"的字段。
//!
//! 本模块的字段覆盖查的是**另一头**：「**内容里有没有人写**这个字段」。
//! 两者互相不能替代，一个字段完全可能：
//!
//! - 决策层读了、但没有任何一条内容给它非默认值（例如
//!   `RaceAttrs.xp_reward`：`ll_sim::experience` 真的会用它算击杀经验，
//!   但本体三个种族全都不声明，于是这条规则在本体内容上永远是死的）
//!   ——脚本判"已接线"，本模块判"未覆盖"；
//! - 内容写了、但决策层没人读（例如 `ClassDef.primary_attribute`：
//!   `mods/example_mod/gameplay.scm` 逐条声明了它，但没有任何结算逻辑
//!   读它）——本模块判"已覆盖"，脚本判"未接线"。
//!
//! 两条检查同时绿，才说明这个字段既有人写、也有人读。这正是本项目
//! 反复复发那 25 处「声明了却从没接线」时缺的那一半。
//!
//! # 为什么做成装载管线里的一个 Rust pass，而不是一个静态脚本检查器
//!
//! 项目所有者已经裁定，理由记在这里免得下次重新论证：
//!
//! 1. **零漂移**：本 pass 走的就是真实装载路径，看到的就是真实装载
//!    出来的内容表。一份手工维护的 schema/脚本解析器与真实注册函数
//!    之间迟早分叉——`ll_mod::content_hash` 模块文档「起因」一节记录的
//!    正是同一类漂移真的发生过。
//! 2. **有真类型**：这里拿到的是 `RaceView`/`ItemView` 这些真结构体，
//!    不需要解析 S 表达式去猜第几个参数是什么。
//! 3. **顺带保护玩家**：引用完整性这一层随游戏一起发货，玩家装了一个
//!    引用写错的 mod 时会在启动那一刻被响亮告知，而不是等到某个物品
//!    算不出伤害。
//!
//! # 字段覆盖为什么只看本体命名空间
//!
//! 「每个字段至少被一条内容设成非默认值」如果按**全部已装载内容**
//! 求并集，结论就会取决于玩家装了哪些 mod：本体自己一条都没覆盖的
//! 字段，可能因为某个第三方 mod 恰好用了它而变绿；那个 mod 被卸载后
//! 又变红。一条**取决于玩家装了什么**的检查既不能当门禁（不可复现），
//! 也不能当启动期硬错误（拿别人的 mod 惩罚玩家）。
//!
//! 因此字段覆盖固定只统计 [`ContentAuditPolicy::namespace`] 命名空间
//! 下的条目。生产策略 [`BASE_CONTENT_AUDIT`] 把它定在 `lostland`
//! ——本体自己的内容，项目所有者百分之百控制、也百分之百负责的那一
//! 部分，正是"声明了字段却从没有内容用它"这个病的发病部位。
//!
//! # 表花名册：不允许"这张表还没有内容，所以静默跳过"
//!
//! 本体内容目前只迁走了种族，其余内容表在 `lostland` 命名空间下**一条
//! 内容都没有**（职业/技能/副职/任务/天赋/资源池/物品/武器类别至今只
//! 存在于 `mods/example_mod/`）。对这些表求字段覆盖只会得到"全部字段
//! 都没覆盖"这种纯噪音。
//!
//! 但"没有内容就跳过"如果做成隐式行为，本模块就会在内容迁移完成那天
//! **静默地什么都不做**——恰恰是这类静默缺口让本项目反复漏掉东西。
//! 所以 [`ContentAuditPolicy`] 要求把每一张表显式登记成两种之一：
//!
//! - [`ContentAuditPolicy::covered`]：在检查范围内。若这张表在本命名
//!   空间下一条内容都没有，报 [`RosterViolation::CoveredButEmpty`]
//!   ——"你说它在范围内，可它是空的"。
//! - [`ContentAuditPolicy::deferred`]：显式推迟，必须写明理由。若这张
//!   表在本命名空间下**已经有内容了**，报
//!   [`RosterViolation::DeferredButPopulated`]——"内容已经搬过来了，
//!   该把它挪进 `covered` 了"。
//!
//! 两个方向都检查，与 `scripts/ci/check_field_consumers.py` 的
//! `EXEMPTIONS` 死豁免检查是同一条纪律：豁免清单本身也会失效，失效的
//! 豁免比没有豁免更危险。[`ContentTableKind`] 新增一个变体时，
//! [`roster_slot`] 那个不带通配分支的 `match` 会直接编译失败，逼下一个
//! 人先做出"新表进 `covered` 还是进 `deferred`"这个决定。
//!
//! # 字段豁免：有些字段合法地保持默认值
//!
//! [`ContentAuditPolicy::exemptions`] 逐条登记"这个字段的默认值就是
//! 本体内容想要的取值"，**每一条必须写明理由**（照
//! `check_field_consumers.py` 的 `EXEMPTIONS` 先例）。同样双向检查：
//! 一条豁免对应的字段如果后来真的被某条内容覆盖了，报
//! [`RosterViolation::StaleExemption`]，逼人把豁免摘掉；一条豁免如果
//! 指向一个本 pass 压根没有观察过的字段名（字段改名/删除/所属表进了
//! `deferred`），报 [`RosterViolation::UnknownExemption`]。
//!
//! # 错误呈现：一次列出全部，不是撞见第一条就返回
//!
//! 沿用 [`crate::base_contract`] 模块文档确立的那条纪律，理由完全相同
//! ——这两类错误的读者是玩家与 mod 作者，"补一条、重启、再被告知缺下
//! 一条"是最难受的呈现。本 pass 因此总是把整份内容走完，把全部违规
//! 收进 [`ContentAuditReport`] 再由调用方一次性报出。
//!
//! # 确定性（约束 C5）
//!
//! 遍历的唯一入口是
//! [`Registry::snapshot`](crate::registry::Registry::snapshot)，返回的
//! 是**注册顺序**的 `Vec`（内部是一个保序 interner，不是 `HashMap`）。
//! 字段覆盖的累积用 `Vec` 线性查找而不是 `HashMap`，因此
//! [`ContentAuditReport`] 里三份列表的顺序完全由"内容注册顺序 + 字段
//! 声明顺序"决定，同一份内容跑两次逐条相同——否则这份报告没法被测试
//! 逐条断言。
//!
//! # 谁调用
//!
//! 生产装载路径 `ll_game::content::load_content` 在
//! [`crate::base_contract`] 契约解析通过之后调用一次：引用完整性直接
//! 变成 `load_content` 的失败原因之一；字段覆盖不阻断启动（本体内容
//! 被玩家改动过并不是一条"游戏坏了"的信号），由 `ll-game` 的门禁测试
//! 对仓库真实 `mods/` 目录断言为空。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_sim::formula::FormulaDef;
use ll_sim::skill::ResourceCost;
use ll_sim::xp_curve::XpCurveDef;
use ll_world::terrain::TerrainKind;

use crate::content_hash::{ContentTableKind, ContentValueTables, classify_index};
use crate::quest::QuestCondition;
use crate::registry::Registry;
use crate::trait_def::RuleModifier;

/// 一个跨表引用字段期望它的目标落在哪里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceExpectation {
    /// 目标必须是这张内容表里一条**已定义**的条目。
    Table(ContentTableKind),
    /// 目标是一个**没有对应内容表**的开放标识符空间，本 pass 不检查
    /// 它——这就是「只 intern、不 define 也合法」这类内容的豁免出口。
    ///
    /// 唯一的现役用例是
    /// [`QuestCondition::KillCount::target_kind`](crate::quest::QuestCondition)：
    /// 它指向"敌人类型"，而代码库至今没有敌人类型注册表，见
    /// [`crate::quest`] 模块文档「跨表引用」一节。把它按
    /// [`ReferenceExpectation::Table`] 检查会把一条**正确的设计**判成
    /// 错误。
    ///
    /// 同一条豁免机制也是 [`crate::base_placeholder`] 那个
    /// `lostland:placeholder_race`（[`crate::race::RaceTable::get`] 对
    /// 它恒返回 `None` 是设计不是遗漏，见 [`crate::race`] 模块文档
    /// 「与 `lostland:placeholder_race` 的协调」一节）将来若被某个内容
    /// 字段引用时该走的出口。**当前它不被任何内容字段引用**——它只被
    /// `ll_sim` 的运行期降级分支当作 `Agent.race` 的取值使用，运行期
    /// 状态不在本 pass 的观察范围内——所以本模块没有为它单独维护一条
    /// id 级豁免（一条永远不会被触发的豁免就是一条死豁免，见模块文档
    /// 「字段豁免」一节对死豁免的立场）。
    UntypedIdSpace,
}

/// 一条引用完整性违规：某条内容的某个跨表引用字段指向了一个"对应
/// 内容表并没有定义它"的索引。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceViolation {
    /// 发出这条引用的内容归属哪张表。
    pub source_kind: ContentTableKind,
    /// 发出这条引用的内容自己的 id。
    pub source_id: NamespacedId,
    /// 字段名，例如 `"ItemDef::damage_formula"`——与
    /// [`crate::base_contract::MissingBaseContent::field`] 同一条理由：
    /// 读到这条错误的人下一步要去看的是"谁在用它"。
    pub field: &'static str,
    /// 被指向的那个索引在注册表里对应的 id；`None` 表示这个索引在本次
    /// 装载的注册表里压根反查不到（理论上不可达，见
    /// [`audit_content`] 文档）。
    pub target_id: Option<NamespacedId>,
    /// 这个字段期望目标落在哪张表。
    pub expected: ContentTableKind,
    /// 目标实际落在哪张表；[`ContentTableKind::Opaque`] 表示"这个 id
    /// 被 intern 过，但没有任何内容表定义它"。
    pub actual: ContentTableKind,
}

/// 一个从来没有被任何一条本命名空间内容设成非默认值的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncoveredField {
    /// 字段所属的内容表。
    pub kind: ContentTableKind,
    /// 字段名，例如 `"RaceAttrs::xp_reward"`。
    pub field: &'static str,
}

/// 花名册/豁免清单自身失效——这类违规针对的不是内容，而是本模块的
/// 策略声明本身，见模块文档「表花名册」「字段豁免」两节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterViolation {
    /// 这张表被登记为在检查范围内，但本命名空间下一条内容都没有。
    CoveredButEmpty {
        /// 出问题的表。
        kind: ContentTableKind,
    },
    /// 这张表被显式推迟，但本命名空间下已经有内容了——内容搬过来了，
    /// 该把它挪进 [`ContentAuditPolicy::covered`]。
    DeferredButPopulated {
        /// 出问题的表。
        kind: ContentTableKind,
        /// 当初写下的推迟理由，进错误文案供人对照。
        reason: &'static str,
    },
    /// 这张表既不在 [`ContentAuditPolicy::covered`] 也不在
    /// [`ContentAuditPolicy::deferred`] 里——策略声明漏了一张表。
    UnclassifiedTable {
        /// 漏登记的表。
        kind: ContentTableKind,
    },
    /// 同一张表同时出现在 `covered` 与 `deferred` 里。
    ContradictoryTable {
        /// 出问题的表。
        kind: ContentTableKind,
    },
    /// [`ContentTableKind::Opaque`] 出现在了 `covered` 或 `deferred`
    /// 里——它不是一张内容表，见 [`check_roster`] 里对它的处理。
    OpaqueMustNotBeClassified,
    /// 一条字段豁免对应的字段现在已经被内容覆盖了——豁免该摘掉。
    StaleExemption {
        /// 字段所属的表。
        kind: ContentTableKind,
        /// 字段名。
        field: &'static str,
    },
    /// 一条字段豁免指向一个本 pass 压根没有观察到的字段（字段改名/
    /// 删除，或它所属的表进了 `deferred`）。
    UnknownExemption {
        /// 豁免声明的表。
        kind: ContentTableKind,
        /// 豁免声明的字段名。
        field: &'static str,
    },
}

/// 一条字段豁免：这个字段合法地保持默认值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldExemption {
    /// 字段所属的内容表。
    pub kind: ContentTableKind,
    /// 字段名，写法与 [`UncoveredField::field`] 完全一致。
    pub field: &'static str,
    /// **必填**理由——照 `scripts/ci/check_field_consumers.py` 的
    /// `EXEMPTIONS` 先例：一条没有理由的豁免等于一次静默跳过。
    pub reason: &'static str,
}

/// 一张显式推迟检查的内容表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredTable {
    /// 推迟的表。
    pub kind: ContentTableKind,
    /// **必填**理由，同 [`FieldExemption::reason`]。
    pub reason: &'static str,
}

/// 一次内容校验的策略：查哪个命名空间的字段覆盖、哪些表在范围内、
/// 哪些字段合法地保持默认值。
///
/// 引用完整性**不受本策略约束**——它对全部已装载内容一视同仁生效，
/// 见模块文档。
pub struct ContentAuditPolicy {
    /// 字段覆盖只统计这个命名空间下的内容，见模块文档「字段覆盖为
    /// 什么只看本体命名空间」一节。
    pub namespace: &'static str,
    /// 在字段覆盖检查范围内的内容表。
    pub covered: &'static [ContentTableKind],
    /// 显式推迟字段覆盖检查的内容表。
    pub deferred: &'static [DeferredTable],
    /// 合法保持默认值的字段。
    pub exemptions: &'static [FieldExemption],
}

/// 生产策略：本体内容（`lostland` 命名空间）的字段覆盖。
///
/// # `covered` 当前为什么只有七张表
///
/// 本体内容迁往 mod 脚本这件事只完成了第一批（种族）。其余表在
/// `lostland` 命名空间下的条目数如下（数据来自仓库真实 `mods/` 目录的
/// 一次完整装载，不是估计）：地形 17、空间层属性 4、动画剪辑 2、经验
/// 曲线 1、伤害公式 1、伤害类别 1、种族 3——这七张有内容；职业/技能/
/// 副职/任务/天赋/资源池/物品/武器类别八张在 `lostland` 下**一条都
/// 没有**（它们只存在于 `mods/example_mod/`）。
///
/// 对空表求字段覆盖，得到的是"这张表的每个字段都没覆盖"这种与内容
/// 本身无关的纯噪音——那不是这条检查该报的错。八张空表因此逐条登记
/// 进 [`ContentAuditPolicy::deferred`] 并写明理由，等对应内容真的迁进
/// `mods/lostland/` 那天，[`RosterViolation::DeferredButPopulated`]
/// 会主动提醒把它挪进 `covered`，不需要有人记得回来看这里。
///
/// [`ContentTableKind::Opaque`] 两个清单里都没有，而且**必须都没有**
/// ——它不是一张内容表，见 [`check_roster`] 里对它的处理。
pub const BASE_CONTENT_AUDIT: ContentAuditPolicy = ContentAuditPolicy {
    namespace: "lostland",
    covered: &[
        ContentTableKind::Terrain,
        ContentTableKind::Race,
        ContentTableKind::SpaceProfile,
        ContentTableKind::Clip,
        ContentTableKind::XpCurve,
        ContentTableKind::Formula,
        ContentTableKind::DamageCategory,
    ],
    deferred: &[
        DeferredTable {
            kind: ContentTableKind::Class,
            reason: "本体职业尚未迁进 mods/lostland/：lostland 命名空间下零条职业内容，\
                     ClassTable 在生产装载路径里拿到的全部条目都来自 mods/example_mod/。\
                     迁移完成后本条会被 DeferredButPopulated 主动顶掉。",
        },
        DeferredTable {
            kind: ContentTableKind::Skill,
            reason: "本体技能尚未迁进 mods/lostland/，理由同 Class 一条。",
        },
        DeferredTable {
            kind: ContentTableKind::Subclass,
            reason: "本体副职尚未迁进 mods/lostland/，理由同 Class 一条。",
        },
        DeferredTable {
            kind: ContentTableKind::Quest,
            reason: "本体任务尚未迁进 mods/lostland/，理由同 Class 一条。",
        },
        DeferredTable {
            kind: ContentTableKind::Trait,
            reason: "本体天赋尚未迁进 mods/lostland/，理由同 Class 一条——本体三个种族\
                     当前不授予任何天赋，天赋内容只存在于 mods/example_mod/。",
        },
        DeferredTable {
            kind: ContentTableKind::ResourcePool,
            reason: "本体资源池尚未迁进 mods/lostland/，理由同 Class 一条。",
        },
        DeferredTable {
            kind: ContentTableKind::Item,
            reason: "本体物品尚未迁进 mods/lostland/，理由同 Class 一条。",
        },
        DeferredTable {
            kind: ContentTableKind::WeaponCategory,
            reason: "本体武器类别尚未迁进 mods/lostland/，理由同 Class 一条。",
        },
    ],
    exemptions: &[
        FieldExemption {
            kind: ContentTableKind::Race,
            field: "RaceAttrs::xp_reward",
            reason: "本体三族是可玩种族不是猎物，「杀死它给多少经验」对它们没有意义，\
                     mods/lostland/races.scm 因此刻意不调用 register-race-xp-reward\
                     （该文件末尾的注释白纸黑字写着这一条）。字段本身不是死的：\
                     mods/example_mod/gameplay.scm 的 examplemod:goblin 声明了 15 点，\
                     ll_sim::experience 真的会读它——只是本体内容用不到。",
        },
        FieldExemption {
            kind: ContentTableKind::Race,
            field: "RaceAttrs::traits",
            reason: "本体三族当前不授予任何种族天赋，与 xp_reward 一条同源：\
                     races.scm 刻意不调用 register-race-trait。天赋系统落地批次的\
                     真实证据在 mods/example_mod/（examplemod:ooze 等），本体这一侧\
                     等内容设计真的需要时再补，不为了让检查变绿硬塞一条天赋。",
        },
        FieldExemption {
            kind: ContentTableKind::Race,
            field: "RaceAttrs::starting_items",
            reason: "出生装备走 register-race-starting-item 追加指令，本体三族不声明——\
                     理由同 traits 一条。本体物品内容本身也还没迁进 mods/lostland/\
                     （见 deferred 里的 Item 一条），本体种族此刻没有可指的本体物品。",
        },
        FieldExemption {
            kind: ContentTableKind::SpaceProfile,
            field: "SpaceProfileAttrs::reverb_tag",
            reason: "混响标签是给未来的音频层留的开放标识符，本体四种空间层属性\
                     （地表/洞窟/地下城/建筑内部）全部不声明——代码库至今没有音频层，\
                     现在填一个没有任何消费者的标签只是制造又一处「声明了没人读」。",
        },
        FieldExemption {
            kind: ContentTableKind::Clip,
            field: "Clip::exit_grace_frames",
            reason: "退出宽限帧是给「状态切换时不要立刻截断动画」留的旋钮，本体行走/\
                     待机两段剪辑都是循环剪辑，切出时立刻停止就是想要的表现，\
                     ll_render::anim::base_hero_clips 因此两段都填 0。\
                     见 crate::clip 模块文档「exit_grace_frames 暴露给脚本」一节。",
        },
        FieldExemption {
            kind: ContentTableKind::DamageCategory,
            field: "DamageCategoryDef::default_formula",
            reason: "「类别默认公式」是伤害公式三级下探（物品 → 类别 → 全局）的中间\
                     一级，本体唯一的伤害类别 lostland:physical 刻意不声明它，\
                     让它继续下探到全局默认公式——None 就是这里想要的语义，\
                     见 crate::damage_category 模块文档「本批次范围」一节。",
        },
    ],
};

/// 一次内容校验的完整产出。三份列表各自的顺序都是确定性的，见模块
/// 文档「确定性」一节。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentAuditReport {
    /// ②：引用完整性违规，按"内容注册顺序 → 字段声明顺序"排列。
    pub reference_violations: Vec<ReferenceViolation>,
    /// ③：从没被任何一条本命名空间内容设成非默认值的字段，按"首次
    /// 观察到该字段的顺序"排列（即内容注册顺序 → 字段声明顺序）。
    pub uncovered_fields: Vec<UncoveredField>,
    /// 花名册/豁免清单自身的失效。
    pub roster_violations: Vec<RosterViolation>,
    /// 本次校验实际检查过多少处跨表引用（不含
    /// [`ReferenceExpectation::UntypedIdSpace`] 那些按设计跳过的）。
    ///
    /// # 这个计数是干什么用的：防「空转通过」
    ///
    /// 一条一处引用都没检查到的引用完整性校验会**恒为绿**，而且绿得
    /// 完全无声——谁不小心把某个 `inspect_*` 里的 `reference` 调用删了、
    /// 或者把整张表的分派接错，报告仍然是"零违规"。这正是本项目反复
    /// 吃亏的那类静默缺口（`ll_mod::content_hash` 模块文档「起因」一节
    /// 记的是同一个病）。把"检查了多少处"如实带出来，门禁测试就能
    /// 断言"真实内容确实喂了这条检查非零的量"，而不是只断言它没报错。
    pub references_checked: usize,
    /// 本次校验实际观察过多少个字段槽（`(表, 字段)` 去重之后），理由
    /// 同 [`Self::references_checked`]。
    pub fields_observed: usize,
}

impl ContentAuditReport {
    /// ②：引用完整性——生产装载路径的**硬失败**条件。
    ///
    /// 一次返回全部违规，不是第一条，见模块文档「错误呈现」一节。
    pub fn reference_integrity(&self) -> Result<(), ReferenceIntegrityError> {
        if self.reference_violations.is_empty() {
            return Ok(());
        }
        Err(ReferenceIntegrityError {
            violations: self.reference_violations.clone(),
        })
    }

    /// ③：字段覆盖（含花名册/豁免清单自身的失效）——**不**阻断启动。
    ///
    /// # 为什么它不是启动期硬错误
    ///
    /// 字段覆盖描述的是"本体内容有没有把自己声明的每个旋钮都用起来"，
    /// 这是一条**开发期不变量**，不是"游戏坏了"的信号：一个玩家把
    /// `mods/lostland/races.scm` 里矮人的暗视改回 0，本体内容就不再
    /// 覆盖 `darkvision_floor` 了——但那个存档完全能玩，为此拒绝启动
    /// 是拿一条开发纪律去惩罚玩家。引用完整性则相反：一个指向不存在
    /// 条目的引用是真的会在运行期表现成"物品算不出伤害"的损坏。
    ///
    /// 所以这一半由 `ll-game` 的门禁测试对仓库真实 `mods/` 目录断言
    /// 为空——检查本身仍然长在真实装载路径上（模块文档「零漂移」那条
    /// 理由完整保留），只是**严重性**接在测试而不是启动流程上。
    pub fn field_coverage(&self) -> Result<(), FieldCoverageError> {
        if self.uncovered_fields.is_empty() && self.roster_violations.is_empty() {
            return Ok(());
        }
        Err(FieldCoverageError {
            uncovered: self.uncovered_fields.clone(),
            roster: self.roster_violations.clone(),
        })
    }
}

/// 引用完整性检查失败：至少有一个跨表引用指向了不存在的条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceIntegrityError {
    /// **全部**违规，不是第一条。
    pub violations: Vec<ReferenceViolation>,
}

impl fmt::Display for ReferenceIntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "内容引用完整性校验失败：{} 处跨表引用指向了不存在的内容。",
            self.violations.len()
        )?;
        for violation in &self.violations {
            let target = match &violation.target_id {
                Some(id) => id.to_string(),
                None => "<索引在本次装载的注册表里反查不到>".to_string(),
            };
            let detail = if violation.actual == ContentTableKind::Opaque {
                "这个 id 只是被 intern 过，没有任何内容表定义它".to_string()
            } else {
                format!("它实际落在{}里", table_label(violation.actual))
            };
            writeln!(
                f,
                "  - {}（{}）的 {} 指向 {}：期望是{}，但{}。",
                violation.source_id,
                table_label(violation.source_kind),
                violation.field,
                target,
                table_label(violation.expected),
                detail,
            )?;
        }
        write!(
            f,
            "跨表引用只写了字符串 id 不等于那条内容真的存在——请确认引用方与被引用方\
             在同一次装载里都被真正注册（`register-*`），而不是只在别处被当作字符串提到过。"
        )
    }
}

impl std::error::Error for ReferenceIntegrityError {}

/// 字段覆盖检查失败：至少有一个字段从没被任何一条内容设成非默认值，
/// 或花名册/豁免清单自身失效了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldCoverageError {
    /// 未覆盖字段，全部列出。
    pub uncovered: Vec<UncoveredField>,
    /// 花名册/豁免清单自身的失效，全部列出。
    pub roster: Vec<RosterViolation>,
}

impl fmt::Display for FieldCoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "内容字段覆盖校验失败：{} 个字段没有任何内容给过非默认值，\
             {} 处花名册/豁免清单自身失效。",
            self.uncovered.len(),
            self.roster.len()
        )?;
        for field in &self.uncovered {
            writeln!(
                f,
                "  - {}::{}：没有任何一条内容把它设成非默认值。",
                table_label(field.kind),
                field.field,
            )?;
        }
        for violation in &self.roster {
            match violation {
                RosterViolation::CoveredButEmpty { kind } => writeln!(
                    f,
                    "  - {} 被登记为在检查范围内，但这个命名空间下一条内容都没有。",
                    table_label(*kind)
                )?,
                RosterViolation::DeferredButPopulated { kind, reason } => writeln!(
                    f,
                    "  - {} 被显式推迟，但这个命名空间下已经有内容了——\
                     内容搬过来了，请把它从 deferred 挪进 covered。当初的推迟理由：{}",
                    table_label(*kind),
                    reason
                )?,
                RosterViolation::UnclassifiedTable { kind } => writeln!(
                    f,
                    "  - {} 既不在 covered 也不在 deferred 里：策略声明漏了这张表。",
                    table_label(*kind)
                )?,
                RosterViolation::ContradictoryTable { kind } => writeln!(
                    f,
                    "  - {} 同时出现在 covered 与 deferred 里：策略声明自相矛盾。",
                    table_label(*kind)
                )?,
                RosterViolation::OpaqueMustNotBeClassified => writeln!(
                    f,
                    "  - 「无归属 id」不是一张内容表，不该出现在 covered 或 deferred 里。"
                )?,
                RosterViolation::StaleExemption { kind, field } => writeln!(
                    f,
                    "  - {}::{} 的豁免已经失效：现在真的有内容覆盖它了，请把豁免摘掉。",
                    table_label(*kind),
                    field
                )?,
                RosterViolation::UnknownExemption { kind, field } => writeln!(
                    f,
                    "  - {}::{} 的豁免指向一个本次校验压根没观察到的字段\
                     （字段改名/删除，或它所属的表在 deferred 里）：这是一条死豁免，请清理。",
                    table_label(*kind),
                    field
                )?,
            }
        }
        write!(
            f,
            "这条检查问的是「内容里有没有人写这个字段」，与 \
             scripts/ci/check_field_consumers.py 问的「Rust 决策层里有没有人读」是两头——\
             要么让某条内容真的用上它，要么在 ll_mod::content_audit 的豁免清单里补一条写明理由。"
        )
    }
}

impl std::error::Error for FieldCoverageError {}

/// 内容表的人类可读名字，进错误文案。
fn table_label(kind: ContentTableKind) -> &'static str {
    match kind {
        ContentTableKind::Opaque => "无归属 id",
        ContentTableKind::Terrain => "地形表",
        ContentTableKind::Class => "职业表",
        ContentTableKind::Skill => "技能表",
        ContentTableKind::Subclass => "副职表",
        ContentTableKind::Quest => "任务表",
        ContentTableKind::Race => "种族表",
        ContentTableKind::SpaceProfile => "空间层属性表",
        ContentTableKind::Clip => "动画剪辑表",
        ContentTableKind::Trait => "天赋表",
        ContentTableKind::ResourcePool => "资源池表",
        ContentTableKind::Item => "物品表",
        ContentTableKind::XpCurve => "经验曲线表",
        ContentTableKind::Formula => "伤害公式表",
        ContentTableKind::WeaponCategory => "武器类别表",
        ContentTableKind::DamageCategory => "伤害类别表",
    }
}

/// 全部 [`ContentTableKind`] 变体——花名册完备性检查要能枚举它们。
///
/// 与 [`roster_slot`] 配套：新增一个变体时，那个不带通配分支的 `match`
/// 会编译失败，逼人回到这里补上数组元素（数组长度也会对不上），见模块
/// 文档「表花名册」一节。
pub const ALL_CONTENT_TABLE_KINDS: [ContentTableKind; 16] = [
    ContentTableKind::Opaque,
    ContentTableKind::Terrain,
    ContentTableKind::Class,
    ContentTableKind::Skill,
    ContentTableKind::Subclass,
    ContentTableKind::Quest,
    ContentTableKind::Race,
    ContentTableKind::SpaceProfile,
    ContentTableKind::Clip,
    ContentTableKind::Trait,
    ContentTableKind::ResourcePool,
    ContentTableKind::Item,
    ContentTableKind::XpCurve,
    ContentTableKind::Formula,
    ContentTableKind::WeaponCategory,
    ContentTableKind::DamageCategory,
];

/// 给 [`ALL_CONTENT_TABLE_KINDS`] 的完备性做编译期强制：不带通配分支
/// 的穷尽 `match`，新增变体必编译失败。
///
/// 返回值是这个变体在 [`ALL_CONTENT_TABLE_KINDS`] 里的下标，由单元
/// 测试断言两者逐项一致——这样"新增了变体但只加进数组、忘了这个
/// `match`"和反过来的情形都会被抓到。
fn roster_slot(kind: ContentTableKind) -> usize {
    match kind {
        ContentTableKind::Opaque => 0,
        ContentTableKind::Terrain => 1,
        ContentTableKind::Class => 2,
        ContentTableKind::Skill => 3,
        ContentTableKind::Subclass => 4,
        ContentTableKind::Quest => 5,
        ContentTableKind::Race => 6,
        ContentTableKind::SpaceProfile => 7,
        ContentTableKind::Clip => 8,
        ContentTableKind::Trait => 9,
        ContentTableKind::ResourcePool => 10,
        ContentTableKind::Item => 11,
        ContentTableKind::XpCurve => 12,
        ContentTableKind::Formula => 13,
        ContentTableKind::WeaponCategory => 14,
        ContentTableKind::DamageCategory => 15,
    }
}

/// 字段覆盖累积槽：`(表, 字段名) -> 是否已被至少一条内容覆盖`。
///
/// 用 `Vec` 线性查找而不是 `HashMap`：字段总数是几十的量级，线性查找
/// 完全够用，而 `Vec` 的顺序是确定的（首次观察顺序），`HashMap` 的
/// 遍历顺序会让报告顺序随机——约束 C5。
#[derive(Debug, Clone, Copy)]
struct CoverageSlot {
    kind: ContentTableKind,
    field: &'static str,
    covered: bool,
}

/// 遍历过程中的累积器。
struct Auditor<'a> {
    registry: &'a Registry,
    tables: &'a ContentValueTables<'a>,
    /// 当前正在检查的这条内容归属哪张表。
    current_kind: ContentTableKind,
    /// 当前正在检查的这条内容自己的 id。
    current_id: NamespacedId,
    /// 当前这条内容是否落在字段覆盖检查的命名空间里。
    current_in_namespace: bool,
    coverage: Vec<CoverageSlot>,
    reference_violations: Vec<ReferenceViolation>,
    /// 见 [`ContentAuditReport::references_checked`]。
    references_checked: usize,
    /// 每张表在检查命名空间下是否至少有一条内容。
    populated: [bool; ALL_CONTENT_TABLE_KINDS.len()],
}

impl Auditor<'_> {
    /// 记一次字段观察：`is_set` 表示这条内容有没有把它设成非默认值。
    ///
    /// 命名空间外的内容只用来做引用检查，不参与字段覆盖统计，见模块
    /// 文档「字段覆盖为什么只看本体命名空间」一节。
    fn field(&mut self, field: &'static str, is_set: bool) {
        if !self.current_in_namespace {
            return;
        }
        let kind = self.current_kind;
        if let Some(slot) = self
            .coverage
            .iter_mut()
            .find(|slot| slot.kind == kind && slot.field == field)
        {
            slot.covered |= is_set;
            return;
        }
        self.coverage.push(CoverageSlot {
            kind,
            field,
            covered: is_set,
        });
    }

    /// 记一次跨表引用观察，不满足期望就记一条违规。
    fn reference(
        &mut self,
        field: &'static str,
        target: ContentIndex,
        expectation: ReferenceExpectation,
    ) {
        let ReferenceExpectation::Table(expected) = expectation else {
            // `UntypedIdSpace`：这个字段指向的标识符空间没有对应的
            // 内容表，「查不到」是它的正常状态，不是违规。不计数——
            // `references_checked` 统计的是"真的做了判定"的次数。
            return;
        };
        self.references_checked += 1;
        let actual = classify_index(target, self.tables);
        if actual == expected {
            return;
        }
        self.reference_violations.push(ReferenceViolation {
            source_kind: self.current_kind,
            source_id: self.current_id.clone(),
            field,
            target_id: self.registry.resolve(target).cloned(),
            expected,
            actual,
        });
    }

    /// 同时记一次字段观察与（当字段有值时）一次引用观察——
    /// `Option<ContentIndex>` 形状的跨表引用字段的公共写法。
    fn optional_reference(
        &mut self,
        field: &'static str,
        target: Option<ContentIndex>,
        expectation: ReferenceExpectation,
    ) {
        self.field(field, target.is_some());
        if let Some(target) = target {
            self.reference(field, target, expectation);
        }
    }

    /// 同上，`Vec<ContentIndex>` 形状：非空即算覆盖，逐条检查引用。
    fn slice_reference(
        &mut self,
        field: &'static str,
        targets: &[ContentIndex],
        expectation: ReferenceExpectation,
    ) {
        self.field(field, !targets.is_empty());
        for target in targets {
            self.reference(field, *target, expectation);
        }
    }
}

/// 跑一遍装载后内容校验，产出一份完整报告。
///
/// `registry`/`tables` 必须出自**同一次装载会话**（同一批 `intern`/
/// `define` 调用的产物）——否则索引与字段值对不上号。这与
/// [`crate::content_hash::apply_value_hashes`] 是同一条前提。
///
/// # 遍历方式
///
/// 唯一入口是 [`Registry::snapshot`](crate::registry::Registry::snapshot)
/// 的注册顺序列表，每个 id 先用
/// [`classify_index`] 判定归属哪
/// 张表（**复用**值哈希那一份判定，不另写一份等价的 if/else 链——两处
/// 判断随时间漂移是本仓库反复吃过的亏），再走对应的
/// `inspect_*` 分支。
///
/// # 反查不回 id 的索引
///
/// [`ReferenceViolation::target_id`] 为 `None` 表示某个字段里的
/// `ContentIndex` 在本次装载的注册表里反查不到字符串。这在
/// `registry`/`tables` 同源的前提下不应该发生，本函数与
/// `content_hash::write_optional_resolved` 取同一条立场：不 panic，
/// 如实记成一条与其他情形可区分的违规。
pub fn audit_content(
    registry: &Registry,
    tables: &ContentValueTables<'_>,
    policy: &ContentAuditPolicy,
) -> ContentAuditReport {
    let mut auditor = Auditor {
        registry,
        tables,
        current_kind: ContentTableKind::Opaque,
        current_id: NamespacedId::parse("lostland:audit_placeholder")
            .expect("固定字面量标识符恒合法"),
        current_in_namespace: false,
        coverage: Vec::new(),
        reference_violations: Vec::new(),
        references_checked: 0,
        populated: [false; ALL_CONTENT_TABLE_KINDS.len()],
    };

    for id in registry.snapshot() {
        let Some(index) = registry.get(&id) else {
            // 理论不可达：`id` 刚从同一个 `registry` 的快照里取出。
            // 与 `content_hash::apply_value_hashes` 同一条防御立场。
            continue;
        };
        let kind = classify_index(index, tables);
        auditor.current_kind = kind;
        auditor.current_in_namespace = id.namespace() == policy.namespace;
        auditor.current_id = id;
        if auditor.current_in_namespace {
            auditor.populated[roster_slot(kind)] = true;
        }
        inspect_entry(&mut auditor, index);
    }

    let roster_violations = check_roster(&auditor, policy);
    let uncovered_fields = auditor
        .coverage
        .iter()
        .filter(|slot| !slot.covered)
        .filter(|slot| {
            !policy
                .exemptions
                .iter()
                .any(|exemption| exemption.kind == slot.kind && exemption.field == slot.field)
        })
        .map(|slot| UncoveredField {
            kind: slot.kind,
            field: slot.field,
        })
        .collect();

    ContentAuditReport {
        reference_violations: auditor.reference_violations,
        uncovered_fields,
        roster_violations,
        references_checked: auditor.references_checked,
        fields_observed: auditor.coverage.len(),
    }
}

/// 花名册与豁免清单自身的双向检查，见模块文档「表花名册」「字段豁免」
/// 两节。
fn check_roster(auditor: &Auditor<'_>, policy: &ContentAuditPolicy) -> Vec<RosterViolation> {
    let mut violations = Vec::new();

    for kind in ALL_CONTENT_TABLE_KINDS {
        let is_covered = policy.covered.contains(&kind);
        let deferred = policy.deferred.iter().find(|entry| entry.kind == kind);
        let populated = auditor.populated[roster_slot(kind)];
        if kind == ContentTableKind::Opaque {
            // `Opaque` 不是一张内容表，是「这个 id 不落在任何表里」这个
            // 判定结果本身（当前唯一的本体成员是
            // `lostland:placeholder_race`，见 `crate::base_placeholder`）。
            // 它没有任何字段可谈覆盖，因此既不该进 `covered`（它永远
            // 没有字段可查）也不该进 `deferred`（「推迟」意味着将来会
            // 检查，而它永远不会）——按 `deferred` 处理还会立刻错报
            // `DeferredButPopulated`：占位内容当然一直「有内容」。
            if is_covered || deferred.is_some() {
                violations.push(RosterViolation::OpaqueMustNotBeClassified);
            }
            continue;
        }
        match (is_covered, deferred) {
            (true, Some(_)) => violations.push(RosterViolation::ContradictoryTable { kind }),
            (false, None) => violations.push(RosterViolation::UnclassifiedTable { kind }),
            (true, None) => {
                if !populated {
                    violations.push(RosterViolation::CoveredButEmpty { kind });
                }
            }
            (false, Some(entry)) => {
                if populated {
                    violations.push(RosterViolation::DeferredButPopulated {
                        kind,
                        reason: entry.reason,
                    });
                }
            }
        }
    }

    for exemption in policy.exemptions {
        match auditor
            .coverage
            .iter()
            .find(|slot| slot.kind == exemption.kind && slot.field == exemption.field)
        {
            None => violations.push(RosterViolation::UnknownExemption {
                kind: exemption.kind,
                field: exemption.field,
            }),
            Some(slot) if slot.covered => violations.push(RosterViolation::StaleExemption {
                kind: exemption.kind,
                field: exemption.field,
            }),
            Some(_) => {}
        }
    }

    violations
}

/// 按内容表分派到具体的字段/引用观察函数。
///
/// # 编译期强制
///
/// `match` 不带通配分支——给 [`ContentTableKind`] 新增一个判别值而忘记
/// 在这里补一个 `inspect_*` 分支会直接编译失败，与
/// [`crate::content_hash::classify_index`] 文档「编译期强制」一节是
/// 同一道防线的第二处落点。
fn inspect_entry(auditor: &mut Auditor<'_>, index: ContentIndex) {
    match auditor.current_kind {
        ContentTableKind::Opaque => {
            // 不落在任何一张表里的纯 id 引用，没有字段可观察。
        }
        ContentTableKind::Terrain => inspect_terrain(auditor, index),
        ContentTableKind::Class => inspect_class(auditor, index),
        ContentTableKind::Skill => inspect_skill(auditor, index),
        ContentTableKind::Subclass => inspect_subclass(auditor, index),
        ContentTableKind::Quest => inspect_quest(auditor, index),
        ContentTableKind::Race => inspect_race(auditor, index),
        ContentTableKind::SpaceProfile => inspect_space_profile(auditor, index),
        ContentTableKind::Clip => inspect_clip(auditor, index),
        ContentTableKind::Trait => inspect_trait(auditor, index),
        ContentTableKind::ResourcePool => inspect_resource_pool(auditor, index),
        ContentTableKind::Item => inspect_item(auditor, index),
        ContentTableKind::XpCurve => inspect_xp_curve(auditor, index),
        ContentTableKind::Formula => inspect_formula(auditor, index),
        ContentTableKind::WeaponCategory => inspect_weapon_category(auditor, index),
        ContentTableKind::DamageCategory => inspect_damage_category(auditor, index),
    }
}

/// [`ll_world::terrain::TerrainDef`] 的全部字段。
fn inspect_terrain(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let table = auditor.tables.terrain;
    let kind = TerrainKind::from_index(index);
    auditor.field("TerrainAttrs::blocks_sight", table.blocks_sight(kind));
    auditor.field("TerrainAttrs::blocks_move", table.blocks_move(kind));
    auditor.field("TerrainAttrs::move_cost", table.move_cost(kind) != 0);
    let opens_into = table.opens_into(kind).map(|target| target.index());
    auditor.optional_reference(
        "TerrainAttrs::opens_into",
        opens_into,
        ReferenceExpectation::Table(ContentTableKind::Terrain),
    );
}

/// [`crate::class::ClassDef`] 的全部字段。
fn inspect_class(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let view = auditor
        .tables
        .class
        .get(index)
        .expect("classify_index 已判定为 Class，get 必返回 Some");
    // `display_name_key`/`primary_attribute` 是 `define` 的必填参数，
    // 类型上没有"默认值"可言（`NamespacedId` 与 `AttributeKind` 都没有
    // `Default`），每条内容都必然给了值——如实记成恒覆盖。这不是漏检：
    // 「有没有人写」这一头对必填字段天然成立，「有没有人读」那一头
    // 由 scripts/ci/check_field_consumers.py 负责（`primary_attribute`
    // 至今就挂在那份清单里）。
    auditor.field("ClassAttrs::display_name_key", true);
    auditor.field("ClassAttrs::primary_attribute", true);
    let trait_ids: Vec<ContentIndex> = view.traits.iter().map(|grant| grant.trait_id).collect();
    auditor.slice_reference(
        "ClassAttrs::traits",
        &trait_ids,
        ReferenceExpectation::Table(ContentTableKind::Trait),
    );
}

/// [`crate::skill::SkillDef`] 的全部字段。
fn inspect_skill(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let view = auditor
        .tables
        .skill
        .get(index)
        .expect("classify_index 已判定为 Skill，get 必返回 Some");
    let owning_class = view.owning_class;
    let prerequisites = view.prerequisites.to_vec();
    let cooldown_ticks = view.cooldown_ticks;
    let resource_cost = view.resource_cost;
    auditor.optional_reference(
        "SkillAttrs::owning_class",
        owning_class,
        ReferenceExpectation::Table(ContentTableKind::Class),
    );
    auditor.slice_reference(
        "SkillAttrs::prerequisites",
        &prerequisites,
        ReferenceExpectation::Table(ContentTableKind::Skill),
    );
    auditor.field("SkillAttrs::cooldown_ticks", cooldown_ticks != 0);
    auditor.field(
        "SkillAttrs::resource_cost",
        resource_cost != ResourceCost::None,
    );
    inspect_resource_cost(auditor, "SkillAttrs::resource_cost", resource_cost);
    // `effect` 是 `define` 的必填参数（`SkillEffect` 没有 `Default`），
    // 理由同 `inspect_class` 里对 `display_name_key` 的处理。
    auditor.field("SkillAttrs::effect", true);
}

/// [`ResourceCost`] 内部携带的跨表引用。
fn inspect_resource_cost(auditor: &mut Auditor<'_>, field: &'static str, cost: ResourceCost) {
    match cost {
        ResourceCost::None | ResourceCost::Amount(_, _) | ResourceCost::Blood(_) => {}
        ResourceCost::PoolAmount(pool, _) | ResourceCost::SlotTier(pool, _) => {
            auditor.reference(
                field,
                pool,
                ReferenceExpectation::Table(ContentTableKind::ResourcePool),
            );
        }
    }
}

/// [`crate::subclass::SubclassDef`] 的全部字段。
fn inspect_subclass(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let _view = auditor
        .tables
        .subclass
        .get(index)
        .expect("classify_index 已判定为 Subclass，get 必返回 Some");
    // 副职当前只有一个必填字段，理由同 `inspect_class`。
    auditor.field("SubclassAttrs::display_name_key", true);
}

/// [`crate::quest::QuestNodeDef`] 的全部字段。
fn inspect_quest(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let view = auditor
        .tables
        .quest
        .get(index)
        .expect("classify_index 已判定为 Quest，get 必返回 Some");
    let prerequisites = view.prerequisites.to_vec();
    let condition = view.condition.clone();
    auditor.slice_reference(
        "QuestAttrs::prerequisites",
        &prerequisites,
        ReferenceExpectation::Table(ContentTableKind::Quest),
    );
    auditor.field("QuestAttrs::condition", true);
    match condition {
        QuestCondition::KillCount { target_kind, .. } => {
            // 「敌人类型」没有对应的内容表，见
            // `ReferenceExpectation::UntypedIdSpace` 文档。
            auditor.reference(
                "QuestAttrs::condition::KillCount::target_kind",
                target_kind,
                ReferenceExpectation::UntypedIdSpace,
            );
        }
        QuestCondition::Script(_) => {}
    }
}

/// [`crate::race::RaceDef`] 的全部字段。
fn inspect_race(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let view = auditor
        .tables
        .race
        .get(index)
        .expect("classify_index 已判定为 Race，get 必返回 Some");
    let stats = view.stat_modifiers;
    let darkvision_floor = view.darkvision_floor;
    let footprint = view.footprint;
    let lifespan_years = view.lifespan_years;
    let xp_reward = view.xp_reward;
    let trait_ids: Vec<ContentIndex> = view.traits.iter().map(|grant| grant.trait_id).collect();
    let starting_items: Vec<ContentIndex> =
        view.starting_items.iter().map(|&(def, _)| def).collect();

    auditor.field("RaceAttrs::display_name_key", true);
    // 七项属性修正是一个整体字段（`BaseStats`），任一项非零即算这个
    // 字段被用上了——`BaseStats` 全零是"这个种族没有属性修正"这个
    // 完全合法的取值，正是这里要问的默认值。
    let has_stat_modifier = stats.strength != 0
        || stats.dexterity != 0
        || stats.constitution != 0
        || stats.intelligence != 0
        || stats.willpower != 0
        || stats.charisma != 0
        || stats.luck != 0;
    auditor.field("RaceAttrs::stat_modifiers", has_stat_modifier);
    auditor.field("RaceAttrs::darkvision_floor", darkvision_floor != 0);
    auditor.field("RaceAttrs::footprint", footprint != (0, 0));
    auditor.field("RaceAttrs::lifespan_years", lifespan_years != 0);
    auditor.field("RaceAttrs::xp_reward", xp_reward != 0);
    auditor.slice_reference(
        "RaceAttrs::traits",
        &trait_ids,
        ReferenceExpectation::Table(ContentTableKind::Trait),
    );
    auditor.slice_reference(
        "RaceAttrs::starting_items",
        &starting_items,
        ReferenceExpectation::Table(ContentTableKind::Item),
    );
}

/// [`ll_world::space_profile::SpaceProfile`] 的全部字段。
fn inspect_space_profile(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let table = auditor.tables.space_profile;
    let ambient_light_floor = table.ambient_light_floor(index);
    let exposed_to_sky = table.exposed_to_sky(index);
    let base_temperature = table.base_temperature(index);
    let diggable = table.diggable(index);
    let buildable = table.buildable(index);
    let reverb_tag = table.reverb_tag(index);
    auditor.field(
        "SpaceProfileAttrs::ambient_light_floor",
        ambient_light_floor != 0,
    );
    auditor.field("SpaceProfileAttrs::exposed_to_sky", exposed_to_sky);
    auditor.field("SpaceProfileAttrs::base_temperature", base_temperature != 0);
    auditor.field("SpaceProfileAttrs::diggable", diggable);
    auditor.field("SpaceProfileAttrs::buildable", buildable);
    auditor.field("SpaceProfileAttrs::reverb_tag", reverb_tag.is_some());
}

/// [`ll_render::anim::Clip`] 的全部字段——[`crate::clip::ClipTable`]
/// 直接存 `Clip` 本身，没有独立的 `*Attrs`/`*View` 类型（见
/// `content_hash::write_clip_fields` 同一条说明）。
fn inspect_clip(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let clip = auditor
        .tables
        .clip
        .get(index)
        .expect("classify_index 已判定为 Clip，get 必返回 Some");
    let frames_empty = clip.frames.is_empty();
    let frames_per_step = clip.frames_per_step;
    let looping = clip.looping;
    let exit_grace_frames = clip.exit_grace_frames;
    auditor.field("Clip::frames", !frames_empty);
    auditor.field("Clip::frames_per_step", frames_per_step != 0);
    auditor.field("Clip::looping", looping);
    auditor.field("Clip::exit_grace_frames", exit_grace_frames != 0);
}

/// [`crate::trait_def::TraitDef`] 的全部字段。
fn inspect_trait(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let view = auditor
        .tables
        .trait_def
        .get(index)
        .expect("classify_index 已判定为 Trait，get 必返回 Some");
    let granted_skills = view.granted_skills.to_vec();
    let stat_modifiers_empty = view.stat_modifiers.is_empty();
    let rule_modifiers = view.rule_modifiers.to_vec();
    let pool_ids: Vec<ContentIndex> = view
        .granted_resource_pools
        .iter()
        .map(|grant| grant.pool)
        .collect();

    auditor.field("TraitAttrs::display_name_key", true);
    auditor.slice_reference(
        "TraitAttrs::granted_skills",
        &granted_skills,
        ReferenceExpectation::Table(ContentTableKind::Skill),
    );
    auditor.field("TraitAttrs::stat_modifiers", !stat_modifiers_empty);
    auditor.field("TraitAttrs::rule_modifiers", !rule_modifiers.is_empty());
    for modifier in &rule_modifiers {
        if let RuleModifier::Resistance {
            damage_category, ..
        } = modifier
        {
            auditor.reference(
                "TraitAttrs::rule_modifiers::Resistance::damage_category",
                *damage_category,
                ReferenceExpectation::Table(ContentTableKind::DamageCategory),
            );
        }
    }
    auditor.slice_reference(
        "TraitAttrs::granted_resource_pools",
        &pool_ids,
        ReferenceExpectation::Table(ContentTableKind::ResourcePool),
    );
}

/// [`crate::resource_pool::ResourcePoolDef`] 的全部字段。
fn inspect_resource_pool(auditor: &mut Auditor<'_>, index: ContentIndex) {
    use ll_sim::resource_pool::{RegenRule, ResourcePoolShape};

    let view = auditor
        .tables
        .resource_pool
        .get(index)
        .expect("classify_index 已判定为 ResourcePool，get 必返回 Some");
    let shape = view.shape;
    let regen_rule = view.regen_rule;
    auditor.field("ResourcePoolAttrs::display_name_key", true);
    // `Scalar`/`None` 分别是两个枚举的"什么都不声明"取值——脚本注册
    // 函数不传对应参数时落到的就是它们，正是这里要问的默认值。
    auditor.field(
        "ResourcePoolAttrs::shape",
        !matches!(shape, ResourcePoolShape::Scalar),
    );
    auditor.field(
        "ResourcePoolAttrs::regen_rule",
        !matches!(regen_rule, RegenRule::None),
    );
}

/// [`crate::item::ItemDef`] 的全部字段。
fn inspect_item(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let view = auditor
        .tables
        .item
        .get(index)
        .expect("classify_index 已判定为 Item，get 必返回 Some");
    let stack_limit = view.stack_limit;
    let base_weight = view.base_weight;
    let base_price = view.base_price;
    let max_durability = view.max_durability;
    let equip_mask = view.equip_mask;
    let stat_bonuses_empty = view.stat_bonuses.is_empty();
    let use_effect = view.use_effect;
    let penetration = view.penetration;
    let damage_formula = view.damage_formula;
    let damage_category = view.damage_category;

    auditor.field("ItemAttrs::display_name_key", true);
    auditor.field("ItemAttrs::stack_limit", stack_limit != 0);
    auditor.field("ItemAttrs::base_weight", base_weight.0 != 0);
    auditor.field("ItemAttrs::base_price", base_price.0 != 0);
    auditor.field("ItemAttrs::max_durability", max_durability.is_some());
    auditor.field("ItemAttrs::equip_mask", equip_mask.bits() != 0);
    auditor.field("ItemAttrs::stat_bonuses", !stat_bonuses_empty);
    auditor.field("ItemAttrs::use_effect", use_effect.is_some());
    auditor.field(
        "ItemAttrs::penetration",
        penetration.flat != 0 || penetration.permille != 0,
    );
    auditor.optional_reference(
        "ItemAttrs::damage_formula",
        damage_formula,
        ReferenceExpectation::Table(ContentTableKind::Formula),
    );
    auditor.optional_reference(
        "ItemAttrs::damage_category",
        damage_category,
        ReferenceExpectation::Table(ContentTableKind::DamageCategory),
    );
}

/// [`ll_sim::xp_curve::XpCurveDef`] 的全部字段——**除 `id` 外**：
/// `id` 的语义是"这条曲线自己的索引"，不是跨表引用，理由与
/// `content_hash::write_xp_curve_fields` 跳过同一个字段完全相同。
fn inspect_xp_curve(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let def: &XpCurveDef = auditor
        .tables
        .xp_curve
        .get(index)
        .expect("classify_index 已判定为 XpCurve，get 必返回 Some");
    let base_requirement = def.base_requirement;
    let instructions_empty = def.instructions.is_empty();
    auditor.field("XpCurveDef::base_requirement", base_requirement != 0);
    auditor.field("XpCurveDef::instructions", !instructions_empty);
}

/// [`ll_sim::formula::FormulaDef`] 的全部字段——**除 `id` 与
/// `needs_rng` 外**。`id` 的理由同 [`inspect_xp_curve`]；`needs_rng`
/// 是编译期从 `instructions` 派生出来的布尔（见
/// `crate::script_damage_formula_api::register_damage_formula` 文档），
/// 不是一个内容作者能独立赋值的字段，对它问"有没有内容把它设成非默认
/// 值"没有意义——它随 `instructions` 自动变化。
/// `content_hash::write_formula_fields` 因同一条理由跳过它。
fn inspect_formula(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let def: &FormulaDef = auditor
        .tables
        .formula
        .get(index)
        .expect("classify_index 已判定为 Formula，get 必返回 Some");
    let instructions_empty = def.instructions.is_empty();
    auditor.field("FormulaDef::instructions", !instructions_empty);
}

/// [`crate::weapon_category::WeaponCategoryDef`] 的全部字段。
fn inspect_weapon_category(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let default_formula = auditor
        .tables
        .weapon_category
        .get(index)
        .expect("classify_index 已判定为 WeaponCategory，get 必返回 Some")
        .default_formula;
    auditor.optional_reference(
        "WeaponCategoryDef::default_formula",
        default_formula,
        ReferenceExpectation::Table(ContentTableKind::Formula),
    );
}

/// [`crate::damage_category::DamageCategoryDef`] 的全部字段。
fn inspect_damage_category(auditor: &mut Auditor<'_>, index: ContentIndex) {
    let default_formula = auditor
        .tables
        .damage_category
        .get(index)
        .expect("classify_index 已判定为 DamageCategory，get 必返回 Some")
        .default_formula;
    auditor.optional_reference(
        "DamageCategoryDef::default_formula",
        default_formula,
        ReferenceExpectation::Table(ContentTableKind::Formula),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class::ClassTable;
    use crate::clip::ClipTable;
    use crate::damage_category::DamageCategoryTable;
    use crate::formula::FormulaTable;
    use crate::item::{ItemAttrs, ItemTable};
    use crate::quest::{QuestAttrs, QuestTable};
    use crate::race::{RaceAttrs, RaceTable};
    use crate::resource_pool::ResourcePoolTable;
    use crate::skill::SkillTable;
    use crate::subclass::SubclassTable;
    use crate::trait_def::TraitTable;
    use crate::weapon_category::WeaponCategoryTable;
    use crate::xp_curve::XpCurveTable;
    use ll_core::scaled::Milli;
    use ll_sim::combat::Penetration;
    use ll_sim::formula::{FormulaOp, FormulaOperand};
    use ll_world::entity::BaseStats;
    use ll_world::item::SlotMask;
    use ll_world::space_profile::SpaceProfileTable;
    use ll_world::terrain::TerrainTable;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 一次测试用的完整装载会话：一个注册表 + 全部十五张内容表。
    ///
    /// 逐字段解构式地手工拼 [`ContentValueTables`] 在本模块的测试里要
    /// 重复十几遍，抽成一个结构体——与 `content_hash` 测试里那个返回
    /// 十四元组的 `empty_non_race_tables` 是同一个需求的更好形状。
    struct Session {
        registry: Registry,
        terrain: TerrainTable,
        class: ClassTable,
        skill: SkillTable,
        subclass: SubclassTable,
        quest: QuestTable,
        race: RaceTable,
        space_profile: SpaceProfileTable,
        clip: ClipTable,
        trait_def: TraitTable,
        resource_pool: ResourcePoolTable,
        item: ItemTable,
        xp_curve: XpCurveTable,
        formula: FormulaTable,
        weapon_category: WeaponCategoryTable,
        damage_category: DamageCategoryTable,
    }

    impl Session {
        fn new() -> Self {
            Session {
                registry: Registry::new(),
                terrain: TerrainTable::new(),
                class: ClassTable::new(),
                skill: SkillTable::new(),
                subclass: SubclassTable::new(),
                quest: QuestTable::new(),
                race: RaceTable::new(),
                space_profile: SpaceProfileTable::new(),
                clip: ClipTable::new(),
                trait_def: TraitTable::new(),
                resource_pool: ResourcePoolTable::new(),
                item: ItemTable::new(),
                xp_curve: XpCurveTable::new(),
                formula: FormulaTable::new(),
                weapon_category: WeaponCategoryTable::new(),
                damage_category: DamageCategoryTable::new(),
            }
        }

        fn tables(&self) -> ContentValueTables<'_> {
            ContentValueTables {
                terrain: &self.terrain,
                class: &self.class,
                skill: &self.skill,
                subclass: &self.subclass,
                quest: &self.quest,
                race: &self.race,
                space_profile: &self.space_profile,
                clip: &self.clip,
                trait_def: &self.trait_def,
                resource_pool: &self.resource_pool,
                item: &self.item,
                xp_curve: &self.xp_curve,
                formula: &self.formula,
                weapon_category: &self.weapon_category,
                damage_category: &self.damage_category,
            }
        }

        fn audit(&self, policy: &ContentAuditPolicy) -> ContentAuditReport {
            audit_content(&self.registry, &self.tables(), policy)
        }

        /// 只 intern，不 define——`base_contract` 模块文档「两层判定」
        /// 说的第一层。
        fn intern(&mut self, raw: &str) -> ContentIndex {
            self.registry.intern(id(raw))
        }

        fn define_formula(&mut self, raw: &str) -> ContentIndex {
            let index = self.intern(raw);
            self.formula
                .define(
                    index,
                    ll_sim::formula::FormulaDef {
                        id: index,
                        instructions: vec![FormulaOp::Ref(FormulaOperand::AttackPower)],
                        needs_rng: false,
                    },
                )
                .expect("测试用公式定义内部自洽");
            index
        }

        fn define_race(&mut self, raw: &str, darkvision_floor: i32) -> ContentIndex {
            let index = self.intern(raw);
            self.race
                .define(
                    index,
                    RaceAttrs {
                        display_name_key: id("test:name"),
                        stat_modifiers: BaseStats {
                            strength: 0,
                            dexterity: 0,
                            constitution: 0,
                            intelligence: 0,
                            willpower: 0,
                            charisma: 0,
                            luck: 0,
                        },
                        darkvision_floor,
                        footprint: (1, 1),
                        lifespan_years: 80,
                        xp_reward: 0,
                        traits: Vec::new(),
                        starting_items: Vec::new(),
                    },
                )
                .expect("测试用种族定义内部自洽");
            index
        }

        fn define_item(&mut self, raw: &str, damage_formula: Option<ContentIndex>) -> ContentIndex {
            let index = self.intern(raw);
            self.item
                .define(
                    index,
                    ItemAttrs {
                        display_name_key: id("test:name"),
                        stack_limit: 1,
                        base_weight: Milli(1),
                        base_price: Milli(1),
                        max_durability: Some(1),
                        equip_mask: SlotMask::EMPTY,
                        stat_bonuses: Vec::new(),
                        use_effect: None,
                        penetration: Penetration::NONE,
                        damage_formula,
                        damage_category: None,
                    },
                )
                .expect("测试用物品定义内部自洽");
            index
        }
    }

    /// 十五张表全部登记成推迟的花名册——引用完整性相关的测试用它，
    /// 好让字段覆盖那一半不产出任何噪音干扰断言。
    const ALL_DEFERRED: &[DeferredTable] = &[
        DeferredTable {
            kind: ContentTableKind::Terrain,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Class,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Skill,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Subclass,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Quest,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Race,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::SpaceProfile,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Clip,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Trait,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::ResourcePool,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Item,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::XpCurve,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::Formula,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::WeaponCategory,
            reason: "测试策略：本测试只查引用完整性。",
        },
        DeferredTable {
            kind: ContentTableKind::DamageCategory,
            reason: "测试策略：本测试只查引用完整性。",
        },
    ];

    /// 只查引用完整性的策略：命名空间取一个没有任何内容的名字，字段
    /// 覆盖因此一条都统计不到。
    fn reference_only_policy() -> ContentAuditPolicy {
        ContentAuditPolicy {
            namespace: "nothing",
            covered: &[],
            deferred: ALL_DEFERRED,
            exemptions: &[],
        }
    }

    /// 只把种族表放进字段覆盖范围的策略，命名空间是 `test`。
    fn race_only_policy(exemptions: &'static [FieldExemption]) -> ContentAuditPolicy {
        ContentAuditPolicy {
            namespace: "test",
            covered: &[ContentTableKind::Race],
            // 花名册必须完备：种族之外的十四张仍旧登记成推迟。
            deferred: &ALL_DEFERRED_EXCEPT_RACE,
            exemptions,
        }
    }

    /// [`ALL_DEFERRED`] 去掉种族一条——[`race_only_policy`] 用。
    const ALL_DEFERRED_EXCEPT_RACE: [DeferredTable; 14] = [
        DeferredTable {
            kind: ContentTableKind::Terrain,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Class,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Skill,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Subclass,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Quest,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::SpaceProfile,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Clip,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Trait,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::ResourcePool,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Item,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::XpCurve,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::Formula,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::WeaponCategory,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
        DeferredTable {
            kind: ContentTableKind::DamageCategory,
            reason: "测试策略：本测试只查种族表的字段覆盖。",
        },
    ];

    #[test]
    fn 引用指向已定义条目时不报违规() {
        // Arrange
        let mut session = Session::new();
        let formula = session.define_formula("test:formula");
        session.define_item("test:sword", Some(formula));

        // Act
        let report = session.audit(&reference_only_policy());

        // Assert
        assert_eq!(report.reference_violations, Vec::new());
        // 非空转：这次校验真的做了判定，不是一处引用都没走到。
        assert!(report.references_checked >= 1);
        assert!(report.reference_integrity().is_ok());
    }

    #[test]
    fn 引用指向只intern未define的id时报违规() {
        // 这正是 `base_contract::MissingReason::NotDefined` 那一层判定
        // 在跨表引用上的对应物：id 在注册表里（有人引用过它），但没有
        // 任何内容表定义它。
        // Arrange
        let mut session = Session::new();
        let ghost = session.intern("test:never_defined");
        session.define_item("test:sword", Some(ghost));

        // Act
        let report = session.audit(&reference_only_policy());

        // Assert
        assert_eq!(report.reference_violations.len(), 1);
        let violation = &report.reference_violations[0];
        assert_eq!(violation.field, "ItemAttrs::damage_formula");
        assert_eq!(violation.source_id, id("test:sword"));
        assert_eq!(violation.expected, ContentTableKind::Formula);
        assert_eq!(violation.actual, ContentTableKind::Opaque);
        assert_eq!(violation.target_id, Some(id("test:never_defined")));
    }

    #[test]
    fn 引用指向别的表时报违规() {
        // 拼错 id 撞上另一张表里真实存在的内容——比「查不到」更隐蔽的
        // 一种错，静默退化时完全看不出来。
        // Arrange
        let mut session = Session::new();
        let race = session.define_race("test:dwarf", 4);
        session.define_item("test:sword", Some(race));

        // Act
        let report = session.audit(&reference_only_policy());

        // Assert
        assert_eq!(report.reference_violations.len(), 1);
        assert_eq!(
            report.reference_violations[0].actual,
            ContentTableKind::Race
        );
    }

    #[test]
    fn 多处违规一次性全部列出且顺序确定() {
        // 模块文档「错误呈现」那条纪律的守卫，与 `base_contract` 的
        // 同名测试同源：谁把遍历改成撞见第一条就返回，本条立刻变红。
        // Arrange
        let mut session = Session::new();
        let ghost = session.intern("test:never_defined");
        session.define_item("test:sword", Some(ghost));
        session.define_item("test:axe", Some(ghost));

        // Act
        let report = session.audit(&reference_only_policy());

        // Assert：顺序就是内容注册顺序。
        assert_eq!(
            report
                .reference_violations
                .iter()
                .map(|violation| violation.source_id.to_string())
                .collect::<Vec<_>>(),
            vec!["test:sword".to_string(), "test:axe".to_string()]
        );
    }

    #[test]
    fn 击杀计数的目标类型不做引用检查() {
        // 「只 intern、不 define 也合法」这条豁免的现役用例：任务的
        // 击杀目标指向「敌人类型」，代码库至今没有敌人类型注册表，见
        // `ReferenceExpectation::UntypedIdSpace` 文档。把它按 Table
        // 检查会把一条正确的设计判成错误。
        // Arrange
        let mut session = Session::new();
        let goblin = session.intern("test:goblin");
        let quest = session.intern("test:kill_goblins");
        session
            .quest
            .define(
                quest,
                QuestAttrs {
                    prerequisites: Vec::new(),
                    condition: QuestCondition::KillCount {
                        target_kind: goblin,
                        count: 3,
                    },
                },
            )
            .expect("测试用任务定义内部自洽");

        // Act
        let report = session.audit(&reference_only_policy());

        // Assert：`test:goblin` 从没被任何表定义过，但这不是违规。
        assert_eq!(report.reference_violations, Vec::new());
    }

    #[test]
    fn 任一条内容设成非默认值即算该字段已覆盖() {
        // Arrange：两个种族，只有第二个声明了暗视。
        let mut session = Session::new();
        session.define_race("test:human", 0);
        session.define_race("test:dwarf", 4);

        // Act
        let report = session.audit(&race_only_policy(&[]));

        // Assert：`darkvision_floor` 不在未覆盖列表里。
        assert!(
            !report
                .uncovered_fields
                .iter()
                .any(|field| field.field == "RaceAttrs::darkvision_floor"),
            "{:?}",
            report.uncovered_fields
        );
        // 非空转：字段槽真的被观察到了。
        assert!(report.fields_observed >= 1);
    }

    #[test]
    fn 从没被设成非默认值的字段报未覆盖() {
        // Arrange：唯一一个种族的暗视是 0。
        let mut session = Session::new();
        session.define_race("test:human", 0);

        // Act
        let report = session.audit(&race_only_policy(&[]));

        // Assert
        assert!(
            report.uncovered_fields.contains(&UncoveredField {
                kind: ContentTableKind::Race,
                field: "RaceAttrs::darkvision_floor",
            }),
            "{:?}",
            report.uncovered_fields
        );
    }

    #[test]
    fn 命名空间之外的内容不参与字段覆盖统计() {
        // 模块文档「字段覆盖为什么只看本体命名空间」一节的守卫：一个
        // 第三方 mod 用上了某个字段，不能让本体那一侧的检查跟着变绿。
        // Arrange：暗视只被命名空间之外的种族声明。
        let mut session = Session::new();
        session.define_race("test:human", 0);
        session.define_race("othermod:dwarf", 4);

        // Act
        let report = session.audit(&race_only_policy(&[]));

        // Assert：仍然报未覆盖。
        assert!(
            report
                .uncovered_fields
                .iter()
                .any(|field| field.field == "RaceAttrs::darkvision_floor"),
            "{:?}",
            report.uncovered_fields
        );
    }

    const DARKVISION_EXEMPTION: &[FieldExemption] = &[FieldExemption {
        kind: ContentTableKind::Race,
        field: "RaceAttrs::darkvision_floor",
        reason: "测试用豁免：本测试里的种族刻意不声明暗视。",
    }];

    #[test]
    fn 豁免的字段不再报未覆盖() {
        // Arrange
        let mut session = Session::new();
        session.define_race("test:human", 0);

        // Act
        let report = session.audit(&race_only_policy(DARKVISION_EXEMPTION));

        // Assert
        assert!(
            !report
                .uncovered_fields
                .iter()
                .any(|field| field.field == "RaceAttrs::darkvision_floor"),
            "{:?}",
            report.uncovered_fields
        );
    }

    #[test]
    fn 豁免的字段后来被覆盖时报死豁免() {
        // 双向检查的另一半，与 `check_field_consumers.py` 的
        // `stale_because_wired` 是同一条纪律：接线了就该把豁免摘掉。
        // Arrange
        let mut session = Session::new();
        session.define_race("test:dwarf", 4);

        // Act
        let report = session.audit(&race_only_policy(DARKVISION_EXEMPTION));

        // Assert
        assert!(
            report
                .roster_violations
                .contains(&RosterViolation::StaleExemption {
                    kind: ContentTableKind::Race,
                    field: "RaceAttrs::darkvision_floor",
                }),
            "{:?}",
            report.roster_violations
        );
    }

    const UNKNOWN_EXEMPTION: &[FieldExemption] = &[FieldExemption {
        kind: ContentTableKind::Race,
        field: "RaceAttrs::field_that_no_longer_exists",
        reason: "测试用豁免：指向一个不存在的字段。",
    }];

    #[test]
    fn 豁免指向不存在的字段时报未知豁免() {
        // Arrange
        let mut session = Session::new();
        session.define_race("test:human", 0);

        // Act
        let report = session.audit(&race_only_policy(UNKNOWN_EXEMPTION));

        // Assert
        assert!(
            report
                .roster_violations
                .contains(&RosterViolation::UnknownExemption {
                    kind: ContentTableKind::Race,
                    field: "RaceAttrs::field_that_no_longer_exists",
                }),
            "{:?}",
            report.roster_violations
        );
    }

    #[test]
    fn 声明在范围内的表一条内容都没有时报错() {
        // Arrange：策略说种族表在范围内，但 `test` 命名空间下没有种族。
        let session = Session::new();

        // Act
        let report = session.audit(&race_only_policy(&[]));

        // Assert
        assert!(
            report
                .roster_violations
                .contains(&RosterViolation::CoveredButEmpty {
                    kind: ContentTableKind::Race,
                }),
            "{:?}",
            report.roster_violations
        );
    }

    #[test]
    fn 推迟的表出现内容时报错提醒挪进范围() {
        // 模块文档「表花名册」一节的核心：内容迁移完成那天，这条会
        // 主动提醒，不需要有人记得回来改策略。
        // Arrange：策略把种族表登记成推迟，但 `test` 命名空间下有种族。
        let mut session = Session::new();
        session.define_race("test:human", 0);
        let mut policy = reference_only_policy();
        policy.namespace = "test";

        // Act
        let report = session.audit(&policy);

        // Assert
        assert!(
            report.roster_violations.iter().any(|violation| matches!(
                violation,
                RosterViolation::DeferredButPopulated {
                    kind: ContentTableKind::Race,
                    ..
                }
            )),
            "{:?}",
            report.roster_violations
        );
    }

    #[test]
    fn 两个清单都没登记的表报未分类() {
        // Arrange：一份什么都没登记的策略。
        let session = Session::new();
        let policy = ContentAuditPolicy {
            namespace: "test",
            covered: &[],
            deferred: &[],
            exemptions: &[],
        };

        // Act
        let report = session.audit(&policy);

        // Assert：`Opaque` 不算（它不是一张表），其余十五张全部报。
        assert_eq!(
            report
                .roster_violations
                .iter()
                .filter(|violation| matches!(violation, RosterViolation::UnclassifiedTable { .. }))
                .count(),
            ALL_CONTENT_TABLE_KINDS.len() - 1
        );
    }

    #[test]
    fn opaque出现在花名册里时报错() {
        // `Opaque` 不是一张内容表，把它登记进任何一个清单都是错的
        // ——而且按 `deferred` 登记还会立刻错报 `DeferredButPopulated`。
        // Arrange
        let session = Session::new();
        let policy = ContentAuditPolicy {
            namespace: "test",
            covered: &[ContentTableKind::Opaque],
            deferred: &[],
            exemptions: &[],
        };

        // Act
        let report = session.audit(&policy);

        // Assert
        assert!(
            report
                .roster_violations
                .contains(&RosterViolation::OpaqueMustNotBeClassified),
            "{:?}",
            report.roster_violations
        );
    }

    #[test]
    fn 全部内容表判别值与花名册下标逐项一致() {
        // `ALL_CONTENT_TABLE_KINDS` 与 `roster_slot` 是两份手写清单，
        // 它们分叉会让 `populated` 数组张冠李戴（某张表的「有没有内容」
        // 记到另一张表头上）。`roster_slot` 那个不带通配分支的 match
        // 保证新增变体必须两处都改，本条保证两处改得一致。
        // Arrange & Act & Assert
        for (slot, kind) in ALL_CONTENT_TABLE_KINDS.iter().enumerate() {
            assert_eq!(roster_slot(*kind), slot, "{kind:?}");
        }
    }

    #[test]
    fn 引用违规文案点名来源字段目标与两侧归属() {
        // 玩家/mod 作者看到的就是这段文字，它必须能直接指向下一步动作。
        // Arrange
        let mut session = Session::new();
        let ghost = session.intern("test:never_defined");
        session.define_item("test:sword", Some(ghost));

        // Act
        let text = session
            .audit(&reference_only_policy())
            .reference_integrity()
            .expect_err("必须失败")
            .to_string();

        // Assert
        assert!(text.contains("test:sword"), "{text}");
        assert!(text.contains("ItemAttrs::damage_formula"), "{text}");
        assert!(text.contains("test:never_defined"), "{text}");
        assert!(text.contains("伤害公式表"), "{text}");
        assert!(text.contains("只是被 intern 过"), "{text}");
    }

    #[test]
    fn 字段覆盖文案点名表与字段并指向两条检查的分工() {
        // Arrange
        let mut session = Session::new();
        session.define_race("test:human", 0);

        // Act
        let text = session
            .audit(&race_only_policy(&[]))
            .field_coverage()
            .expect_err("必须失败")
            .to_string();

        // Assert
        assert!(text.contains("种族表"), "{text}");
        assert!(text.contains("RaceAttrs::darkvision_floor"), "{text}");
        assert!(text.contains("check_field_consumers.py"), "{text}");
    }

    #[test]
    fn 同一份内容跑两次报告逐条相同() {
        // 约束 C5：报告顺序不得依赖任何哈希容器的迭代顺序，否则测试
        // 会随机红绿。
        // Arrange
        let mut session = Session::new();
        let ghost = session.intern("test:never_defined");
        session.define_item("test:sword", Some(ghost));
        session.define_item("test:axe", Some(ghost));
        session.define_race("test:human", 0);

        // Act
        let first = session.audit(&race_only_policy(&[]));
        let second = session.audit(&race_only_policy(&[]));

        // Assert
        assert_eq!(first, second);
    }
}
