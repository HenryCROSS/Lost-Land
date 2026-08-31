//! 玩家（`hero_*`）与 boss（`boss_idle_0`）的角色贴图。
//!
//! # 玩家贴图重画（所有者：「目前的贴图有点丑了」）
//!
//! 重画之前，`hero_*` 是**一整块 16×24 的实心钢蓝矩形**，上面压一个
//! 金色头部方块与一个金十字：没有轮廓、没有明暗、四角全部不透明。它在
//! `crates/ll-game/tests/visual/surface_preview.png` 里与旁边的 NPC 并排
//! 一摆就很刺眼——NPC 已经是有肩、有头、有耳朵、四周透明的人形，玩家
//! 还是一块砖。
//!
//! **重画的是同八张，不新增第二套**（`hero_idle_0/1`、`hero_walk_0..5`
//! 文件名与语义一个字没动，动画状态机与三处剪辑定义不受影响）。四条
//! 可核实的改进，每条都有一条断言盯着：
//!
//! | 改进 | 判据 |
//! | --- | --- |
//! | 有轮廓，不再是实心矩形 | 四角透明，不透明像素占比落在 `0.35..0.80`（此前恒为 1.00） |
//! | 一圈近黑描边把人形从任何底色上切出来 | 每个「贴着透明的」不透明像素都必须是描边色 |
//! | 三档明暗（受光/主体/背光），不是一片平色 | 三个钢蓝档在同一张图里同时出现 |
//! | 与任何地形底色都拉得开 | 同一张图里同时存在极暗与极亮像素（最大亮度差 ≥ 140） |
//!
//! 最后一条与 `ll_ui::hud::world_map::PLAYER_MARKER_COLOR` 那条「要在深
//! 蓝的海、深绿的林、灰白的雪山上同样一眼可见」是同一个道理：**单一
//! 颜色满足不了**，底色一多总有一种跟它接近；同时带极暗描边与极亮金饰
//! 的图形，任何底色要么与暗的拉得开、要么与亮的拉得开。
//!
//! # 主体色与标志色沿用既有取值
//!
//! 钢蓝 `(70,130,180)` 与暖金 `(255,220,120)` 一个字没改：「玩家=蓝、
//! boss=红」这条视觉约定当年跑通过全部验收 demo，`world_marks.rs` 的
//! `npc主体色与玩家和boss都不同` 还在拿钢蓝当基准。重画换的是**结构**
//! （轮廓、明暗、人形），不是身份色。
//!
//! # 与 NPC 贴图共用同一条地平线
//!
//! 脚底行号取 21（与 `npc.rs` 的 `FEET_TOP` 相同），锚点 `pivot.y = 24`
//! 也与种族身子一致。玩家和 NPC 站在同一格里时脚踩在同一条线上——此前
//! 那块实心矩形的「脚」其实是画布底边，看着像浮着。

use crate::EntryRect;
use image::{Rgba, RgbaImage};

/// 玩家精灵主体色（钢蓝），取自既有像素，未改动。
const HERO_BODY: (u8, u8, u8) = (70, 130, 180);
/// 玩家标志色（暖金），取自既有像素，未改动——蓝色（色相约 207°）与
/// 金色（色相约 45°）相差约 160°，接近互补色，本就是「一眼分清主体与
/// 标志」的经典配色，无需重新设计。
const HERO_MARK: (u8, u8, u8) = (255, 220, 120);
/// 玩家靴子色（深藏青），取自既有像素，未改动——重画之前它是行走帧的
/// 「脚部标记」，现在它就是靴子本身，语义与像素都对得上了。
const HERO_FOOT_MARK: (u8, u8, u8) = (30, 30, 60);

/// 玩家受光面（左上打光）。与 [`HERO_SHADE`] 一起把躯干从一片平色变成
/// 三档明暗——像素画里「体积」全靠这三档，不靠渐变。
const HERO_LIGHT: (u8, u8, u8) = (126, 186, 228);
/// 玩家背光面。
const HERO_SHADE: (u8, u8, u8) = (40, 82, 126);
/// 玩家描边色（近黑藏青）。
///
/// **它是这张图能压在任何地形上都看得清的那一半**（另一半是暖金）：
/// 描边把人形的轮廓从底色里切出来，无论底下是浅沙、深海还是灰白的雪。
const HERO_OUTLINE: (u8, u8, u8) = (16, 22, 42);
/// 金饰暗部（盔的护颊、腰带下沿）。暖金只有一档的话，头盔与腰带会读成
/// 两块贴纸而不是两件金属。
const HERO_MARK_DARK: (u8, u8, u8) = (182, 142, 54);
/// 玩家肤色（脸与双手）。
const HERO_SKIN: (u8, u8, u8) = (236, 196, 158);
/// 玩家肤色暗面（脸的右侧一列）。
const HERO_SKIN_SHADE: (u8, u8, u8) = (188, 144, 108);
/// 眼睛。
const HERO_EYE: (u8, u8, u8) = (24, 28, 44);
/// **后腿**的裤色——比前腿再暗一档。
///
/// 两条腿必须有前后之分，不只是为了好看：`hero_walk_0` 与
/// `hero_walk_1` 的两条腿位置集合完全一致（镜像摆动），同色的话两帧会
/// 画成**逐像素相同的两张图**，行走动画当场退化成静止。见
/// [`draw_hero`] 里画腿那一段的注释。
const HERO_LEG_BACK: (u8, u8, u8) = (28, 58, 92);
/// **后腿**的靴色，理由同 [`HERO_LEG_BACK`]。
const HERO_BOOT_BACK: (u8, u8, u8) = (20, 20, 42);

/// 玩家贴图宽度（与 `assets/atlas/placeholder.json` 里 `hero_*` 的
/// `rect` 一致）。
const HERO_W: u32 = 16;
/// 玩家贴图高度，理由同 [`HERO_W`]。
const HERO_H: u32 = 24;
/// 脚底那一档的行号——**与 `npc.rs` 的 `FEET_TOP` 相同**，玩家和 NPC
/// 站在同一格里时必须踩在同一条地平线上。
const HERO_FEET_TOP: u32 = 21;
/// 腿从第几行开始。
const HERO_LEG_TOP: u32 = 17;
/// 待机姿态下躯干顶端的行号。呼吸帧在此基础上抬 1 行（胸腔张开），
/// 见 [`decorate_hero_idle_breath`]。
const HERO_TORSO_TOP: u32 = 9;
/// 躯干宽度。
const HERO_TORSO_W: u32 = 8;
/// 躯干左边界列号——由画布宽居中算出，不写死。
const HERO_TORSO_X: u32 = (HERO_W - HERO_TORSO_W) / 2;
/// 头宽。
const HERO_HEAD_W: u32 = 6;
/// 头左边界列号，同样居中算出。
const HERO_HEAD_X: u32 = (HERO_W - HERO_HEAD_W) / 2;
/// 头高。
const HERO_HEAD_H: u32 = 6;
/// 手臂宽度（躯干两侧各一条）。
const HERO_ARM_W: u32 = 2;
/// 一条腿的宽度。
const HERO_LEG_W: u32 = 3;
/// 腰带上沿的行号。**比手高三行**——中间那一行躯干色是把「腰带 + 双手」
/// 切成两条的那一刀，见 [`draw_hero`] 里画腰带那一段。
const HERO_BELT_TOP: u32 = HERO_LEG_TOP - 3;
/// 一只靴子的宽度——比腿宽 1 像素，读成「脚尖朝前」。
const HERO_BOOT_W: u32 = 4;

/// 一帧玩家的姿态。三个绘制入口（待机、呼吸、行走）**共用同一段绘制
/// 代码**，差异全部收在这个结构体里——这正是 ADR 0021 说的「抽象的
/// 正当理由是有算法要共用」：八张图之间没有任何画法差异，只有四个数
/// 的差异。
#[derive(Debug, Clone, Copy)]
struct HeroPose {
    /// 胸腔抬起的行数（0 = 待机/行走，1 = 吸气）。上半身整体跟着抬，
    /// 腿与脚不动。
    chest_rise: u32,
    /// 前腿左边界列号。
    lead_leg_dx: u32,
    /// 后腿左边界列号。
    trail_leg_dx: u32,
    /// 前脚是否离地一像素（过腿相位）。
    lead_lifted: bool,
}

/// 待机与行走共用的双腿站位：躯干两侧各一条，不跨步。
const HERO_STANCE_LEGS: (u32, u32) = (HERO_TORSO_X, HERO_TORSO_X + HERO_TORSO_W - HERO_LEG_W);

/// 后腿列号由前腿列号镜像得出：两条腿绕画布中线对称摆动。
///
/// `foot_dx` 的取值域是 `2..=10`（见 `crate::draw_entry` 的六帧表），
/// 镜像轴取 6（`(2 + 10) / 2`），因此后腿列号 = `12 - foot_dx`，同样落在
/// `2..=10`。**不写成两个独立参数**：写成两个的话，六帧表里就要手填十二
/// 个数，其中任何一个填错都会画出一条长短腿，而那种错在 16 像素宽的图上
/// 极难看出来。
fn mirrored_trail_leg(foot_dx: u32) -> u32 {
    12 - foot_dx
}

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

/// 在胸口画一枚小十字纹章：竖条 2px 宽 3px 高，横条 4px 宽 2px 高。
///
/// 选十字而非更复杂的图案，是因为它在只有 8 像素宽的躯干上仍能保持轴
/// 对称，不会因为画布太窄而变形走样。`chest_top` 是躯干顶端行号，纹章
/// 跟着躯干走——呼吸帧胸腔抬 1 行，纹章也抬 1 行。
fn paint_chest_crest(image: &mut RgbaImage, rect: EntryRect, chest_top: u32) {
    paint_patch(image, rect, 7, chest_top + 1, 2, 3, HERO_MARK);
    paint_patch(image, rect, 6, chest_top + 2, 4, 2, HERO_MARK);
    debug_assert!(
        chest_top + 4 <= HERO_BELT_TOP,
        "纹章与腰带之间必须留出一行躯干色，否则两块暖金会连成一个锚形"
    );
}

/// 画一帧玩家：腿 → 靴 → 躯干 → 手臂 → 头 → 盔 → 纹章 → 描边。
///
/// 八张 `hero_*` 全部走这一段代码，差异只来自 [`HeroPose`] 的四个数。
fn draw_hero(image: &mut RgbaImage, rect: EntryRect, pose: HeroPose) {
    let chest_top = HERO_TORSO_TOP - pose.chest_rise;
    let head_top = chest_top - HERO_HEAD_H;
    let torso_h = HERO_LEG_TOP - chest_top;

    // 双腿与靴子。**后腿先画、前腿后画**，前腿盖住后腿——这两条必须有
    // 前后之分，否则 `hero_walk_0`（前腿在 2、后腿在 10）与
    // `hero_walk_1`（前腿在 10、后腿在 2）会画出**逐像素相同的两张图**
    // （镜像摆动让两条腿的位置集合完全一致）。本次开发实测撞到过这个坑：
    // 那时两条腿同色，六帧循环的基准差异当场变成 0。
    for (leg_dx, lifted, leg_color, boot_color) in [
        (pose.trail_leg_dx, false, HERO_LEG_BACK, HERO_BOOT_BACK),
        (
            pose.lead_leg_dx,
            pose.lead_lifted,
            HERO_SHADE,
            HERO_FOOT_MARK,
        ),
    ] {
        let boot_top = if lifted {
            HERO_FEET_TOP - 1
        } else {
            HERO_FEET_TOP
        };
        paint_patch(
            image,
            rect,
            leg_dx,
            HERO_LEG_TOP,
            HERO_LEG_W,
            boot_top - HERO_LEG_TOP,
            leg_color,
        );
        paint_patch(
            image,
            rect,
            leg_dx,
            boot_top,
            HERO_BOOT_W,
            HERO_H - HERO_FEET_TOP,
            boot_color,
        );
    }

    // 躯干：主体色打底，左两列受光、右两列背光——三档明暗。
    paint_patch(
        image,
        rect,
        HERO_TORSO_X,
        chest_top,
        HERO_TORSO_W,
        torso_h,
        HERO_BODY,
    );
    paint_patch(image, rect, HERO_TORSO_X, chest_top, 2, torso_h, HERO_LIGHT);
    paint_patch(
        image,
        rect,
        HERO_TORSO_X + HERO_TORSO_W - 2,
        chest_top,
        2,
        torso_h,
        HERO_SHADE,
    );
    // 腰带：暖金一亮一暗两行，**与手之间留出一行躯干色**。
    //
    // 腰带与手贴在同一档高度时，浅肤色的左手 + 暖金腰带 + 浅肤色的右手
    // 会连成一条横贯全身的亮带（读起来像端着一个金托盘）。留出的这一行
    // 是把两者切开的那一刀，见下面画手臂那一段的注释。
    for (row, color) in [
        (HERO_BELT_TOP, HERO_MARK),
        (HERO_BELT_TOP + 1, HERO_MARK_DARK),
    ] {
        paint_patch(image, rect, HERO_TORSO_X, row, HERO_TORSO_W, 1, color);
    }

    // 双臂：袖子取背光色，末端两行是手（肤色）。
    //
    // **手必须落在腰带那两行之下**（`HERO_LEG_TOP` 起，垂到大腿外侧）。
    // 手与腰带同高时，浅肤色的左手 + 暖金腰带 + 浅肤色的右手在 16 像素宽
    // 的画布上连成一条横贯全身的亮带，读起来像「端着一个金托盘」——本次
    // 开发画出来第一版就是这样，看图才发现。垂到腰带之下既解决了这个
    // 问题，也更像人：手臂本来就比躯干长。
    let arm_top = chest_top + 1;
    let hand_top = HERO_LEG_TOP;
    debug_assert!(hand_top > HERO_BELT_TOP + 1, "手必须落在腰带之下");
    for arm_x in [HERO_TORSO_X - HERO_ARM_W, HERO_TORSO_X + HERO_TORSO_W] {
        paint_patch(
            image,
            rect,
            arm_x,
            arm_top,
            HERO_ARM_W,
            hand_top - arm_top,
            HERO_SHADE,
        );
        paint_patch(image, rect, arm_x, hand_top, HERO_ARM_W, 2, HERO_SKIN);
    }

    // 头：肤色，右侧一列压暗。
    paint_patch(
        image,
        rect,
        HERO_HEAD_X,
        head_top,
        HERO_HEAD_W,
        HERO_HEAD_H,
        HERO_SKIN,
    );
    paint_patch(
        image,
        rect,
        HERO_HEAD_X + HERO_HEAD_W - 1,
        head_top,
        1,
        HERO_HEAD_H,
        HERO_SKIN_SHADE,
    );
    // 头盔：顶两行暖金，左右各外扩一列当护颊（暗金）。
    paint_patch(
        image,
        rect,
        HERO_HEAD_X - 1,
        head_top,
        HERO_HEAD_W + 2,
        2,
        HERO_MARK,
    );
    for cheek_x in [HERO_HEAD_X - 1, HERO_HEAD_X + HERO_HEAD_W] {
        paint_patch(image, rect, cheek_x, head_top + 2, 1, 2, HERO_MARK_DARK);
    }
    // 眼睛。
    paint_patch(image, rect, HERO_HEAD_X + 1, head_top + 3, 1, 1, HERO_EYE);
    paint_patch(image, rect, HERO_HEAD_X + 4, head_top + 3, 1, 1, HERO_EYE);

    paint_chest_crest(image, rect, chest_top);
    outline_silhouette(image, rect, HERO_OUTLINE);
}

/// 给已经画好的人形描一圈边：**矩形内**每一个透明、且四邻里至少有一个
/// 不透明的像素，涂成 `color`。
///
/// # 为什么先快照再涂
///
/// 边涂边判会让描边自己长出第二圈（刚涂上的像素立刻算作「不透明的
/// 邻居」）。先把不透明掩码整份快照下来，再照着快照涂，描边恒定一像素宽。
///
/// # 为什么四邻不出 `rect`
///
/// 遗留共享画布 `assets/atlas/placeholder.png` 上条目是紧挨着摆的，
/// 越界一个像素就会把隔壁那张图涂脏——而那张画布是四张冻结基准的
/// 冻结基准。人形贴着矩形下沿（靴底就是最后一行）的地方因此没有描边，
/// 这是对的：那一行本来就在地面之下。
fn outline_silhouette(image: &mut RgbaImage, rect: EntryRect, color: (u8, u8, u8)) {
    let opaque: Vec<bool> = (0..rect.height)
        .flat_map(|dy| (0..rect.width).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| image.get_pixel(rect.x + dx, rect.y + dy).0[3] != 0)
        .collect();
    let at = |dx: u32, dy: u32| opaque[(dy * rect.width + dx) as usize];

    for dy in 0..rect.height {
        for dx in 0..rect.width {
            if at(dx, dy) {
                continue;
            }
            let touches_body = (dx > 0 && at(dx - 1, dy))
                || (dx + 1 < rect.width && at(dx + 1, dy))
                || (dy > 0 && at(dx, dy - 1))
                || (dy + 1 < rect.height && at(dx, dy + 1));
            if touches_body {
                paint_patch(image, rect, dx, dy, 1, 1, color);
            }
        }
    }
}

/// 画 `hero_idle_0`：站姿，双腿并在躯干两侧。
pub(crate) fn decorate_hero_idle(image: &mut RgbaImage, rect: EntryRect) {
    draw_hero(
        image,
        rect,
        HeroPose {
            chest_rise: 0,
            lead_leg_dx: HERO_STANCE_LEGS.0,
            trail_leg_dx: HERO_STANCE_LEGS.1,
            lead_lifted: false,
        },
    );
}

/// 画 `hero_idle_1`：待机呼吸动画的第二帧。
///
/// 与 `hero_idle_0` 的唯一差别是 `chest_rise = 1`——**胸腔张开一行**，
/// 头、手臂、纹章跟着抬一行，腿与脚一动不动。幅度刻意压到最小：呼吸
/// 动画若挪动太多像素，在像素风画面里会读成「抖动」而不是「起伏」，
/// 项目所有者明确要求「不要做成明显的抖动」，1 像素是这张 16×24 画布
/// 上能表达出可见变化的最小单位。
///
/// 重画之前这两帧靠「把头部方块和金十字各挪 1 像素」表达呼吸；现在
/// 挪的是**整个上半身**，因此差异比此前大，但仍然只有 1 像素的位移，
/// 读起来仍是起伏而不是抖动。
pub(crate) fn decorate_hero_idle_breath(image: &mut RgbaImage, rect: EntryRect) {
    draw_hero(
        image,
        rect,
        HeroPose {
            chest_rise: 1,
            lead_leg_dx: HERO_STANCE_LEGS.0,
            trail_leg_dx: HERO_STANCE_LEGS.1,
            lead_lifted: false,
        },
    );
}

/// 画一帧 `hero_walk_*`。六帧共用这一个函数，靠两个参数区分姿态，避免
/// 同一份绘制逻辑在 `main.rs` 里被抄六遍：
///
/// - `foot_dx`：**前腿**的水平位置（取值域 `2..=10`）。六帧按
///   2 → 4 → 7 → 10 → 8 → 5 → （循环回 2）取值，每相邻两帧只挪 2~3
///   像素——腿宽 3 像素，位移小于宽度时新旧位置有重叠，读起来是「腿在
///   迈」而不是「换了张完全不同的图」。后腿由 [`mirrored_trail_leg`]
///   镜像得出，两条腿绕画布中线对称摆动。
/// - `passing`：是否处于「过腿」相位（前脚摆到接近身体中线、尚未落地）。
///   为真时前脚整只抬高 1 像素。
///
/// 上半身在六帧里**完全不动**（`chest_rise` 恒 0）：把变化幅度全部交给
/// 腿，才能让每一对相邻帧的差异都小于 `hero_walk_0`/`hero_walk_1` 那次
/// 硬切——见 `main.rs` 里的
/// `六帧行走循环相邻帧像素差异全部小于两帧方案的直接互跳`。
pub(crate) fn decorate_hero_walk(
    image: &mut RgbaImage,
    rect: EntryRect,
    foot_dx: u32,
    passing: bool,
) {
    draw_hero(
        image,
        rect,
        HeroPose {
            chest_rise: 0,
            lead_leg_dx: foot_dx,
            trail_leg_dx: mirrored_trail_leg(foot_dx),
            lead_lifted: passing,
        },
    );
}

/// 画 `boss_idle_0`。
///
/// # 这张图现在零消费者，仍然留图
///
/// `ll-game` 本体二进制一处都不消费 `boss_idle_0`。此前它还有两个
/// 消费者——`crates/ll-render/examples/p1_acceptance` 与
/// `crates/ll-sim/examples/p3_acceptance`——因此上一批把它的身份记成
/// 「两个验收 demo 的测试夹具」。**〔2026-08-29〕那两个 demo 已随所有者
/// 裁定删除（ADR 0030），那个身份随之失效，不要再照抄。**
///
/// 处置**仍然是留图**，但理由换了：p3 的冻结截图基准里真的画着这只 boss
/// （它还是整张画布上唯一一个 2×2 占地的条目），而那张基准现在连重截的
/// 生产者都没有了，删条目会让它彻底失去对照。至于它当年验的那条性质
/// 「footprint 从图集条目读取」，真正的守门人一直是
/// `ll-render/src/sprite.rs` 里那两条锁精确数值的测试，**不受影响**。
/// 项目所有者早前那句「现在应该不太需要 boss 这东西」说的是不做这个
/// 内容，没有说删图。完整记录见 `assets/atlas/README.md` 的同名一节。
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

    /// 八张 `hero_*` 各自的画法，供下面几条断言遍历。**手写一次**，理由
    /// 与 `furniture.rs` 的 `all_furniture` 相同：这些断言要验的正是
    /// 「这八张各自长什么样」，从别处现查会把断言变成同义反复。
    fn all_hero_frames() -> Vec<(&'static str, RgbaImage)> {
        let mut frames: Vec<(&'static str, RgbaImage)> = vec![
            ("hero_idle_0", render(decorate_hero_idle)),
            ("hero_idle_1", render(decorate_hero_idle_breath)),
        ];
        for (name, foot_dx, passing) in [
            ("hero_walk_0", 2, false),
            ("hero_walk_1", 10, false),
            ("hero_walk_2", 4, false),
            ("hero_walk_3", 7, true),
            ("hero_walk_4", 8, false),
            ("hero_walk_5", 5, true),
        ] {
            frames.push((
                name,
                render(|i, r| decorate_hero_walk(i, r, foot_dx, passing)),
            ));
        }
        frames
    }

    fn render(draw: impl Fn(&mut RgbaImage, EntryRect)) -> RgbaImage {
        let mut image = RgbaImage::new(HERO_W, HERO_H);
        draw(&mut image, HERO_RECT);
        image
    }

    fn luminance(pixel: &Rgba<u8>) -> i32 {
        // 与 `ll_ui` 那侧的口径无关，这里只要一个单调的明暗度量。
        (pixel.0[0] as i32 * 299 + pixel.0[1] as i32 * 587 + pixel.0[2] as i32 * 114) / 1000
    }

    #[test]
    fn 玩家贴图不再是一整块实心矩形而是有轮廓的人形() {
        // 所有者报的正是这件事：「目前的贴图有点丑了」。重画之前
        // `fill_rect` 把整张 16×24 铺满，不透明占比恒为 1.00，四角全是
        // 钢蓝——摆在四周透明的 NPC 人形旁边就是一块砖。
        //
        // 反例（本次开发实跑）：把 `draw_hero` 第一行换回
        // `fill_rect(image, rect, HERO_BODY)`，本条报「四角 (0, 0) 应当
        // 透明」。
        // Arrange & Act & Assert
        for (name, image) in all_hero_frames() {
            for corner in [
                (0, 0),
                (HERO_W - 1, 0),
                (0, HERO_H - 1),
                (HERO_W - 1, HERO_H - 1),
            ] {
                assert_eq!(
                    image.get_pixel(corner.0, corner.1).0[3],
                    0,
                    "{name} 的角落 {corner:?} 应当透明"
                );
            }
            let opaque = image.pixels().filter(|p| p.0[3] != 0).count();
            let ratio = opaque as f32 / (HERO_W * HERO_H) as f32;
            assert!(
                (0.35..0.80).contains(&ratio),
                "{name} 的不透明像素占比 {ratio:.2} 不在 0.35..0.80——\
                 太高说明又铺成了实心矩形，太低说明人形没画全"
            );
        }
    }

    #[test]
    fn 玩家人形被一圈近黑描边整个包住() {
        // 描边是这张图能压在任何地形上都看得清的那一半。判据取「每一个
        // 贴着透明的不透明像素都必须是描边色」——等价于「人形与背景之间
        // 恒有一圈描边」，但不需要另外算一遍轮廓。
        //
        // 矩形四条边上的像素除外：那里的四邻出了 `rect`，
        // `outline_silhouette` 刻意不越界（越界会涂脏遗留共享画布上紧挨着
        // 的隔壁条目）。
        //
        // 反例（本次开发实跑）：把 `draw_hero` 末尾那句
        // `outline_silhouette` 删掉，本条报「hero_idle_0 的 (4, 9) 贴着
        // 透明却不是描边色」。
        // Arrange
        let outline = Rgba([HERO_OUTLINE.0, HERO_OUTLINE.1, HERO_OUTLINE.2, 255]);

        // Act & Assert
        for (name, image) in all_hero_frames() {
            for y in 1..HERO_H - 1 {
                for x in 1..HERO_W - 1 {
                    let pixel = *image.get_pixel(x, y);
                    if pixel.0[3] == 0 || pixel == outline {
                        continue;
                    }
                    for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                        assert_ne!(
                            image.get_pixel(nx, ny).0[3],
                            0,
                            "{name} 的 ({x}, {y}) 贴着透明却不是描边色——人形没被描边包住"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn 玩家身上同时有三档钢蓝明暗() {
        // 重画之前躯干是一片平色，读不出体积。三档（受光/主体/背光）是
        // 像素画里表达体积的最小手段，不需要任何渐变或抗锯齿。
        // Arrange & Act & Assert
        for (name, image) in all_hero_frames() {
            for (label, color) in [
                ("受光", HERO_LIGHT),
                ("主体", HERO_BODY),
                ("背光", HERO_SHADE),
            ] {
                let target = Rgba([color.0, color.1, color.2, 255]);
                assert!(
                    image.pixels().any(|p| *p == target),
                    "{name} 上找不到{label}那一档"
                );
            }
        }
    }

    #[test]
    fn 玩家贴图同时有极暗与极亮的像素() {
        // 与 `ll_ui::hud::world_map::PLAYER_MARKER_COLOR` 那条「要在深蓝
        // 的海、深绿的林、灰白的雪山上同样一眼可见」同一个道理：**单一
        // 颜色满足不了**，底色一多总有一种跟它接近。同时带极暗描边与极亮
        // 暖金的图形，任何底色要么与暗的拉得开、要么与亮的拉得开。
        //
        // 门槛 140：重画之前整张图只有钢蓝 (亮度约 120) 与暖金
        // (约 215) 两档，差 95，压在浅沙地上金色那半会糊。
        const MIN_LUMINANCE_SPAN: i32 = 140;

        // Arrange & Act & Assert
        for (name, image) in all_hero_frames() {
            let opaque: Vec<i32> = image
                .pixels()
                .filter(|p| p.0[3] != 0)
                .map(luminance)
                .collect();
            let span = opaque.iter().max().expect("恒有不透明像素")
                - opaque.iter().min().expect("恒有不透明像素");
            assert!(
                span >= MIN_LUMINANCE_SPAN,
                "{name} 最亮与最暗只差 {span} 档（门槛 {MIN_LUMINANCE_SPAN}），\
                 压在某些地形底色上会糊成一团"
            );
        }
    }

    #[test]
    fn 玩家的脚底与npc踩在同一条地平线上() {
        // `npc.rs` 的 `FEET_TOP` 也是 21。两边对不齐的话，玩家和 NPC 站在
        // 相邻两格里会一高一低——重画之前玩家的「脚」其实是画布底边，
        // 看着像浮着。
        // Arrange & Act & Assert
        assert_eq!(HERO_FEET_TOP, 21);
        let image = render(decorate_hero_idle);
        let boots = Rgba([HERO_FOOT_MARK.0, HERO_FOOT_MARK.1, HERO_FOOT_MARK.2, 255]);
        assert!(
            (0..HERO_W).any(|x| *image.get_pixel(x, HERO_FEET_TOP) == boots),
            "脚底那一行上找不到靴子"
        );
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
    fn 待机呼吸帧的上半身比待机帧高一像素而腿脚一动不动() {
        // Arrange
        let idle = render(decorate_hero_idle);
        let breath = render(decorate_hero_idle_breath);

        // Act：只比脚底那三行（靴子）。
        let boots_differ = (0..HERO_W)
            .flat_map(|x| (HERO_FEET_TOP..HERO_H).map(move |y| (x, y)))
            .filter(|&(x, y)| idle.get_pixel(x, y) != breath.get_pixel(x, y))
            .count();

        // Assert：呼吸帧的头顶（盔的最上一行）比待机帧高一行；靴子逐
        // 像素不动——「吸气时胸腔张开」而不是「整个人上下弹跳」。
        let head_top_of = |image: &RgbaImage| {
            (0..HERO_H)
                .find(|&y| (0..HERO_W).any(|x| image.get_pixel(x, y).0[3] != 0))
                .expect("恒有不透明像素")
        };
        assert_eq!(
            head_top_of(&idle) - head_top_of(&breath),
            1,
            "呼吸帧的上半身应当恰好高一像素"
        );
        assert_eq!(boots_differ, 0, "呼吸不该让脚也跟着动");
    }

    #[test]
    fn 两个接触帧的前腿落在不同列() {
        // Arrange：hero_walk_0/hero_walk_1 那两个接触极值姿态。
        let left = render(|i, r| decorate_hero_walk(i, r, 2, false));
        let right = render(|i, r| decorate_hero_walk(i, r, 10, false));

        // Act & Assert：左脚帧在 x=2 的脚底行是靴色，右脚帧在同一位置不是。
        let boots = Rgba([HERO_FOOT_MARK.0, HERO_FOOT_MARK.1, HERO_FOOT_MARK.2, 255]);
        assert_eq!(*left.get_pixel(2, HERO_FEET_TOP), boots);
        assert_ne!(*right.get_pixel(2, HERO_FEET_TOP), boots);
    }

    #[test]
    fn 过腿帧的前脚比接触帧高一像素() {
        // Arrange：同一水平位置，只切换 passing。
        let contact = render(|i, r| decorate_hero_walk(i, r, 7, false));
        let passing = render(|i, r| decorate_hero_walk(i, r, 7, true));

        // Act & Assert：过腿帧在 `FEET_TOP - 1` 那一行已经是靴色（脚离地
        // 抬高一像素），接触帧同一行还是腿。
        let boots = Rgba([HERO_FOOT_MARK.0, HERO_FOOT_MARK.1, HERO_FOOT_MARK.2, 255]);
        assert_eq!(*passing.get_pixel(7, HERO_FEET_TOP - 1), boots);
        assert_ne!(*contact.get_pixel(7, HERO_FEET_TOP - 1), boots);
    }

    #[test]
    fn 行走六帧的上半身逐像素不动() {
        // 理由见 `decorate_hero_walk` 文档最后一段：把变化幅度全部交给
        // 腿，才能让每一对相邻帧的差异都小于两帧方案那次硬切。上半身
        // 取「腿顶端之上」的全部行。
        // Arrange
        let frames: Vec<RgbaImage> = [
            (2, false),
            (4, false),
            (7, true),
            (10, false),
            (8, false),
            (5, true),
        ]
        .into_iter()
        .map(|(dx, passing)| render(move |i, r| decorate_hero_walk(i, r, dx, passing)))
        .collect();

        // Act & Assert
        for frame in &frames[1..] {
            let upper_differ = (0..HERO_W)
                .flat_map(|x| (0..HERO_LEG_TOP).map(move |y| (x, y)))
                .filter(|&(x, y)| frames[0].get_pixel(x, y) != frame.get_pixel(x, y))
                .count();
            assert_eq!(upper_differ, 0, "行走帧的上半身不该随迈步变化");
        }
    }

    #[test]
    fn 后腿由前腿镜像得出() {
        // 六帧表里只填前腿一个数，后腿现算——填两个数的话，其中任何一个
        // 填错都会画出一条长短腿，而那种错在 16 像素宽的图上极难看出来。
        // Arrange & Act & Assert
        for foot_dx in 2..=10u32 {
            let trail = mirrored_trail_leg(foot_dx);
            assert!((2..=10).contains(&trail), "后腿列号 {trail} 跑出了取值域");
            assert_eq!(
                mirrored_trail_leg(trail),
                foot_dx,
                "镜像必须是自反的，否则前后腿会越走越偏"
            );
        }
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
