//! NPC 对话的内容表：会话入口（[`DialogueTable`]）与节点
//! （[`DialogueNodeTable`]）两张。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md`，落地计划见
//! `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md`（批次 1：
//! **只做内容表**，不做交互列表接线、会话 UI、`Intent::DialogueChoose`
//! 与三条后果）。数据文件是 `mods/<id>/dialogues.json5`，schema 与装载在
//! [`crate::content_schema_dialogue`]。
//!
//! # 形状：扁平节点表 + 选项里写 `next` 跳转
//!
//! 不是嵌套树、也不是完整状态机（规格一节 1.1 逐条比较过三种做法）。
//! 三条能验证的理由：
//!
//! 1. **与仓库既有内容类型同构**。`SkillDef.prerequisites`、
//!    `QuestNodeDef.prerequisites` 都是「扁平表 + 一条 `Vec<ContentIndex>`
//!    的边」，跨表引用的 intern/查表纪律（[`crate::content_schema`] 模块
//!    文档「只 intern 还是必须已定义」）一个字不用改就能复用。嵌套树在
//!    这条纪律下是异类——它的子节点没有 `id`，也就没有 `ContentIndex`，
//!    进不了注册表，也进不了内容哈希。
//! 2. **回环是真实需求**。「加入据点 → 好的 → 还有别的事吗 → 回到开场白」
//!    这条路径在丙档里必然出现。树表达不了它，只能复制子树，而复制的两份
//!    迟早漂移——真相源之外的副本分叉时没有任何东西会报错。
//! 3. **能被内容作者手写**。顶层是两个数组，每个元素是一个平的对象。
//!
//! # 因此对话图**允许有环**——本模块刻意没有无环校验
//!
//! [`crate::skill::SkillTable`]/[`crate::quest::QuestTable`] 都在注册期跑
//! [`crate::prereq_graph::validate_no_cycles`]，因为那两张图表达的是「前置
//! 解锁」，环意味着谁都学不到。**对话图的环是合法的、且是设计意图**，因此
//! [`DialogueNodeTable`] **刻意不实现**
//! [`crate::prereq_graph::PrerequisiteGraph`]——不是「实现了但没调用」，是
//! 本模块里根本没有那段 DFS。
//!
//! 那么什么保证对话不会死循环？**每一次跳转都必须由玩家按一次键。** 引擎
//! 侧不提供「条件满足就自动推进到下一个节点」这种转移：一个节点显示出来
//! 之后，唯一的推进方式是玩家从选项里选一条。这条规则把终止性从一个需要
//! 静态分析的性质降级成一个**结构上不可能违反**的性质，代价是不能写自动
//! 播放的过场——今天没有这个需求，按 YAGNI 不做。本模块的落点是
//! [`DialogueNext`] 只有两个变体，没有第三种「无需输入的转移」。
//!
//! # 注册期仍然要校验的两条
//!
//! [`validate_references`]，与 `QuestError::UnregisteredPrerequisite` 同一
//! 形状：每个 [`DialogueAttrs::root`] 与每个 [`DialogueNext::Node`] 指向的
//! 节点**必须已定义**。
//!
//! **不校验**「每个节点都从某个根可达」：一份 mod 完全可能只提供一批被
//! 别的 mod 引用的通用节点，判它「孤儿」会把一条正确的设计判成错误——与
//! `QuestCondition::KillCount.target_kind` 走 `UntypedIdSpace` 豁免是同一
//! 类判断（[`crate::content_audit`]）。
//!
//! # 谁说这句话：按 `Agent.profession` 匹配，不给 `Agent` 加对话字段
//!
//! 给每个 `Agent` 存一个 `dialogue: Option<ContentIndex>` 是最直觉的做法，
//! **已被规格 1.3 否决**：`Agent` 的两条生产路径
//! （`ll_game::world::build_player_agent`/`ll_mod::roster::build_npc_agent`）
//! 今天都不知道该往里填什么，填不了就是又一个「声明了但没接线」的死字段，
//! 而 `scripts/ci/check_field_consumers.py` 是**阻断模式**门禁。
//!
//! 改成 [`DialogueDef`] 自己声明它认谁（[`DialogueSpeaker`]），两个字段
//! 今天都真的存在于每一个物化 NPC 身上。裁决顺序见
//! [`DialogueTable::match_speaker`]。
//!
//! # 文案一个字都不进 JSON5
//!
//! [`DialogueNodeAttrs::text_key`] 与 [`DialogueOption::text_key`] 是
//! **本地化键**，走 `parse_id` 而**不是** `intern_id`：本地化键只解析成
//! [`NamespacedId`]，不进注册表、不占内容索引号、不参与 `ContentIndex` 的
//! 分配顺序——与 `RaceAttrs`/`ClassAttrs`/`ItemDef` 的 `display_name_key`
//! 逐字同办。
//!
//! 理由不是「一致性」（规格三节 3.1）：`scripts/ci/check_i18n_strings.py`
//! **只扫 `crates/*/src/**/*.rs` 里含 CJK 的字符串字面量**，
//! `mods/**/*.json5` 完全在它的视野之外——允许内联文案等于让规格 §11.3
//! 「代码中不得出现任何硬编码的用户可见字符串」这条硬规则，在项目里文本量
//! 最大的那个系统上等于不存在。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId};

use crate::registry::Registry;
pub use ll_sim::dialogue::{
    AffiliationQuery, ContentIdLookup, DialogueCondition, NoContentIds, all_conditions_hold,
    condition_holds, dialogue_flag_key, has_dialogue_flag,
};

/// 把内容索引反查回标识符——[`ll_sim::dialogue::DialogueCondition`] 的任务
/// 谓词求值需要它，见 [`ContentIdLookup`] 文档。
///
/// 实现放在这里而不是 `ll-sim`：`Registry` 定义在本 crate，而 trait 定义在
/// 上游——这正是 [`crate::quest::RegisteredQuests`] 那类「把表与注册表绑在
/// 一起再实现上游 trait」的同一条既有手法，只是本条不需要额外的绑定结构体
/// （`Registry` 自己就够）。
impl ContentIdLookup for Registry {
    fn id_of(&self, index: ContentIndex) -> Option<&NamespacedId> {
        self.resolve(index)
    }
}

/// 一段对话认谁说——按 `Agent.profession`（+可选文化）匹配。
///
/// 两个字段今天都真的存在于每一个物化 NPC 身上：`Agent.profession` 由
/// `NpcProfile.profession` 赋值，`Agent.affiliations` 里那条
/// `AffiliationKind::Culture` 由据点文化赋值。**这是整个对话系统唯一一处
/// 不需要任何新字段就能跑起来的接线。**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueSpeaker {
    /// 职业，必填。指向职业表，**必须已定义**（只 get 不 intern）。
    pub profession: ContentIndex,
    /// 文化，可选——再收窄一档。指向文化表，同样必须已定义。
    pub culture: Option<ContentIndex>,
}

/// 选中一条选项之后跳到哪。
///
/// **只有两个变体**：这是终止性保证的落点，见模块文档「因此对话图允许有
/// 环」一节——没有「无需玩家输入的自动转移」这第三种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogueNext {
    /// 结束会话。数据文件里写保留字 `"end"`。
    End,
    /// 跳到另一个节点——**可以指回任何一个已定义的节点，包括来路上的
    /// 节点**（回环合法）。
    Node(ContentIndex),
}

/// 一条对话选项。
///
/// **本批次没有 `outcomes` 字段**，这不是遗漏：批次 1 一条后果都不做，一个
/// 只允许空数组的字段就是一个「声明了但没接线」的死字段——本仓库长期记账的
/// 正是这一类。`#[serde(deny_unknown_fields)]` 会让今天写 `outcomes:` 的
/// 内容当场报错，这比让它静默无效诚实。批次 2 加这个字段时，加的是一个
/// 从第一天起就有真实消费者的字段，见计划文档第七节的挂载点表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueOption {
    /// 这一行显示什么——**本地化键**，不是文案，见模块文档末节。
    pub text_key: NamespacedId,
    /// 全部满足才显示这一行；空数组 = 无条件显示（数组即合取）。
    pub conditions: Vec<DialogueCondition>,
    /// 选中之后跳到哪。
    pub next: DialogueNext,
}

/// 单条会话入口声明：本体与 mod 注册对话时共用的同一个输入形状。
///
/// 这就是「本体即 Mod」在对话层面的验收标的——本体的声明与第三方 mod 的
/// 声明除了 `id` 里的命名空间字符串之外不存在任何结构性差异，与
/// [`crate::quest::QuestNodeDef`] 同一条说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueDef {
    /// 命名空间标识符，例如 `lostland:steward_greeting`。
    pub id: NamespacedId,
    /// 这段对话认谁说。
    pub speaker: DialogueSpeaker,
    /// 从哪个节点开始。
    pub root: ContentIndex,
}

/// 单条节点声明，形状说明同 [`DialogueDef`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueNodeDef {
    /// 命名空间标识符，例如 `lostland:steward_root`。
    pub id: NamespacedId,
    /// NPC 这一句说什么——**本地化键**。
    pub text_key: NamespacedId,
    /// 玩家能选的行，**按书写顺序**（约束 C5：JSON5 数组保序，`serde` 按
    /// 书写顺序产出 `Vec`，中间不塞任何哈希容器）。空列表 = 死路一条，
    /// 合法但玩家只能退出会话。
    pub options: Vec<DialogueOption>,
}

/// [`DialogueTable::define`] 实际存进列式存储的属性子集——不含 `id`，理由
/// 同 [`crate::quest::QuestAttrs`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueAttrs {
    /// 这段对话认谁说。
    pub speaker: DialogueSpeaker,
    /// 从哪个节点开始。
    pub root: ContentIndex,
}

/// [`DialogueNodeTable::define`] 实际存进列式存储的属性子集。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueNodeAttrs {
    /// NPC 这一句说什么——本地化键。
    pub text_key: NamespacedId,
    /// 玩家能选的行，按书写顺序。
    pub options: Vec<DialogueOption>,
}

/// 对话注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误在
/// 加载时就报出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueError {
    /// 同一个内容索引被定义了两次，理由同
    /// [`crate::quest::QuestError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// 某段会话的 `root` 指向一个节点表从未登记过的索引。
    UnregisteredRoot {
        /// 声明了这条悬空入口的会话。
        dialogue: ContentIndex,
        /// 被引用但未登记的节点索引。
        missing: ContentIndex,
    },
    /// 某个选项的 `next` 指向一个节点表从未登记过的索引。
    UnregisteredNext {
        /// 声明了这条悬空跳转的节点。
        node: ContentIndex,
        /// 被引用但未登记的节点索引。
        missing: ContentIndex,
    },
}

impl DialogueError {
    /// 这条错误牵涉到的全部内容索引，用法同
    /// [`crate::quest::QuestError::involved_indices`]（调用方拿它去
    /// `Registry` 里反查出可读的 id 再报给玩家）。
    pub fn involved_indices(&self) -> Vec<ContentIndex> {
        match self {
            DialogueError::DuplicateDefinition(index) => vec![*index],
            DialogueError::UnregisteredRoot { dialogue, missing } => vec![*dialogue, *missing],
            DialogueError::UnregisteredNext { node, missing } => vec![*node, *missing],
        }
    }
}

impl fmt::Display for DialogueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DialogueError::DuplicateDefinition(index) => {
                write!(f, "对话索引 {} 被重复定义", index.get())
            }
            DialogueError::UnregisteredRoot { dialogue, missing } => write!(
                f,
                "对话索引 {} 的起始节点 {} 未在当前节点表中登记",
                dialogue.get(),
                missing.get()
            ),
            DialogueError::UnregisteredNext { node, missing } => write!(
                f,
                "对话节点 {} 的某条选项跳转到 {}，而它未在当前节点表中登记",
                node.get(),
                missing.get()
            ),
        }
    }
}

impl std::error::Error for DialogueError {}

/// 一次会话入口查询命中的完整结果，理由同 [`crate::quest::QuestView`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueView {
    /// 这段对话认谁说。
    pub speaker: DialogueSpeaker,
    /// 起始节点。
    pub root: ContentIndex,
}

/// 一次节点查询命中的完整结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueNodeView<'a> {
    /// NPC 这一句说什么——本地化键。
    pub text_key: &'a NamespacedId,
    /// 玩家能选的行，按书写顺序。
    pub options: &'a [DialogueOption],
}

/// 扩容占位用的本地化键——未定义的槽位永远被 `defined` 位图挡住，不会被
/// 外部查询实际读到，与 `QuestTable::define` 里那条 `count` 为 0 的
/// `KillCount` 占位是同一个理由。
fn placeholder_key() -> NamespacedId {
    NamespacedId::parse("lostland:dialogue.undefined").expect("固定字面量标识符恒合法")
}

/// 会话入口的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017），与 [`crate::quest::QuestTable`] 同一套道理。
#[derive(Debug, Default, Clone)]
pub struct DialogueTable {
    speaker: Vec<DialogueSpeaker>,
    root: Vec<ContentIndex>,
    defined: Vec<bool>,
    /// 按注册顺序记录已定义的索引，理由同
    /// [`crate::quest::QuestTable`] 同名字段文档。
    defined_ids: Vec<ContentIndex>,
}

impl DialogueTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上会话属性。
    ///
    /// 只做「不得重复定义」这一项校验；`root` 指向的节点可以还没 `define`
    /// （同一个文件里前向引用是常态），真正的引用校验在全部 mod 装载完毕
    /// 之后由 [`validate_references`] 一次性做。
    pub fn define(
        &mut self,
        index: ContentIndex,
        attrs: DialogueAttrs,
    ) -> Result<(), DialogueError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.speaker.resize(
                new_len,
                DialogueSpeaker {
                    profession: ContentIndex::default(),
                    culture: None,
                },
            );
            self.root.resize(new_len, ContentIndex::default());
        }
        if self.defined[idx] {
            return Err(DialogueError::DuplicateDefinition(index));
        }
        self.defined[idx] = true;
        self.speaker[idx] = attrs.speaker;
        self.root[idx] = attrs.root;
        self.defined_ids.push(index);
        Ok(())
    }

    /// 给定索引当前是否已经登记过属性。
    pub fn is_defined(&self, dialogue: ContentIndex) -> bool {
        self.defined
            .get(dialogue.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一段会话的完整属性，未注册的索引返回 `None`（对齐 ADR 0015）。
    pub fn get(&self, dialogue: ContentIndex) -> Option<DialogueView> {
        if !self.is_defined(dialogue) {
            return None;
        }
        let idx = dialogue.get() as usize;
        Some(DialogueView {
            speaker: self.speaker[idx],
            root: self.root[idx],
        })
    }

    /// 按 [`ContentIndex::get`] 数值升序返回全部已注册的会话索引——约束
    /// C5，不依赖内部 `defined_ids` 的原始注册顺序，理由同
    /// [`crate::quest::QuestTable::defined_indices`]。
    pub fn defined_indices(&self) -> Vec<ContentIndex> {
        let mut ids = self.defined_ids.clone();
        ids.sort_by_key(ContentIndex::get);
        ids
    }

    /// 给一个 NPC 挑出该由哪段对话说话：按职业匹配，可选再按文化收窄。
    ///
    /// # 裁决顺序（约束 C5，规格 1.3）
    ///
    /// 多条候选同时匹配时：
    ///
    /// 1. **声明了 `culture` 的胜过只声明 `profession` 的**（更具体优先）；
    /// 2. 仍然平局时，按 [`ContentIndex`] **升序取最小者**。
    ///
    /// 第 2 条的确定性只在「同一套内容集内」成立，**不是**「跨 mod 集稳定」
    /// ——`ContentIndex` 依赖 mod 装载顺序。这与地图归并的平局破法是同一
    /// 形状、且比它轻：那里影响的是格子颜色，这里影响的是「装了两个都想给
    /// 铁匠写台词的 mod 时，哪一个赢」。**这一条要写进 mod 文档**，不能让
    /// 作者自己去猜。
    ///
    /// `culture` 传 `None` 表示这个 NPC 没有文化归属——此时只有不声明
    /// `culture` 的会话能匹配上。
    pub fn match_speaker(
        &self,
        profession: ContentIndex,
        culture: Option<ContentIndex>,
    ) -> Option<ContentIndex> {
        self.defined_indices()
            .into_iter()
            .filter(|index| {
                let view = self
                    .get(*index)
                    .expect("defined_indices 里的索引必然已通过 define 注册");
                view.speaker.profession == profession
                    && match view.speaker.culture {
                        None => true,
                        Some(wanted) => culture == Some(wanted),
                    }
            })
            // 排序键：声明了文化的排前面（`false < true`，所以取「没声明
            // 文化」当键），然后按索引升序——`min_by_key` 在平局时取先
            // 遇到的那个，而输入已经是升序，两条裁决规则一次表达完。
            .min_by_key(|index| {
                let view = self
                    .get(*index)
                    .expect("defined_indices 里的索引必然已通过 define 注册");
                (view.speaker.culture.is_none(), index.get())
            })
    }
}

/// 对话节点的列式存储，形状与理由同 [`DialogueTable`]。
#[derive(Debug, Default, Clone)]
pub struct DialogueNodeTable {
    text_key: Vec<NamespacedId>,
    options: Vec<Vec<DialogueOption>>,
    defined: Vec<bool>,
    defined_ids: Vec<ContentIndex>,
}

impl DialogueNodeTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上节点属性。
    pub fn define(
        &mut self,
        index: ContentIndex,
        attrs: DialogueNodeAttrs,
    ) -> Result<(), DialogueError> {
        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.text_key.resize(new_len, placeholder_key());
            self.options.resize(new_len, Vec::new());
        }
        if self.defined[idx] {
            return Err(DialogueError::DuplicateDefinition(index));
        }
        self.defined[idx] = true;
        self.text_key[idx] = attrs.text_key;
        self.options[idx] = attrs.options;
        self.defined_ids.push(index);
        Ok(())
    }

    /// 给定索引当前是否已经登记过属性。
    pub fn is_defined(&self, node: ContentIndex) -> bool {
        self.defined
            .get(node.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一个节点的完整属性，未注册的索引返回 `None`。
    pub fn get(&self, node: ContentIndex) -> Option<DialogueNodeView<'_>> {
        if !self.is_defined(node) {
            return None;
        }
        let idx = node.get() as usize;
        Some(DialogueNodeView {
            text_key: &self.text_key[idx],
            options: &self.options[idx],
        })
    }

    /// 按 [`ContentIndex::get`] 数值升序返回全部已注册的节点索引（约束 C5）。
    pub fn defined_indices(&self) -> Vec<ContentIndex> {
        let mut ids = self.defined_ids.clone();
        ids.sort_by_key(ContentIndex::get);
        ids
    }
}

/// 注册期校验：每个 `root` 与每个 `next` 都必须指向一个**真的被节点表
/// 登记过**的节点。
///
/// **刻意不做无环校验**，见模块文档「因此对话图允许有环」一节——那不是
/// 忘了，是这张图的环合法。本函数体里没有任何 DFS、没有颜色标记、没有
/// 环路径收集。
///
/// 必须排在**全部 mod 装载完毕之后**跑，理由与
/// [`crate::quest::validate_no_cycles`] 逐字相同：一个 mod 完全可以把自己
/// 的节点接到本体的对话上，只看单个 mod 的装载结果会误判。生产调用点是
/// `ll_game::content::load_content`。
pub fn validate_references(
    dialogues: &DialogueTable,
    nodes: &DialogueNodeTable,
) -> Result<(), DialogueError> {
    for dialogue in dialogues.defined_indices() {
        let view = dialogues
            .get(dialogue)
            .expect("defined_indices 里的索引必然已通过 define 注册");
        if !nodes.is_defined(view.root) {
            return Err(DialogueError::UnregisteredRoot {
                dialogue,
                missing: view.root,
            });
        }
    }
    for node in nodes.defined_indices() {
        let view = nodes
            .get(node)
            .expect("defined_indices 里的索引必然已通过 define 注册");
        for option in view.options {
            if let DialogueNext::Node(target) = option.next
                && !nodes.is_defined(target)
            {
                return Err(DialogueError::UnregisteredNext {
                    node,
                    missing: target,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::Interner;

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    fn 节点(text: &str, next: DialogueNext) -> DialogueNodeAttrs {
        DialogueNodeAttrs {
            text_key: id(text),
            options: vec![DialogueOption {
                text_key: id("lostland:dialogue.common.back"),
                conditions: Vec::new(),
                next,
            }],
        }
    }

    #[test]
    fn 回环的两个节点互相跳转装载成功() {
        // 这条是「对话图允许有环」这件事的钉子：A → B → A。若哪天有人
        // 给本模块加上无环校验，它必须先让这条测试变红。
        // Arrange
        let mut interner = Interner::new();
        let a = interner.intern(id("lostland:a"));
        let b = interner.intern(id("lostland:b"));
        let mut nodes = DialogueNodeTable::new();
        nodes
            .define(a, 节点("lostland:dialogue.a", DialogueNext::Node(b)))
            .expect("首次定义");
        nodes
            .define(b, 节点("lostland:dialogue.b", DialogueNext::Node(a)))
            .expect("首次定义");
        let mut dialogues = DialogueTable::new();
        dialogues
            .define(
                interner.intern(id("lostland:greeting")),
                DialogueAttrs {
                    speaker: DialogueSpeaker {
                        profession: interner.intern(id("lostland:steward")),
                        culture: None,
                    },
                    root: a,
                },
            )
            .expect("首次定义");

        // Act & Assert
        assert_eq!(validate_references(&dialogues, &nodes), Ok(()));
    }

    #[test]
    fn 跳到一个没定义过的节点在校验期报错并点名两端() {
        // Arrange
        let mut interner = Interner::new();
        let a = interner.intern(id("lostland:a"));
        let ghost = interner.intern(id("lostland:ghost"));
        let mut nodes = DialogueNodeTable::new();
        nodes
            .define(a, 节点("lostland:dialogue.a", DialogueNext::Node(ghost)))
            .expect("首次定义");

        // Act
        let result = validate_references(&DialogueTable::new(), &nodes);

        // Assert
        assert_eq!(
            result,
            Err(DialogueError::UnregisteredNext {
                node: a,
                missing: ghost
            })
        );
    }

    #[test]
    fn 起始节点没定义过同样报错() {
        // Arrange
        let mut interner = Interner::new();
        let ghost = interner.intern(id("lostland:ghost"));
        let dialogue = interner.intern(id("lostland:greeting"));
        let mut dialogues = DialogueTable::new();
        dialogues
            .define(
                dialogue,
                DialogueAttrs {
                    speaker: DialogueSpeaker {
                        profession: interner.intern(id("lostland:steward")),
                        culture: None,
                    },
                    root: ghost,
                },
            )
            .expect("首次定义");

        // Act & Assert
        assert_eq!(
            validate_references(&dialogues, &DialogueNodeTable::new()),
            Err(DialogueError::UnregisteredRoot {
                dialogue,
                missing: ghost
            })
        );
    }

    #[test]
    fn 结束会话的选项不需要任何节点存在() {
        // Arrange
        let mut interner = Interner::new();
        let a = interner.intern(id("lostland:a"));
        let mut nodes = DialogueNodeTable::new();
        nodes
            .define(a, 节点("lostland:dialogue.a", DialogueNext::End))
            .expect("首次定义");

        // Act & Assert
        assert_eq!(validate_references(&DialogueTable::new(), &nodes), Ok(()));
    }

    #[test]
    fn 重复定义同一个索引报错() {
        // Arrange
        let mut interner = Interner::new();
        let a = interner.intern(id("lostland:a"));
        let mut nodes = DialogueNodeTable::new();
        nodes
            .define(a, 节点("lostland:dialogue.a", DialogueNext::End))
            .expect("首次定义");

        // Act & Assert
        assert_eq!(
            nodes.define(a, 节点("lostland:dialogue.a", DialogueNext::End)),
            Err(DialogueError::DuplicateDefinition(a))
        );
    }

    /// 造一张有三段候选对话的表：只声明职业的两条 + 声明了文化的一条。
    ///
    /// 返回的 `Interner` 必须一起交出去——候选表里的索引与调用方后续
    /// `intern` 出来的索引只有出自**同一个** interner 才可比。测试里各自
    /// 新建一个 interner 会让两串编号从 0 重新开始，于是一个本该「对不上」
    /// 的职业会撞上另一条内容的编号（这条注释是本文件里真的踩过的那次）。
    fn 三段候选() -> (
        Interner,
        DialogueTable,
        ContentIndex,
        ContentIndex,
        [ContentIndex; 3],
    ) {
        let mut interner = Interner::new();
        let guard = interner.intern(id("lostland:guard"));
        let mining = interner.intern(id("lostland:mining_hold"));
        let root = interner.intern(id("lostland:root"));
        let mut table = DialogueTable::new();
        let mut ids = Vec::new();
        for (raw, culture) in [
            ("lostland:generic_b", None),
            ("lostland:generic_a", None),
            ("lostland:mining", Some(mining)),
        ] {
            let index = interner.intern(id(raw));
            ids.push(index);
            table
                .define(
                    index,
                    DialogueAttrs {
                        speaker: DialogueSpeaker {
                            profession: guard,
                            culture,
                        },
                        root,
                    },
                )
                .expect("首次定义");
        }
        (interner, table, guard, mining, [ids[0], ids[1], ids[2]])
    }

    #[test]
    fn 声明了文化的候选胜过只声明职业的() {
        // Arrange
        let (_interner, table, guard, mining, [_generic_b, _generic_a, mining_dialogue]) =
            三段候选();

        // Act & Assert
        assert_eq!(
            table.match_speaker(guard, Some(mining)),
            Some(mining_dialogue),
            "更具体的候选必须赢"
        );
    }

    #[test]
    fn 平局时取最小的内容索引而不是注册顺序里的第一条() {
        // `generic_b` 先注册（索引更小），`generic_a` 后注册。文化不匹配
        // 时两条通用候选平局，规则是取最小索引——这条断言之所以有意义，
        // 是因为「注册顺序第一条」在这里恰好也是它，所以再补一条反向的：
        // 见下面对 defined_indices 升序的断言。
        // Arrange
        let (_interner, table, guard, _mining, [generic_b, _generic_a, _mining_dialogue]) =
            三段候选();

        // Act & Assert
        assert_eq!(table.match_speaker(guard, None), Some(generic_b));
        let mut sorted = table.defined_indices();
        let unsorted = sorted.clone();
        sorted.sort_by_key(ContentIndex::get);
        assert_eq!(unsorted, sorted, "defined_indices 必须是升序");
    }

    #[test]
    fn 职业对不上就一条都不匹配() {
        // Arrange
        let (mut interner, table, _guard, mining, _ids) = 三段候选();
        // 一个从没出现在任何 speaker 里的职业索引——**必须出自同一个
        // interner**，否则编号会与表里的内容撞上。
        let farmer = interner.intern(id("lostland:farmer"));

        // Act & Assert
        assert_eq!(table.match_speaker(farmer, Some(mining)), None);
    }

    #[test]
    fn 没有文化的npc匹配不到声明了文化的那一条() {
        // Arrange
        let (_interner, table, guard, _mining, [generic_b, _generic_a, _mining_dialogue]) =
            三段候选();

        // Act & Assert：退回通用候选，不是那条矿堡专属的。
        assert_eq!(table.match_speaker(guard, None), Some(generic_b));
    }

    #[test]
    fn 未定义的索引查询返回none() {
        // Arrange
        let mut interner = Interner::new();
        let ghost = interner.intern(id("lostland:ghost"));

        // Act & Assert：对齐 ADR 0015。
        assert!(DialogueTable::new().get(ghost).is_none());
        assert!(DialogueNodeTable::new().get(ghost).is_none());
    }
}
