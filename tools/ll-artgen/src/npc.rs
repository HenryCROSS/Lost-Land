//! NPC 的**种族身子**与**职业挂件**两套贴图。
//!
//! # 这个模块解的是一道乘法题
//!
//! 项目所有者的要求是「npc 根据职业种族做出区别，多画点」。本体现有
//! 4 个种族 × 13 个职业 = 52 种组合。逐个组合画一张图，加第 10 个种族
//! 要补 13 张、加第 14 个职业要补 9 张——是乘法级的负担。
//!
//! **所有者后来裁定就要那 52 张**（「我觉得还是每个种族的每个职业画上
//! 风格不同的图片。虽然美术资源会很多。」），落点是 [`crate::composite`]。
//! 但那道乘法题并没有消失，只是换了个答案：合成图**复用本模块的配方**
//! （[`draw_race_body`] 一行没重写、[`draw_profession_badge`] 直接叠上
//! 去），52 张图仍然只有「种族数 + 职业数」份配方。
//!
//! 本模块的两套图**一张都没退休**：它们是回退链的最后一段——mod 新加
//! 的种族或职业没有合成图时自动落到这里（见 `ll_game::surface_draw`
//! 模块文档「合成图的回退链」一节）。
//!
//! 这里画的是**两套可以叠在一起的图**，不是一套组合图：
//!
//! - [`race_bodies`]：每个种族一张 16×24 的身子，决定体型、肤色、耳朵、
//!   胡子这些「他是什么」的东西。
//! - [`profession_badges`]：每个职业一张 16×24、**四周全透明、只在胸口
//!   有一块徽记**的挂件，叠在身子上，决定「他干什么」。
//!
//! 本模块的资产量因此是 `种族数 + 职业数`（4 + 13 = 17 张）；
//! [`crate::composite`] 另外产出 `种族数 × 职业数`（52 张），但它靠的
//! 是这里的同一批配方。渲染侧怎么把两张叠起来，见
//! `ll_game::surface_draw` 模块文档「NPC 为什么是两条指令」一节——两张
//! 图同尺寸、同 `pivot`、同 `footprint`，因此像素级对齐，不需要任何
//! 额外的偏移换算。
//!
//! # 名字必须与内容 id 的本地名逐字一致
//!
//! 与 [`crate::world_marks::decorate_forge`] 同一条约定：`ll_mod::asset_vfs`
//! 把清单条目名与所属命名空间拼成图集条目名，渲染层拿内容的完整 ID
//! （`lostland:dwarf`、`lostland:blacksmith`）当查找键。**这条对齐靠
//! 约定，不靠引擎里的分支**——把 `dwarf.png` 删掉，矮人自动退回通用
//! NPC 记号；把 `blacksmith.png` 删掉，铁匠就是不带挂件的普通人；两种
//! 情况引擎都一行不用改。
//!
//! # 为什么这些图不进遗留共享画布
//!
//! [`crate::generate_legacy_shared_atlas`] 那张 `placeholder.png` 是四张
//! 冻结像素基准的来源（见 `main.rs` 的 `LooseOnlyEntry` 文档更正段）。
//! 本模块的图**只**进松散贴图树
//! （`assets/sprites/`），画布尺寸因此仍是 96×144、既有条目的矩形一个
//! 都没动，那四张基准完全不受影响。做法上的落点是
//! [`crate::LooseOnlyEntry`]：一份与 `placeholder.json` 平行、只喂给
//! [`crate::generate_loose_sprites`] 的条目清单。
//!
//! # 确定性（约束 C5）
//!
//! 两张表都是数组字面量，绘制与写盘顺序即数组顺序，不经任何
//! `HashMap`/`HashSet`。

use crate::EntryRect;
use crate::color::Hsl;
use crate::sprite::paint_patch;
use image::RgbaImage;

/// 一张 NPC 贴图的画布宽度——与 `npc_idle_0`/`hero_*` 同一档（见
/// `assets/atlas/placeholder.json`），身子与挂件必须同尺寸才能像素级
/// 对齐。
pub(crate) const NPC_WIDTH: u32 = 16;
/// 一张 NPC 贴图的画布高度，理由同 [`NPC_WIDTH`]。
pub(crate) const NPC_HEIGHT: u32 = 24;

/// 脚底那一档的行号：`FEET_TOP..NPC_HEIGHT` 是鞋，全部种族一致——脚必须
/// 落在同一条地平线上，否则矮人会浮在半空。身高差异全部由
/// [`BodySpec::head_top`] 表达。
const FEET_TOP: u32 = 21;

/// 头的宽度。全部种族一致：16 像素宽的画布上再让头宽也变，头会窄到放不
/// 下两只眼睛。种族的轮廓差异交给 [`BodySpec::head_top`]（身高）、
/// [`BodySpec::shoulder_w`]（体型）与 [`BodySpec::ears`]（耳朵）三项。
const HEAD_W: u32 = 6;
/// 头的高度，理由同 [`HEAD_W`]。
const HEAD_H: u32 = 6;

/// 徽记底板左上角的列号：胸口。
///
/// 这一块必须落在**每一个**种族的躯干范围内。躯干最窄的精灵/哥布林是
/// 8 像素宽（第 4..12 列），把 6 像素宽的底板放在第 5..11 列正好落在
/// 里面。`徽记落在每一个种族的躯干范围内` 钉住这条。
const BADGE_LEFT: u32 = 5;
/// 徽记底板左上角的行号。躯干最高的精灵从第 6 行起、最矮的哥布林从第
/// 10 行起，两者都在这一行之前。
const BADGE_TOP: u32 = 11;
/// 徽记底板的边长（正方形）。
const BADGE_SIZE: u32 = 6;

/// 笔画色相对底板色的明度偏移量。底板偏亮就把笔画压暗、偏暗就把笔画
/// 提亮，保证任何一块底板上笔画都看得见——不需要为每个职业手工再挑一
/// 个笔画色，也就不会出现「某一行手滑挑了个和底板差不多的颜色」。
const GLYPH_LIGHTNESS_SHIFT: f32 = 0.34;

/// 底板算「偏亮」的明度门槛。
const GLYPH_CONTRAST_PIVOT: f32 = 0.5;

/// 耳朵样式。种族之间**除了颜色还要有轮廓差异**：只换肤色的话，缩略图
/// 或者被 tint 压暗之后就分不出谁是谁了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ears {
    /// 贴着头的小圆耳（人类、矮人）。
    Round,
    /// 向斜上方挑出的尖耳（精灵、半精灵）。
    Pointed,
    /// 向两侧张开的大阔耳（哥布林）。
    Wide,
}

/// 一个种族的身子配方。字段全部是**几何或配色参数**，没有一处按种族名
/// 分支——加一个种族就是往 [`RACE_BODIES`] 里加一行。
#[derive(Debug, Clone, Copy)]
pub(crate) struct BodySpec {
    /// 肤色。
    skin: (u8, u8, u8),
    /// 发色（也是胡子色）。
    hair: (u8, u8, u8),
    /// 衣服色。裤子与鞋由它现算（见 [`darken`]），不另填。
    cloth: (u8, u8, u8),
    /// 头顶距贴图顶端的行数。**这个数就是身高**：越大越矮，脚底恒定在
    /// [`FEET_TOP`]。
    head_top: u32,
    /// 肩宽（像素，取偶数才能对称居中）。
    shoulder_w: u32,
    /// 腿从第几行开始（越大腿越短）。
    leg_top: u32,
    /// 耳朵样式。
    ears: Ears,
    /// 胡子高度（行数，0 表示不留胡子）。
    beard_h: u32,
}

/// 一个种族身子上几块可寻址区域的坐标，由 [`BodySpec`] 的几何参数现算。
///
/// # 为什么把它抽出来
///
/// [`draw_race_body`] 与 [`crate::composite`] 都要知道「躯干在哪几行几
/// 列」：前者用它填衣服色，后者用它把衣服换成职业色。这些坐标**只能有
/// 一份**——抄第二份的话，改一次肩宽公式就会让合成图的衣服与身子错位，
/// 而错位在 16 像素宽的画布上一眼就是「衣服穿歪了」。
#[derive(Debug, Clone, Copy)]
pub(crate) struct BodyGeometry {
    /// 躯干（衣服）矩形：`(x, y, 宽, 高)`。
    pub(crate) torso: (u32, u32, u32, u32),
    /// 头（肤色）矩形。
    pub(crate) head: (u32, u32, u32, u32),
    /// 头顶发际那两行——合成图把它换成职业色的帽子。
    pub(crate) hair: (u32, u32, u32, u32),
    /// 左右两条手臂（露出的皮肤）矩形。
    pub(crate) arms: [(u32, u32, u32, u32); 2],
}

impl BodySpec {
    /// 这个种族身子各块区域的坐标。[`draw_race_body`] 自己也走这一份。
    pub(crate) fn geometry(self) -> BodyGeometry {
        let shoulder_x0 = NPC_WIDTH / 2 - self.shoulder_w / 2;
        let head_x0 = NPC_WIDTH / 2 - HEAD_W / 2;
        let torso_top = self.head_top + HEAD_H;
        let arm_h = self.leg_top - torso_top - 1;
        BodyGeometry {
            torso: (
                shoulder_x0,
                torso_top,
                self.shoulder_w,
                self.leg_top - torso_top,
            ),
            head: (head_x0, self.head_top, HEAD_W, HEAD_H),
            hair: (head_x0, self.head_top, HEAD_W, 2),
            arms: [
                (shoulder_x0 - 2, torso_top + 1, 2, arm_h),
                (shoulder_x0 + self.shoulder_w, torso_top + 1, 2, arm_h),
            ],
        }
    }
}

/// 一个职业挂件的配方。只有一个颜色与一张 6×6 图形——**没有任何一个种族
/// 相关的字段**，这正是「挂件与种族正交」的落点：同一张挂件叠在任何种族
/// 的身子上都成立，因此挂件的数量与种族的数量无关。
#[derive(Debug, Clone, Copy)]
pub(crate) struct BadgeSpec {
    /// 徽记底板色。十三个职业两两不同，见 [`PROFESSION_BADGES`]。
    plate: (u8, u8, u8),
    /// 徽记图形，6×6，`'#'` 是笔画、其余是底板。
    glyph: [&'static str; BADGE_SIZE as usize],
}

/// 本体四个种族的身子配方，顺序即 `mods/lostland/races.json5` 的声明
/// 顺序。名字是种族 id 的**本地名**，见模块文档。
const RACE_BODIES: [(&str, BodySpec); 4] = [
    (
        // 人类：中等身量、中等体格，褐发小麦肤——四族里的基准线，
        // 另外三族都是相对它的偏移。
        "human",
        BodySpec {
            skin: (214, 168, 132),
            hair: (96, 62, 38),
            cloth: (108, 96, 148),
            head_top: 1,
            shoulder_w: 10,
            leg_top: 18,
            ears: Ears::Round,
            beard_h: 0,
        },
    ),
    (
        // 矮人：比人类矮两格、比人类宽两像素，赤褐大胡子。「矮」与
        // 「壮」两个维度同时偏移，才不会只读成「站得靠下的人类」。
        "dwarf",
        BodySpec {
            skin: (198, 140, 110),
            hair: (156, 66, 42),
            cloth: (122, 92, 58),
            head_top: 3,
            shoulder_w: 12,
            leg_top: 19,
            ears: Ears::Round,
            beard_h: 4,
        },
    ),
    (
        // 精灵：最高、最窄，近白肤配浅金发，尖耳。
        "elf",
        BodySpec {
            skin: (232, 216, 198),
            hair: (226, 214, 158),
            cloth: (86, 132, 96),
            head_top: 0,
            shoulder_w: 8,
            leg_top: 17,
            ears: Ears::Pointed,
            beard_h: 0,
        },
    ),
    (
        // 哥布林：最矮最瘦、绿皮、黑发、招风大耳——唯一一个肤色跳出
        // 「肉色」色域的种族，远景里靠颜色就能先认出来。
        "goblin",
        BodySpec {
            skin: (124, 158, 84),
            hair: (40, 44, 34),
            cloth: (92, 74, 60),
            head_top: 4,
            shoulder_w: 8,
            leg_top: 19,
            ears: Ears::Wide,
            beard_h: 0,
        },
    ),
];

/// 本体十三个职业的挂件配方，顺序即 `mods/lostland/classes.json5` 的
/// 声明顺序。
///
/// 十三块底板色两两不同，且刻意分散在色相环上——本模块的
/// `十三个职业挂件两两之间每一个像素都不同` 钉住这一点。
const PROFESSION_BADGES: [(&str, BadgeSpec); 13] = [
    (
        // 战士：竖刃 + 横护手，一把剑。
        "warrior",
        BadgeSpec {
            plate: (176, 66, 58),
            glyph: ["..##..", "..##..", "######", "..##..", "..##..", "..##.."],
        },
    ),
    (
        // 法师：四向星芒。
        "mage",
        BadgeSpec {
            plate: (110, 78, 178),
            glyph: ["..##..", "#.##.#", ".####.", ".####.", "#.##.#", "..##.."],
        },
    ),
    (
        // 游侠：弓臂与弓弦。
        "ranger",
        BadgeSpec {
            plate: (66, 138, 82),
            glyph: ["..##..", ".#..#.", "#....#", "#....#", ".#..#.", "..##.."],
        },
    ),
    (
        // 卫兵：盾。
        "guard",
        BadgeSpec {
            plate: (66, 106, 168),
            glyph: ["######", "######", "######", ".####.", "..##..", "..##.."],
        },
    ),
    (
        // 据点管理者：钥匙。
        "steward",
        BadgeSpec {
            plate: (206, 168, 62),
            glyph: [".####.", ".#..#.", ".####.", "..##..", "..###.", "..##.."],
        },
    ),
    (
        // 民兵：斜握的长矛。
        "militia",
        BadgeSpec {
            plate: (150, 122, 70),
            glyph: ["....##", "...##.", "..##..", ".##...", "##....", "#....."],
        },
    ),
    (
        // 农夫：一株麦。
        "farmer",
        BadgeSpec {
            plate: (154, 176, 74),
            glyph: ["#.##.#", ".####.", "..##..", ".####.", "..##..", "..##.."],
        },
    ),
    (
        // 猎户：箭镞。
        "hunter",
        BadgeSpec {
            plate: (140, 96, 54),
            glyph: ["..##..", ".####.", "##..##", "..##..", "..##..", "..##.."],
        },
    ),
    (
        // 屠夫：剁刀。
        "butcher",
        BadgeSpec {
            plate: (188, 82, 96),
            glyph: ["######", "######", "######", "#####.", "..#...", "..#..."],
        },
    ),
    (
        // 铁匠：锤。
        "blacksmith",
        BadgeSpec {
            plate: (218, 118, 44),
            glyph: ["######", "######", "..##..", "..##..", "..##..", "..##.."],
        },
    ),
    (
        // 渔夫：鱼。
        "fisher",
        BadgeSpec {
            plate: (70, 166, 176),
            glyph: ["......", ".####.", "######", "######", ".####.", "......"],
        },
    ),
    (
        // 牧羊人：牧杖的弯钩。
        "shepherd",
        BadgeSpec {
            plate: (222, 214, 190),
            glyph: [".####.", "##..##", "##..##", "..##..", "..##..", "..##.."],
        },
    ),
    (
        // 石匠：错缝的两层砖。
        "mason",
        BadgeSpec {
            plate: (150, 150, 156),
            glyph: ["##.###", "##.###", "......", "###.##", "###.##", "......"],
        },
    ),
];

/// `mods/example_mod` 那张示范身子的配方。
///
/// 存在的理由是**验收而不是美术**：它证明「加一个种族只要加数据加图，
/// 引擎一行都不用改」——`examplemod:half_elf` 是示例 mod 自己声明的
/// 内容，本体一个字都没为它写过。见 `crates/ll-game/tests/npc_appearance.rs`。
const EXAMPLE_MOD_RACE: (&str, BodySpec) = (
    "half_elf",
    BodySpec {
        skin: (222, 190, 164),
        hair: (150, 118, 62),
        cloth: (150, 92, 140),
        head_top: 1,
        shoulder_w: 8,
        leg_top: 17,
        ears: Ears::Pointed,
        beard_h: 0,
    },
);

/// `mods/example_mod` 那张示范挂件的配方，理由同 [`EXAMPLE_MOD_RACE`]。
const EXAMPLE_MOD_BADGE: (&str, BadgeSpec) = (
    "necromancer",
    BadgeSpec {
        plate: (76, 92, 78),
        glyph: [".####.", "#.##.#", "######", ".####.", "..##..", ".#..#."],
    },
);

/// 本体全部种族身子的 `(条目名, 配方)`，顺序固定。
pub(crate) fn race_bodies() -> &'static [(&'static str, BodySpec)] {
    &RACE_BODIES
}

/// 徽记底板色——[`crate::composite`] 拿它当这个职业的**签名色**，衣服
/// 与工具的配色都从它推。十三块底板两两不同（本模块的
/// `十三个职业挂件两两之间每一个像素都不同` 钉住），因此十三件衣服也
/// 两两不同，不需要再手工挑十三个颜色、也就不会手滑挑重。
pub(crate) fn badge_plate(spec: BadgeSpec) -> (u8, u8, u8) {
    spec.plate
}

/// 沿明度轴压暗，供 [`crate::composite`] 从签名色推衣服色用。
///
/// 与本模块给裤子/鞋用的是同一个函数，理由也一样：同一套明暗关系只写
/// 一份。
pub(crate) fn darken_color(color: (u8, u8, u8), delta: f32) -> (u8, u8, u8) {
    darken(color, delta)
}

/// 本体全部职业挂件的 `(条目名, 配方)`，顺序固定。
pub(crate) fn profession_badges() -> &'static [(&'static str, BadgeSpec)] {
    &PROFESSION_BADGES
}

/// 示例 mod 的种族身子配方。
pub(crate) fn example_mod_race() -> (&'static str, BodySpec) {
    EXAMPLE_MOD_RACE
}

/// 示例 mod 的职业挂件配方。
pub(crate) fn example_mod_badge() -> (&'static str, BadgeSpec) {
    EXAMPLE_MOD_BADGE
}

/// 按条目名查一份画法：先查种族身子，再查职业挂件；都不认识返回 `false`。
///
/// [`crate::draw_entry`] 那个统一分派点因此不需要知道「身子」与「挂件」
/// 是两种不同的东西。
pub(crate) fn draw_named(image: &mut RgbaImage, name: &str, rect: EntryRect) -> bool {
    if let Some((_, spec)) = RACE_BODIES.iter().find(|(entry, _)| *entry == name) {
        draw_race_body(image, rect, *spec);
        return true;
    }
    if let Some((_, spec)) = PROFESSION_BADGES.iter().find(|(entry, _)| *entry == name) {
        draw_profession_badge(image, rect, *spec);
        return true;
    }
    false
}

/// 把一个种族画成一个站着的人：躯干 → 手臂 → 腿 → 脚 → 头 → 耳 → 发 →
/// 眼 → 胡子。
///
/// **全部种族走的是这同一段代码**，差异只来自 [`BodySpec`] 的八个参数
/// ——这正是 ADR 0021 说的「抽象的正当理由是有算法要共用」：加一个种族
/// 是加八个数字，不是抄一遍这几十行。
pub(crate) fn draw_race_body(image: &mut RgbaImage, rect: EntryRect, spec: BodySpec) {
    let geometry = spec.geometry();
    let shoulder_x0 = geometry.torso.0;
    let head_x0 = geometry.head.0;
    let boot = darken(spec.cloth, 0.16);
    let trouser = darken(spec.cloth, 0.08);

    // 躯干（衣服）。
    let (tx, ty, tw, th) = geometry.torso;
    paint_patch(image, rect, tx, ty, tw, th, spec.cloth);
    // 双臂（露出的皮肤），贴着躯干两侧各 2 像素宽。
    for (ax, ay, aw, ah) in geometry.arms {
        paint_patch(image, rect, ax, ay, aw, ah, spec.skin);
    }
    // 双腿：躯干两侧各一条，宽 3。
    let leg_h = FEET_TOP - spec.leg_top;
    paint_patch(image, rect, shoulder_x0, spec.leg_top, 3, leg_h, trouser);
    paint_patch(
        image,
        rect,
        shoulder_x0 + spec.shoulder_w - 3,
        spec.leg_top,
        3,
        leg_h,
        trouser,
    );
    // 双脚，落在同一条地平线上。
    let foot_h = NPC_HEIGHT - FEET_TOP;
    paint_patch(image, rect, shoulder_x0, FEET_TOP, 3, foot_h, boot);
    paint_patch(
        image,
        rect,
        shoulder_x0 + spec.shoulder_w - 3,
        FEET_TOP,
        3,
        foot_h,
        boot,
    );
    // 头。
    paint_patch(
        image,
        rect,
        head_x0,
        spec.head_top,
        HEAD_W,
        HEAD_H,
        spec.skin,
    );
    draw_ears(image, rect, spec, head_x0);
    // 头发：头顶两行。
    paint_patch(image, rect, head_x0, spec.head_top, HEAD_W, 2, spec.hair);
    // 眼睛。
    let eye = darken(spec.skin, 0.55);
    paint_patch(image, rect, head_x0 + 1, spec.head_top + 3, 1, 1, eye);
    paint_patch(
        image,
        rect,
        head_x0 + HEAD_W - 2,
        spec.head_top + 3,
        1,
        1,
        eye,
    );
    // 胡子：挂在头下沿，压住一部分躯干。
    if spec.beard_h > 0 {
        paint_patch(
            image,
            rect,
            head_x0,
            spec.head_top + HEAD_H - 1,
            HEAD_W,
            spec.beard_h,
            spec.hair,
        );
    }
}

/// 三种耳朵各自的画法。抽出来只是为了让 [`draw_race_body`] 保持在一屏
/// 之内，不是另一套配方。
fn draw_ears(image: &mut RgbaImage, rect: EntryRect, spec: BodySpec, head_x0: u32) {
    let right = head_x0 + HEAD_W;
    match spec.ears {
        Ears::Round => {
            paint_patch(image, rect, head_x0 - 1, spec.head_top + 2, 1, 2, spec.skin);
            paint_patch(image, rect, right, spec.head_top + 2, 1, 2, spec.skin);
        }
        Ears::Pointed => {
            // 竖直一列 + 更靠外更靠上的一枚尖，读成「向斜上方挑出」。
            paint_patch(image, rect, head_x0 - 1, spec.head_top + 1, 1, 3, spec.skin);
            paint_patch(image, rect, head_x0 - 2, spec.head_top + 1, 1, 1, spec.skin);
            paint_patch(image, rect, right, spec.head_top + 1, 1, 3, spec.skin);
            paint_patch(image, rect, right + 1, spec.head_top + 1, 1, 1, spec.skin);
        }
        Ears::Wide => {
            paint_patch(image, rect, head_x0 - 2, spec.head_top + 2, 2, 3, spec.skin);
            paint_patch(image, rect, right, spec.head_top + 2, 2, 3, spec.skin);
        }
    }
}

/// 把一个职业画成胸口的一块徽记：底板 + 笔画，其余全部留透明。
///
/// 留透明不是省事：这张图是**叠**在种族身子上的，只有徽记那一块该盖住
/// 身子，其余每一个像素都必须让下面的身子透出来，否则挂件会把种族差异
/// 整个抹掉——那正好回到「所有 NPC 长得一模一样」。
pub(crate) fn draw_profession_badge(image: &mut RgbaImage, rect: EntryRect, spec: BadgeSpec) {
    let ink = glyph_ink(spec.plate);
    for (row, line) in spec.glyph.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let color = if ch == '#' { ink } else { spec.plate };
            paint_patch(
                image,
                rect,
                BADGE_LEFT + col as u32,
                BADGE_TOP + row as u32,
                1,
                1,
                color,
            );
        }
    }
}

/// 笔画色：底板色沿明度轴推 [`GLYPH_LIGHTNESS_SHIFT`]，方向由底板本身
/// 的明暗决定。
fn glyph_ink(plate: (u8, u8, u8)) -> (u8, u8, u8) {
    let hsl = Hsl::from_rgb(plate.0, plate.1, plate.2);
    let shift = if hsl.lightness() > GLYPH_CONTRAST_PIVOT {
        -GLYPH_LIGHTNESS_SHIFT
    } else {
        GLYPH_LIGHTNESS_SHIFT
    };
    hsl.lighten(shift).to_rgb()
}

/// 把一个颜色沿明度轴压暗 `delta`。裤子/鞋比上衣暗一档，靠这一个函数
/// 从衣服色现算，而不是让每个种族再手填两个颜色。
fn darken(color: (u8, u8, u8), delta: f32) -> (u8, u8, u8) {
    Hsl::from_rgb(color.0, color.1, color.2)
        .lighten(-delta)
        .to_rgb()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: NPC_WIDTH,
        height: NPC_HEIGHT,
    };

    fn body_image(spec: BodySpec) -> RgbaImage {
        let mut image = RgbaImage::new(NPC_WIDTH, NPC_HEIGHT);
        draw_race_body(&mut image, RECT, spec);
        image
    }

    fn badge_image(spec: BadgeSpec) -> RgbaImage {
        let mut image = RgbaImage::new(NPC_WIDTH, NPC_HEIGHT);
        draw_profession_badge(&mut image, RECT, spec);
        image
    }

    fn differing(a: &RgbaImage, b: &RgbaImage) -> usize {
        a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count()
    }

    #[test]
    fn 四个种族的身子两两之间至少四分之一像素不同() {
        // 门槛与 `atlas_coverage.rs` 那条地形断言取同一把尺子：
        // 16×24 = 384 像素，四分之一是 96。这不是「画得好不好看」的
        // 判据，是「两张图有没有被写成几乎一样」的下界。
        //
        // 反例（本次开发实跑）：把 `goblin` 的肤色/发色/衣色/身高/肩宽/
        // 腿长六个参数逐字改成 `human` 的（只留耳朵不同），本条报
        // 「human 与 goblin 只有 8 个像素不同」。
        // Arrange
        let rendered: Vec<(&str, RgbaImage)> = race_bodies()
            .iter()
            .map(|(name, spec)| (*name, body_image(*spec)))
            .collect();

        // Act & Assert
        let threshold = (NPC_WIDTH * NPC_HEIGHT / 4) as usize;
        for (i, (name_a, a)) in rendered.iter().enumerate() {
            for (name_b, b) in &rendered[i + 1..] {
                let diff = differing(a, b);
                assert!(
                    diff >= threshold,
                    "种族 {name_a} 与 {name_b} 只有 {diff} 个像素不同（门槛 {threshold}）"
                );
            }
        }
    }

    #[test]
    fn 十三个职业挂件两两之间每一个像素都不同() {
        // 挂件只占胸口 6×6 = 36 像素，其余全透明，因此门槛不能照搬
        // 「整张图的四分之一」（96）——那是任何一对挂件都不可能达到的
        // 数，门槛定在那里等于这条断言永远红。
        //
        // 门槛取徽记的**全部** 36 个像素，不是某个留了余量的小一点的
        // 数：底板色十三个两两不同、笔画色由底板现算所以也两两不同，
        // 于是「整块徽记逐像素全不同」是这套画法的设计下界，不是运气。
        // 门槛压到 36 以下反而会放过「两个职业挑了相近颜色」这种真正
        // 该拦的事。
        //
        // 反例（本次开发实跑）：把 `fisher` 的底板色改成与 `guard`
        // 相同的 `(66, 106, 168)`，本条报「guard 与 fisher 只有 14 个
        // 像素不同」（只剩两者图形不同的那几笔）。
        // Arrange
        let rendered: Vec<(&str, RgbaImage)> = profession_badges()
            .iter()
            .map(|(name, spec)| (*name, badge_image(*spec)))
            .collect();

        // Act & Assert
        for (i, (name_a, a)) in rendered.iter().enumerate() {
            for (name_b, b) in &rendered[i + 1..] {
                let diff = differing(a, b);
                let threshold = (BADGE_SIZE * BADGE_SIZE) as usize;
                assert!(
                    diff >= threshold,
                    "职业挂件 {name_a} 与 {name_b} 只有 {diff} 个像素不同（门槛 {threshold}）"
                );
            }
        }
    }

    #[test]
    fn 挂件除胸口徽记外全部透明() {
        // 这条守的是「挂件不能把种族差异抹掉」：徽记之外每一个像素都
        // 必须让下面的身子透出来。
        // Arrange
        let image = badge_image(profession_badges()[0].1);

        // Act & Assert
        for y in 0..NPC_HEIGHT {
            for x in 0..NPC_WIDTH {
                let inside = (BADGE_LEFT..BADGE_LEFT + BADGE_SIZE).contains(&x)
                    && (BADGE_TOP..BADGE_TOP + BADGE_SIZE).contains(&y);
                let alpha = image.get_pixel(x, y).0[3];
                if inside {
                    assert_eq!(alpha, 255, "徽记内的 ({x}, {y}) 应当不透明");
                } else {
                    assert_eq!(alpha, 0, "徽记外的 ({x}, {y}) 应当透明");
                }
            }
        }
    }

    #[test]
    fn 每个种族的脚都落在同一条地平线上() {
        // 身高差异必须由头顶位置表达，不能由脚底位置表达——脚底一浮，
        // 矮人就站到半空里去了。
        // Act & Assert
        for (name, spec) in race_bodies() {
            let image = body_image(*spec);
            let bottom_opaque = (0..NPC_WIDTH)
                .filter(|x| image.get_pixel(*x, NPC_HEIGHT - 1).0[3] == 255)
                .count();
            assert_eq!(
                bottom_opaque, 6,
                "种族 {name} 的最后一行该正好是两只 3 像素宽的脚"
            );
        }
    }

    #[test]
    fn 矮人比精灵矮而精灵比谁都高() {
        // Arrange
        let tallest = race_bodies()
            .iter()
            .min_by_key(|(_, spec)| spec.head_top)
            .expect("本体有种族");

        // Act & Assert：`head_top` 就是身高，越大越矮。
        assert_eq!(tallest.0, "elf");
        let dwarf_top = race_bodies()
            .iter()
            .find(|(name, _)| *name == "dwarf")
            .expect("本体有矮人")
            .1
            .head_top;
        assert!(dwarf_top > tallest.1.head_top);
    }

    #[test]
    fn 徽记落在每一个种族的躯干范围内() {
        // 挂件与种族正交的前提：那块 6×6 徽记在**任何**种族身上都盖在
        // 躯干上，不会飘在头顶或者悬在身体外侧。加第 10 个种族时，
        // 这条会替你拦下「新种族太矮/太窄，徽记戳出身体外」。
        // Act & Assert
        let all = race_bodies()
            .iter()
            .copied()
            .chain(std::iter::once(example_mod_race()));
        for (name, spec) in all {
            let body = body_image(spec);
            for y in BADGE_TOP..BADGE_TOP + BADGE_SIZE {
                for x in BADGE_LEFT..BADGE_LEFT + BADGE_SIZE {
                    assert_eq!(
                        body.get_pixel(x, y).0[3],
                        255,
                        "种族 {name} 的 ({x}, {y}) 是透明的，徽记会飘在身体外面"
                    );
                }
            }
        }
    }

    #[test]
    fn 名字查得到对应画法且不认识的名字返回假() {
        // Arrange & Act & Assert
        for (name, _) in race_bodies() {
            let mut image = RgbaImage::new(NPC_WIDTH, NPC_HEIGHT);
            assert!(draw_named(&mut image, name, RECT), "{name} 应当查得到画法");
        }
        for (name, _) in profession_badges() {
            let mut image = RgbaImage::new(NPC_WIDTH, NPC_HEIGHT);
            assert!(draw_named(&mut image, name, RECT), "{name} 应当查得到画法");
        }
        let mut image = RgbaImage::new(NPC_WIDTH, NPC_HEIGHT);
        assert!(!draw_named(&mut image, "根本不存在的条目", RECT));
    }
}
