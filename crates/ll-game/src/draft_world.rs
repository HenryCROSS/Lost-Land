//! 新游戏草稿手里那个世界的**来处**——它同时回答两个问题：
//!
//! 1. 这个世界**从哪来**（本流程现场生成的，还是磁盘上已经存在的那一局）；
//! 2. 将来**存进哪个槽位**（新开一个，还是沿用那一局自己的）。
//!
//! # 为什么这两件事必须是同一个类型
//!
//! 它们本来就是同一件事的两面，而在本模块落地之前它们住在
//! [`crate::chargen::NewGameDraft`] 上的**三个互不相干的字段**里：
//! `world: Option<GameWorld>`、`world_already_exists: bool`、
//! `existing_target: Option<SaveTarget>`。三者必然同真同假，却没有任何
//! 东西保证它们同步——于是 `knowledge/design/ui-and-navigation.md` 2.2 节
//! 记的 **D1** 就是它们漂移出的那个非法组合：
//!
//! > **一个刚生成出来的新世界 + 一个指着玩家老存档的槽位。**
//!
//! 玩家在转生流程里按两下键（选出生地屏 Esc → 世界配置屏「生成」）就能
//! 走到这个组合上，此后每一次存档都把新世界写在他原来那份存档上。没有
//! 确认框，没有提示，进度永久消失。
//!
//! # 修法：让那个组合**写不出来**
//!
//! 规格十一节明确否决过「加一个确认对话框」，理由是治标，并要求
//! 「**写不出来**比**提醒一下**可靠」，点名与
//! `ll_content::mode::SaveMode`（普通档改不回肉鸽档）、
//! `ll_content::header::SaveHeaderMeta`（存档时重算生成期 mod 集合）
//! 是同一种解法。本模块照做：
//!
//! - 能**接收一个新生成的世界**的只有 [`FreshWorld`]，而那个类型里
//!   **根本没有槽位字段**；
//! - **持有槽位**的只有 [`RebornWorld`]，而那个类型**没有任何替换世界的
//!   方法**，字段私有，模块外拿到 `&mut RebornWorld` 也换不掉里面那一局。
//!
//! 于是「新世界 + 老槽位」不是「不该写」，是**表示不出来**：
//! [`DraftWorld::generatable`] 在转生草稿上返回 `None`，
//! `generate_draft_world` 那条路径在转生草稿上**拿不到可写的目标**。
//!
//! # 残余的那一处口子，如实记下来
//!
//! [`DraftWorld::reborn`] 本身仍然可以被人拿一个新生成的世界和一个老槽位
//! 去调。它是转生的**唯一**构造入口（生产代码里只有
//! `crate::chargen::NewGameDraft::for_reincarnation` 一处调用），且调用方
//! 必须把那个老槽位**显式写出来**——那是明写，不是漂移。D1 那条端到端
//! 断言（`crate::app` 的 `app_save_tests`）兜住这一处。

use crate::menu_screen::ScreenState;
use crate::save_slot::SaveTarget;
use crate::spawn_pick::SpawnOrigin;
use crate::world::GameWorld;

/// 草稿手里那个世界的来处。
///
/// 两个变体的载荷都是**字段私有**的结构体，见模块文档「修法」一节——
/// 私有性正是这个类型全部的保护力，改成 `pub` 字段等于把 D1 放回来。
///
/// **不派生 `Debug`**：`crate::world::GameWorld` 装着整个世界，把它印进
/// 日志既没用又能把一行日志撑到几十兆。要看「这是哪条路」用
/// [`DraftWorld::is_reborn`]。
pub enum DraftWorld {
    /// 新游戏：世界由本流程现场生成，槽位要等命名屏之后才开。
    Fresh(FreshWorld),
    /// 转生：世界与槽位都来自磁盘上已经存在的那一局。
    Reborn(RebornWorld),
}

/// 新游戏那条路手里的世界。**这个类型里没有槽位字段。**
///
/// 「生成了一个新世界，却还指着老槽位」因此在这里写不出来——它连一个
/// 老槽位都装不下。
#[derive(Default)]
pub struct FreshWorld {
    /// 按玩家选的参数建出来的那一局；按下「生成世界」之前是 `None`。
    world: Option<GameWorld>,
}

/// 转生那条路手里的世界与它的槽位，**绑在一起**。
///
/// **没有任何方法能换掉里面那个世界。** 转生的语义就是「世界原样不动，
/// 只换一个角色」（`crate::save_slot` 模块文档「一份存档 = 一个世界」），
/// 所以「重新生成」这件事在这个类型上根本不存在。
///
/// # 「新世界 + 老槽位」编译不过
///
/// 先例是 `ll_content::degrade::ReadOnlySave` 那条 `compile_fail` 文档
/// 测试：把「不该写出来」这件事**交给编译器去证**，而不是交给评审去记。
///
/// ```compile_fail
/// # use ll_game::draft_world::RebornWorld;
/// # use ll_game::world::GameWorld;
/// fn 把一个新生成的世界塞进转生草稿(reborn: &mut RebornWorld, 新世界: GameWorld) {
///     // 字段私有，而且这个类型上没有任何替换世界的方法。
///     reborn.world = 新世界;
/// }
/// ```
///
/// 对照组——**这一条编译得过**，它是转生真正的构造入口，调用方必须把那个
/// 老槽位显式写出来（见模块文档「残余的那一处口子」一节）：
///
/// ```no_run
/// # use ll_game::draft_world::DraftWorld;
/// # fn demo(世界: ll_game::world::GameWorld, 槽位: ll_game::save_slot::SaveTarget) {
/// let draft = DraftWorld::reborn(世界, 槽位);
/// assert!(draft.is_reborn());
/// # }
/// ```
pub struct RebornWorld {
    /// 玩家已经玩过的那一局，从 `Session` 手里交回来的原件。
    world: GameWorld,
    /// 那一局自己的槽位——此后每一次存档仍然写它。
    target: SaveTarget,
}

impl DraftWorld {
    /// 新游戏那条路：世界还没生成。
    pub fn fresh() -> DraftWorld {
        DraftWorld::Fresh(FreshWorld::default())
    }

    /// 转生那条路：世界与槽位都是现成的。
    ///
    /// 生产代码里唯一的调用点是
    /// `crate::chargen::NewGameDraft::for_reincarnation`，见模块文档
    /// 「残余的那一处口子」一节。
    pub fn reborn(world: GameWorld, target: SaveTarget) -> DraftWorld {
        DraftWorld::Reborn(RebornWorld { world, target })
    }

    /// 这份草稿走的是不是转生那条路。
    pub fn is_reborn(&self) -> bool {
        matches!(self, DraftWorld::Reborn(_))
    }

    /// 草稿手里那个世界；新游戏还没按「生成世界」时是 `None`。
    pub fn world(&self) -> Option<&GameWorld> {
        match self {
            DraftWorld::Fresh(fresh) => fresh.world.as_ref(),
            DraftWorld::Reborn(reborn) => Some(&reborn.world),
        }
    }

    /// 这一局要写回哪个已有槽位；新游戏那条路没有（等命名屏之后才开）。
    pub fn existing_target(&self) -> Option<&SaveTarget> {
        match self {
            DraftWorld::Fresh(_) => None,
            DraftWorld::Reborn(reborn) => Some(&reborn.target),
        }
    }

    /// 拿到「可以往里放一个新生成的世界」的那个槽——**只有新游戏那条路
    /// 有**。
    ///
    /// 转生草稿返回 `None`，于是 `crate::app::Demo::generate_draft_world`
    /// 在转生草稿上**拿不到可写的目标**：D1 那个「新世界覆盖草稿、老槽位
    /// 原样留着」的组合从这里就走不通了。这是规格 N6 那道闸门真正的形状
    /// ——不是一句 `if`，是一个拿不到的可变引用。
    pub fn generatable(&mut self) -> Option<&mut FreshWorld> {
        match self {
            DraftWorld::Fresh(fresh) => Some(fresh),
            DraftWorld::Reborn(_) => None,
        }
    }

    /// 角色创建屏按「下一步」该去哪块屏。
    ///
    /// # 转生那条路**必须跳过世界配置屏**
    ///
    /// 世界早就存在，再进一次世界配置屏等于让玩家重新生成一个世界——
    /// 这局玩过的一切当场被抹掉，而他只是想换个角色。这条判据此前住在
    /// `crate::chargen` 里、读的是一个裸布尔；搬到这里之后它读的是**世界
    /// 的来处本身**，两者不可能再对不上。
    pub fn screen_after_character_creation(&self) -> ScreenState {
        match self {
            DraftWorld::Fresh(_) => ScreenState::WorldSetup { cursor: 0 },
            DraftWorld::Reborn(_) => ScreenState::SpawnPick {
                origin: SpawnOrigin::CharacterCreation,
            },
        }
    }

    /// 拆开交给 `Session::begin` 那一步：世界与（可能有的）槽位。
    ///
    /// 拆开之后这个类型就没了——真正进世界那一刻，「草稿」这个概念本来
    /// 就该结束。
    pub fn into_parts(self) -> (Option<GameWorld>, Option<SaveTarget>) {
        match self {
            DraftWorld::Fresh(fresh) => (fresh.world, None),
            DraftWorld::Reborn(reborn) => (Some(reborn.world), Some(reborn.target)),
        }
    }
}

impl FreshWorld {
    /// 把刚生成出来的那一局放进来。
    ///
    /// **本类型没有槽位字段**，所以调用它绝不可能产生「新世界 + 老槽位」
    /// 那个组合；而拿到 `&mut FreshWorld` 的唯一途径是
    /// [`DraftWorld::generatable`]，它在转生草稿上返回 `None`。
    pub fn put(&mut self, world: GameWorld) {
        self.world = Some(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_slot::SaveTarget;

    fn 测试世界() -> GameWorld {
        let content = crate::test_support::test_content();
        crate::world::build_new_world(
            &content,
            ll_world::generate::GenParams {
                seed: 7,
                ..ll_world::generate::GenParams::default()
            },
        )
        .expect("测试用布局满足全部构造前置条件")
    }

    fn 测试槽位() -> SaveTarget {
        SaveTarget::create_in(
            &crate::test_support::unique_temp_path("ll-game-draft-world-slot"),
            "slot",
            0,
        )
    }

    #[test]
    fn 转生草稿拿不到可以写新世界的那个槽() {
        // **这一条就是 D1 的类型层判据**：拿不到 `&mut FreshWorld`，
        // 「生成一个新世界盖掉草稿」这件事在转生草稿上无从下手。
        //
        // 反例验证（已实跑）：把 `generatable` 的 `Reborn` 分支改成也返回
        // 一个可写的槽（例如给 `RebornWorld` 加一个 `FreshWorld` 字段并
        // 返回它），本条当场变红。
        // Arrange
        let mut draft = DraftWorld::reborn(测试世界(), 测试槽位());

        // Act & Assert
        assert!(
            draft.generatable().is_none(),
            "转生草稿上不该存在「重新生成世界」这条路径"
        );
    }

    #[test]
    fn 新游戏草稿拿得到那个槽且放进去之后没有任何老槽位() {
        // Arrange
        let mut draft = DraftWorld::fresh();
        assert!(draft.world().is_none(), "Arrange：还没生成");

        // Act
        draft
            .generatable()
            .expect("新游戏那条路必须拿得到")
            .put(测试世界());

        // Assert：世界进去了，而槽位**仍然是空的**——`FreshWorld` 里
        // 根本没有那个字段可填。
        assert!(draft.world().is_some());
        assert!(
            draft.existing_target().is_none(),
            "新生成的世界绝不该带着一个已有槽位"
        );
    }

    #[test]
    fn 转生草稿手里的世界与槽位一起交出去() {
        // Arrange
        let 世界哈希 = 测试世界().world.hash();
        let 槽位 = 测试槽位();
        let 槽位号 = 槽位.id.clone();
        let draft = DraftWorld::reborn(测试世界(), 槽位);

        // Act
        let (world, target) = draft.into_parts();

        // Assert
        assert_eq!(
            world.expect("转生草稿必然有世界").world.hash(),
            世界哈希,
            "交出去的必须是原样那一局"
        );
        assert_eq!(target.expect("转生草稿必然有槽位").id, 槽位号);
    }

    #[test]
    fn 只有转生那条路跳过世界配置屏() {
        // 规格 N6 的另一半：新游戏仍然经过世界配置屏。
        // Arrange & Act & Assert
        assert_eq!(
            DraftWorld::fresh().screen_after_character_creation(),
            ScreenState::WorldSetup { cursor: 0 }
        );
        assert_eq!(
            DraftWorld::reborn(测试世界(), 测试槽位()).screen_after_character_creation(),
            ScreenState::SpawnPick {
                origin: SpawnOrigin::CharacterCreation
            },
            "转生必须跳过世界配置屏，且选点屏的取消目标是角色创建"
        );
    }
}
