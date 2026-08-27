//! 角色贴图标志：给玩家（`hero_*`）与 boss（`boss_idle_0`）加简单标志，
//! 让两者在测试截图里一眼可辨，而不是像之前那样只凭矩形位置猜测
//! 「这块蓝色是玩家、这块红色是 boss」。
//!
//! 玩家与 boss 的主体色（蓝色 `hero_*`、红色 `boss_idle_0`）沿用既有
//! 取值，本模块只新增标志图案，不改主体色：保留主体色是为了不破坏
//! 「玩家=蓝、boss=红」这条已经跑通全部验收 demo 的既有视觉约定。

use crate::EntryRect;
use image::{Rgba, RgbaImage};

/// 玩家精灵主体色（钢蓝），取自既有像素，未改动。
const HERO_BODY: (u8, u8, u8) = (70, 130, 180);
/// 玩家标志色（暖金），取自既有像素，未改动——蓝色（色相约 207°）与
/// 金色（色相约 45°）相差约 160°，接近互补色，本就是「一眼分清主体与
/// 标志」的经典配色，无需重新设计。
const HERO_MARK: (u8, u8, u8) = (255, 220, 120);
/// 玩家行走帧脚部标记色（深藏青），取自既有像素，未改动。
const HERO_FOOT_MARK: (u8, u8, u8) = (30, 30, 60);

/// boss 主体色（暗红），取自既有像素，未改动。
const BOSS_BODY: (u8, u8, u8) = (180, 40, 40);
/// boss 面甲标志色（暖金），取自既有像素，未改动。
const BOSS_VISOR: (u8, u8, u8) = (255, 220, 120);
/// boss 眼部暗色（新增）。
const BOSS_EYE: (u8, u8, u8) = (20, 20, 20);
/// boss 胸口警示标志色（青色，新增）。红色（色相约 0°）与青色（色相约
/// 180°）恰是互补色——刻意不用玩家同款的暖金色，让 boss 在只能看清
/// 「有没有一块高对比标志」而看不清细节的场景（远景、缩略图）里，也能
/// 与玩家区分开，而不只是靠主体色的红蓝之别。
const BOSS_CHEST_MARK: (u8, u8, u8) = (60, 220, 210);

/// 在 `rect` 描述的矩形内填一种纯色，是本模块全部绘制函数的公共底子。
pub(crate) fn fill_rect(image: &mut RgbaImage, rect: EntryRect, color: (u8, u8, u8)) {
    for local_y in 0..rect.height {
        for local_x in 0..rect.width {
            image.put_pixel(
                rect.x + local_x,
                rect.y + local_y,
                Rgba([color.0, color.1, color.2, 255]),
            );
        }
    }
}

/// 在精灵矩形内、相对精灵左上角偏移 `(dx, dy)` 处画一块 `w×h` 的纯色
/// 小块。
pub(crate) fn paint_patch(
    image: &mut RgbaImage,
    origin: EntryRect,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
    color: (u8, u8, u8),
) {
    let patch = EntryRect {
        x: origin.x + dx,
        y: origin.y + dy,
        width: w,
        height: h,
    };
    fill_rect(image, patch, color);
}

/// 在精灵胸口画一个「十」字标志：竖条 2px 宽、6px 高，横条 6px 宽、
/// 2px 高，交叉居中。选十字而非更复杂的图案，是因为它在只有 16 像素
/// 宽的画布上仍能保持轴对称，不会因为画布太窄而变形走样。
fn paint_chest_cross(image: &mut RgbaImage, rect: EntryRect, color: (u8, u8, u8)) {
    paint_chest_cross_shifted(image, rect, 0, color);
}

/// [`paint_chest_cross`] 的通用版本，纵向额外偏移 `dy_shift`（可为负）。
/// 待机呼吸帧（[`decorate_hero_idle_breath`]）与行走的「过腿」帧
/// （[`decorate_hero_walk`] 的 `passing = true`）都要在原十字位置上下
/// 挪 1 像素模拟胸腔起伏，抽出这个通用版本避免两处各抄一份坐标算术。
fn paint_chest_cross_shifted(
    image: &mut RgbaImage,
    rect: EntryRect,
    dy_shift: i32,
    color: (u8, u8, u8),
) {
    let vertical_dy = (11 + dy_shift).max(0) as u32;
    let horizontal_dy = (13 + dy_shift).max(0) as u32;
    paint_patch(image, rect, 7, vertical_dy, 2, 6, color);
    paint_patch(image, rect, 5, horizontal_dy, 6, 2, color);
}

/// 画 `hero_idle_0`：蓝色主体 + 头部方块标记（位置与既有像素一致）+
/// 新增的胸口十字标志。
pub(crate) fn decorate_hero_idle(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, HERO_BODY);
    paint_patch(image, rect, 6, 2, 4, 4, HERO_MARK);
    paint_chest_cross(image, rect, HERO_MARK);
}

/// 画 `hero_idle_1`：待机呼吸动画的第二帧，与 `hero_idle_0` 差两处
/// ——头部标记纵向 1 像素（`dy` 从 2 挪到 1）、胸口十字纵向同向挪 1
/// 像素（`dy_shift = -1`），一起模拟吸气时胸腔/头部整体略微抬起。
///
/// 此前只挪头部标记一处，两帧只差 8 个像素（384 像素画布的 2%），呼吸
/// 效果基本不可见；这次补上胸口十字同向的 1 像素挪动，实测把差异抬到
/// 20 像素，仍然明显低于行走相邻帧的差异（16~26 像素，见
/// `decorate_hero_walk` 文档），不会喧宾夺主。幅度依旧刻意压到最小：
/// 呼吸动画若挪动太多像素，在像素风画面
/// 里会读成「抖动」而不是「起伏」——项目所有者明确要求「不要做成明显
/// 的抖动」，1 像素是这张 16×24 画布上能表达出可见变化的最小单位。
pub(crate) fn decorate_hero_idle_breath(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, HERO_BODY);
    paint_patch(image, rect, 6, 1, 4, 4, HERO_MARK);
    paint_chest_cross_shifted(image, rect, -1, HERO_MARK);
}

/// 画一帧 `hero_walk_*`：蓝色主体 + 顶部整行标记（肩线，固定不动）+
/// 沿水平方向挪动的脚部标记 + 胸口十字标志。六帧共用这一个函数，靠
/// 两个参数区分姿态，避免同一份绘制逻辑在 `main.rs` 里被抄六遍：
///
/// - `foot_dx`：脚部标记的水平位置（0..=12，标记本身 4px 宽）。六帧
///   按 2 → 4 → 7 → 10 → 8 → 5 → （循环回 2）取值，每相邻两帧只挪 2~3
///   像素——标记宽度是 4px，位移小于宽度时新旧位置有重叠，读起来是
///   「脚在挪」而不是「换了张完全不同的图」。落点 2 与 10 是接触地面
///   的极值姿态（`hero_walk_0`/`hero_walk_1` 沿用的既有像素，未改动），
///   中间值是脚正在摆动过程中的过渡姿态。
/// - `passing`：是否处于「过腿」相位（脚摆到接近身体中线、尚未落地）。
///   为真时脚标记纵向抬高 1 像素（`dy` 从 20 变 19，模拟脚离地）——
///   「挪腿」这一种过渡手法的最小实现。
///
///   刻意不让 `passing` 再顺带牵动胸口十字（呼吸帧才用
///   [`paint_chest_cross_shifted`] 表达身体起伏，见
///   [`decorate_hero_idle_breath`]）：脚部水平位移与纵向抬高已经是
///   两个同时变化的量，若再叠加胸口纵向偏移，会让「passing 翻转」的
///   那几对相邻帧一步变化过大（实测会超过原先两帧直接互跳的 32 像素
///   基准）。胸口十字在六帧里保持不动，把「变化幅度」完全交给脚部，
///   使全部相邻帧对的像素差异都不超过 26 像素——见 `main.rs` 里的
///   `六帧行走循环相邻帧像素差异全部小于两帧方案的直接互跳` 测试。
pub(crate) fn decorate_hero_walk(
    image: &mut RgbaImage,
    rect: EntryRect,
    foot_dx: u32,
    passing: bool,
) {
    fill_rect(image, rect, HERO_BODY);
    paint_patch(image, rect, 0, 0, rect.width, 1, HERO_MARK);
    let foot_dy = if passing { 19 } else { 20 };
    paint_patch(image, rect, foot_dx, foot_dy, 4, 4, HERO_FOOT_MARK);
    paint_chest_cross(image, rect, HERO_MARK);
}

/// 画 `boss_idle_0`。
///
/// # 这张图现在的身份是测试夹具，不是待接线的内容
///
/// `ll-game` 本体二进制一处都不消费 `boss_idle_0`，只有
/// `crates/ll-render/examples/p1_acceptance` 与
/// `crates/ll-sim/examples/p3_acceptance` 在用（后者的冻结截图基准里
/// 真的画着它，它还是唯一一个 2×2 占地的条目，p3 靠它验「footprint 从
/// 图集条目读取」）。项目所有者的裁定是「现在应该不太需要 boss 这
/// 东西」——处置是**留图但不再当作待办**，理由见
/// `assets/atlas/README.md` 的同名一节。
///
/// 红色主体 + 面甲（位置与既有像素一致）+ 新增的
/// 面甲内暗色眼部 + 新增的胸口菱形警示标志。boss 的 2×2 占地格让它的
/// 画布是玩家的四倍面积，标志因此也画得比玩家的十字更大、更居中——
/// 「更醒目」直接体现在标志本身的像素占比上，不是靠加更多颜色。
pub(crate) fn decorate_boss(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, BOSS_BODY);
    paint_patch(image, rect, 12, 2, 8, 6, BOSS_VISOR);
    paint_patch(image, rect, 14, 4, 1, 2, BOSS_EYE);
    paint_patch(image, rect, 17, 4, 1, 2, BOSS_EYE);
    paint_diamond(image, rect, 16, 26, 4, BOSS_CHEST_MARK);
}

/// 以 `(center_dx, center_dy)`（相对精灵左上角）为中心、`radius` 为半径
/// 画一个曼哈顿距离菱形——比矩形更能读出「这是个标志，不是身体轮廓的
/// 一部分」，同时仍然是硬边缘的整像素填色，不引入任何抗锯齿或渐变。
pub(crate) fn paint_diamond(
    image: &mut RgbaImage,
    rect: EntryRect,
    center_dx: i32,
    center_dy: i32,
    radius: i32,
    color: (u8, u8, u8),
) {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let x = (rect.x as i32 + center_dx + dx) as u32;
            let y = (rect.y as i32 + center_dy + dy) as u32;
            image.put_pixel(x, y, Rgba([color.0, color.1, color.2, 255]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERO_RECT: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 24,
    };
    const BOSS_RECT: EntryRect = EntryRect {
        x: 16,
        y: 0,
        width: 32,
        height: 48,
    };

    #[test]
    fn 玩家主体色像素占比仍是压倒多数() {
        // Arrange
        let mut image = RgbaImage::new(16, 24);

        // Act
        decorate_hero_idle(&mut image, HERO_RECT);
        let body = Rgba([HERO_BODY.0, HERO_BODY.1, HERO_BODY.2, 255]);
        let body_count = image.pixels().filter(|&&p| p == body).count();

        // Assert：头部标记 16 像素 + 十字标志约 20 像素，主体仍占绝大多数。
        assert!(body_count as f32 / (16.0 * 24.0) > 0.8);
    }

    #[test]
    fn 玩家与boss主体色不同() {
        // Arrange & Act & Assert：颜色本身就是二者最基本的区分依据。
        assert_ne!(HERO_BODY, BOSS_BODY);
    }

    #[test]
    fn boss胸口标志色与玩家标志色不同() {
        // boss 刻意不复用玩家的暖金色标志，见 BOSS_CHEST_MARK 文档。
        // Arrange & Act & Assert
        assert_ne!(BOSS_CHEST_MARK, HERO_MARK);
    }

    #[test]
    fn 待机呼吸帧头部标记比待机帧高一像素() {
        // Arrange
        let mut idle = RgbaImage::new(16, 24);
        let mut breath = RgbaImage::new(16, 24);

        // Act
        decorate_hero_idle(&mut idle, HERO_RECT);
        decorate_hero_idle_breath(&mut breath, HERO_RECT);

        // Assert：呼吸帧在头部标记顶行（y=1）已经是标记色，待机帧同一
        // 行仍是主体色——两帧只差这一行像素，幅度克制在 1 像素，不是
        // 明显的抖动。
        let mark = Rgba([HERO_MARK.0, HERO_MARK.1, HERO_MARK.2, 255]);
        assert_eq!(*breath.get_pixel(7, 1), mark);
        assert_ne!(*idle.get_pixel(7, 1), mark);
    }

    #[test]
    fn 两个接触帧的脚部标记落在不同列() {
        // Arrange：hero_walk_0/hero_walk_1 沿用的两个接触极值姿态。
        let mut left = RgbaImage::new(16, 24);
        let mut right = RgbaImage::new(16, 24);

        // Act
        decorate_hero_walk(&mut left, HERO_RECT, 2, false);
        decorate_hero_walk(&mut right, HERO_RECT, 10, false);

        // Assert：左脚帧在 x=2 处是脚部标记色，右脚帧在同一位置不是。
        let foot = Rgba([HERO_FOOT_MARK.0, HERO_FOOT_MARK.1, HERO_FOOT_MARK.2, 255]);
        assert_eq!(*left.get_pixel(2, 20), foot);
        assert_ne!(*right.get_pixel(2, 20), foot);
    }

    #[test]
    fn 过腿帧的脚部标记比接触帧高一像素() {
        // Arrange：同一水平位置，只切换 passing。
        let mut contact = RgbaImage::new(16, 24);
        let mut passing = RgbaImage::new(16, 24);

        // Act
        decorate_hero_walk(&mut contact, HERO_RECT, 7, false);
        decorate_hero_walk(&mut passing, HERO_RECT, 7, true);

        // Assert：过腿帧脚部标记顶行（y=19）已经是标记色（脚离地抬高
        // 1 像素），接触帧同一行仍是主体色。
        let foot = Rgba([HERO_FOOT_MARK.0, HERO_FOOT_MARK.1, HERO_FOOT_MARK.2, 255]);
        assert_eq!(*passing.get_pixel(7, 19), foot);
        assert_ne!(*contact.get_pixel(7, 19), foot);
    }

    #[test]
    fn 行走帧的胸口十字不随passing挪动() {
        // Arrange：胸口十字在行走的六帧里保持不动，只有脚部标记随
        // passing 变化——理由见 decorate_hero_walk 文档「刻意不让
        // passing 再顺带牵动胸口十字」一节：把变化幅度全部交给脚部，
        // 才能让 passing 翻转的相邻帧对差异保持在可控范围内。
        let mut contact = RgbaImage::new(16, 24);
        let mut passing = RgbaImage::new(16, 24);

        // Act
        decorate_hero_walk(&mut contact, HERO_RECT, 7, false);
        decorate_hero_walk(&mut passing, HERO_RECT, 7, true);

        // Assert：胸口十字竖条顶行（y=11）两帧都是标记色，未随
        // passing 挪动。
        let mark = Rgba([HERO_MARK.0, HERO_MARK.1, HERO_MARK.2, 255]);
        assert_eq!(*contact.get_pixel(7, 11), mark);
        assert_eq!(*passing.get_pixel(7, 11), mark);
    }

    #[test]
    fn boss胸口菱形标志确实被画出() {
        // Arrange：画布宽度取到 BOSS_RECT 右边界（16 + 32 = 48），
        // 因为 BOSS_RECT.x 是它在真实图集里的坐标（16），不是 0。
        let mut image = RgbaImage::new(48, 48);

        // Act
        decorate_boss(&mut image, BOSS_RECT);
        let mark = Rgba([BOSS_CHEST_MARK.0, BOSS_CHEST_MARK.1, BOSS_CHEST_MARK.2, 255]);

        // Assert：菱形中心相对 boss 矩形是 (16, 26)，矩形左上角在画布
        // 上是 (16, 0)，故绝对坐标是 (32, 26)。
        assert_eq!(*image.get_pixel(32, 26), mark);
    }
}
