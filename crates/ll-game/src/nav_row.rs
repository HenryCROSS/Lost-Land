//! 「返回」与「关闭」是**两个不同的动作**，一块屏最多只有其中一行。
//!
//! # 规格 N3 与它要防的东西
//!
//! `knowledge/design/ui-and-navigation.md` §7.2 N3 原文：
//!
//! > - **返回**：栈深度减一，回到上一层。子菜单、多步流程用它。
//! > - **关闭**：把整条模态栈弹空，回到世界。只有**游戏内菜单**这一处
//! >   有它（`pause_menu.rs:127` 的「继续」）。
//! >
//! > **判据**：任何屏最多有其中一行，不能两行都有。今天没有屏违反，
//! > 这条是防将来。
//!
//! 两行都有会让玩家面对一个他答不上来的问题：**「返回」与「关闭」这两行
//! 到底差在哪**——它们在屏幕上长得一样，都是一行字，而后果一个退一层、
//! 一个退到底。多步流程（角色创建 → 世界配置 → 选点 → 命名）上按错一次
//! 就是白填一遍。
//!
//! # 为什么不是「再写一张表」
//!
//! 直觉的做法是在门禁里写一张「哪一行算返回、哪一行算关闭」的清单。
//! 那就是**真相源之外的第二份副本**——本仓库反复付过代价的形状
//! （`ContentIndex` 的裸数值当判据、`atlas_coverage.rs` 的手写地形清单、
//! `skin.rs` 查的裸贴图名）。副本迟早分叉，而分叉时没有任何东西会报错。
//!
//! 因此本模块的做法是：**角色是行枚举自己的一个属性**（[`NavRow`]），
//! 而且**真实分派要去问它**——`crate::pause_menu::update_menu` 判「这一行
//! 是不是关闭」走的就是 [`NavRow::nav_role`]，不再对
//! `MenuRow::Continue` 直接 `match`。角色标错不是只有门禁会红，菜单的
//! 行为当场就变了。
//!
//! # 今天各块屏的角色分布（逐条写明，不留「漏了还是本来就没有」的歧义）
//!
//! | 屏 | 行枚举 | 带角色的行 |
//! |---|---|---|
//! | 游戏内菜单 | `crate::pause_menu::MenuRow` | 「继续」= [`NavRole::Close`] |
//! | 设置 | `crate::menu_screen::SettingsRow` | 「返回」= [`NavRole::Back`] |
//! | 角色创建 | `crate::chargen::CharacterRow` | 「返回」= [`NavRole::Back`] |
//! | 世界配置 | `crate::world_setup::WorldSetupRow` | 「返回」= [`NavRole::Back`] |
//!
//! **首页**的「离开」是 `ScreenOutcome::Quit`（退出整个进程），**不是**
//! 关闭——它不回到世界，它把游戏关掉。首页也没有「返回」：它是栈底，
//! 上一层不存在（规格 N2：「没有上一层时明确地什么都不做」）。
//!
//! **存档列表 / 选出生地 / 存档命名 / 会话屏**四块没有任何导航行：它们
//! 的行分别是存档槽位、地图（不是行列表）、一个输入框、对话选项，退出
//! 一律走取消键。这不是漏了——给它们加一行「返回」等于在一块只有内容行
//! 的列表末尾插一条语义不同的行，而取消键已经把这件事做完了。
//!
//! # 与 N2 的关系
//!
//! N2（取消键永远只退一层）管的是**键**，本条管的是**行**。两者的交点是
//! [`NavRole::Back`]：那一行按下去应当与按取消键完全同义。今天四块屏
//! 都是这样写的（`chargen.rs:290`、`world_setup.rs:192`、
//! `menu_screen.rs:862` 与各自的取消键分支去同一个地方）。

use ll_platform::input::{GameKey, InputState};

/// 一行在导航里扮演的角色——见模块文档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavRole {
    /// **返回**：退掉最上面那一层，回到上一层。子菜单与多步流程用它。
    Back,
    /// **关闭**：把整条模态栈弹空，回到世界。今天只有游戏内菜单的
    /// 「继续」一处。
    Close,
}

/// 一个行枚举回答「这一行是返回、是关闭，还是两者都不是」。
///
/// 实现它的是各块屏**自己那份行枚举**，不是另建一张表——见模块文档
/// 「为什么不是『再写一张表』」一节。
pub trait NavRow: Copy {
    /// 这一行的导航角色；`None` 表示它既不是返回也不是关闭。
    fn nav_role(self) -> Option<NavRole>;
}

/// 一块屏违反了 N3：同时有两行带导航角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavRoleConflict {
    /// 先出现的那一行的角色。
    pub first: NavRole,
    /// 又出现的那一行的角色。
    pub second: NavRole,
}

/// 这一屏**唯一**的那条导航行的角色；一条都没有时是 `Ok(None)`。
///
/// 两行都有（无论是「返回 + 关闭」还是「两行返回」）一律 `Err`——规格
/// N3 的判据是「最多有其中一行」，两行同类同样是玩家答不上来的那个
/// 问题。
pub fn sole_nav_role<R: NavRow>(rows: &[R]) -> Result<Option<NavRole>, NavRoleConflict> {
    let mut found: Option<NavRole> = None;
    for row in rows {
        let Some(role) = row.nav_role() else {
            continue;
        };
        if let Some(first) = found {
            return Err(NavRoleConflict {
                first,
                second: role,
            });
        }
        found = Some(role);
    }
    Ok(found)
}

/// 左右键落在这一行上时该做什么（规格 N12，
/// `knowledge/design/ui-and-navigation.md` §7.6）。
///
/// # 规格原文与它要修的东西
///
/// > **N12（P2）左右键在列表屏里一律等同上下键（移动焦点），只在有数值
/// > 的行上改数值。**
///
/// 今天三块「光标 + 行」的屏（设置 / 角色创建 / 世界配置）都是一个
/// `match row`：有取值的行左右键改值，**其余行左右键什么都不做**。
/// 玩家在「返回」那一行上按左右键，屏幕纹丝不动——与按坏了没有区别。
///
/// # 为什么是行枚举自己的属性，不是另写一张表
///
/// 与 [`NavRow`] 逐字同一条理由（见模块文档「为什么不是『再写一张表』」
/// 一节）：另写一张表就是真相源之外的第二份副本，副本迟早分叉，而分叉
/// 时没有任何东西会报错。
///
/// 而且**真实分派要去问它**——三块屏的左右键分支走的就是
/// [`HorizontalRow::horizontal_role`]。角色标错不是只有门禁会红：把
/// `SettingsRow::Language` 标成 [`HorizontalRole::MovesFocus`] 的那一刻，
/// 左右键就不再切语言了。
///
/// # 用焦点表的那几块屏不需要实现它
///
/// 首页与游戏内菜单走的是 `ll_ui::widget::focus`，而那一层**已经**把
/// 左右键当上下键使（`focus.rs` 里 `Down || Right` / `Up || Left` 那两
/// 行）。它们没有任何取值行，因此也不存在「哪一行该改值」这个问题——
/// 给它们实现本 trait 只会得到一个恒返回同一个值的函数，那是 ADR 0021
/// 拦的那种「为对称而抽象」。
///
/// **存档列表 / 存档命名 / 选出生地 / 会话屏**同样不实现：规格原文
/// 「存档列表、命名屏按左右键仍然什么都不做是可以的（它们没有数值行，
/// 也只有一列），但要在代码里写明这是『本屏无横向维度』而不是漏了」
/// ——这一段就是那句写明。选出生地屏的左右键是**地图光标**（空间坐标，
/// 不是列表），会话屏是对话选项，两者都不是「行 + 取值」的形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalRole {
    /// 这一行有一个可以左右调的取值——左右键改值。
    AdjustsValue,
    /// 这一行没有取值——左右键**等同上下键**，移动焦点。
    MovesFocus,
}

/// 一个行枚举回答「左右键落在这一行上该做什么」，见 [`HorizontalRole`]。
pub trait HorizontalRow: Copy {
    /// 这一行的横向角色。
    fn horizontal_role(self) -> HorizontalRole;
}

/// 一列 `len` 行的光标往前/往后走一格，**到边循环**——规格 N11
/// 「上下键一律循环」与 N12「左右键等同上下键」在**同一个函数**上落地。
///
/// # 循环，不是到边即停（规格 N11，批次 33）
///
/// 批次 30 落 N12 时这里刻意写的是「到边即停」，原话是：
///
/// > 不循环是**保守取舍**：规格 N11（上下键一律循环）是 P1、还没落地，
/// > 今天上下键到边即停。左右键此刻循环就会比上下键更「新」，两者在
/// > 同一块屏上行为不一致。**N11 落地时这一个函数跟着改，三块屏不用动。**
///
/// 本批就是它说的那一天：N11 已落地（`docs/superpowers/plans/
/// 2026-09-01-batch33-ui-final.md`），上下键与左右键在**九块屏上**走的
/// 都是这一个函数，循环语义因此不可能在两个轴上分叉。
///
/// # 为什么循环
///
/// 列表短（本体最长的设置屏也就二十来行，多数屏三五行），从头绕到尾
/// 比一路按回去快，且不需要玩家记得「到顶了」——这正是首页与游戏内
/// 菜单（`ll_ui::widget::focus::move_focus`）与 HUD 动作菜单
/// （`crate::player_action`）从一开始就在用的那一套。
///
/// **例外一处，不在这里**：选出生地屏的**地图光标**保持到边即停——那是
/// 空间坐标不是列表，循环意味着从地图左边缘瞬移到右边缘（见规格 §7.5
/// N11 的「例外一处」）。它走的是 `crate::app` 自己的 `dx/dy`，压根不
/// 经过本函数。
pub fn stepped_cursor(cursor: usize, forward: bool, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let cursor = cursor.min(len - 1);
    if forward {
        (cursor + 1) % len
    } else {
        (cursor + len - 1) % len
    }
}

/// 这一帧上下键把光标移到第几行——没有移动时返回 `None`，调用方据此
/// 接着判动作键。**九块屏共用的那一份**（规格 N11）。
///
/// # `was_activated` 而非 `was_just_pressed`：长按连发
///
/// 方向键参与自动重复（`GameKey::is_repeatable`）。连发**不是合成
/// 按键**（ADR 0025 禁止那种做法）：它由
/// [`InputState::begin_frame`](ll_platform::input::InputState::begin_frame)
/// 按**时钟**判定——按住不放，超过 `initial_delay` 才第一次重复，此后
/// 每 `interval` 一次。玩家按一次键，帧循环推进时间，重复自己就来了；
/// 没有任何一层伪造第二次按下。
///
/// 二十几行的设置屏长按滚动是刚需，而它与方向键在地图上长按连续移动
/// 是同一种手感，不该在菜单里退化成一次一格。
///
/// # 同时按住上下视为无输入
///
/// 与 `crate::player_action` 里 `direction_from_input` 对相反方向同时
/// 按住的处理一致：两者抵消，不猜测玩家意图。
///
/// # 为什么 `ll_ui::widget::focus` 不并进来
///
/// 首页与游戏内菜单走 `ll_ui::widget::focus::navigate_focus`，它**已经**
/// 循环、也已经用 `was_activated`——N11 在那一侧本来就成立。但它操作的
/// 是一张 `WidgetStateTable`（「谁的 `focused` 为真」）而不是一个 `usize`
/// 光标，塞进同一个函数只会得到一个带分支的四不像，那是 ADR 0021 拦的
/// 那种「为对称而抽象」。两边共享的是**语义**（循环 + 连发），不是
/// 同一行代码——而语义分叉了会当场被两侧各自的断言抓住。
pub fn moved_cursor(input: &InputState, cursor: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let up = input.was_activated(GameKey::Up);
    let down = input.was_activated(GameKey::Down);
    if up == down {
        return None;
    }
    Some(stepped_cursor(cursor, down, len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chargen::{CharacterRow, character_rows};
    use crate::menu_screen::{SettingsRow, settings_rows};
    use crate::pause_menu::{MenuRow, menu_rows};
    use crate::world_setup::{WorldSetupRow, world_setup_rows};

    #[test]
    fn 四块屏各自至多只有一行导航行() {
        // 规格 N3 的判据本体。**逐块屏都从它自己那份行列表现取**，不抄
        // 一份静态清单——抄一份的那一刻，「以后新增的行有没有角色」就
        // 变成了要靠人记得同步的事。
        //
        // 反例（已实跑）：把 `MenuRow::BackToTitle` 的角色也标成
        // `Close`，本条当场红在暂停菜单那一行，`Err` 里两个角色都是
        // `Close`。
        // Arrange & Act & Assert：暂停菜单（普通模式与肉鸽模式行数不同，
        // 两种都测——「保存」那一行的有无不该影响这条不变式）。
        for can_save in [true, false] {
            assert_eq!(
                sole_nav_role(&menu_rows(can_save)),
                Ok(Some(NavRole::Close)),
                "游戏内菜单唯一的导航行是「继续」，角色是关闭"
            );
        }

        assert_eq!(
            sole_nav_role(&settings_rows()),
            Ok(Some(NavRole::Back)),
            "设置屏唯一的导航行是「返回」"
        );
        assert_eq!(
            sole_nav_role(&character_rows()),
            Ok(Some(NavRole::Back)),
            "角色创建屏唯一的导航行是「返回」"
        );
        assert_eq!(
            sole_nav_role(&world_setup_rows()),
            Ok(Some(NavRole::Back)),
            "世界配置屏唯一的导航行是「返回」"
        );
    }

    #[test]
    fn 被断言的那几行确实存在于各自的行列表里() {
        // 防「断言恒绿是因为被断言的对象根本不存在」：上一条若某块屏的
        // 行列表恰好是空的，`sole_nav_role` 会返回 `Ok(None)` 而不是
        // 红——所以先把「那一行真的在列表里」证出来。
        // Arrange & Act & Assert
        assert!(menu_rows(true).contains(&MenuRow::Continue));
        assert!(settings_rows().contains(&SettingsRow::Back));
        assert!(character_rows().contains(&CharacterRow::Back));
        assert!(world_setup_rows().contains(&WorldSetupRow::Back));
    }

    #[test]
    fn 声明成关闭的那一行按下去真的关掉整块屏() {
        // 让「角色」这条声明是**载重的**，不是只有门禁读的一张表：
        // `update_menu` 判「这一行是不是关闭」问的就是 `nav_role`。角色
        // 标错的那一刻，暂停菜单的「继续」就不再关得掉菜单。
        //
        // 反例（已实跑）：把 `MenuRow::Continue` 的角色改成 `None`，
        // 本条红在 `outcome`——`update_menu` 里那个 `match` 的
        // `Continue` 分支是 `ScreenOutcome::Idle`（见那里的注释），
        // 「继续」当场关不掉菜单。
        // Arrange
        let ids = crate::pause_menu::menu_item_ids(true);
        let mut table = crate::menu_screen::preselected_focus(&ids);
        let mut input = ll_platform::input::InputState::new();
        input.press(ll_platform::input::GameKey::Confirm);

        // Act
        let (outcome, next) = crate::pause_menu::update_menu(
            &mut table,
            &input,
            crate::pointer::RowPointer::Idle,
            true,
        );

        // Assert
        assert_eq!(
            outcome,
            crate::menu_screen::ScreenOutcome::Close,
            "「继续」按下去必须关掉整块屏"
        );
        assert_eq!(next, None, "关闭不换屏，它把整块屏关掉");
        assert_eq!(MenuRow::Continue.nav_role(), Some(NavRole::Close));
    }

    #[test]
    fn 三块屏的每一行都声明了自己有没有横向维度() {
        // 规格 N12 的判据本体。**逐块屏从它自己那份行列表现取**，不抄一份
        // 静态清单——`horizontal_role` 是没有 `_ =>` 兜底的 `match`，新增
        // 一行时编译器就会逼人回答，本条则保证「行列表本身非空、且真的
        // 两类都有」。
        //
        // 反例验证（已实跑）：把 `SettingsRow::Language` 的角色改成
        // `MovesFocus`，本条红在「设置屏应当有取值行」……不，实测红在
        // 下一条（左右键不再切语言），本条因为还有 Vsync/ScaleFilter
        // 仍然绿——两条各咬一头，见下一条。
        // Arrange & Act & Assert
        for (屏, 角色) in [
            (
                "设置屏",
                settings_rows()
                    .iter()
                    .map(|r| r.horizontal_role())
                    .collect::<Vec<_>>(),
            ),
            (
                "角色创建屏",
                character_rows()
                    .iter()
                    .map(|r| r.horizontal_role())
                    .collect::<Vec<_>>(),
            ),
            (
                "世界配置屏",
                world_setup_rows()
                    .iter()
                    .map(|r| r.horizontal_role())
                    .collect::<Vec<_>>(),
            ),
        ] {
            assert!(!角色.is_empty(), "{屏}的行列表是空的，本条无从谈起");
            assert!(
                角色.contains(&HorizontalRole::AdjustsValue),
                "{屏}一个取值行都没有——那它就不该是「光标 + 行 + 取值」这种屏"
            );
            assert!(
                角色.contains(&HorizontalRole::MovesFocus),
                "{屏}每一行都有取值？那「返回」那一行去哪了"
            );
        }
    }

    #[test]
    fn 声明成没有横向维度的那一行按左右键真的移动焦点() {
        // 让「横向角色」这条声明是**载重的**，不是只有门禁读的一张表
        // ——与 `声明成关闭的那一行按下去真的关掉整块屏` 同一条思路。
        //
        // 拿世界配置屏验（它的 `update_*` 只吃普通类型，不需要装载内容）：
        // 光标停在「返回」（`MovesFocus`），按左键应当把光标往上挪一格，
        // 而不是什么都不发生。
        //
        // 反例验证（已实跑）：把 `WorldSetupRow::Back` 的角色改成
        // `AdjustsValue`，本条红——光标纹丝不动（`adjust_row` 对 `Back`
        // 是空实现）。
        // Arrange
        let rows = world_setup_rows();
        let 返回 = rows
            .iter()
            .position(|r| *r == WorldSetupRow::Back)
            .expect("世界配置屏应当有「返回」那一行");
        assert!(返回 > 0, "「返回」是首行的话「往上挪一格」无从谈起");
        assert_eq!(
            WorldSetupRow::Back.horizontal_role(),
            HorizontalRole::MovesFocus,
            "「返回」不该有横向取值"
        );
        let mut cursor = 返回;
        let mut shape = ll_world::generate::TerrainShape::default();
        let mut preset = 0usize;
        let mut mode = ll_content::mode::SaveMode::Permadeath;
        let mut input = ll_platform::input::InputState::new();
        input.press(ll_platform::input::GameKey::Left);

        // Act
        let _ = crate::world_setup::update_world_setup(
            &mut cursor,
            &mut shape,
            &mut preset,
            &mut mode,
            &input,
            crate::pointer::RowPointer::Idle,
        );

        // Assert
        assert_eq!(
            cursor,
            返回 - 1,
            "左右键落在没有横向维度的行上应当移动焦点（规格 N12），实际光标没动"
        );
    }

    #[test]
    fn 声明成有取值的那一行按左右键改的是值不是焦点() {
        // 上一条的对照：同一块屏、同一个键，落在「海平面」上时光标**不动**，
        // 变的是取值。两条合起来才说明分派真的按角色走，而不是「一律移动
        // 焦点」或「一律改值」。
        //
        // 反例验证（已实跑）：把 `WorldSetupRow::SeaLevel` 的角色改成
        // `MovesFocus`，本条红在「海平面没变」。
        // Arrange
        let rows = world_setup_rows();
        let 海平面 = rows
            .iter()
            .position(|r| *r == WorldSetupRow::SeaLevel)
            .expect("世界配置屏应当有「海平面」那一行");
        let mut cursor = 海平面;
        let mut shape = ll_world::generate::TerrainShape::default();
        let 原海平面 = shape.sea_level;
        let mut preset = 0usize;
        let mut mode = ll_content::mode::SaveMode::Permadeath;
        let mut input = ll_platform::input::InputState::new();
        input.press(ll_platform::input::GameKey::Right);

        // Act
        let _ = crate::world_setup::update_world_setup(
            &mut cursor,
            &mut shape,
            &mut preset,
            &mut mode,
            &input,
            crate::pointer::RowPointer::Idle,
        );

        // Assert
        assert_eq!(cursor, 海平面, "取值行上左右键不该移动焦点");
        assert_ne!(
            shape.sea_level, 原海平面,
            "取值行上左右键应当改值，海平面没变"
        );
    }

    /// 按住一个键：`press` 一次，此后**再也不调 `press`**。
    fn 按住(key: GameKey) -> InputState {
        let mut input = InputState::new();
        input.press(key);
        input
    }

    #[test]
    fn 末行按下移动到首行首行按上移动到末行() {
        // **规格 N11 的主判据**：上下键一律循环。批次 30 这里刻意写的是
        // 「到边即停」，原话「N11 落地时这一个函数跟着改」——本批就是
        // 那一天，见 `stepped_cursor` 文档。
        //
        // 反例验证（已实跑）：把 `stepped_cursor` 改回
        // `(cursor + 1).min(len - 1)` / `saturating_sub(1)`，本条红在
        // 「末行按下应当回到首行 4 ≠ 0」。
        // Arrange & Act & Assert
        assert_eq!(stepped_cursor(4, true, 5), 0, "末行按下应当回到首行");
        assert_eq!(stepped_cursor(0, false, 5), 4, "首行按上应当回到末行");
        assert_eq!(stepped_cursor(1, true, 5), 2);
        assert_eq!(stepped_cursor(1, false, 5), 0);
        // 越界的光标先钳回最后一行再走——行数会随内容变，而光标是跨帧
        // 带过来的一个裸 `usize`。
        assert_eq!(stepped_cursor(99, true, 5), 0);
        // 零行不做除零也不越界（ADR 0015「查不到就是查不到」）。
        assert_eq!(stepped_cursor(3, true, 0), 0);
    }

    #[test]
    fn 角色创建与世界配置两块屏的上下键真的循环了() {
        // 上一条测的是算法，这一条测**那两块屏真的在用它**——规格 N11
        // 点名的正是这两块（它们共用 `chargen::move_cursor`）。
        //
        // 反例验证（已实跑）：把 `chargen::move_cursor` 改回自己那份
        // 「到边即停」，本条红在「角色创建屏末行按下应当回到首行」。
        // Arrange
        let 下 = 按住(GameKey::Down);
        let 上 = 按住(GameKey::Up);
        let 角色 = character_rows().len();
        let 世界 = world_setup_rows().len();

        // Act & Assert：先自证这两块屏真的有好几行（一行的列表循环恒真）。
        assert!(角色 > 1 && 世界 > 1);
        assert_eq!(
            crate::chargen::move_cursor(角色 - 1, 角色, &下),
            0,
            "角色创建屏末行按下应当回到首行"
        );
        assert_eq!(
            crate::chargen::move_cursor(0, 角色, &上),
            角色 - 1,
            "角色创建屏首行按上应当回到末行"
        );
        assert_eq!(
            crate::chargen::move_cursor(世界 - 1, 世界, &下),
            0,
            "世界配置屏末行按下应当回到首行"
        );
    }

    #[test]
    fn 同时按住上下视为无输入() {
        // 与 `direction_from_input` 对相反方向同时按住的处理一致：
        // 两者抵消，不猜测玩家意图。
        // Arrange
        let mut 两个都按 = InputState::new();
        两个都按.press(GameKey::Up);
        两个都按.press(GameKey::Down);

        // Act & Assert
        assert_eq!(moved_cursor(&两个都按, 2, 5), None);
        assert_eq!(moved_cursor(&InputState::new(), 2, 5), None, "没按键不动");
        assert_eq!(moved_cursor(&按住(GameKey::Down), 2, 0), None, "零行不动");
    }

    #[test]
    fn 长按方向键连发是由时钟驱动的不是由按键次数驱动的() {
        // **ADR 0025 的落点**：连发不许用「假装连续按了很多次」实现。
        // 本条全程**只调一次 `press`**，此后一次都不再调——光标继续
        // 往下走，靠的是 `InputState::begin_frame(now, RepeatConfig)`
        // 按时钟判定的自动重复。
        //
        // 反例验证（已实跑）：把 `interval` 调成 10 秒（**不碰任何
        // `press` 调用**），本条红在「初次重复之后每个 interval 应当
        // 再动一格」——改的是时钟，红的是断言，这就是「由时钟驱动」
        // 的证据。
        // Arrange
        let config = ll_platform::input::RepeatConfig::default();
        let t0 = std::time::Instant::now();
        let mut input = InputState::new();
        let mut cursor = 0_usize;
        let len = 100_usize; // 长到不会绕回来，免得「动了几格」被循环掩盖
        let mut 按下次数 = 0_usize;

        // Act：第 0 帧按下（唯一一次 `press`），之后每 16ms 推进一帧。
        input.begin_frame(t0, config);
        input.press(GameKey::Down);
        按下次数 += 1;
        if let Some(next) = moved_cursor(&input, cursor, len) {
            cursor = next;
        }
        let 初次按下之后 = cursor;
        input.end_frame();

        let mut 首次重复的帧 = None;
        let mut 走过的帧 = Vec::new();
        for frame in 1..80 {
            let now = t0 + std::time::Duration::from_millis(16 * frame);
            input.begin_frame(now, config);
            let 动了 = match moved_cursor(&input, cursor, len) {
                Some(next) => {
                    cursor = next;
                    true
                }
                None => false,
            };
            if 动了 {
                走过的帧.push(frame);
                if 首次重复的帧.is_none() {
                    首次重复的帧 = Some(frame);
                }
            }
            input.end_frame();
        }

        // Assert
        assert_eq!(按下次数, 1, "全程只允许按一次键——多按就成了合成按键");
        assert_eq!(初次按下之后, 1, "按下那一帧走一格");
        let 首次 = 首次重复的帧.expect("按住不放应当在等满初次延迟后开始连发");
        let 首次毫秒 = 16 * 首次;
        assert!(
            首次毫秒 >= config.initial_delay.as_millis() as u64,
            "连发不该早于初次延迟（{}ms），实测第 {首次} 帧 = {首次毫秒}ms",
            config.initial_delay.as_millis()
        );
        assert!(
            走过的帧.len() >= 5,
            "80 帧（约 1.28s）里应当连发好几次，实测 {走过的帧:?}"
        );
        assert!(
            cursor > 5,
            "只按了一次键，光标却应当被时钟推着走了好几格，实测 {cursor}"
        );
    }

    #[test]
    fn 两行都有导航角色时报冲突() {
        // `sole_nav_role` 自己的判据——不靠「今天恰好没有屏违反」来
        // 证明它咬得住。
        // Arrange
        #[derive(Clone, Copy)]
        struct 假行(Option<NavRole>);
        impl NavRow for 假行 {
            fn nav_role(self) -> Option<NavRole> {
                self.0
            }
        }
        let 两行都有 = [
            假行(Some(NavRole::Back)),
            假行(None),
            假行(Some(NavRole::Close)),
        ];

        // Act & Assert
        assert_eq!(
            sole_nav_role(&两行都有),
            Err(NavRoleConflict {
                first: NavRole::Back,
                second: NavRole::Close,
            })
        );
        assert_eq!(sole_nav_role(&[假行(None), 假行(None)]), Ok(None));
    }
}
