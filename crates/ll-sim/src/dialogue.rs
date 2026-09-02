//! 对话选项的显示条件：一张**封闭的谓词清单**，以及对它求值的纯函数。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md` **四节**，落地计划见
//! `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md`。
//!
//! # 为什么是封闭清单，不是表达式
//!
//! 条件是一个数组，数组里每一项是一条谓词，**全部满足**才显示这个选项
//! ——数组即合取。**没有 `or`、没有可嵌套的 `not`（否定由成对的 `kind`
//! 表达）、没有嵌套、没有算术、没有变量、没有比较两个动态值。**
//!
//! 被否决的替代方案是复用 `ll_mod::content_expr` 那套嵌套数组表达式
//! （**注意依赖方向：那个模块在下游的 `ll-mod`，本 crate 引用不到它，
//! 这里只能点名不能 intra-doc link**）。否决判据正是 ADR 0021 本身：与
//! 伤害公式共享的只有**语法**（嵌套数组），不是**算法**——伤害公式把
//! 固定的几个数值输入折成一个整数，对话条件是对世界状态的一组**查询**。
//! 为语法相似而共用一个编译器，就要往一个今天只认识 `attack-power`/
//! `str-mod` 的封闭符号表里塞进 `affiliations`/`mod_state`/`inventory`；
//! 而 `RawExpr` 的形状**天然支持任意嵌套**，`["and", ["or", …]]` 会在第一
//! 个内容作者想要它的那天被加进来。一旦有了布尔嵌套，紧接着就会要「比较
//! 两个动态值」，然后是算术，然后是变量——那时它已经是一个解释器，而
//! 「不要在玩法层放解释器」正是拆掉脚本层那次裁定（ADR 0028）的全部内容。
//!
//! # 清单是可增长的，被禁止的是组合子
//!
//! 加一条 `time-of-day` 谓词的成本是一个 `match` 分支 + 一条 schema + 一条
//! 哈希字段，这是允许的。**但有一条硬规则**（规格四节 4.3）：**新增谓词
//! 必须同批带一条真实内容用例**——本体或 `mods/example_mod/` 里真的有一句
//! 台词在用它。没有用例的谓词不加，与 ADR 0021「不要为将来可能的对称性
//! 建抽象」、以及本仓库对「声明了但没接线」的长期记账是同一条纪律。
//!
//! # 为什么住在 `ll-sim` 而不是 `ll-mod`
//!
//! 三条，第二条是决定性的：
//!
//! 1. 谓词读的全是**世界状态**（[`Agent`] 的归属/背包/钱包/种族/
//!    `mod_state`），那些类型住在 `ll-world`/`ll-sim`。
//! 2. 规格七节 7.2：**条件判定的代码只写一份，UI 与 `resolve` 共用同一个
//!    函数。** UI 算出「这一行该显示」用的是某一帧的世界快照，`resolve`
//!    结算时世界可能已经变了（NPC 死了、物品被别人捡走了），因此
//!    `resolve` 必须重新校验、不能相信 UI 传来的选项序号。而 `resolve`
//!    在本 crate、UI 在 `ll-ui`/`ll-game`，唯一能被两边共用的位置是这里。
//! 3. `scripts/ci/check_field_consumers.py` 的「决策层」定义就是
//!    `ll-sim/src/**`——放在这里，这批谓词从第一天起就不是死代码。
//!
//! 与 [`crate::quest`] 的分工逐条同构：那边也是「判定函数在 `ll-sim`、
//! 内容表在 `ll-mod`」，`ll_mod::quest` 只 `pub use` 判定函数。
//!
//! # 本模块不产出任何 `Effect`
//!
//! 求值是纯函数。「选中一条选项之后发生什么」（后果）属于对话系统的批次
//! 2 及之后，见计划文档第七节的挂载点表。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_world::entity::{AffiliationKind, Agent, EntityId, OrgRef};
use ll_world::mod_state::{ModStateValue, ModStateWrite};

use crate::quest::is_quest_completed;

/// 对话标志在 [`Agent::mod_state`] 里的键前缀。
///
/// 与 [`crate::quest::quest_progress_key`] 同一套存储、同一种写法：拼的是
/// **完整标识符字符串**而不是 `ContentIndex` 的数值——`ContentIndex` 依赖
/// mod 加载顺序，不可持久化（`ll_core::ident` 模块文档）。
const DIALOGUE_FLAG_KEY_PREFIX: &str = "dialogue_flag:";

/// 一条对话标志在 [`Agent::mod_state`] 里的键。
///
/// 存储按 `(mod_namespace, key)` 两级隔离，命名空间取标志自己的命名空间
/// ——与 [`crate::quest::mark_quest_completed`] 逐条同办。
pub fn dialogue_flag_key(flag: &NamespacedId) -> String {
    format!("{DIALOGUE_FLAG_KEY_PREFIX}{flag}")
}

/// 查询 `agent` 身上是否设过某条对话标志。
///
/// 值固定是 [`ModStateValue::Bool(true)`](ModStateValue::Bool)——「设过」
/// 是一个存在性判断（写过就是设过），不需要一个可以取多种值的状态机，
/// 理由与 [`crate::quest::mark_quest_completed`] 用 `Int(1)` 逐字相同。
///
/// 〔2026-08-31，批次 21〕原文说「**写入这一侧属于批次 2**」——那一批就是
/// 本批，写入侧现在是 [`set_dialogue_flag`]（`outcomes` 里的 `set-flag`，经
/// [`crate::effect::Effect::SetModState`] 落地，约束 C1）。本条谓词因此从
/// 本批起有非假的读数。
pub fn has_dialogue_flag(agent: &Agent, flag: &NamespacedId) -> bool {
    matches!(
        agent
            .mod_state
            .get(&(flag.namespace().to_string(), dialogue_flag_key(flag))),
        Some(ModStateValue::Bool(true))
    )
}

/// 产出一条「在 `actor` 身上设下 `flag`」的脚本状态写入记录。
///
/// **不直接改任何 `WorldState`**——本函数只产出数据，调用方
/// （[`crate::resolve`] 的 `Intent::DialogueChoose` 一支）负责把返回值包进
/// [`crate::effect::Effect::SetModState`] 交给 [`crate::apply::apply`]
/// （约束 C1，唯一写入口）。与 [`crate::quest::mark_quest_completed`] 逐条
/// 同办，只是值取 [`ModStateValue::Bool`] 而不是 `Int(1)`——读那一侧
/// （[`has_dialogue_flag`]）从批次 1 起就是按 `Bool(true)` 判的。
pub fn set_dialogue_flag(actor: EntityId, flag: &NamespacedId) -> ModStateWrite {
    ModStateWrite {
        entity: actor,
        mod_namespace: flag.namespace().to_string(),
        key: dialogue_flag_key(flag),
        value: ModStateValue::Bool(true),
    }
}

/// 选中一条选项之后**世界**发生什么——数据里的一条**声明**，把声明变成
/// [`crate::effect::Effect`] 的是 `resolve`（规格五节 5.0）。
///
/// # 为什么变体是一支一支加进来的
///
/// 规格八节的分批表：`set-flag` 是批次 2、`join-settlement` 是批次 3、
/// `complete-quest` 与 `give-item` 是批次 4、`open-trade` 是批次 5，
/// 每一支都各自缺着自己的前置。**先声明一个只能写空数组的变体，就是又
/// 一个「声明了但没接线」的死字段**——本仓库长期记账的正是这一类，
/// 批次 1 因此连 `outcomes` 这个字段本身都没有加。
///
/// 〔2026-08-31，批次 26〕`JoinSettlement` 落地：它的前置
/// （`ll_world::entity::Agent::home` 与
/// `ll_world::faction::FactionTable`）这一批补齐了。剩下三种在
/// `ll_mod::content_schema_dialogue` 里仍然**报「尚未实现」而不是静默
/// 接受**。
///
/// 〔2026-09-01，批次 31〕`OpenTrade` 落地，**五支到齐**——
/// `ll_mod::content_schema_dialogue` 里那条「尚未实现」的兜底分支就此
/// 只剩「不认识的 kind」一种情形。
///
/// 而**枚举**这个形状本身是有价值的，本批第一次兑现：
/// `write_dialogue_outcome`（内容哈希）与 `resolve` 两处都是穷尽
/// `match`，加这一支时编译器逼那两处各自表态，没有出现「加了一种后果、
/// 哈希没混进去」这种静默分叉。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueOutcome {
    /// 在发起者身上设下一条对话标志——[`DialogueCondition::FlagSet`] /
    /// [`DialogueCondition::FlagNotSet`] 读的就是它。
    ///
    /// 标志**没有内容表**（它是对话系统自己在 [`Agent::mod_state`] 里写的
    /// 一条记录），因此携带的是 [`NamespacedId`] 而不是 [`ContentIndex`]，
    /// 与条件那两支逐字相同。
    SetFlag(NamespacedId),
    /// **加入说话人所属据点的势力**（规格五节 5.1）——所有者提出整个
    /// 对话系统的动机原话（「玩家可以没有势力归属，这个可以通过后面和
    /// 据点的管理者对话加入」）在数据里的那一条声明。
    ///
    /// # 为什么不带任何参数
    ///
    /// 「加入哪座」由**说话人**回答（他的 `ll_world::entity::Agent::home`），
    /// 不由内容作者写死：据点号是 `ll_core::ident::WorldId`，世界生成期
    /// 分配，**内容文件里根本写不出来**。这与
    /// [`AffiliationQuery::org` 只能指向内容空间的组织](AffiliationQuery)
    /// 是同一处能力边界的两面。
    ///
    /// 因此这条后果的语义完整地写成一句话是：「把发起者挂进**说话人
    /// 那座据点所属的那个势力**」。[`crate::resolve`] 那一侧的五道闸门
    /// （说话人还在、他有 `home`、那座据点查得到势力……）见
    /// `crates/ll-sim/src/resolve/dialogue.rs`。
    ///
    /// # 加入的是**势力**，不是据点
    ///
    /// `ll_world::faction::FactionTable::faction_of` 把据点号翻译成势力
    /// 号。规格 5.1 原文里那条「拿据点 `WorldId` 冒充 `Faction` 归属」
    /// 的变通**已经作废**（势力播种批次落地之后），本批指向的是真正的
    /// `ll_world::faction::Faction`。
    JoinSettlement,
    /// **把这条任务标记成已完成**（规格五节 5.2 第 2 条）。
    ///
    /// 结算时调的是**既有**的 [`crate::quest::mark_quest_completed`]，
    /// 不重写一份完成逻辑：任务「已完成」这件事的存储形状（按命名空间
    /// 隔离的 `mod_state` + [`crate::quest::quest_progress_key`] 那个键 +
    /// `Int(1)`）只有一处真相源，就是 [`crate::quest`]。在对话这一侧再
    /// 抄一份，两处会各自漂移，而漂移时没有任何东西会报错——ADR 0021
    /// 点名要拦的正是这个形状。
    ///
    /// # 为什么是 `ContentIndex` 而不是 `NamespacedId`
    ///
    /// 任务**有内容表**（`ll_mod::quest::QuestTable`），因此这条引用走
    /// 跨表引用的既有纪律：装载期 `required_id`（只 get 不 intern），
    /// 拼错的任务 id 当场报错并点名文件。与
    /// [`DialogueCondition::QuestCompleted`] 逐字同办。
    /// [`SetFlag`](DialogueOutcome::SetFlag) 之所以拿的是
    /// [`NamespacedId`]，是因为对话标志**没有内容表**——两者不是同一档。
    ///
    /// 结算侧因此要一次 [`ContentIdLookup`] 反查（`mark_quest_completed`
    /// 按标识符字符串写 `mod_state`），用的就是批次 1 为
    /// `quest-completed` 谓词建的那条窄接口，不新加依赖。
    ///
    /// # 这一支让 `QuestCondition::Script` 第一次有了替代品
    ///
    /// `ll_mod::quest::QuestCondition::Script` 的文档原话是「处理『拜访
    /// 某个 NPC 并说出特定台词』这类无法穷举成数据的条件」——**那正是
    /// 对话**。三档脚本回调已随脚本系统消失（ADR 0028），本变体把这类
    /// 条件变回了数据可表达的东西。
    CompleteQuest(ContentIndex),
    /// **说话人把自己背包里的一件这种东西交给发起者**（规格五节 5.2
    /// 第 3 条：发奖）。
    ///
    /// # 硬前置：不属于他的东西送不出去
    ///
    /// `ll_world::ownership` 的设计文档四节给「合法转移」立的那条前置，
    /// 规格 5.2 原样接受并落到对话上：**那东西必须真的在他背包里、且
    /// 归他所有**（[`ll_world::ownership::Owner::Unowned`] 除外——没有
    /// 人的权益受损）。校验落在 `crate::resolve` 一侧、在产出任何
    /// [`crate::effect::Effect`] **之前**（约束 C1：`apply` 只机械执行，
    /// 一句判断都不做）。
    ///
    /// 少了它，销赃计时会被一条作弊路径绕开：把赃物「送」给自己控制的
    /// 另一个角色，标记瞬间清空。
    ///
    /// **一个 NPC 凭空变出奖励物品是另一件事**（规格 5.2 原文），本变体
    /// 不假装能做到：他拿不出来就什么都不发生。
    ///
    /// # 为什么不带 `count`
    ///
    /// 规格 5.2 的草图写的是 `{ item, count: N }`，本批**刻意收窄成一次
    /// 一件**：`apply` 侧因此直接复用既有的
    /// [`crate::effect::Effect::ConsumeInventoryItem`]（「数量减一，减到
    /// 零时整条堆移除」正是这个语义，一个字都不用改），而 N > 1 需要一
    /// 套拆堆机械、今天一条内容都不需要它（YAGNI）。反转成本：schema 加
    /// 一个默认为 1 的 `count` 字段，既有内容一个字不改。
    ///
    /// # 收方是**发起者**，不是说话人
    ///
    /// 与 [`JoinSettlement`](DialogueOutcome::JoinSettlement) 把归属挂在
    /// 发起者身上同一条：后果作用于「按下这一行的那个人」。
    GiveItem(ContentIndex),
    /// **把 UI 推进交易屏**（规格五节 5.3）——本枚举里唯一一支
    /// **不改世界状态**的后果，见 [`DialogueOutcome::is_ui_only`]。
    ///
    /// # 「不产 `Effect`」与 C1 不矛盾
    ///
    /// 规格 5.3 原话：这条后果「**不产出任何 `Effect`**，只把 UI 推进
    /// 交易界面——与 `InteractTarget::Facility` 落到『打开制作菜单』是
    /// 同一形状」。**真正的交易（钱货两清）走
    /// [`crate::intent::Intent::Trade`]**，它照旧经
    /// [`crate::apply::apply`] 这个唯一写入口（约束 C1）。
    ///
    /// 分界与规格七节 7.1 那条逐字同一条：**「玩家现在停在哪块屏上」
    /// 是 UI 状态，「谁的钱和货变了」是世界状态。**
    ///
    /// # 为什么不带参数
    ///
    /// 「跟谁交易」由**说话人**回答（会话屏一路带着的那个
    /// [`EntityId`]），与 [`JoinSettlement`](DialogueOutcome::JoinSettlement)
    /// 同理：内容文件里写不出一个世界生成期分配的实体号。
    OpenTrade,
}

impl DialogueOutcome {
    /// 这一支**只推 UI、不改世界**吗？
    ///
    /// 今天只有 [`OpenTrade`](DialogueOutcome::OpenTrade) 返回真。
    ///
    /// # 谁读它，为什么这条判据不能写在 UI 里
    ///
    /// 规格 7.2：「纯导航的选项（`outcomes` 为空）不提交 `Intent`
    /// ……提交一个恒产出空效果的意图只会污染意图日志。」`open-trade`
    /// 是同一档——它恒产出空效果。会话屏因此把「要不要提交
    /// [`crate::intent::Intent::DialogueChoose`]」的判据从
    /// 「`outcomes` 非空」收窄成「**存在一条不是只推 UI 的后果**」：
    /// 一条只有 `open-trade` 的选项一个意图都不提交，一条
    /// `[set-flag, open-trade]` 混着的照样提交（`set-flag` 那一半要落地）。
    ///
    /// 判据长在这里而不是 `ll-game`，与
    /// [`all_conditions_hold`] 是同一条理由（模块文档「为什么住在
    /// `ll-sim`」第 2 条）：**加一种后果时，编译器逼这个 `match` 表态**，
    /// 而一份抄在 UI 里的 `matches!` 只会在下一支后果落地时静默漏掉它。
    pub fn is_ui_only(&self) -> bool {
        match self {
            DialogueOutcome::OpenTrade => true,
            DialogueOutcome::SetFlag(_)
            | DialogueOutcome::JoinSettlement
            | DialogueOutcome::CompleteQuest(_)
            | DialogueOutcome::GiveItem(_) => false,
        }
    }
}

/// 「加入一座据点」给多少声望——项目所有者裁定
/// （`knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节第 5 条）：
/// 「加入据点给 **+250**，满值 1000」。
///
/// # 为什么这一半住在这里，另一半住在 `ll-world`
///
/// 「满值 1000」是 `ll_world::entity::Affiliation::standing` **这个字段的
/// 量纲**，与谁来写它无关，因此是
/// [`ll_world::entity::Affiliation::STANDING_FULL`]；「加入据点给多少」
/// 是**这一条后果**的数值，与产出它的 [`DialogueOutcome::JoinSettlement`]
/// 同住。将来若有第二条会改 `standing` 的后果（交易折扣、任务奖励），
/// 它带自己的常量，不复用本条。
///
/// # 它与内容里那条 `standing-at-least: 250` 是两回事
///
/// `mods/lostland/dialogues.json5` 里管理者「今年的税」那一行的阈值也写着
/// 250。**刻意不让内容去引用本常量**：那等于把一次数值决定从内容作者手里
/// 拿走。两者相等是那一批内容的设计选择，由端到端测试
/// （`crates/ll-game/tests/dialogue_session.rs`）钉住「加入之后那一行真的
/// 出现」，不是靠两处数字长得一样。
pub const JOIN_SETTLEMENT_STANDING: i32 = 250;

/// 把 `ContentIndex` 反查回 `NamespacedId` 的最小接口。
///
/// 与 [`crate::skill::SkillCatalog`]/[`crate::quest::QuestCatalog`] 同一套
/// 依赖倒置手法：真正的注册表（`ll_mod::registry::Registry`）定义在下游的
/// `ll-mod`，本 crate 只声明「给我一个索引，还我它的标识符」这个接口。
///
/// 只有 [`DialogueCondition::QuestCompleted`] 这一支需要它——任务进度按
/// **标识符字符串**存在 `mod_state` 里（见 [`crate::quest`] 模块文档
/// 「计数键」一节的论证），而条件里存的是 `ContentIndex`（跨表引用的既有
/// 纪律，能被装载后校验查出拼错）。两种表示之间必须有一次反查。
pub trait ContentIdLookup {
    /// 反查一个内容索引的标识符；查不到返回 `None`。
    fn id_of(&self, index: ContentIndex) -> Option<&NamespacedId>;
}

/// 空反查：什么都查不到。
///
/// 给「手上没有注册表、但确实想求值一批不含
/// [`DialogueCondition::QuestCompleted`] 的条件」的调用方用，与
/// [`crate::quest::NoQuests`]/[`crate::skill::NoSkills`] 同一个理由：这是
/// 保底实现，不是特殊路径。任务谓词在它下面恒判为「未完成」。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoContentIds;

impl ContentIdLookup for NoContentIds {
    fn id_of(&self, _index: ContentIndex) -> Option<&NamespacedId> {
        None
    }
}

/// 透过一层引用照样是一份反查——让 [`all_conditions_hold`] 这类收
/// `&impl ContentIdLookup` 的泛型函数能直接接住一个
/// `&dyn ContentIdLookup`（[`crate::catalogs::ResolveCatalogs`] 里存的
/// 就是后者）。
///
/// 写成 `?Sized` 的一揽子实现而不是只给 `&dyn ContentIdLookup` 写一条：
/// 后者会让 `&SomeConcreteTable` 走不通，于是调用方要按手上拿的是具体
/// 类型还是 trait 对象分两种写法——那是一处只为绕开类型系统而存在的
/// 分叉。
impl<T: ContentIdLookup + ?Sized> ContentIdLookup for &T {
    fn id_of(&self, index: ContentIndex) -> Option<&NamespacedId> {
        (**self).id_of(index)
    }
}

/// 一条对话选项里 `resolve` 需要看的两样东西。
///
/// **不含 `text_key` 与 `next`**：前者是纯呈现层的事，后者是 UI 状态
/// （规格 7.1「会话内的位置是 UI 状态」）——`resolve` 两样都不该读。
/// 视图只开放它真正需要的字段，与 `ll_mod` 那批 `*View` 类型同一条
/// 既有手法。
#[derive(Debug, Clone, Copy)]
pub struct DialogueOptionView<'a> {
    /// 这一行的显示条件——`resolve` 要**重新校验**一遍，见
    /// [`crate::resolve`] 的 `Intent::DialogueChoose` 一支。
    pub conditions: &'a [DialogueCondition],
    /// 选中之后世界发生什么。
    pub outcomes: &'a [DialogueOutcome],
}

/// 「这个节点的第几条选项长什么样」——`Intent::DialogueChoose` 结算的
/// 唯一内容来源。
///
/// 与 [`crate::skill::SkillCatalog`]/[`crate::quest::QuestCatalog`] 同一
/// 套依赖倒置：真正的表（`ll_mod::dialogue::DialogueNodeTable`）定义在
/// 下游的 `ll-mod`，本 crate 只声明这个接口。
pub trait DialogueCatalog {
    /// `node` 这个节点的第 `option` 条选项；节点或下标不存在时返回
    /// `None`（**不 panic**：那两样都可能来自一个已经过时的 UI 帧）。
    fn option(&self, node: ContentIndex, option: usize) -> Option<DialogueOptionView<'_>>;
}

/// 空对话目录：任何查询都查不到。
///
/// 与 [`NoContentIds`]/[`crate::quest::NoQuests`] 同一个理由：这是保底
/// 实现，不是特殊路径。在它下面 `Intent::DialogueChoose` 恒产出空效果
/// ——与「玩家选了一条不存在的选项」同一个结果，诚实且确定。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDialogues;

impl DialogueCatalog for NoDialogues {
    fn option(&self, _node: ContentIndex, _option: usize) -> Option<DialogueOptionView<'_>> {
        None
    }
}

/// 一条对话选项的显示条件。**这十条就是全部**，见模块文档。
///
/// 否定不是一个可嵌套的算子，是**成对的变体**：`Affiliated` /
/// `NotAffiliated`、`QuestCompleted` / `QuestNotCompleted`、`FlagSet` /
/// `FlagNotSet`。这样写死了「否定只能出现在最外层一次」，而一个
/// `Not(Box<DialogueCondition>)` 变体会在结构上允许任意深度的嵌套。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueCondition {
    /// 属于某一类归属（可再指定具体组织）。
    Affiliated(AffiliationQuery),
    /// **不**属于某一类归属，参数同上。
    NotAffiliated(AffiliationQuery),
    /// 在某一类归属上的声望至少是 `value`（千分比，见
    /// `ll_world::entity::Affiliation::standing`）。
    ///
    /// 不指定 `org` 时取该类归属里 `standing` 的**最大值**——「你在任何
    /// 一个势力里声望够高」。
    StandingAtLeast {
        /// 归属查询（`org` 可选）。
        query: AffiliationQuery,
        /// 声望下界，含。
        value: i32,
    },
    /// 已完成某条任务。
    QuestCompleted(ContentIndex),
    /// 尚未完成某条任务。
    QuestNotCompleted(ContentIndex),
    /// 设过某条对话标志。
    FlagSet(NamespacedId),
    /// 没设过某条对话标志。
    FlagNotSet(NamespacedId),
    /// 背包里有至少 `count` 件某种物品。
    HasItem {
        /// 物品定义。
        item: ContentIndex,
        /// 数量下界，含；注册期校验恒 ≥ 1。
        count: u32,
    },
    /// 钱包至少有 `value`（最小货币单位）。
    WalletAtLeast(i64),
    /// 是某个种族。
    IsRace(ContentIndex),
}

/// 「属于哪一类归属、（可选）具体是哪一个组织」这个查询。
///
/// # `org` 今天只能指向**内容空间**的组织
///
/// `ll_world::entity::OrgRef` 有两支：`Def(ContentIndex)`（文化，mod 装载期
/// 确定）与 `Instance(WorldId)`（势力/宗教/行会/家族，世界生成期造出来）。
/// **`WorldId` 是世界生成期分配的，内容文件里根本写不出来。** 因此本字段
/// 写了值就只能匹配 `OrgRef::Def`；不写 = 「这一类归属里任意一条」。
///
/// 这是一处**如实登记的能力缺口**（计划文档四节 4.4）：今天写不出「加入了
/// **某一个具体势力**」这条条件。将来势力有了内容空间的表示、或者加一条按
/// 势力 id 匹配的参数时，加的是一个新字段，既有内容一个字不用改。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffiliationQuery {
    /// 归属类别。
    pub kind: AffiliationKind,
    /// 具体组织，`None` = 该类别下任意一条。
    pub org: Option<ContentIndex>,
}

impl AffiliationQuery {
    /// 这条归属查询在 `agent` 身上命中的全部条目的最大 `standing`；
    /// 一条都不命中时返回 `None`。
    ///
    /// 遍历的是 `Vec`（[`Agent::affiliations`]），保序、不碰任何哈希容器
    /// ——约束 C5。返回「最大值」而不是「第一条」，是为了让结果不依赖
    /// 归属的写入顺序：同一组归属无论以什么次序挂上去，判定结果逐位一致。
    fn best_standing(&self, agent: &Agent) -> Option<i32> {
        agent
            .affiliations
            .iter()
            .filter(|affiliation| affiliation.kind == self.kind)
            .filter(|affiliation| match (self.org, affiliation.org) {
                (None, _) => true,
                (Some(wanted), OrgRef::Def(actual)) => wanted == actual,
                // 实例空间的组织（势力/宗教/行会/家族）没有内容 id 可比，
                // 见本类型文档「`org` 今天只能指向内容空间的组织」。
                (Some(_), OrgRef::Instance(_)) => false,
            })
            .map(|affiliation| affiliation.standing)
            .max()
    }
}

/// 对一条谓词求值。
///
/// `ids` 只被 [`DialogueCondition::QuestCompleted`]/
/// [`DialogueCondition::QuestNotCompleted`] 两支用到，见
/// [`ContentIdLookup`] 文档。反查不回标识符时按「未完成」处理——与
/// `ll_mod::content_hash::write_optional_resolved` 取同一条立场：不 panic，
/// 如实退化成一个确定的结果。
pub fn condition_holds(
    condition: &DialogueCondition,
    agent: &Agent,
    ids: &impl ContentIdLookup,
) -> bool {
    match condition {
        DialogueCondition::Affiliated(query) => query.best_standing(agent).is_some(),
        DialogueCondition::NotAffiliated(query) => query.best_standing(agent).is_none(),
        DialogueCondition::StandingAtLeast { query, value } => query
            .best_standing(agent)
            .is_some_and(|best| best >= *value),
        DialogueCondition::QuestCompleted(quest) => quest_done(*quest, agent, ids),
        DialogueCondition::QuestNotCompleted(quest) => !quest_done(*quest, agent, ids),
        DialogueCondition::FlagSet(flag) => has_dialogue_flag(agent, flag),
        DialogueCondition::FlagNotSet(flag) => !has_dialogue_flag(agent, flag),
        DialogueCondition::HasItem { item, count } => held_count(agent, *item) >= u64::from(*count),
        DialogueCondition::WalletAtLeast(value) => agent.wallet >= *value,
        DialogueCondition::IsRace(race) => agent.race == *race,
    }
}

/// 一条选项的全部条件是否都满足——**数组即合取**，空数组恒真
/// （`Iterator::all` 对空迭代器恒真，这正是「无条件显示」的字面含义）。
///
/// 这是规格七节 7.2 点名的那个「只写一份」的函数：批次 2 的会话 UI 与
/// `resolve` 侧的重新校验**都调它**，不各写一份。
pub fn all_conditions_hold(
    conditions: &[DialogueCondition],
    agent: &Agent,
    ids: &impl ContentIdLookup,
) -> bool {
    conditions
        .iter()
        .all(|condition| condition_holds(condition, agent, ids))
}

/// 任务谓词的共同一半：反查标识符再问 [`is_quest_completed`]。
fn quest_done(quest: ContentIndex, agent: &Agent, ids: &impl ContentIdLookup) -> bool {
    ids.id_of(quest)
        .is_some_and(|id| is_quest_completed(agent, id))
}

/// 背包里某种物品的总数。
///
/// 用 `u64` 累加而不是 `u32`：单个 [`ll_world::item::ItemStack::count`] 是
/// `u32`，多堆相加会溢出，而 `u32::checked_add` 链在这里只会把「东西太多」
/// 这件事变成一个假的「不满足」。判据本身是 `>=`，放宽累加宽度是零成本。
fn held_count(agent: &Agent, item: ContentIndex) -> u64 {
    agent
        .inventory
        .iter()
        .filter(|stack| stack.def == item)
        .map(|stack| u64::from(stack.count))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use ll_core::ident::Interner;
    use ll_world::entity::{Affiliation, BaseStats, Gender};
    use ll_world::item::ItemStack;
    use ll_world::ownership::Owner;

    /// 一份最小的 `ContentIndex → NamespacedId` 反查，测试里够用。
    struct Ids(Vec<(ContentIndex, NamespacedId)>);

    impl ContentIdLookup for Ids {
        fn id_of(&self, index: ContentIndex) -> Option<&NamespacedId> {
            self.0
                .iter()
                .find(|(idx, _)| *idx == index)
                .map(|(_, id)| id)
        }
    }

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    /// 一个除本组用例要读的那几个字段之外全取默认值的 `Agent`——写法
    /// 与 `crate::skill_overview` 的 `blank_agent` 同源（那一份是私有的，
    /// 不能跨模块借用），本模块的谓词只读
    /// `affiliations`/`wallet`/`race`/`inventory`/`mod_state` 五个字段。
    fn 空角色() -> Agent {
        let mut interner = Interner::new();
        let profession = interner.intern(id("lostland:steward"));
        let race = interner.intern(id("lostland:human"));
        Agent {
            // 性别：测试夹具里的角色不经角色创建界面，取默认占位值。
            gender: Gender::default(),
            pos: ll_core::torus::TorusSize::new(64, 64)
                .expect("64x64 是合法尺寸")
                .wrap(0, 0),
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
            resource_pools: BTreeMap::new(),
            spent_slots: BTreeMap::new(),
            inventory: Vec::new(),
            equipment: BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                ll_core::torus::TorusSize::new(1, 1)
                    .expect("1x1 是合法尺寸")
                    .wrap(0, 0),
                ContentIndex::default(),
            ),
            mod_state: BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
            home: None,
        }
    }

    #[test]
    fn 空条件数组恒真() {
        // Arrange
        let agent = 空角色();

        // Act & Assert：这是「无条件显示这一行」的字面含义。
        assert!(all_conditions_hold(&[], &agent, &NoContentIds));
    }

    #[test]
    fn 数组即合取一条不满足整条选项就不显示() {
        // Arrange
        let mut agent = 空角色();
        agent.wallet = 100;
        let conditions = [
            DialogueCondition::WalletAtLeast(50),
            DialogueCondition::WalletAtLeast(500),
        ];

        // Act & Assert
        assert!(condition_holds(&conditions[0], &agent, &NoContentIds));
        assert!(!all_conditions_hold(&conditions, &agent, &NoContentIds));
    }

    #[test]
    fn 归属谓词成对否定互为反面() {
        // Arrange
        let mut interner = Interner::new();
        let farmstead = interner.intern(id("lostland:farmstead"));
        let mut agent = 空角色();
        agent.affiliations.push(Affiliation {
            kind: AffiliationKind::Culture,
            org: OrgRef::Def(farmstead),
            standing: 250,
        });
        let query = AffiliationQuery {
            kind: AffiliationKind::Culture,
            org: Some(farmstead),
        };

        // Act & Assert
        assert!(condition_holds(
            &DialogueCondition::Affiliated(query),
            &agent,
            &NoContentIds
        ));
        assert!(!condition_holds(
            &DialogueCondition::NotAffiliated(query),
            &agent,
            &NoContentIds
        ));
    }

    #[test]
    fn 归属查询不指定组织时匹配该类别里任意一条() {
        // Arrange
        let mut interner = Interner::new();
        let farmstead = interner.intern(id("lostland:farmstead"));
        let mut agent = 空角色();
        agent.affiliations.push(Affiliation {
            kind: AffiliationKind::Culture,
            org: OrgRef::Def(farmstead),
            standing: 400,
        });

        // Act & Assert：不指定 org ⇒ 命中；指定成另一条 ⇒ 不命中。
        let any = AffiliationQuery {
            kind: AffiliationKind::Culture,
            org: None,
        };
        let other = AffiliationQuery {
            kind: AffiliationKind::Culture,
            org: Some(interner.intern(id("lostland:harbour"))),
        };
        assert!(condition_holds(
            &DialogueCondition::Affiliated(any),
            &agent,
            &NoContentIds
        ));
        assert!(condition_holds(
            &DialogueCondition::NotAffiliated(other),
            &agent,
            &NoContentIds
        ));
    }

    #[test]
    fn 声望谓词取该类归属里的最大值不依赖写入顺序() {
        // 两条同类归属，standing 一低一高；判据是 300 —— 只有取最大值
        // 才会通过，取「第一条」会随写入顺序翻转。
        // Arrange
        let mut interner = Interner::new();
        let low = interner.intern(id("lostland:farmstead"));
        let high = interner.intern(id("lostland:harbour"));
        let query = AffiliationQuery {
            kind: AffiliationKind::Culture,
            org: None,
        };
        let condition = DialogueCondition::StandingAtLeast { query, value: 300 };

        for order in [[low, high], [high, low]] {
            let mut agent = 空角色();
            for org in order {
                agent.affiliations.push(Affiliation {
                    kind: AffiliationKind::Culture,
                    org: OrgRef::Def(org),
                    standing: if org == high { 500 } else { 100 },
                });
            }

            // Act & Assert
            assert!(
                condition_holds(&condition, &agent, &NoContentIds),
                "声望判定不该依赖归属的写入顺序"
            );
        }
    }

    #[test]
    fn 实例空间的组织不会被内容id匹配上() {
        // 势力/宗教/行会/家族走 OrgRef::Instance(WorldId)，内容里写不出
        // 那个号；写了 org 的查询在它身上必须判不命中，而不是碰巧撞上
        // 一个数值相同的 ContentIndex。
        // Arrange
        let mut interner = Interner::new();
        let some_id = interner.intern(id("lostland:farmstead"));
        let mut agent = 空角色();
        agent.affiliations.push(Affiliation {
            kind: AffiliationKind::Faction,
            org: OrgRef::Instance(ll_core::ident::WorldId::next(&mut some_id.get())),
            standing: 900,
        });
        let query = AffiliationQuery {
            kind: AffiliationKind::Faction,
            org: Some(some_id),
        };

        // Act & Assert
        assert!(!condition_holds(
            &DialogueCondition::Affiliated(query),
            &agent,
            &NoContentIds
        ));
        // 不写 org 时照样命中——缺的只是「具体是哪一个」这一档。
        assert!(condition_holds(
            &DialogueCondition::Affiliated(AffiliationQuery {
                kind: AffiliationKind::Faction,
                org: None
            }),
            &agent,
            &NoContentIds
        ));
    }

    #[test]
    fn 任务谓词读的是与任务系统同一套存储() {
        // Arrange：不自己拼 mod_state 的键，走 quest 模块的真实写入路径。
        let mut interner = Interner::new();
        let quest = interner.intern(id("lostland:main_quest_1"));
        let ids = Ids(vec![(quest, id("lostland:main_quest_1"))]);
        let mut agent = 空角色();

        // Act & Assert：没写过 ⇒ 未完成。
        assert!(condition_holds(
            &DialogueCondition::QuestNotCompleted(quest),
            &agent,
            &ids
        ));

        // 键用 quest 模块自己的 `quest_progress_key` 拼，不在这里抄一份
        // 字面量——「与任务进度同一套存储」这条断言只有在共用同一个键
        // 函数时才是真的。（不走 `mark_quest_completed` 是因为它要一个
        // `EntityId`，而那个类型只能从 `spawn` 拿到，构造不出来。）
        let quest_id = id("lostland:main_quest_1");
        agent.mod_state.insert(
            (
                quest_id.namespace().to_string(),
                crate::quest::quest_progress_key(&quest_id),
            ),
            ModStateValue::Int(1),
        );

        assert!(condition_holds(
            &DialogueCondition::QuestCompleted(quest),
            &agent,
            &ids
        ));
    }

    #[test]
    fn 反查不回标识符的任务索引判为未完成() {
        // Arrange：NoContentIds 什么都查不到。
        let agent = 空角色();

        // Act & Assert：退化成一个确定的结果，不 panic。
        assert!(condition_holds(
            &DialogueCondition::QuestNotCompleted(ContentIndex::default()),
            &agent,
            &NoContentIds
        ));
    }

    #[test]
    fn 对话标志与任务进度互不干扰() {
        // 两者同住 mod_state，键前缀不同；一个设了不该让另一个也成立。
        // Arrange
        let flag = id("lostland:dialogue_flag.guard_rumour_heard");
        let mut agent = 空角色();
        agent.mod_state.insert(
            (flag.namespace().to_string(), dialogue_flag_key(&flag)),
            ModStateValue::Bool(true),
        );

        // Act & Assert
        assert!(condition_holds(
            &DialogueCondition::FlagSet(flag.clone()),
            &agent,
            &NoContentIds
        ));
        assert!(!is_quest_completed(&agent, &flag));
    }

    #[test]
    fn 背包谓词把多堆同种物品加起来() {
        // Arrange：两堆各 2 个，判据是 3——只有累加才会通过。
        let mut interner = Interner::new();
        let item = interner.intern(id("lostland:iron_ingot"));
        let mut agent = 空角色();
        for _ in 0..2 {
            agent.inventory.push(ItemStack {
                def: item,
                count: 2,
                durability: None,
                owner: Owner::Unowned,
            });
        }

        // Act & Assert
        assert!(condition_holds(
            &DialogueCondition::HasItem { item, count: 3 },
            &agent,
            &NoContentIds
        ));
        assert!(!condition_holds(
            &DialogueCondition::HasItem { item, count: 5 },
            &agent,
            &NoContentIds
        ));
    }

    #[test]
    fn 钱包与种族谓词读的是各自那一个字段() {
        // Arrange
        let mut interner = Interner::new();
        let dwarf = interner.intern(id("lostland:dwarf"));
        let elf = interner.intern(id("lostland:elf"));
        let mut agent = 空角色();
        agent.wallet = 50_000;
        agent.race = dwarf;

        // Act & Assert
        assert!(condition_holds(
            &DialogueCondition::WalletAtLeast(50_000),
            &agent,
            &NoContentIds
        ));
        assert!(!condition_holds(
            &DialogueCondition::WalletAtLeast(50_001),
            &agent,
            &NoContentIds
        ));
        assert!(condition_holds(
            &DialogueCondition::IsRace(dwarf),
            &agent,
            &NoContentIds
        ));
        assert!(!condition_holds(
            &DialogueCondition::IsRace(elf),
            &agent,
            &NoContentIds
        ));
    }
}
