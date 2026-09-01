//! 会话屏：跟一个 NPC 说话的那一块模态屏。
//!
//! 设计冻结在 `knowledge/design/dialogue-system.md`（**七节 7.1/7.2**），
//! 落地计划见 `docs/superpowers/plans/2026-08-31-batch21-dialogue-ui.md`。
//!
//! # 它就是既有的模态屏，不是新造的一套
//!
//! `ll_ui::screen::ScreenData` 的五个槽刚好装得下一场会话：
//!
//! | 槽 | 装什么 |
//! |---|---|
//! | `title_key` | **节点的 `text_key`**——NPC 这一句 |
//! | `rows` | 过滤后的选项文案，按书写顺序 |
//! | `cursor` | 预选第一项（所有者裁定，见下） |
//! | `empty_key` | 一条选项都显示不出来时的占位行 |
//! | `hint_key` | 底部按键提示 |
//!
//! 把 NPC 那一句放进 `title_key` 不是凑合：这块屏的标题**就是**他说的
//! 那句话。给 `ScreenData` 加一个 `lead` 槽要改十余处构造点，换来的只是
//! 一个更符合直觉的字段名。
//!
//! 白送的三样东西：行矩形（= 鼠标点在第几行）、聚焦/悬停高亮、面板按
//! 内容宽度伸缩，全部由 `ll_ui::screen::screen_geometry` 与其余模态屏
//! **同一次**产出——`crate::pointer` 那四条约定因此一行代码都不用重写。
//!
//! # 选项过滤调的是 `resolve` 那一侧的同一个函数
//!
//! [`dialogue_rows`] 用 `ll_sim::dialogue::all_conditions_hold`，
//! `Intent::DialogueChoose` 的重新校验用的也是它（规格 7.2）。**不各写
//! 一份**：UI 算出「这一行该显示」用的是某一帧的世界快照，`resolve`
//! 结算时世界可能已经变了，两份判据分叉时没有任何东西会报错。
//!
//! # 会话位置是 UI 状态
//!
//! 「玩家现在停在哪个节点上」不进 `WorldState`、不进存档、不进世界哈希
//! （规格 7.1）——它与背包光标停在第几行是同一类东西。代价是中途存盘
//! 退出会丢失会话位置，下次要重新开口，与背包开着时退出完全一致。
//!
//! # 对话不消耗回合
//!
//! 所有者裁定（交接文档第〇之二节第 2 条）。落点在 `ll-sim` 那一侧
//! （`resolve_dialogue_choose` 不产 `Effect::ScheduleNext`），本模块只是
//! 不做任何与时间有关的事。

use ll_core::ident::ContentIndex;
use ll_i18n::Catalog;
use ll_mod::dialogue::{ContentIdLookup, DialogueNext, DialogueNodeTable, all_conditions_hold};
use ll_platform::input::{GameKey, InputState};
use ll_sim::intent::Intent;
use ll_world::entity::{Agent, EntityId};

use crate::menu_screen::{ScreenOutcome, ScreenState};
use crate::pointer::RowPointer;

/// 会话屏这一帧的一行：显示什么字，以及它对应**原始选项列表里的第几
/// 条**。
///
/// # 为什么必须把原始下标带着走
///
/// 显示出来的是**过滤后**的行；而 `Intent::DialogueChoose` 传给
/// `resolve` 的必须是原始下标——`resolve` 手上只有这个节点的完整选项
/// 列表，它没有、也不该有 UI 这一帧过滤出了哪几行这份信息（规格 7.2：
/// **不能相信 UI 传来的序号**，它要按自己看到的世界重新校验一遍）。
///
/// 传过滤后的行号会在「UI 算出来之后、结算之前世界变了」时静悄悄地
/// 作用到另一条选项上。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueRow {
    /// 在 `DialogueNodeAttrs::options` 里的下标。
    pub option: usize,
    /// 这一行显示什么（已经过 `Catalog` 解析）。
    pub text: String,
}

/// 这一帧会话屏要显示的行——**条件不满足的选项一行都不出现**。
///
/// 查不到这个节点（内容被换掉、或者调用方传了一个野索引）时返回空列表：
/// 玩家会看到占位行，按取消退出。**不 panic**——一块屏的内容问题不该
/// 拖垮整局，与本 crate 其余降级路径同一条纪律。
///
/// # 顺序确定（约束 C5）
///
/// `options` 是 `Vec`（JSON5 数组按书写顺序），全程线性扫描，不碰任何
/// 哈希容器。这条在这里是**真陷阱**而不是形式要求：玩家按的是「第几
/// 行」。
pub fn dialogue_rows(
    node: ContentIndex,
    nodes: &DialogueNodeTable,
    agent: &Agent,
    ids: &impl ContentIdLookup,
    catalog: &Catalog,
    language: &str,
) -> Vec<DialogueRow> {
    let Some(view) = nodes.get(node) else {
        return Vec::new();
    };
    view.options
        .iter()
        .enumerate()
        // **规格 7.2 点名的那个「只写一份」的函数**，与 `resolve` 侧
        // 的重新校验是同一个。
        .filter(|(_, option)| all_conditions_hold(&option.conditions, agent, ids))
        .map(|(option, def)| DialogueRow {
            option,
            text: catalog.resolve(language, &def.text_key.to_string()),
        })
        .collect()
}

/// 这一帧会话屏的标题键——**就是这个节点的 `text_key`**（NPC 说的那
/// 一句）。
///
/// 返回 `String` 而不是 `&str`：`text_key` 是一个 `NamespacedId`，而
/// `Catalog::resolve` 认的是 `"命名空间:路径"` 这个字符串形式（见
/// `ll_i18n::Catalog::resolve` 文档）。节点查不到时退回一句通用的
/// 占位文案键，与 [`dialogue_rows`] 的降级同一条理由。
pub fn dialogue_title_key(node: ContentIndex, nodes: &DialogueNodeTable) -> String {
    nodes
        .get(node)
        .map(|view| view.text_key.to_string())
        .unwrap_or_else(|| "screen-dialogue-missing".to_string())
}

/// 一场会话的两位当事人。
///
/// 把 `actor` 与 `speaker` 收成一个结构体，不是为了绕过
/// `clippy::too_many_arguments`（虽然它确实先叫起来了），而是因为这两个
/// `EntityId` 在 [`update_dialogue`] 的签名里紧挨着、类型相同、传反了
/// 编译器一个字都不会说——「玩家加入了说话人所属的据点」会变成「说话人
/// 加入了玩家所属的据点」，而玩家的 `home` 恒是 `None`，于是那条后果
/// 静默什么都不做。具名字段让传反这件事在调用点就看得见。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueParticipants {
    /// 发起者（玩家）——条件按他求值，后果也写在他身上。
    pub actor: EntityId,
    /// 说话人（NPC）——`join-settlement` 那一支读他的
    /// `ll_world::entity::Agent::home`。
    pub speaker: EntityId,
}

/// 会话屏这一帧的产出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogueUpdate {
    /// 处理完这一帧输入之后调用方该做什么。
    pub outcome: ScreenOutcome,
    /// 要切到哪一块屏，`None` 表示留在这块屏上（含只挪了光标那种）。
    pub next: Option<ScreenState>,
    /// 这一帧要提交的意图，`None` 表示什么都不提交。
    ///
    /// # 为什么它是一个独立字段，而不是 `ScreenOutcome` 的一个变体
    ///
    /// 「提交一条意图」与「这块屏接下来怎么办」是**两件正交的事**：一条
    /// 带 `outcomes` 的告别选项要**既**把后果提交出去、**又**关掉整块屏。
    /// 压进一个枚举就得为「提交并关闭」「提交并留下」各造一个变体，而
    /// 那两个变体的第二半与既有的 `Close`/`Idle` 是同一个东西。
    pub submit: Option<Intent>,
}

impl DialogueUpdate {
    /// 什么都不做的那一帧——绝大多数帧都是这一支。
    fn idle() -> Self {
        Self {
            outcome: ScreenOutcome::Idle,
            next: None,
            submit: None,
        }
    }

    /// 关掉整块屏，什么都不提交。
    fn close() -> Self {
        Self {
            outcome: ScreenOutcome::Close,
            next: None,
            submit: None,
        }
    }
}

/// 处理会话屏这一帧的输入。
///
/// 键盘与鼠标走**同一条动作分派**（`input.was_just_pressed(Confirm)
/// || pointer.activated()`），不为鼠标另写一套——与
/// `crate::save_list::update_save_list` 逐条同形，理由见
/// `crate::pointer` 模块文档。
///
/// # 选中一条选项之后发生什么，分两条路
///
/// - **带 `outcomes`** → 产出 `Intent::DialogueChoose`（经
///   [`DialogueUpdate::submit`] 送出去），**同时**按 `next` 换节点。
/// - **不带 `outcomes`**（纯导航）→ 只换节点，**不提交任何意图**
///   （规格 7.2：提交一个恒产出空效果的 `Intent` 只会污染 `Intent` 日志）。
///
/// 两条路的 `next` 处理完全一样：`End` 关掉整块屏，`Node` 换节点并把
/// 光标复位到第一项（**每换一个节点都重新预选第一项**，所有者裁定第 1
/// 条；不复位的话光标会停在一个新节点里毫不相干的行上）。
pub fn update_dialogue(
    node: ContentIndex,
    cursor: &mut usize,
    rows: &[DialogueRow],
    nodes: &DialogueNodeTable,
    who: DialogueParticipants,
    input: &InputState,
    pointer: RowPointer,
) -> DialogueUpdate {
    let DialogueParticipants { actor, speaker } = who;
    // 取消键**关掉整块屏**而不是退一层：会话屏只有一层，它的上一层就是
    // 世界（规格 N7 的「退一层」在这里就是「退出会话」）。
    if input.was_just_pressed(GameKey::Cancel) {
        return DialogueUpdate::close();
    }
    if rows.is_empty() {
        // 一条选项都显示不出来（死路节点，或者条件把每一条都挡住了）：
        // 上下键无处可动，确认键无行可选。留在屏上，玩家按取消退出。
        return DialogueUpdate::idle();
    }
    if input.was_just_pressed(GameKey::Down) {
        *cursor = (*cursor + 1) % rows.len();
    } else if input.was_just_pressed(GameKey::Up) {
        *cursor = (*cursor + rows.len() - 1) % rows.len();
    }
    // 指针按下把光标挪过去（约定一：**悬停**不改焦点，按下才改）。
    if let Some(row) = pointer.focus_row() {
        *cursor = row.min(rows.len() - 1);
    }
    if !(input.was_just_pressed(GameKey::Confirm) || pointer.activated()) {
        return DialogueUpdate::idle();
    }
    let Some(row) = rows.get(*cursor) else {
        return DialogueUpdate::idle();
    };
    // 选中这一条之后跳到哪、要不要提交意图，都从**内容表**里现查，
    // 不从行里带出来：行只带原始下标，形状见 [`DialogueRow`]。
    let Some(view) = nodes.get(node) else {
        return DialogueUpdate::close();
    };
    let Some(option) = view.options.get(row.option) else {
        return DialogueUpdate::close();
    };
    // **纯导航选项不提交任何意图**（规格 7.2）。
    let submit = (!option.outcomes.is_empty()).then_some(Intent::DialogueChoose {
        actor,
        speaker,
        node,
        option: row.option,
    });
    match option.next {
        // 「说完这句就散了」：屏关掉。带 `outcomes` 的告别选项**仍然会
        // 把那条后果提交出去**——两件事互不排斥，这正是 `submit` 与
        // `outcome` 分成两个字段的理由。
        DialogueNext::End => DialogueUpdate {
            outcome: ScreenOutcome::Close,
            next: None,
            submit,
        },
        DialogueNext::Node(target) => {
            *cursor = 0;
            DialogueUpdate {
                outcome: ScreenOutcome::Idle,
                next: Some(ScreenState::Dialogue {
                    speaker,
                    node: target,
                    cursor: 0,
                }),
                submit,
            }
        }
    }
}
