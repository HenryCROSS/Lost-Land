//! `app::save_flow`：三条存档路（自动、手动、退出）与读档，以及存档槽的选中与进入。
//!
//! 本模块由 [`crate::app`] 按职责拆出（批次 16，纯搬移，没有改动任何逻辑）。
//! 拆分的依据不是行数而是「下一批要往哪里加东西」：对话批次要加一块屏、
//! UI 布局批次要改 HUD，两批原先撞在同一个文件的同两个函数上。主循环
//! （`impl AppHandler for Demo`）与 `Demo` 自身的状态仍然在 [`crate::app`]。

use ll_platform::input::InputState;

use crate::content::LoadedContent;
use crate::menu_screen::{ScreenNotice, ScreenState};
use crate::save::save_game;
use crate::session::Session;

use super::{AUTOSAVE_INTERVAL_TICKS, Demo};

/// 把一局游戏写到**它自己的槽位**——手动存档、自动存档、退出存档三条
/// 路共用的唯一一处。
///
/// # 为什么必须只有一处
///
/// 三条路都要回答同样几个问题：写到哪个文件、写什么名字、目录不存在
/// 怎么办。各写一份的话，「自动存档写对了目录但手动存档没建目录」这
/// 类缺陷会以「偶尔存不上」的形态出现，而玩家看到的只是进度消失。
///
/// 存档模式**不是参数**：它住在世界身份里，见
/// [`crate::save::save_game`] 文档「存档模式也不再是参数」一节。
pub(super) fn write_save(
    content: &LoadedContent,
    session: &Session,
    character_name: &str,
) -> Result<(), ll_content::save_file::SaveError> {
    if let Some(dir) = session.save_target.path.parent()
        && let Err(error) = std::fs::create_dir_all(dir)
    {
        return Err(ll_content::save_file::SaveError::Io(error));
    }
    save_game(
        &session.save_target.path,
        content,
        &session.game_world,
        character_name,
        // 当前所在区域的人类可读名字——今天世界里还没有「区域」这个
        // 概念的生产者（据点有名字，旷野没有），先如实写一个通名。
        "旷野",
        &session.save_target.name,
    )
}

impl Demo {
    /// 世界时钟走满一个自动存档周期就存一次。
    ///
    /// # 为什么必须按世界时间，不能按墙钟
    ///
    /// 墙钟会让存档时机取决于**玩家盯着屏幕想了多久**：同一串输入在两
    /// 台机器上、甚至同一台机器的两次运行里，会在不同的世界状态上触发
    /// 存档。那正是约束 C4 禁止的那类隐藏输入——世界的演化不该是「真实
    /// 时间过了多久」的函数。
    ///
    /// 世界时钟只由回合推进驱动（`ll_sim::turn::TurnEngine`），它是玩家
    /// 输入的纯函数，因此同一串输入的存档时机逐次相同。
    ///
    /// # 间隔为什么是游戏内一小时
    ///
    /// [`AUTOSAVE_INTERVAL_TICKS`] 直接取既有常量 `TICKS_PER_HOUR`，不
    /// 新造一个魔数。游戏内一小时对应几十到上百个回合：既不会频繁到每
    /// 走几步就卡一次盘，也不会久到死一次要退回很远。
    ///
    /// # 写盘失败只记一条日志
    ///
    /// 自动存档是背景动作，玩家没有在等它。弹一句提示会在他正走路时突然
    /// 盖住屏幕；而**下一次自动存档还会再试一遍**，一次失败不是终局。
    /// 真正要紧的那次（退出、回主菜单、手动存档）都会各自报错。
    pub(super) fn maybe_autosave(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let now = session.game_world.world.clock;
        if now.0.saturating_sub(session.last_autosave.0) < AUTOSAVE_INTERVAL_TICKS {
            return;
        }
        match write_save(&self.content, session, &self.character_name) {
            Ok(()) => tracing::info!(
                path = %session.save_target.path.display(),
                world_tick = now.0,
                "自动存档完成"
            ),
            Err(error) => tracing::error!(%error, "自动存档失败，下一次周期会再试"),
        }
        // 无论成败都把节拍往前推：失败时不推的话，下一帧会立刻再试一次，
        // 磁盘满/无权限这类持续性故障会变成每帧一次写盘 + 每帧一行日志
        // ——又一次刷屏。
        if let Some(session) = self.session.as_mut() {
            session.last_autosave = now;
        }
    }

    /// 存档列表屏选中的那一份：读回来并进去。
    ///
    /// # 读不回来时**留在首页**，不悄悄开一局新游戏
    ///
    /// 启动期的 `crate::load_or_new_game` 在读档失败时回退到新游戏，
    /// 那条语义在**那里**是对的：玩家已经决定要玩了，给他一个能玩的
    /// 世界总比什么都没有强。首页这条不同——玩家明确点的是「读取
    /// 存档」，给他一个新世界是答非所问。
    pub(super) fn load_saved_game(&mut self, input: &mut InputState) {
        let Some(slot) = self.selected_slot() else {
            tracing::warn!("读取存档：一份存档都没有，留在原地");
            self.screen_notice = Some(ScreenNotice::NoSave);
            return;
        };
        let path = slot.path.clone();
        let target = crate::save_slot::SaveTarget::existing(&slot);
        tracing::info!(path = %path.display(), name = %target.name, "读取存档");
        let Some(world) = crate::load_saved_game(&path, &self.content) else {
            tracing::warn!(path = %path.display(), "读档失败，留在原地");
            self.screen_notice = Some(ScreenNotice::LoadFailed);
            return;
        };
        // 读回来之后继续往**同一个槽位**写：手动存档、自动存档、退出
        // 存档三条路从此都对着这一份，不会凭空多出一份新档。
        self.enter_world_in_slot(world, target, input);
    }

    /// 当前这一局允许手动存档吗——**判据只有这一处**，走
    /// [`ll_content::world_identity::WorldIdentity::allows_manual_save`]。
    ///
    /// 没有世界时返回假：首页底下没有暂停菜单，这个问题在那儿没有意义。
    pub(super) fn can_save_manually(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.game_world.identity.allows_manual_save())
    }

    /// 暂停菜单的「保存」：把当前进度写回这一局自己的槽位。
    ///
    /// 存完**留在菜单里**（见 [`ScreenOutcome::SaveNow`](crate::menu_screen::ScreenOutcome::SaveNow) 文档），只把
    /// 结果说一句。
    pub(super) fn save_now(&mut self) {
        let Some(session) = self.session.as_ref() else {
            // 首页底下没有世界，这一行根本不该出现在那儿。
            tracing::warn!("没有进行中的世界，手动存档跳过");
            return;
        };
        match write_save(&self.content, session, &self.character_name) {
            Ok(()) => {
                tracing::info!(path = %session.save_target.path.display(), "手动存档完成");
                self.screen_notice = Some(ScreenNotice::GameSaved);
                // 列表随之变旧了——玩家回主菜单时要看到新的时间戳。
                self.save_slots = crate::save_slot::list_slots(&self.saves_dir);
            }
            Err(error) => {
                tracing::error!(%error, "手动存档失败");
                self.screen_notice = Some(ScreenNotice::GameSaveFailed);
            }
        }
    }

    /// 暂停菜单的「返回主菜单」：**先存一次，存成功才回去**。
    ///
    /// # 未保存的进度怎么办（本批次的裁定，所有者没有裁定过）
    ///
    /// 选定「回去之前自动存一次」。理由按重要性排：
    ///
    /// 1. **绝不静默丢弃玩家进度。**
    /// 2. **它与「退出游戏」是同一条规则，不是第二条。** `on_exit` 本来
    ///    就无条件存一次（[`Demo::save_on_exit`]）。让回主菜单也存，玩家
    ///    只需要记住一件事：**离开世界就会存**。两条路径给出两种结果才是
    ///    真正会咬人的形状。
    /// 3. **肉鸽模式下这是唯一正确的做法。** 肉鸽没有手动存档入口，回去
    ///    不存的话，玩家从上一次自动存档到现在的进度就凭空消失了——而
    ///    肉鸽的约束是「后果不可撤销」，不是「进度可以蒸发」。
    /// 4. **「弹一个确认框」是一块本仓库今天没有的 UI。**
    ///    `ll_ui::screen::ScreenData` 是「标题 + 若干行 + 一句提示」的
    ///    居中面板，没有「是/否」模态的概念，造一个是独立的一批。
    /// 5. **最容易反转。** 将来所有者要「问一句再回」，改动是在这条路径
    ///    前面插一块屏，存档那一句原样保留。
    ///
    /// # 写盘失败时**留在暂停菜单**
    ///
    /// 这一条比「回去了但没存上」重要得多：玩家至少还站在世界里，可以
    /// 再按一次，或者先去解决磁盘满/权限的问题。回去了才发现没存上，
    /// 那份进度就真的没了。
    pub(super) fn back_to_title(&mut self, input: &mut InputState) {
        let Some(session) = self.session.as_ref() else {
            // 已经在首页了，什么都不用做。
            return;
        };
        if let Err(error) = write_save(&self.content, session, &self.character_name) {
            tracing::error!(%error, "回主菜单前存档失败，留在暂停菜单，不丢弃进度");
            self.screen_notice = Some(ScreenNotice::GameSaveFailed);
            return;
        }
        tracing::info!(
            path = %session.save_target.path.display(),
            "回主菜单前已存档"
        );
        // 世界从这一刻起不再存在——`session` 为 `None` 就是「停在首页」
        // 这个状态的唯一表示（见 `Demo::session` 字段文档）。
        self.session = None;
        self.new_game_draft = None;
        self.save_slots = crate::save_slot::list_slots(&self.saves_dir);
        // 屏换成首页，模态栈保持压着一层——首页也是一块模态屏，输入
        // 上下文仍然是 `Menu`。`close_screen` 会把栈弹空，那是「回到
        // 游戏」用的，这里不能用。
        self.modal.set_screen(Some(ScreenState::Title), input);
        self.screen_notice = None;
        // 首页的第一项预先选中（规格 N10），见 [`Demo::open_menu`]。
        self.screen_focus =
            crate::menu_screen::preselected_focus(&crate::title_screen::TITLE_ITEM_IDS);
        // 按住的键视为全部松开：玩家在菜单里按着方向键回主菜单，光标不
        // 该立刻在首页上窜出去（与 `close_screen` 同一条纪律）。
        input.clear();
    }

    /// 存档列表屏当前光标落在哪一份上。
    ///
    /// # 读的是列表屏的光标，不是「最近那一份」
    ///
    /// 此前这里恒返回 `save_slots.first()`，于是存档列表屏的光标是一个
    /// **装饰品**：玩家把它移到第三份、按确认，进的是第一份，而
    /// [`Demo::enter_world_in_slot`] 之后每一次存档都写进那个错的槽位
    /// （`knowledge/design/ui-and-navigation.md` 2.2 节 D5）。
    ///
    /// `ScreenOutcome::LoadSave` 的**唯一生产者**是
    /// `crate::save_list::update_save_list`，而 [`Demo::update_screen`] 的
    /// 屏切换漏斗在分派 `outcome` **之前**就把本帧的新光标写回了
    /// `self.screen`——所以这里读到的就是玩家这一刻看到的那一行。
    ///
    /// 不在列表屏时返回 `None`：那是一种不该发生的状态，调用方走既有的
    /// 「留在原地 + 提示」降级路径，不 panic。
    pub(super) fn selected_slot(&self) -> Option<crate::save_slot::SaveSlot> {
        let Some(ScreenState::SaveList { cursor }) = self.modal.screen() else {
            tracing::warn!("不在存档列表屏却要读「选中的那一份」，不猜一份给他");
            return None;
        };
        // 夹一次而不是直接索引：玩家离开这块屏期间列表可能变短。判据只有
        // `crate::save_list::clamp_cursor` 这一处，不写第二份。
        let cursor = crate::save_list::clamp_cursor(cursor, &self.save_slots);
        self.save_slots.get(cursor).cloned()
    }

    /// 真正进世界：建出这一局的运行期状态，并把首页那一层从模态栈里
    /// 弹掉（输入上下文随之回到 `Gameplay`，按住的键视为全部松开）。
    pub(super) fn enter_world_in_slot(
        &mut self,
        world: crate::world::GameWorld,
        target: crate::save_slot::SaveTarget,
        input: &mut InputState,
    ) {
        self.session = Some(Session::begin(world, &self.content, target));
        self.close_screen(input);
    }

    /// 退出前存档——`on_exit` 恰好调用一次（`ll_platform::window`
    /// 文档保证），是「游玩 → 存档 → 退出」这条闭环里存档动作唯一的
    /// 触发点。
    pub(super) fn save_on_exit(&self) {
        let Some(session) = self.session.as_ref() else {
            // 从首页直接离开：这一局从来没有开始过，没有任何世界状态
            // 可存。
            //
            // **这一条不只是防 panic，是防数据丢失**：照旧无条件存档
            // 会把一个「玩家从未玩过」的空世界写进存档目录——启动、进
            // 首页、直接离开，就凭空多出一份垃圾档。
            tracing::info!("从首页退出，没有进行中的世界，跳过退出存档");
            return;
        };
        if let Err(error) = write_save(&self.content, session, &self.character_name) {
            tracing::error!(%error, "退出前存档失败");
        } else {
            tracing::info!(path = %session.save_target.path.display(), "退出前存档完成");
        }
    }
}
