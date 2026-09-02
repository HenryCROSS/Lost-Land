//! `app::screen_flow`：八块模态屏的流程：吃输入、算转屏、产每行文本、把屏画出来。
//!
//! 本模块由 [`crate::app`] 按职责拆出（批次 16，纯搬移，没有改动任何逻辑）。
//! 拆分的依据不是行数而是「下一批要往哪里加东西」：对话批次要加一块屏、
//! UI 布局批次要改 HUD，两批原先撞在同一个文件的同两个函数上。主循环
//! （`impl AppHandler for Demo`）与 `Demo` 自身的状态仍然在 [`crate::app`]。

use ll_i18n::Catalog;
use ll_platform::config::{GameConfig, ScaleFilter};
use ll_platform::input::InputState;
use ll_render::target::BlitFilter;
use ll_render::wgpu;
use ll_ui::screen::render::render_screen;
use ll_ui::widget::state::WidgetStateTable;

use crate::content::LoadedContent;
use crate::menu_screen::{
    ScreenNotice, ScreenOutcome, ScreenState, SettingsContext, screen_data, settings_rows,
    update_settings,
};
use crate::pause_menu::{menu_focus_index, update_menu};
use crate::settings_view::{menu_row_texts, settings_row_texts, title_row_texts};
use crate::title_screen::{title_focus_index, update_title};

use super::Demo;
use super::gpu::GpuResources;

/// 一块屏这一帧的去向——`update_screen` 里那几条私有分支的共同返回形状。
///
/// 比裸元组 `(ScreenOutcome, Option<ScreenState>)` 多的只有名字，而这两
/// 项在调用点上恰恰读不出含义（哪个是「做什么」、哪个是「去哪儿」）。
pub(super) struct ScreenTransition {
    pub(super) outcome: ScreenOutcome,
    pub(super) next: Option<ScreenState>,
}

impl ScreenTransition {
    pub(super) fn idle() -> ScreenTransition {
        ScreenTransition {
            outcome: ScreenOutcome::Idle,
            next: None,
        }
    }

    pub(super) fn going(next: ScreenState) -> ScreenTransition {
        ScreenTransition {
            outcome: ScreenOutcome::Idle,
            next: Some(next),
        }
    }

    /// 屏整个关掉——玩家真的进世界了。
    pub(super) fn closed() -> ScreenTransition {
        ScreenTransition {
            outcome: ScreenOutcome::Close,
            next: None,
        }
    }
}

/// 这一帧这块模态屏的**每一行显示成什么字、光标停在第几行**。
///
/// # 为什么抽出来
///
/// 两个调用方要的是同一份东西：渲染侧（[`draw_screen`]）拿它排版，
/// 输入侧（[`Demo::resolve_screen_pointer`]）拿它算出行矩形、判断鼠标
/// 点在第几行。两处各算一遍就是两份同一个算法——**分叉时点击会静悄悄
/// 地落到隔壁那一行上**，而没有任何东西会报错。
///
/// 返回 `None` 表示这块屏不画那块居中面板（今天只有选出生地屏，它的
/// 「屏」是整张世界地图）。
#[allow(clippy::too_many_arguments)]
pub(super) fn screen_row_texts(
    state: ScreenState,
    config: &GameConfig,
    catalog: &Catalog,
    focus: &WidgetStateTable,
    has_save: bool,
    can_save_manually: bool,
    slots: &[crate::save_slot::SaveSlot],
    content: &LoadedContent,
    draft: Option<&crate::chargen::NewGameDraft>,
    // 会话屏要读世界（按玩家这一刻的状态过滤选项）。`None` = 底下没有
    // 世界，此时会话屏画不出任何行——与角色创建屏没有草稿时同一条降级。
    session: Option<(&ll_world::state::WorldState, ll_world::entity::EntityId)>,
) -> Option<ScreenRows> {
    let language = config.language.as_str();
    let (rows, cursor) = match state {
        ScreenState::Title => (
            title_row_texts(catalog, language, has_save),
            title_focus_index(focus),
        ),
        ScreenState::Menu => (
            menu_row_texts(catalog, language, can_save_manually),
            menu_focus_index(focus, can_save_manually),
        ),
        ScreenState::SaveList { cursor } => (
            crate::save_list::save_list_row_texts(slots, catalog, language),
            crate::save_list::clamp_cursor(cursor, slots),
        ),
        ScreenState::SaveNaming { .. } => (
            match draft {
                Some(draft) => {
                    crate::save_name::save_name_row_texts(&draft.save_name, catalog, language)
                }
                None => Vec::new(),
            },
            usize::MAX,
        ),
        ScreenState::CharacterCreation { cursor } => (
            match draft {
                Some(draft) => crate::chargen::character_row_texts(
                    &draft.choice,
                    &draft.roster,
                    content,
                    catalog,
                    language,
                ),
                // 没有草稿却停在这块屏上是不该发生的状态；画一块空面板
                // 比 panic 好，且下一帧 `update_screen` 会把玩家退回首页。
                None => Vec::new(),
            },
            cursor,
        ),
        ScreenState::WorldSetup { cursor } => (
            match draft {
                Some(draft) => crate::world_setup::world_setup_row_texts(
                    &draft.shape,
                    draft.preset,
                    draft.mode,
                    catalog,
                    language,
                ),
                None => Vec::new(),
            },
            cursor,
        ),
        ScreenState::Settings {
            cursor, capturing, ..
        } => {
            let rows = settings_rows();
            (
                settings_row_texts(&rows, config, catalog, capturing, cursor),
                cursor,
            )
        }
        // 会话屏：行是**过滤后**的选项，标题是 NPC 这一句（在下面
        // 单独算）。见 `crate::dialogue_screen` 模块文档。
        ScreenState::Dialogue { node, cursor, .. } => (
            match session {
                Some((world, player)) => match world.actors.get(player) {
                    Some(agent) => crate::dialogue_screen::dialogue_rows(
                        node,
                        &content.dialogue_node_table,
                        agent,
                        &content.registry,
                        catalog,
                        language,
                    )
                    .into_iter()
                    .map(|row| row.text)
                    .collect(),
                    None => Vec::new(),
                },
                None => Vec::new(),
            },
            cursor,
        ),
        // 交易屏：行是两边的货，各带价钱。价钱与 `resolve` 结算用的是
        // 同一个函数（`ll_sim::trade::trade_price`），见
        // `crate::trade_screen` 模块文档「判据只写一份」。
        ScreenState::Trade { partner, cursor } => (
            match session {
                Some((world, player)) => crate::trade_screen::trade_rows(
                    world,
                    player,
                    partner,
                    &content.item_table,
                    catalog,
                    language,
                )
                .into_iter()
                .map(|row| row.text)
                .collect(),
                None => Vec::new(),
            },
            cursor,
        ),
        // 选出生地屏**不画这块居中面板**：它的「屏」就是整张世界地图，
        // 一块盖在正中央的面板会挡住玩家要点的地方。它的画面全部由
        // `draw_hud` 那一侧产出（地图 + 光标标记 + 提示行），见
        // `crate::spawn_pick` 模块文档。
        ScreenState::SpawnPick { .. } => return None,
    };
    Some(ScreenRows {
        rows,
        cursor,
        title_key: screen_title_key(state, content),
    })
}

/// 这块屏的标题键。
///
/// 除会话屏外全部是写死的字面量，由 `crate::menu_screen::screen_data`
/// 自己认；会话屏的标题**是 NPC 说的那一句**，只有查了内容表才知道，
/// 见 `crate::dialogue_screen::dialogue_title_key`。
///
/// 与行文字出自**同一个产出点**（[`ScreenRows`]）：渲染侧与输入侧因此
/// 拿到的是同一份标题，面板宽度（`ll_ui::screen::panel_width` 要量全部
/// 行，标题也算一行）不可能在两侧算出两个值。
fn screen_title_key(state: ScreenState, content: &LoadedContent) -> String {
    match state {
        ScreenState::Dialogue { node, .. } => {
            crate::dialogue_screen::dialogue_title_key(node, &content.dialogue_node_table)
        }
        // 其余各屏的标题在 `screen_data` 里写死，这里给什么都不会被读到
        // ——给一个空串而不是随便挑一个键，是为了让「谁读了它」在调试时
        // 一眼可辨。
        _ => String::new(),
    }
}

/// [`screen_row_texts`] 的产出：这一帧这块屏的全部行、光标位置与标题键。
///
/// **三样必须同源**：渲染侧（[`draw_screen`]）与输入侧
/// （[`Demo::resolve_screen_pointer`]）各算一遍就是两份同一个算法，
/// 分叉时点击会静悄悄地落到隔壁那一行上。此前只有前两样，标题键是会话
/// 屏落地时加进来的第三样——它进面板宽度的计算，因此同样不能两侧各算。
pub(super) struct ScreenRows {
    /// 每一行显示成什么字。
    pub rows: Vec<String>,
    /// 光标停在第几行。
    pub cursor: usize,
    /// 标题的 Fluent 键，见 [`screen_title_key`]。
    pub title_key: String,
}

/// 把模态屏（菜单/设置）画到 `view` 上——**排在 [`draw_hud`](super::hud_draw::draw_hud) 之后**，
/// 因此那层压暗背板会把世界层与 HUD 一起压暗，见 `ll_ui::screen::render`
/// 模块文档。
///
/// `screen` 为 `None` 时整块不参与本次产出——不是「画出来但透明」，是
/// 压根不调用渲染函数，与 `draw_hud` 对世界地图/动作菜单的同一条纪律。
// 八个参数：全部是不同类型的具名值，调用点只有一处（`on_frame` 的渲染
// 段）。本批次**净减了四个**——行文字改由 [`screen_row_texts`] 在调用点
// 算好传进来（渲染侧与输入侧共用同一份），于是 `config`/`focus`/
// `has_save`/`can_save_manually`/`slots`/`content`/`draft` 七个换成了
// 一个 `rows_and_cursor` 加一个 `language`。收拢剩下这几个的正确形状是
// 把「屏 + 提示 + 悬停行」打包成一个类型，那是一次独立的重构。
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_screen(
    screen: Option<ScreenState>,
    // 行文字与光标位置由 [`screen_row_texts`] 现算——**渲染侧与输入侧
    // 共用同一份**，见那个函数的文档。`None` 表示这块屏不画居中面板。
    rows_and_cursor: Option<ScreenRows>,
    catalog: &Catalog,
    language: &str,
    notice: Option<ScreenNotice>,
    // 指针这一刻悬停在第几行——只用来画那块淡高亮，**不改焦点**，见
    // `crate::pointer` 模块文档约定一。
    hovered_row: Option<usize>,
    resources: &mut GpuResources,
    // 见 `Demo::measurer` 字段文档：输入侧与渲染侧共用同一个测量器。
    measure: &mut dyn ll_text::MeasureText,
    view: &wgpu::TextureView,
) {
    let Some(state) = screen else {
        return;
    };
    let Some(ScreenRows {
        rows,
        cursor,
        title_key,
    }) = rows_and_cursor
    else {
        return;
    };
    let notice_text = notice.map(|notice| notice.resolve(catalog, language));
    let mut data = screen_data(state, &rows, cursor, notice_text.as_deref(), &title_key);
    data.hovered = hovered_row;
    let size = resources.window_size;
    render_screen(
        &mut resources.quad_renderer,
        &mut resources.textured_quad_renderer,
        &mut resources.text_renderer,
        measure,
        resources.gpu.device(),
        resources.gpu.queue(),
        view,
        size.width,
        size.height,
        &data,
        catalog,
        language,
        &resources.skin,
    );
}

impl Demo {
    /// 打开游戏内菜单：压一层模态 UI 栈（这一步同时把输入上下文切到
    /// `InputContext::Menu`、把这一刻按住的键视为全部松开），并把屏
    /// 状态置成菜单、**焦点预置在第一项上**。
    ///
    /// # 「刻意不预置焦点」那条论证被推翻了，原文与推翻理由都记在这里
    ///
    /// 本方法此前写的是：焦点**刻意不预置**在任何一项上
    /// （`screen_focus` 保持全空），玩家第一次按方向键才出现焦点，与
    /// `ll_ui::widget::focus::move_focus` 文档「起点」一节的既有约定
    /// 一致。
    ///
    /// **项目所有者 2026-08-29 裁定改成预选第一项**（交接文档
    /// `knowledge/handoff/2026-08-28-session-handoff.md` 第〇之二节
    /// 第 1 条，规格 `knowledge/design/ui-and-navigation.md` N10）。
    /// 原论证**在它写下的那一刻是成立的**——不预选就不会误触，这在
    /// 一个鼠标也能用的界面里是对的。它作废是因为**前提变了**：那时候
    /// 这些屏只有键盘一条路，代价是新玩家进游戏第一眼看到首页、第一
    /// 反应按 Enter，**什么都不发生**（规格 I6：这条对新玩家的伤害最
    /// 直接）。而「误触」那一侧现在由鼠标那套约定自己管：指针要
    /// **按下**才移焦点，光把鼠标划过去什么都不会发生，见
    /// `crate::pointer` 模块文档。
    ///
    /// `move_focus` 文档那条「起点」约定**不动**：冷启动
    /// `Next`→0 / `Prev`→末位仍然照旧，本方法只是不再让表保持全空。
    /// 预置本身也复用它，不新造第二套「怎么算第一项」的逻辑。
    /// 进 `next` 这块屏时，焦点表该长什么样——规格 N10「所有列表一律
    /// 进入时第 0 行预选中」。
    ///
    /// # 两套选中模型，这里只管其中一套
    ///
    /// 九块屏分两类（规格 §4 I6 盘点过这两套并存）：
    ///
    /// - **焦点表模型**（首页、游戏内菜单）：选中状态住在
    ///   `Demo::screen_focus` 这张 [`WidgetStateTable`] 里。它是本方法
    ///   要回答的那一类，答案走 `crate::menu_screen::preselected_focus`
    ///   ——批次 15 就是用它给首页与菜单屏预置第一项的，**不新造第二套
    ///   「怎么算第一项」的逻辑**。
    /// - **光标模型**（角色创建 / 世界配置 / 存档列表 / 设置 / 选点 /
    ///   命名 / 会话屏）：选中状态是 `ScreenState` 自己带的那个
    ///   `cursor: usize`，一律以 `cursor: 0` 构造，**批次 21 的会话屏
    ///   就是这么做的**（`Dialogue { node, cursor: 0 }`）。它们不读
    ///   `screen_focus`，本方法对它们返回一张空表，把上一块屏留下的
    ///   焦点顺手清掉。
    fn preselected_focus_for(&self, next: ScreenState) -> WidgetStateTable {
        match next {
            ScreenState::Title => {
                crate::menu_screen::preselected_focus(&crate::title_screen::TITLE_ITEM_IDS)
            }
            ScreenState::Menu => crate::menu_screen::preselected_focus(
                &crate::pause_menu::menu_item_ids(self.can_save_manually()),
            ),
            ScreenState::CharacterCreation { .. }
            | ScreenState::WorldSetup { .. }
            | ScreenState::SpawnPick { .. }
            | ScreenState::SaveList { .. }
            | ScreenState::SaveNaming { .. }
            | ScreenState::Dialogue { .. }
            | ScreenState::Trade { .. }
            | ScreenState::Settings { .. } => WidgetStateTable::default(),
        }
    }

    pub(super) fn open_menu(&mut self, input: &mut InputState) {
        let ids = crate::pause_menu::menu_item_ids(self.can_save_manually());
        self.modal.set_screen(Some(ScreenState::Menu), input);
        self.screen_focus = crate::menu_screen::preselected_focus(&ids);
        self.screen_notice = None;
    }

    /// 关掉整块模态屏，回到游戏——屏那一层连同压在它上面的文本输入层
    /// 一起弹掉（同样清空按键状态：玩家在菜单里按着方向键就关掉菜单时，
    /// 角色不该立刻窜出去）。
    ///
    /// **只弹屏那一层，不再把整个栈弹空**：玩家菜单与世界地图现在也在
    /// 栈里（规格 N8），把栈清空等于把它们的状态与栈的配对当场打断。
    /// 今天没有任何路径能让屏与那两者同时开着（`on_frame` 里那道
    /// `!menu.is_open()` 的闸门挡着），但依赖「今天恰好没有」正是本
    /// 仓库反复付过代价的形状。
    pub(super) fn close_screen(&mut self, input: &mut InputState) {
        self.modal.set_screen(None, input);
        self.screen_notice = None;
    }

    /// 处理模态屏这一帧的输入。返回 `true` 表示玩家要退出整局。
    ///
    /// 拆成独立方法而不是塞进 [`Demo::on_frame`](ll_platform::window::AppHandler::on_frame)：`on_frame` 已经同时
    /// 承担着「退出判定 + 世界推进 + 三条渲染通道」，再往里塞一段
    /// 二十行的菜单路由会让它越过 50 行的函数上限。
    /// 这一帧指针对这块模态屏的行做了什么，见 [`crate::pointer`] 模块
    /// 文档那四条约定。
    ///
    /// # 三条降级，各有各的理由
    ///
    /// - 窗口还没建出来（[`Demo::viewport`] 为 `None`）→ 没有窗口坐标
    ///   可言，鼠标一律不生效。与 [`Demo::clicked_spawn_zone`] 那条既有
    ///   降级同一条。
    /// - 这块屏不画居中面板（选出生地屏）→ 它的鼠标交互是**在地图上
    ///   点一格**，早已由 `clicked_spawn_zone` 接上，不走行列表这一套。
    /// - 行矩形算出来是空的（列表为空，只显示一行占位文字）→ 那一行
    ///   不是按钮，点它没有可对应的动作。
    ///
    /// 三条都返回 [`crate::pointer::RowPointer::Idle`]，并且**照样推进
    /// 一次跨帧状态**（松开时解除武装），否则在这些屏上按下再切走会
    /// 留下一个永不清除的「已武装」。
    pub(super) fn resolve_screen_pointer(
        &mut self,
        state: ScreenState,
        input: &InputState,
    ) -> crate::pointer::RowPointer {
        let Some((width, height)) = self.viewport else {
            return crate::pointer::RowPointer::Idle;
        };
        let rects = {
            // 先把要整个 `&self` 的东西取完，再去借 `&mut self.measurer`
            // ——行矩形现在要一个文本测量器（行高按渲染行数算，见
            // `ll_ui::screen`），两处借用必须错开。
            let can_save = self.can_save_manually();
            let session = self
                .session
                .as_ref()
                .map(|session| (&session.game_world.world, session.game_world.player));
            let Some(ScreenRows {
                rows,
                cursor,
                title_key,
            }) = screen_row_texts(
                state,
                &self.config,
                &self.catalog,
                &self.screen_focus,
                !self.save_slots.is_empty(),
                can_save,
                &self.save_slots,
                &self.content,
                self.new_game_draft.as_ref(),
                session,
            )
            else {
                return crate::pointer::RowPointer::Idle;
            };
            let notice_text = self
                .screen_notice
                .map(|notice| notice.resolve(&self.catalog, &self.config.language));
            let data = screen_data(state, &rows, cursor, notice_text.as_deref(), &title_key);
            ll_ui::screen::screen_row_rects(
                &data,
                &self.catalog,
                &self.config.language,
                &mut self.measurer,
                width,
                height,
            )
        };
        crate::pointer::resolve_row_pointer(&mut self.pointer, input, &rects)
    }

    pub(super) fn update_screen(&mut self, input: &mut InputState) -> bool {
        let Some(state) = self.modal.screen() else {
            return false;
        };
        // 规格 N14：换屏时把**上一块屏留下**的那句提示清掉。记下进这个
        // 函数时的值，是为了分得出「旧的」与「本帧刚产生的」——下面
        // 六个分支都会在换屏之前置 `screen_notice`，无条件清会把刚说的
        // 那句话也吃掉。见换屏漏斗里那一段。
        let notice_before = self.screen_notice;
        // 这一帧鼠标对这块屏的行做了什么——**先算，再把它与键盘输入
        // 一起交给各屏的状态机**，两条路径因此走同一个动作分派分支。
        let pointer = self.resolve_screen_pointer(state, input);
        let (outcome, next_state) = match state {
            ScreenState::Title => {
                let update = update_title(
                    &mut self.screen_focus,
                    input,
                    pointer,
                    !self.save_slots.is_empty(),
                );
                if update.notice.is_some() {
                    self.screen_notice = update.notice;
                }
                (update.outcome, update.next)
            }
            ScreenState::Menu => {
                let can_save_manually = self.can_save_manually();
                update_menu(&mut self.screen_focus, input, pointer, can_save_manually)
            }
            ScreenState::CharacterCreation { cursor } => {
                let mut cursor = cursor;
                let update = self.update_character_creation(&mut cursor, input, pointer);
                if update.notice.is_some() {
                    self.screen_notice = update.notice;
                }
                let next = update
                    .next
                    .unwrap_or(ScreenState::CharacterCreation { cursor });
                (ScreenOutcome::Idle, Some(next))
            }
            ScreenState::WorldSetup { cursor } => {
                let mut cursor = cursor;
                let update = self.update_world_setup(&mut cursor, input, pointer);
                if update.notice.is_some() {
                    self.screen_notice = update.notice;
                }
                let next = update.next.unwrap_or(ScreenState::WorldSetup { cursor });
                (ScreenOutcome::Idle, Some(next))
            }
            ScreenState::SpawnPick { origin } => {
                // 死亡重生那条路直接从角色创建屏跳过来，选点屏要的地图
                // 视野还没建过——**只建视野，不碰世界**（世界早就存在）。
                self.prepare_spawn_pick_view();
                let update = self.update_spawn_pick(input, origin);
                if update.notice.is_some() {
                    self.screen_notice = update.notice;
                }
                if update.entered {
                    // 玩家确认了出生地、世界已经开始跑：整块屏关掉。
                    (ScreenOutcome::Close, None)
                } else {
                    (ScreenOutcome::Idle, update.next)
                }
            }
            ScreenState::SaveList { cursor } => {
                let mut cursor = crate::save_list::clamp_cursor(cursor, &self.save_slots);
                let update = crate::save_list::update_save_list(
                    &mut cursor,
                    &self.save_slots,
                    input,
                    pointer,
                );
                (
                    update.outcome,
                    Some(update.next.unwrap_or(ScreenState::SaveList { cursor })),
                )
            }
            ScreenState::SaveNaming { origin } => {
                let update = self.update_save_naming(input, origin);
                (update.outcome, update.next)
            }
            ScreenState::Dialogue {
                speaker,
                node,
                cursor,
            } => {
                let mut cursor = cursor;
                let outcome =
                    self.update_dialogue_screen(speaker, node, &mut cursor, input, pointer);
                (
                    outcome.0,
                    outcome.1.or(Some(ScreenState::Dialogue {
                        speaker,
                        node,
                        cursor,
                    })),
                )
            }
            ScreenState::Trade { partner, cursor } => {
                let mut cursor = cursor;
                let outcome = self.update_trade_screen(partner, &mut cursor, input, pointer);
                (
                    outcome.0,
                    outcome.1.or(Some(ScreenState::Trade { partner, cursor })),
                )
            }
            ScreenState::Settings { .. } => {
                let mut state = state;
                let mut ctx = SettingsContext {
                    config: &mut self.config,
                    config_path: &self.config_path,
                    catalog: &self.catalog,
                };
                let update = update_settings(&mut state, input, pointer, &mut ctx);
                if update.notice.is_some() {
                    self.screen_notice = update.notice;
                }
                // 把改好的键位表送回平台层的通道是 `take_rebound_keys`，
                // 见其文档。**只在真的改过的那些帧克隆整表**：
                // `SettingsUpdate::rebound` 由
                // `crate::menu_screen` 里那两处、也是全仓库仅有的两处
                // 改键位入口（重绑成功、退格解绑）置位。
                //
                // 这一行此前是无条件执行的，而旁边的注释却写着「不是
                // 每帧」——注释与代码直接矛盾，实机表现是设置屏一开就
                // 每帧克隆整表、每帧刷一行 `键位绑定表已由上层替换`，
                // 把一条为稀有事件准备的诊断日志烧成了噪音。
                if update.rebound {
                    self.pending_bindings = Some(self.config.bindings.clone());
                }
                let outcome = update.outcome;
                // 滤波方式当场生效（`blit_filter` 是一个普通字段）；
                // 垂直同步做不到，它只在 `GpuContext::new` 时决定呈现
                // 模式，屏上那一行因此带着「重启后生效」的提示。
                if let Some(resources) = self.resources.as_mut() {
                    resources.blit_filter = match self.config.display.scale_filter {
                        ScaleFilter::Nearest => BlitFilter::Nearest,
                        ScaleFilter::SharpBilinear => BlitFilter::SharpBilinear,
                    };
                }
                (outcome, Some(state))
            }
        };
        if let Some(next) = next_state {
            // 进存档列表屏那一刻刷新一次槽位——玩家可能刚在游戏里存过
            // 一次再回主菜单，缓存下来的那份列表已经旧了。只在**进入**
            // 时刷，不是每帧刷：每帧 `read_dir` 加逐份读头部是白付的开销。
            if matches!(next, ScreenState::SaveList { .. })
                && !matches!(state, ScreenState::SaveList { .. })
            {
                self.save_slots = crate::save_slot::list_slots(&self.saves_dir);
            }
            // **屏别真的变了**才做下面两件事。判据是枚举的判别式，不是
            // 整个 `ScreenState` 相等：设置屏的光标每动一格 `next != state`
            // 都成立，按整体相等判会退化成「每帧重置一次焦点、每帧清一次
            // 提示」。
            if std::mem::discriminant(&next) != std::mem::discriminant(&state) {
                // 规格 N10：进一块屏，第 0 行就该是选中的。**放在这个
                // 漏斗里而不是每个「去某某屏」的调用点各补一句**——漏斗
                // 是换屏唯一的必经之路，补在调用点上则是一笔每加一条路径
                // 就要多记一项、且漏了不报错的账。
                //
                // 这一条今天真的会退化：玩家死亡那条路把 `screen_focus`
                // 清空（`app.rs` 的 `handle_player_death`）并进角色创建屏，
                // 玩家在角色创建屏按 Esc 回首页时焦点表仍是空的，
                // `focus_index` 返回 `usize::MAX` ⇒ 按 Enter 什么都不发生，
                // 正是 N10 要消灭的那个症状。
                self.screen_focus = self.preselected_focus_for(next);
                // 规格 N14：只清**旧的**那一句。本帧刚产生的提示（例如
                // 「没有存档」）不动，见函数开头 `notice_before`。
                if self.screen_notice == notice_before {
                    self.screen_notice = None;
                }
            }
            // 换屏与「新屏要不要玩家打字」（今天只有命名屏）是同一步
            // 里做完的，见 `crate::modal::Modal::set_screen`。
            self.modal.set_screen(Some(next), input);
        }
        match outcome {
            ScreenOutcome::Idle => false,
            ScreenOutcome::Close => {
                self.close_screen(input);
                false
            }
            ScreenOutcome::Quit => true,
            ScreenOutcome::StartNewGame => {
                self.start_new_game(input);
                false
            }
            ScreenOutcome::SaveNow => {
                self.save_now();
                false
            }
            ScreenOutcome::BackToTitle => {
                self.back_to_title(input);
                false
            }
            ScreenOutcome::LoadSave => {
                self.load_saved_game(input);
                false
            }
        }
    }

    /// 首页的「开始游戏」：建一局全新的世界并进去。
    ///
    /// # 这里就是下一批（角色创建 / 世界配置 / 选重生点）的衔接点
    ///
    /// 本批次它直接 `crate::new_game(内容, 配置里的 new_game 段)`。
    /// 下一批要把这一句换成「先进一串新的 `ScreenState`」（种族/性别/
    /// 职业 → 世界配置 → 选重生点），走完之后再落到
    /// [`Session::begin`](crate::session::Session::begin)——那个函数是「世界准备好了，开始玩」这件事
    /// 唯一的入口，四条路径共用，见本批次计划文档第七节。
    pub(super) fn start_new_game(&mut self, input: &mut InputState) {
        tracing::info!("首页：开始新游戏，进入角色创建");
        // **本批次把这里从「直接建世界进游戏」换成了「先进一串屏」**
        // ——批次 6 计划文档第七节写明的那个衔接点。三块屏走完之后，
        // 终点仍然是 `Session::begin`（见 [`Demo::enter_world`]）。
        self.new_game_draft = Some(crate::chargen::NewGameDraft::new(
            &self.content,
            &self.config.new_game,
        ));
        self.modal
            .set_screen(Some(ScreenState::CharacterCreation { cursor: 0 }), input);
        self.screen_notice = None;
    }

    /// 角色创建屏这一帧。
    pub(super) fn update_character_creation(
        &mut self,
        cursor: &mut usize,
        input: &InputState,
        pointer: crate::pointer::RowPointer,
    ) -> crate::chargen::ChargenUpdate {
        let Some(draft) = self.new_game_draft.as_mut() else {
            // 停在这块屏上却没有草稿，是一种不该发生的状态。退回首页而
            // 不是 panic：一块屏的状态错乱不该拖垮整局游戏，与本模块
            // 其余降级路径同一条纪律。
            tracing::warn!("角色创建屏没有草稿，退回首页");
            return crate::chargen::ChargenUpdate::going(ScreenState::Title);
        };
        let roster = draft.roster.clone();
        // 「下一步」去哪块屏是**草稿手里那个世界的属性**，不是这块屏
        // 自己的判断，见 `crate::draft_world::DraftWorld` 模块文档。
        let next_screen = draft.world.screen_after_character_creation();
        crate::chargen::update_character_creation(
            cursor,
            &mut draft.choice,
            &roster,
            input,
            pointer,
            next_screen,
        )
    }

    /// 世界配置屏这一帧；按下「生成世界」时真的把世界建出来。
    pub(super) fn update_world_setup(
        &mut self,
        cursor: &mut usize,
        input: &InputState,
        pointer: crate::pointer::RowPointer,
    ) -> crate::chargen::ChargenUpdate {
        let Some(draft) = self.new_game_draft.as_mut() else {
            tracing::warn!("世界配置屏没有草稿，退回首页");
            return crate::chargen::ChargenUpdate::going(ScreenState::Title);
        };
        let mut preset = draft.preset;
        let mut mode = draft.mode;
        let update = crate::world_setup::update_world_setup(
            cursor,
            &mut draft.shape,
            &mut preset,
            &mut mode,
            input,
            pointer,
        );
        draft.preset = preset;
        draft.mode = mode;
        if update.next
            != Some(ScreenState::SpawnPick {
                origin: crate::spawn_pick::SpawnOrigin::WorldSetup,
            })
        {
            return update;
        }
        // 「生成世界」：真的建一局出来，随后进选出生地屏。**世界必须先
        // 存在**，否则没有地图可看（见 `crate::spawn_pick` 模块文档
        // 「顺序」一节）。
        self.generate_draft_world()
    }

    /// 按草稿的参数建一局世界，并把选出生地屏需要的东西准备好。
    ///
    /// 建不出来时（区块布局非法等，正常运行不该发生）**留在世界配置屏**
    /// 并记一条错误日志，不 panic——与首页读档失败留在首页同一条纪律。
    ///
    /// # 转生草稿在这里**根本拿不到可写的目标**（规格 N6）
    ///
    /// [`crate::draft_world::DraftWorld::generatable`] 在转生草稿上返回
    /// `None`：那条路上的世界与槽位绑在同一个类型里，而那个类型没有任何
    /// 替换世界的方法。这不是一句「顺手加的 if」——它是 D1 那条数据丢失
    /// 路径在类型层面的闸门，见该模块文档。
    pub(super) fn generate_draft_world(&mut self) -> crate::chargen::ChargenUpdate {
        let Some(draft) = self.new_game_draft.as_mut() else {
            return crate::chargen::ChargenUpdate::going(ScreenState::Title);
        };
        let params = draft.gen_params();
        let mode = draft.mode;
        let Some(_) = draft.world.generatable() else {
            tracing::error!(
                "转生草稿上不存在「生成世界」这条路径：世界与槽位绑在一起，                 重新生成会把玩家那一局抹掉。拒绝生成，留在原地"
            );
            return crate::chargen::ChargenUpdate::idle();
        };
        // 模式在建世界那一刻绑进世界身份，此后只被搬运——存档路径上
        // 没有第二个来源，见 `ll_content::world_identity` 模块文档。
        let world = match crate::world::build_new_world_with_mode(&self.content, params, mode) {
            Ok(world) => world,
            Err(error) => {
                tracing::error!(?error, "按玩家选的参数建世界失败，留在世界配置屏");
                return crate::chargen::ChargenUpdate::idle();
            }
        };
        // 玩家选的三项**不在这里**落到实体上，而在
        // [`Demo::finish_entering_world`]——与死亡重生那条路共用同一处。
        // 放在这里会让新游戏那条路应用两次（这里一次、进世界时再一次），
        // 而重生那条路根本不经过本函数。
        draft
            .world
            .generatable()
            .expect("本函数开头那道闸门已经确认这是新游戏那条路")
            .put(world);
        self.prepare_spawn_pick_view();
        crate::chargen::ChargenUpdate::going(ScreenState::SpawnPick {
            origin: crate::spawn_pick::SpawnOrigin::WorldSetup,
        })
    }

    /// 把选出生地屏要的地图视野准备好——**只读草稿里那个世界，一个字节
    /// 都不改它**。
    ///
    /// 幂等：已经准备好就直接返回。选出生地屏每帧都会调它一次（死亡重生
    /// 那条路从角色创建屏直接跳过来，没有经过「生成世界」那一步），每帧
    /// 重算一次粗粒度地形场是几千个区块的白工。
    ///
    /// # 为什么从 `generate_draft_world` 里拆出来
    ///
    /// 进选出生地屏有两条路：新游戏（先生成世界，再准备视野）与死亡重生
    /// （世界早就在，只准备视野）。两条路共用这一份，就不会出现「重生那
    /// 条路顺手又把世界重新生成了一遍」——那会把这局玩过的一切抹掉。
    pub(super) fn prepare_spawn_pick_view(&mut self) {
        let Some(draft) = self.new_game_draft.as_mut() else {
            return;
        };
        if draft.continent_field.is_some() && draft.map_view.is_some() {
            return;
        }
        let Some(world) = draft.world.world() else {
            return;
        };
        let layout = *world.world.terrain.layout();
        let field = ll_world::overview::generate_continent_field(
            &layout,
            &world.noise,
            &world.params,
            &self.content.terrain_ids,
        );
        // 光标初值对准玩家现在（或默认会）站的那一格。死亡重生时玩家实体
        // 已经不在了，退回世界原点——那只是光标的落脚点，玩家马上会自己
        // 挑一个。
        let player_pos = world
            .world
            .actors
            .get(world.player)
            .map(|agent| agent.pos)
            .unwrap_or_else(|| layout.tile_size().wrap(0, 0));
        let view = ll_world::world_map::WorldMapView::centered_on_tile(&field, player_pos);
        // 全图可见：一份「全部已探索」的记忆，**只活在草稿里**，绝不
        // 写进 `WorldState`，见 `ExplorationMemory::fully_explored` 文档。
        let exploration = ll_world::exploration::ExplorationMemory::fully_explored(&layout);
        let slice = ll_world::world_map::world_map_slice(&field, &layout, &exploration, &view);
        draft.cursor_cell = slice.cell_of_tile(player_pos).unwrap_or((0, 0));
        draft.exploration = Some(exploration);
        draft.continent_field = Some(field);
        draft.map_view = Some(view);
    }

    /// 选出生地屏这一帧。
    pub(super) fn update_spawn_pick(
        &mut self,
        input: &mut InputState,
        origin: crate::spawn_pick::SpawnOrigin,
    ) -> crate::spawn_pick::SpawnPickUpdate {
        let Some(slice) = self.spawn_pick_slice() else {
            // **降级也不许把玩家扔到世界配置屏上**（规格 N6 后半句）：
            // 那块屏在转生流程里按 `crate::chargen` 的论证必须跳过，而
            // 一条降级路径把玩家送过去，与 D1 的后果一模一样。角色创建
            // 屏是两条路都回得去、且什么都抹不掉的那一块。
            tracing::warn!("选出生地屏没有世界，退回角色创建屏");
            return crate::spawn_pick::SpawnPickUpdate::going(ScreenState::CharacterCreation {
                cursor: 0,
            });
        };
        let clicked = self.clicked_spawn_zone(&slice, input);
        let Some(draft) = self.new_game_draft.as_mut() else {
            return crate::spawn_pick::SpawnPickUpdate::going(ScreenState::Title);
        };
        let layout = *draft
            .world
            .world()
            .expect("spawn_pick_slice 已经确认世界存在")
            .world
            .terrain
            .layout();
        let decision = crate::spawn_pick::update_spawn_pick(
            &mut draft.cursor_cell,
            &slice,
            &layout,
            input,
            clicked,
            origin,
        );
        let Some(zone) = decision.confirmed else {
            return decision.update;
        };
        let world = draft.world.world().expect("上面已经确认世界存在");
        // 玩家确认了一个区块：在那个区块内确定性地挑一格陆地。
        let picked = crate::spawn_pick::pick_spawn_in_zone(
            &layout,
            &world.noise,
            &world.params,
            &self.content.terrain_ids,
            &self.content.terrain_table,
            zone,
        );
        let Some(pos) = picked else {
            // 退化策略：**提示玩家重选，不自动换邻近区块**，理由见
            // `crate::spawn_pick::pick_spawn_in_zone` 文档「退化策略」一节。
            return crate::spawn_pick::SpawnPickUpdate::saying(ScreenNotice::NoLandInZone);
        };
        self.enter_world_at(pos, input, origin)
    }

    /// 选出生地屏这一帧的地图切片；世界还没建出来时返回 `None`。
    ///
    /// **传的是草稿里那份「全部已探索」的记忆**，`explored` 因此恒为真，
    /// 同一份呈现代码自然变成全图可见——没有 `reveal_all` 标志，见
    /// `ll_world::exploration::ExplorationMemory::fully_explored` 文档。
    pub(super) fn spawn_pick_slice(&self) -> Option<ll_world::world_map::WorldMapSlice> {
        let draft = self.new_game_draft.as_ref()?;
        let world = draft.world.world()?;
        Some(ll_world::world_map::world_map_slice(
            draft.continent_field.as_ref()?,
            world.world.terrain.layout(),
            draft.exploration.as_ref()?,
            draft.map_view.as_ref()?,
        ))
    }

    /// 玩家这一帧有没有用鼠标点中某个区块。
    ///
    /// 换算走 `ll_ui::hud::world_map::world_map_zone_at_pixel`（上上批
    /// 备好的那个函数，带 4 条属性测试）。它要面板外框矩形与皮肤——
    /// **两样都传与画图时逐字相同的那一份**，由它自己按边框粗细内缩，
    /// 因此玩家点的地方与选中的区块不可能系统性偏移一个边框宽度。
    pub(super) fn clicked_spawn_zone(
        &self,
        slice: &ll_world::world_map::WorldMapSlice,
        input: &InputState,
    ) -> Option<ll_world::space::ZoneCoord> {
        if !input.was_mouse_just_pressed(ll_platform::input::MouseButton::Left) {
            return None;
        }
        let resources = self.resources.as_ref()?;
        let pixel = input.cursor_position()?;
        let draft = self.new_game_draft.as_ref()?;
        let layout = *draft.world.world()?.world.terrain.layout();
        let rect = ll_ui::hud::render::world_map_rect(
            resources.window_size.width as f32,
            resources.window_size.height as f32,
        );
        ll_ui::hud::world_map::world_map_zone_at_pixel(slice, &layout, rect, &resources.skin, pixel)
    }

    /// 把玩家挪到选中的那一格，然后真正进世界。
    ///
    /// `origin` 只是**过路**：命名屏自己没有第二个入口，它带着这个来处
    /// 是为了在玩家从那里按取消退回选点屏时，把当初进选点屏的那一块屏
    /// 原样交还（规格 N5 判据 3）。
    ///
    /// # 死亡重生那条路不问名字
    ///
    /// 那个世界早就有自己的槽位，再起一个名字会让同一个世界在列表里出现
    /// 两份，而玩家只是换了个角色（`crate::save_slot` 模块文档「一份
    /// 存档 = 一个世界」）。**短路点在 [`Demo::update_save_naming`]**
    /// ——此处此前有一段按 `existing_target` 分叉的代码，两个分支的返回值
    /// 逐字相同，读起来像「转生走了另一条道」而实际没有，已删。
    pub(super) fn enter_world_at(
        &mut self,
        pos: ll_core::torus::TorusPos,
        _input: &mut InputState,
        origin: crate::spawn_pick::SpawnOrigin,
    ) -> crate::spawn_pick::SpawnPickUpdate {
        let Some(draft) = self.new_game_draft.as_mut() else {
            return crate::spawn_pick::SpawnPickUpdate::going(ScreenState::Title);
        };
        draft.spawn = Some(pos);
        crate::spawn_pick::SpawnPickUpdate::going(ScreenState::SaveNaming { origin })
    }

    /// 命名屏这一帧；玩家按确认时真正进世界。
    pub(super) fn update_save_naming(
        &mut self,
        input: &mut InputState,
        origin: crate::spawn_pick::SpawnOrigin,
    ) -> ScreenTransition {
        let Some(draft) = self.new_game_draft.as_mut() else {
            tracing::warn!("命名屏没有草稿，退回首页");
            return ScreenTransition::going(ScreenState::Title);
        };
        // 死亡重生那条路没有名字可打——世界已经有槽位了，直接进去。
        if draft.world.existing_target().is_some() {
            return self.finish_entering_world(input);
        }
        let update = crate::save_name::update_save_name(&mut draft.save_name, input, origin);
        if let Some(next) = update.next {
            return ScreenTransition::going(next);
        }
        if !update.confirmed {
            return ScreenTransition::idle();
        }
        self.finish_entering_world(input)
    }

    /// 把玩家挪到选好的那一格、开出（或沿用）槽位，然后真正进世界。
    ///
    /// 这是新游戏与死亡重生**共用**的终点：两条路的差别只有「槽位是新
    /// 开的还是沿用的」，其余（挪玩家、`Session::begin`、关屏）逐字相同。
    pub(super) fn finish_entering_world(&mut self, input: &mut InputState) -> ScreenTransition {
        let Some(draft) = self.new_game_draft.take() else {
            return ScreenTransition::going(ScreenState::Title);
        };
        // 世界与（可能有的）槽位一起从草稿里拆出来——它们本来就是一件
        // 事的两面，见 `crate::draft_world::DraftWorld::into_parts`。
        let (drafted_world, existing_target) = draft.world.into_parts();
        let (Some(mut world), Some(pos)) = (drafted_world, draft.spawn) else {
            tracing::warn!("要进世界了却没有世界或没有出生地，退回首页");
            return ScreenTransition::going(ScreenState::Title);
        };
        // 玩家实体还在不在，决定这里是「挪」还是「造」。
        //
        // - 新游戏：`build_new_world` 已经造好了一个玩家，选出生地只是把他
        //   挪过去 —— 批次 8 接缝 3 `move_player_to`。
        // - 死亡重生：实体在死亡那一刻就被 `Despawn` 摘掉了，这里必须造一个
        //   新的 —— 批次 8 接缝 2 `build_player_agent`（经
        //   `respawn_player` 装配）。
        //
        // 两条路都不抄第三份逻辑。
        if world.world.actors.get(world.player).is_some() {
            crate::world::move_player_to(&mut world, pos);
            // 批次 8 接缝之一：玩家选的三项落到那个真实实体上。
            crate::world::apply_character_choice(
                &mut world,
                &self.content,
                draft.choice.race(&draft.roster),
                draft.choice.profession(&draft.roster),
                draft.choice.gender(),
            );
        } else {
            let race = draft
                .choice
                .race(&draft.roster)
                .unwrap_or(self.content.race_ids.human);
            let profession = draft
                .choice
                .profession(&draft.roster)
                .unwrap_or(self.content.class_ids.warrior);
            crate::world::respawn_player(
                &mut world,
                &self.content,
                pos,
                race,
                profession,
                draft.choice.gender(),
            );
        }
        let target = match existing_target {
            Some(target) => target,
            None => {
                let name = crate::save_name::resolved_name(
                    &draft.save_name,
                    &self.catalog,
                    &self.config.language,
                );
                crate::save_slot::SaveTarget::create_in(
                    &self.saves_dir,
                    &name,
                    crate::save::now_unix_seconds(),
                )
            }
        };
        tracing::info!(
            spawn_x = pos.x(),
            spawn_y = pos.y(),
            slot = target.id.as_str(),
            name = %target.name,
            "玩家选定出生地，进入世界"
        );
        // 终点仍然是 `Session::begin`——进世界的每一条路径共用它，见
        // `crate::session::Session::begin` 文档。
        self.enter_world_in_slot(world, target, input);
        ScreenTransition::closed()
    }
}
