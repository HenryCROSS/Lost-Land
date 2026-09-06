//! 自动存档留下的那条痕迹：什么时候出现、什么时候自己消失（规格
//! §9.2 F3）。
//!
//! # 这条痕迹要解决的问题
//!
//! `crate::app::Demo::maybe_autosave` 写盘成功只记一行 `tracing::info!`
//! ——玩家在屏幕上**一个像素都看不到**，于是他不知道游戏在替他存档，
//! 会反复去暂停菜单里手动存。失败那一侧仍然静默（那条论证是对的，见
//! `maybe_autosave` 文档「写盘失败只记一条日志」），本模块只管成功。
//!
//! # 两条时钟，各管各的——不要混
//!
//! | 问题 | 谁回答 | 时钟 |
//! |---|---|---|
//! | 这一刻**要不要存** | `maybe_autosave` | **世界时钟**（`ll_core::time`），约束 C4 禁止用墙钟决定世界的演化 |
//! | 存过的痕迹**还要显示多久** | 本模块 | **帧计数**（`ll_platform::window::FrameId`） |
//!
//! 第二条纯属表现层：它不进世界状态、不进存档主体、不影响任何一次结算，
//! 因此走帧计数是对的（`ll_ui::widget::anim` 模块文档「时钟源：帧计数，
//! 不是墙钟时间」一节论证过这一族，帧计数属 ADR 0020 的甲区）。
//! **两条时钟不能互相借用**：拿世界时钟去算「显示多久」会让玩家站着不动
//! 时痕迹永远不消失；拿帧计数去决定「要不要存」正是 C4 点名的那类隐藏
//! 输入。
//!
//! # 它由时钟驱动，不是由「又按了几下键」驱动
//!
//! ADR 0025 禁止用合成按键做验收，本模块因此把判定抽成一个**纯函数**
//! [`autosave_notice_visible`]：喂给它「存档成功那一帧的帧号」与「现在
//! 第几帧」，它就回答该不该显示。测试推帧号即可，一次 `press` 都不需要
//! ——与批次 33 的 `crate::nav_row` 那条连发断言同一种形状。
//!
//! # 「不打扰人」落在哪四件事上
//!
//! 1. **不抢焦点**：本模块不碰 `crate::modal::Modal`、不碰焦点表、不改
//!    输入上下文。
//! 2. **不消耗回合**：痕迹只是 `Demo` 上一个 `Option<FrameTick>`，不进
//!    `crate::world::GameWorld`。
//! 3. **不挡住玩家正在看的东西**：画在屏幕最下沿那条底栏里（见
//!    `ll_ui::hud::bottom_rows`），屏幕中段——玩家看世界的地方——一个
//!    像素都不占。
//! 4. **有模态屏盖着时看不见**：它在 `UiLayer::Notice`，模态屏在
//!    `UiLayer::Modal`（规格 N9），压暗背板排在它之后提交。这一条不需要
//!    再写一句 `if`，是分层白送的。

use ll_ui::widget::anim::{DEFAULT_ANIM_DURATION_FRAMES, FrameTick};

/// 痕迹在屏上停留多少帧。
///
/// # 为什么是这个数
///
/// 规格 §9.2 F3 的**裁定句**写的是「两秒后自渐隐」，**判据句**写的却是
/// 「`AFTERGLOW_DURATION_FRAMES` 帧之后归空」——而那个常量就是
/// `DEFAULT_ANIM_DURATION_FRAMES * 3` = 60 帧，本项目主循环 60 帧每秒，
/// 也就是**一秒**。两句自相矛盾，本批取判据那一句（它是可执行的那一句），
/// 并在批次 35 的计划文档里记了这处矛盾。
///
/// 写成同一条派生式而不是抄一个 60：`AFTERGLOW_DURATION_FRAMES` 是
/// `ll_ui::hud::render` 的**私有**常量（它说的是双层血条余晖层的过渡
/// 时长，与本模块不是同一件事，不该为了共用一个数把它公开）。派生式相同
/// 保证两者永远是同一档手感刻度。
pub const AUTOSAVE_NOTICE_FRAMES: u32 = DEFAULT_ANIM_DURATION_FRAMES * 3;

/// 这一帧还该不该显示那条痕迹。
///
/// - `marked` 是自动存档**成功**那一帧的帧号；`None` = 这一局还没自动存
///   过，什么都不显示。
/// - `now` 是当前帧号。
///
/// 时钟倒流（`now < marked`，实践中只可能出现在测试里）按「已经过去 0 帧」
/// 算，用 `saturating_sub` 而不是让它下溢成一个巨大的数——那会让痕迹凭空
/// 消失，是最难查的那一类。
pub fn autosave_notice_visible(marked: Option<FrameTick>, now: FrameTick) -> bool {
    let Some(marked) = marked else {
        return false;
    };
    now.saturating_sub(marked) < AUTOSAVE_NOTICE_FRAMES as FrameTick
}

/// 这条痕迹的 Fluent 键——用户可见文本一律走 i18n，与
/// `crate::player_action::Feedback::i18n_key` 同一条理由。
pub const AUTOSAVE_NOTICE_KEY: &str = "hud-autosave-saved";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 没存过就什么都不显示() {
        // 先证明「不显示」这一侧真的存在，后面那两条才不是在比空气。
        // Arrange & Act & Assert
        assert!(!autosave_notice_visible(None, 0));
        assert!(!autosave_notice_visible(None, 10_000));
    }

    #[test]
    fn 存过之后那一帧起就显示() {
        // Arrange & Act & Assert
        assert!(
            autosave_notice_visible(Some(500), 500),
            "存档成功那一帧就该看得见"
        );
        assert!(autosave_notice_visible(Some(500), 500 + 1));
    }

    /// 规格 §9.2 F3 判据里那个数：一秒。本项目主循环 60 帧每秒。
    ///
    /// **故意不写成 `AUTOSAVE_NOTICE_FRAMES`**——第一版就是那么写的，
    /// 而它让整条判据对时长的任何改动**恒绿**：把常量改成 `u32::MAX`
    /// 之后，边界跟着一起挪，断言照样成立（本批实测过这个坑，记在
    /// 计划文档第八节）。判据必须来自规格，不能来自被判的那个数。
    const 一秒的帧数: FrameTick = 60;

    #[test]
    fn 痕迹在一秒之后自己消失_由时钟驱动() {
        // **规格 F3 的判据**，也是「由时钟驱动」的证据：本条**一次按键
        // 都没有**（ADR 0025），推的只有帧号。
        //
        // 反例验证（已实跑）：把 `AUTOSAVE_NOTICE_FRAMES` 改成
        // `u32::MAX`（不碰任何按键、不碰 `maybe_autosave`），本条红在
        // 「一秒之后就不该再显示」——改的是时钟，红的是断言。
        // 另一条（已实跑）：把 `autosave_notice_visible` 里那句比较改成
        // 恒 `true`，红在同一句。
        // Arrange
        let 存档帧: FrameTick = 1_234;

        // Act & Assert：最后一帧仍在，再走一帧就没了——边界两侧都咬。
        assert!(
            autosave_notice_visible(Some(存档帧), 存档帧 + 一秒的帧数 - 1),
            "一秒内的最后一帧应当还看得见"
        );
        assert!(
            !autosave_notice_visible(Some(存档帧), 存档帧 + 一秒的帧数),
            "一秒之后就不该再显示"
        );
        assert!(!autosave_notice_visible(
            Some(存档帧),
            存档帧 + 一秒的帧数 * 10
        ));
    }

    #[test]
    fn 时钟倒流时按已经过去零帧算() {
        // `saturating_sub` 那一句的字面判据：下溢会让痕迹凭空消失。
        // Arrange & Act & Assert
        assert!(autosave_notice_visible(Some(1_000), 0));
    }

    #[test]
    fn 再存一次会把停留时间从头算起() {
        // Arrange：第一次存在第 0 帧，痕迹本该在一秒后消失。
        assert!(!autosave_notice_visible(Some(0), 一秒的帧数));

        // Act & Assert：第二次存把标记挪到当下，痕迹重新出现。
        assert!(autosave_notice_visible(Some(一秒的帧数), 一秒的帧数));
    }
}
