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
fn fill_rect(image: &mut RgbaImage, rect: EntryRect, color: (u8, u8, u8)) {
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
fn paint_patch(
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
    paint_patch(image, rect, 7, 11, 2, 6, color);
    paint_patch(image, rect, 5, 13, 6, 2, color);
}

/// 画 `hero_idle_0`：蓝色主体 + 头部方块标记（位置与既有像素一致）+
/// 新增的胸口十字标志。
pub(crate) fn decorate_hero_idle(image: &mut RgbaImage, rect: EntryRect) {
    fill_rect(image, rect, HERO_BODY);
    paint_patch(image, rect, 6, 2, 4, 4, HERO_MARK);
    paint_chest_cross(image, rect, HERO_MARK);
}

/// 画一帧 `hero_walk_*`：蓝色主体 + 顶部整行标记 + 左右交替的脚部标记
/// （`foot_dx` 由调用方传 2 或 10，与既有两帧的既有位置一致）+ 新增的
/// 胸口十字标志。两帧行走姿态共用这一个函数，避免同一份绘制逻辑在
/// `main.rs` 里被抄两遍。
pub(crate) fn decorate_hero_walk(image: &mut RgbaImage, rect: EntryRect, foot_dx: u32) {
    fill_rect(image, rect, HERO_BODY);
    paint_patch(image, rect, 0, 0, rect.width, 1, HERO_MARK);
    paint_patch(image, rect, foot_dx, 20, 4, 4, HERO_FOOT_MARK);
    paint_chest_cross(image, rect, HERO_MARK);
}

/// 画 `boss_idle_0`：红色主体 + 面甲（位置与既有像素一致）+ 新增的
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
fn paint_diamond(
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
    fn 两帧行走姿态的脚部标记落在不同列() {
        // Arrange
        let mut left = RgbaImage::new(16, 24);
        let mut right = RgbaImage::new(16, 24);

        // Act
        decorate_hero_walk(&mut left, HERO_RECT, 2);
        decorate_hero_walk(&mut right, HERO_RECT, 10);

        // Assert：左脚帧在 x=2 处是脚部标记色，右脚帧在同一位置不是。
        let foot = Rgba([HERO_FOOT_MARK.0, HERO_FOOT_MARK.1, HERO_FOOT_MARK.2, 255]);
        assert_eq!(*left.get_pixel(2, 20), foot);
        assert_ne!(*right.get_pixel(2, 20), foot);
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
