//! 世界内容记号：地面物品堆的「团」、通用放置家具记号、通用 NPC 记号，
//! 以及一张**内容自己声明**的家具贴图（锻炉）。
//!
//! # 这一组图为什么现在才存在
//!
//! 在此之前 `render_surface`（`ll_game::app`）只画两样东西：地形瓦片与
//! 玩家标记。地面物品、放置家具、NPC 三类世界内容在引擎里全都存在、
//! 能交互、有测试，**屏幕上一个都看不见**——图从来没画过，渲染路径也
//! 从来没接过。本模块补的是前半截（图），后半截（接线）在
//! `ll_game::surface_draw` 与 `ll_game::app::render_surface`。
//!
//! # 项目所有者的裁定：地面物品统一用一个「团」
//!
//! > 当物品丢在地上，无论是一个还是N个，交互的时候都统一以列表显示，
//! > 并且统一用一个团表示哪一个地方有东西
//!
//! 因此 [`decorate_ground_pile`] 是**唯一**一张地面物品贴图：那一格躺着
//! 一把剑、六块铁锭还是一堆杂物，画出来都是同一个记号，只表达「这地方
//! 有东西」。具体是什么，玩家开交互列表才知道——这条裁定同时也是
//! `ll_game::surface_draw::ground_pile_draws` 按坐标去重、不按物品种类
//! 分别成条的理由。
//!
//! # 底色留空，不是疏漏
//!
//! 本模块四张图都**不铺满底色**（对比 `sprite.rs` 的 `decorate_hero_*`
//! 开头那一句 `fill_rect`）：它们都画在地形之上（`Layer::DECOR`/
//! `Layer::ENTITY`），透明的背景像素让下面那格地形透出来。铺满不透明
//! 底色会让每一堆地面物品变成一块盖住地形的色板，读起来像「这一格地形
//! 变了」而不是「这一格上摆着东西」。
//!
//! # 配色为什么是这几个
//!
//! 唯一的硬要求是「能看出是什么、能和别的东西区分开」（项目所有者：
//! 「你先画点东西。以后我再精细化处理。」）。取值因此只避开两类既有
//! 颜色，不追求画风：
//!
//! - **地形主色**（`terrain.rs` 的 `TERRAIN_SPECS`）：绿（草/森林）、
//!   褐（土/丘陵）、蓝（深/浅水）、沙黄、灰（山）、近白（雪）。
//! - **既有角色色**（`sprite.rs`）：玩家钢蓝 `(70,130,180)`、boss 暗红
//!   `(180,40,40)`、两者的暖金标志 `(255,220,120)`。
//!
//! 于是：地面物品堆取**琥珀橙**（暖、饱和度高于沙黄，不会跟沙地糊在
//! 一起），家具记号取**紫罗兰**（色相 ~260°，地形里完全没有这一段），
//! NPC 取**紫红**（色相 ~290°，与玩家蓝相差 ~80°，与草地绿相差 ~160°
//! ——NPC 最常站在草地上，这条差距比「跟玩家不同」更要紧）。

use crate::EntryRect;
use crate::sprite::{paint_diamond, paint_patch};
use image::RgbaImage;

/// 地面物品堆的堆身色（琥珀橙）。
const PILE_BODY: (u8, u8, u8) = (224, 160, 60);
/// 地面物品堆的暗边色（深褐），兼作「堆里有好几样东西」的碎点色。
const PILE_EDGE: (u8, u8, u8) = (74, 50, 24);
/// 地面物品堆的高光色（浅奶油）。
const PILE_HIGHLIGHT: (u8, u8, u8) = (245, 216, 154);

/// 通用放置家具记号的主体色（紫罗兰）。
const FURNITURE_BODY: (u8, u8, u8) = (130, 120, 180);
/// 通用放置家具记号的暗边色。
const FURNITURE_EDGE: (u8, u8, u8) = (58, 54, 82);
/// 通用放置家具记号的顶面色（比主体亮，读成「立着的东西的上表面」）。
const FURNITURE_TOP: (u8, u8, u8) = (176, 168, 220);

/// 锻炉的石砌主体色。
const FORGE_STONE: (u8, u8, u8) = (96, 90, 86);
/// 锻炉的底座/烟囱暗色。
const FORGE_DARK: (u8, u8, u8) = (44, 40, 40);
/// 锻炉炉口的火色。
const FORGE_EMBER: (u8, u8, u8) = (238, 118, 32);
/// 锻炉炉心的最亮色。
const FORGE_CORE: (u8, u8, u8) = (255, 226, 140);

/// NPC 主体色（紫红）——见模块文档「配色为什么是这几个」。
const NPC_BODY: (u8, u8, u8) = (150, 110, 160);
/// NPC 标志色（近白）。刻意不复用玩家/boss 的暖金 `(255,220,120)`：
/// 那样一来「远处那个高对比小块是谁」又要靠主体色去猜，正是
/// `sprite.rs` 里 `BOSS_CHEST_MARK` 文档记下的同一条理由。
const NPC_MARK: (u8, u8, u8) = (235, 240, 250);
/// NPC 脚部暗色。
const NPC_FOOT: (u8, u8, u8) = (60, 40, 70);

/// 堆身的逐行轮廓：`(dy, dx, width)`，相对精灵左上角。
///
/// 手写成一张表而不是用某种曲线算出来：16×16 这个尺寸下总共只有八行，
/// 表本身比任何生成式都短，也更容易一眼看出「这是个下宽上窄的堆」。
/// 暗边通过把同一张表整体外扩一圈先画一遍实现（见
/// [`decorate_ground_pile`]），不需要第二张表。
const PILE_ROWS: &[(u32, u32, u32)] = &[
    (8, 7, 2),
    (9, 6, 4),
    (10, 5, 6),
    (11, 4, 8),
    (12, 3, 10),
    (13, 3, 10),
    (14, 2, 12),
    (15, 2, 12),
];

/// 画 `ground_pile`：地面物品堆的「团」。
///
/// 一格上无论躺着一件还是二十件东西，画的都是这一张——项目所有者的
/// 裁定，见模块文档。图里那两粒暗色碎点是唯一暗示「这可能不止一件」
/// 的笔画，但它**不随实际件数变化**：件数是交互列表的事，不是这张图
/// 的事。
pub(crate) fn decorate_ground_pile(image: &mut RgbaImage, rect: EntryRect) {
    // 先按外扩一圈的轮廓铺暗边，再把堆身画在里面——两遍共用同一张
    // `PILE_ROWS`，暗边因此不可能与堆身的形状对不上。
    for &(dy, dx, width) in PILE_ROWS {
        let edge_dx = dx.saturating_sub(1);
        let edge_width = (width + 2).min(rect.width - edge_dx);
        let edge_dy = dy.saturating_sub(1);
        paint_patch(image, rect, edge_dx, edge_dy, edge_width, 2, PILE_EDGE);
    }
    for &(dy, dx, width) in PILE_ROWS {
        paint_patch(image, rect, dx, dy, width, 1, PILE_BODY);
    }
    // 高光：左上受光面的两笔。
    paint_patch(image, rect, 6, 10, 2, 2, PILE_HIGHLIGHT);
    paint_patch(image, rect, 9, 12, 2, 1, PILE_HIGHLIGHT);
    // 两粒暗色碎点：让这一团读成「一堆东西」而不是「一块石头」。
    paint_patch(image, rect, 4, 13, 1, 1, PILE_EDGE);
    paint_patch(image, rect, 11, 13, 1, 1, PILE_EDGE);
}

/// 画 `furniture_placed`：通用的「这一格立着一件家具」记号。
///
/// # 为什么需要一张**通用**记号
///
/// 引擎绝不能按物品 id 分支决定家具画成什么样（那是把内容特例焊进
/// 引擎）。渲染层的规矩是：先按内容自己的完整命名空间 ID 去图集里查
/// 一张同名贴图（例如 `lostland:forge` → [`decorate_forge`] 生成的那
/// 张），查不到就退化到这一张通用记号。这张图因此是**兜底**，它必须
/// 对任何一件家具都说得通——所以画的是「一个立着的、有上表面的箱体」
/// 这种没有具体所指的形状，而不是任何一种具体家具。
pub(crate) fn decorate_furniture_placed(image: &mut RgbaImage, rect: EntryRect) {
    // 箱体外框（暗边）与内胆（主体色）。
    paint_patch(image, rect, 2, 4, 12, 12, FURNITURE_EDGE);
    paint_patch(image, rect, 3, 5, 10, 10, FURNITURE_BODY);
    // 上表面：比箱体略宽、颜色更亮，读成「这东西是立起来的，有个顶」。
    paint_patch(image, rect, 1, 2, 14, 3, FURNITURE_EDGE);
    paint_patch(image, rect, 2, 3, 12, 1, FURNITURE_TOP);
    // 正面中缝：两扇门/两个抽屉的暗示，纯粹让 10×10 的内胆不至于是
    // 一块死板的纯色。
    paint_patch(image, rect, 7, 6, 2, 8, FURNITURE_EDGE);
}

/// 画 `forge`：**内容自己声明的**那一张家具贴图。
///
/// 名字必须与 `mods/lostland/items.json5` 里那件家具的本地名一致
/// （`lostland:forge` → 本地名 `forge`）——`ll_mod::asset_vfs` 把清单
/// 条目名与所属命名空间拼成图集条目名（见其
/// `ResolvedSprite::atlas_name` 文档），渲染层拿内容的完整 ID 当查找键
/// 就能查到这一张。**这条对齐是靠约定，不是靠引擎里的分支**：本函数
/// 只是「本体这份内容顺手也画了张占位图」，把这张图删掉，锻炉会自动
/// 退回 [`decorate_furniture_placed`] 那张通用记号，引擎一行都不用改。
pub(crate) fn decorate_forge(image: &mut RgbaImage, rect: EntryRect) {
    // 底座
    paint_patch(image, rect, 1, 13, 14, 3, FORGE_DARK);
    // 炉身
    paint_patch(image, rect, 2, 5, 12, 8, FORGE_STONE);
    // 炉口与炉心
    paint_patch(image, rect, 4, 8, 7, 4, FORGE_EMBER);
    paint_patch(image, rect, 6, 9, 3, 2, FORGE_CORE);
    // 烟囱
    paint_patch(image, rect, 11, 1, 3, 5, FORGE_DARK);
    paint_patch(image, rect, 11, 3, 3, 2, FORGE_STONE);
}

/// 画 `npc_idle_0`：通用 NPC 记号。
///
/// 与玩家精灵同样是 16×24、同一档 `pivot`/`footprint`（见
/// `assets/atlas/placeholder.json`），因此站位规则与玩家完全一致——脚
/// 落在格子里、头探出格子顶部（见 `ll_render::sprite::sprite_draw_position`
/// 文档）。区分靠两处：主体色（紫红 vs 玩家钢蓝）与胸口标志形状
/// （菱形 vs 玩家的十字、boss 的大菱形位置不同）。
///
/// 与家具同理，这张也是**兜底**：渲染层先拿这个 NPC 的种族完整 ID
/// （例如 `lostland:human`）去查同名贴图，查不到才用这一张。种族因此
/// 可以在自己的 mod 里带一张 `assets/sprites/<种族本地名>.png` 直接
/// 生效，不需要动引擎。
pub(crate) fn decorate_npc(image: &mut RgbaImage, rect: EntryRect) {
    // 躯干：不铺满整张画布，留出四周透明——16×24 里画一个 10 宽的
    // 身子，比铺满一整块更像「一个人站在那」。
    paint_patch(image, rect, 3, 6, 10, 14, NPC_BODY);
    // 头
    paint_patch(image, rect, 5, 1, 6, 6, NPC_BODY);
    paint_patch(image, rect, 6, 2, 4, 3, NPC_MARK);
    // 胸口菱形标志
    paint_diamond(image, rect, 8, 12, 3, NPC_MARK);
    // 双脚
    paint_patch(image, rect, 3, 20, 4, 4, NPC_FOOT);
    paint_patch(image, rect, 9, 20, 4, 4, NPC_FOOT);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    const TILE: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };
    const NPC_RECT: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 24,
    };

    /// 这张图里有多少个像素完全透明。
    fn transparent_count(image: &RgbaImage) -> usize {
        image.pixels().filter(|p| p.0[3] == 0).count()
    }

    #[test]
    fn 地面物品堆四角留透明让地形透出来() {
        // Arrange
        let mut image = RgbaImage::new(16, 16);

        // Act
        decorate_ground_pile(&mut image, TILE);

        // Assert：四角必须仍是透明的——铺满不透明底色会把这一堆读成
        // 「地形变了」，见模块文档「底色留空，不是疏漏」。
        for corner in [(0, 0), (15, 0), (0, 15), (15, 15)] {
            assert_eq!(
                image.get_pixel(corner.0, corner.1).0[3],
                0,
                "角落 {corner:?} 应当透明"
            );
        }
    }

    #[test]
    fn 地面物品堆确实画了东西而不是一张空图() {
        // Arrange
        let mut image = RgbaImage::new(16, 16);

        // Act
        decorate_ground_pile(&mut image, TILE);

        // Assert：不透明像素占比落在「看得见但不糊住整格」的区间内。
        let opaque = 16 * 16 - transparent_count(&image);
        assert!(
            (40..=200).contains(&opaque),
            "不透明像素 {opaque} 个，既要看得见也不该盖满整格"
        );
    }

    #[test]
    fn 三种记号的主体色互不相同() {
        // Arrange & Act & Assert：颜色是三者最基本的区分依据，与
        // `sprite.rs` 的 `玩家与boss主体色不同` 同一条判据。
        assert_ne!(PILE_BODY, FURNITURE_BODY);
        assert_ne!(PILE_BODY, NPC_BODY);
        assert_ne!(FURNITURE_BODY, NPC_BODY);
    }

    #[test]
    fn npc主体色与玩家和boss都不同() {
        // NPC 站在玩家旁边必须一眼分得开，见模块文档配色一节。
        // Arrange & Act & Assert
        assert_ne!(NPC_BODY, (70, 130, 180));
        assert_ne!(NPC_BODY, (180, 40, 40));
    }

    #[test]
    fn npc标志色不是玩家和boss那款暖金() {
        // Arrange & Act & Assert
        assert_ne!(NPC_MARK, (255, 220, 120));
    }

    #[test]
    fn npc头顶留透明不占满整张画布() {
        // Arrange
        let mut image = RgbaImage::new(16, 24);

        // Act
        decorate_npc(&mut image, NPC_RECT);

        // Assert：左上角 (0,0) 在身子之外，应当透明。
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn 锻炉炉口画的是火色不是石色() {
        // Arrange
        let mut image = RgbaImage::new(16, 16);

        // Act
        decorate_forge(&mut image, TILE);

        // Assert：炉口中心 (7, 9) 落在炉心那一块里。
        assert_eq!(
            *image.get_pixel(7, 9),
            Rgba([FORGE_CORE.0, FORGE_CORE.1, FORGE_CORE.2, 255])
        );
    }

    #[test]
    fn 锻炉与通用家具记号在同一像素上颜色不同() {
        // 内容自带贴图与兜底记号必须一眼分得开，否则「查到了专属图」
        // 与「退回了通用记号」在画面上无从分辨，本批的接线也就无法
        // 被肉眼验收。
        // Arrange
        let mut forge = RgbaImage::new(16, 16);
        let mut generic = RgbaImage::new(16, 16);

        // Act
        decorate_forge(&mut forge, TILE);
        decorate_furniture_placed(&mut generic, TILE);

        // Assert
        assert_ne!(*forge.get_pixel(7, 9), *generic.get_pixel(7, 9));
    }
}
