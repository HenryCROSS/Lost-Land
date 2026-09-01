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
