//! 「种族 × 职业」合成图：本体 4 个种族 × 13 个职业 = 52 张。
//!
//! # 所有者裁定与它推翻了什么
//!
//! > 「我觉得还是每个种族的每个职业画上风格不同的动画或者图片。虽然
//! > 美术资源会很多。」
//! >
//! > 「我觉得还是先做静态图吧，行动帧不需要。」
//!
//! [`crate::npc`] 模块文档开头写着「逐个组合画一张图……是乘法级的负担，
//! 不可接受」。**那句话已经被所有者推翻了**，本模块就是推翻之后的落点。
//! 但 `npc.rs` 那两套图**一张都没删**：它们成了回退链的最后一段——mod
//! 新加的种族/职业没有合成图时，仍然自动退回「身子 + 挂件」的分层合成
//! （见 `ll_game::surface_draw` 模块文档「合成图的回退链」一节）。
//!
//! 因此这 52 张不是「抄 52 遍人形」，而是**在既有配方之上再叠两个维度**：
//! 身子直接调 [`npc::draw_race_body`]（种族那八个几何/配色参数一个都没
//! 重写），职业那一层换衣服 + 挂徽记 + 塞一件工具。加第 5 个种族要补 13
//! 张——但补的是 13 行 `RACE_BODIES` 里已经有的东西，**不是 13 段绘制
//! 代码**。
//!
//! # 条目名必须与回退链拼出来的键逐字一致
//!
//! `ll_game::surface_draw::composite_keys` 拼的是
//! `"{种族完整ID}_{压平的职业完整ID}"`，例如
//! `lostland:human_lostland_blacksmith`。而
//! `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 恒等于
//! `"{命名空间}:{清单条目名}"`。两边对齐 ⇒ **清单条目名（也就是本模块
//! 产出的文件名主干）必须是 `"{种族本地名}_{职业命名空间}_{职业本地名}"`**
//! ——`human_lostland_blacksmith`。
//!
//! 这一步是本批最容易静默失效的地方：回退链查不到只会退回分层合成，
//! **不打任何日志**，52 张图会一张都用不上而屏幕上毫无异常（`skin.rs`
//! 查裸名字导致五张 HUD 贴图全军覆没就是同一种失效）。因此对齐由
//! `crates/ll-game/tests/npc_appearance.rs` 的
//! `本体每一个种族与职业的组合在真实图集里都查得到合成图` 端到端钉住
//! ——那条断言的清单从注册表现查，加种族的那一刻它自动开始管。
//!
//! # 走 `LooseOnlyEntry`，不进 `placeholder.json`
//!
//! 与家具那批逐字同一条理由：`assets/atlas/placeholder.png` 是四张冻结
//! 像素基准的来源（见 `main.rs` 的 `LooseOnlyEntry` 文档，含 2026-08-29
//! 的更正段），往里加 52 条会把画布撑大、把那批基准
//! 卷进来，而 `ll-game` 本体二进制早就不读那张图了。见
//! [`crate::LooseOnlyEntry`] 文档。
//!
//! # 两个维度各自靠什么被看出来
//!
//! 所有者的硬要求是「要真的区分得开」，不是同一张图换个色调。
//!
//! | 维度 | 表达它的东西 |
//! | --- | --- |
//! | 种族 | 身高（`head_top`）、体宽（`shoulder_w`）、腿长、肤色、发色、耳型、胡子——全部来自 [`npc::BodySpec`]，一个字没改 |
//! | 职业 | 衣服色（签名色压暗）、帽子、袖子、胸口徽记、**手里那件工具** |
//!
//! 「手里那件工具」是本模块相对挂件那一套真正新增的东西：挂件只有胸口
//! 6×6 一块，两个同族不同职业的人在远处几乎看不出差别；一件从肩到膝的
//! 工具（铁匠的锤、渔夫的网、石匠的凿、法师的杖）是**轮廓级**的差异。
//!
//! 可核实的判据是 `crates/ll-game/tests/npc_appearance.rs` 里那条
//! 「任意两张合成图之间至少四分之一像素不同」（16×24 = 384，门槛 96），
//! 与家具那批的 `本体家具的贴图两两之间至少四分之一像素不同` 同一把
//! 尺子。本模块自己的单测在绘制函数这一侧先拦一道。
//!
//! # 确定性（约束 C5）
//!
//! 名字表是 [`npc::race_bodies`] × [`npc::profession_badges`] 两个数组
//! 字面量的笛卡尔积，种族在外层、职业在内层，不经任何哈希容器。

use crate::EntryRect;
use crate::npc::{self, BadgeSpec, BodySpec};
use crate::sprite::paint_patch;
use image::RgbaImage;

/// 本体命名空间。合成图的条目名里嵌的是**职业那一侧**的命名空间
/// （见模块文档「条目名必须与回退链拼出来的键逐字一致」）；种族那一侧
/// 的命名空间由 `asset_vfs` 在打包时自己补上，不出现在条目名里。
///
/// 本工具只产出本体（`lostland`）的资产，因此两侧都是它。
const BASE_NAMESPACE: &str = "lostland";

/// 工具区左上角的列号。落在画布右侧，压在右臂上——读成「握在手里」。
const TOOL_LEFT: u32 = 11;
/// 工具区左上角的行号。比最高的种族（精灵，`head_top = 0`）的肩线略低，
/// 比最矮的种族（哥布林，`head_top = 4`）的头顶略高。
const TOOL_TOP: u32 = 4;
/// 工具区宽度。
const TOOL_WIDTH: u32 = 5;
/// 工具区高度。底端落在 `TOOL_TOP + TOOL_HEIGHT = 20`，恰好在脚底
/// （`FEET_TOP = 21`）之上——工具不该埋进地里。
const TOOL_HEIGHT: u32 = 16;

/// 衣服色相对职业签名色压暗多少。压暗而不是直接用签名色：胸口那块徽记
/// 底板**就是**签名色，衣服与它同色的话徽记会整个消失在衣服里。
const TUNIC_DARKEN: f32 = 0.20;
/// 帽子色相对签名色压暗多少。比衣服再暗一档，让「帽子 / 衣服 / 徽记」
/// 三层在同一个色相上仍然分得出前后。
const CAP_DARKEN: f32 = 0.32;
/// 袖子色相对签名色压暗多少。取衣服与帽子之间。
const SLEEVE_DARKEN: f32 = 0.26;
/// 袖子盖住手臂顶端的行数。剩下的手臂保持肤色——手臂全被袖子盖住的话
/// 种族肤色在身上就只剩一张脸，跨种族的可分性会掉一大截。
const SLEEVE_ROWS: u32 = 4;

/// 一件职业工具的配方。
///
/// 与 [`npc::BadgeSpec`] 一样**没有任何种族相关的字段**：同一件工具画在
/// 任何种族的手里都成立。这正是「工具数 = 职业数」而不是「职业数 × 种族
/// 数」的原因，也是加第 5 个种族一行都不用碰本表的原因。
#[derive(Debug, Clone, Copy)]
struct ToolSpec {
    /// 工具主体色（金属/木料）。
    main: (u8, u8, u8),
    /// 点缀色（刃口、握把、绳结、宝石……）。
    accent: (u8, u8, u8),
    /// [`TOOL_WIDTH`]×[`TOOL_HEIGHT`] 的图形：`'#'` 主体、`'+'` 点缀、
    /// 其余透明（露出下面的身子）。
    glyph: [&'static str; TOOL_HEIGHT as usize],
}

/// 钢（刃、锤头、凿）。
const STEEL: (u8, u8, u8) = (176, 184, 196);
/// 木（柄、杖、弓臂）。
const WOOD: (u8, u8, u8) = (124, 88, 52);
/// 深木（与 [`WOOD`] 拉开一档，给以木为主体的那几件当点缀）。
const WOOD_DARK: (u8, u8, u8) = (72, 50, 30);
/// 绳/网/麻线。
const CORD: (u8, u8, u8) = (222, 208, 168);
/// 黄铜（钥匙、扣件）。
const BRASS: (u8, u8, u8) = (206, 168, 74);
/// 血/肉（屠夫的挂钩）。
const FLESH: (u8, u8, u8) = (166, 72, 76);
/// 麦秆黄（农夫的麦捆）。
const STRAW: (u8, u8, u8) = (214, 186, 92);
/// 叶绿（游侠/牧人的绿意）。
const LEAF: (u8, u8, u8) = (96, 148, 88);
/// 法术辉光（法师杖头）。
const ARCANE: (u8, u8, u8) = (156, 118, 232);
/// 石（石匠的砖）。
const STONE: (u8, u8, u8) = (148, 146, 140);
/// 水蓝（渔夫的网结与鱼）。
const WATER: (u8, u8, u8) = (86, 176, 190);

/// 十三件职业工具，顺序与 [`npc::profession_badges`] 一致（也就是
/// `mods/lostland/classes.json5` 的声明顺序）。**名字是职业 id 的本地
/// 名**，与挂件那张表逐字一致——[`tool_of`] 靠名字配对，对不上就 panic，
/// 不静默画一张没有工具的图。
const PROFESSION_TOOLS: [(&str, ToolSpec); 13] = [
    (
        // 战士：直剑，宽刃 + 十字护手。
        "warrior",
        ToolSpec {
            main: STEEL,
            accent: WOOD_DARK,
            glyph: [
                "..#..", ".###.", ".###.", ".###.", ".###.", ".###.", ".###.", ".###.", "+++++",
                "+++++", "..+..", "..+..", "..+..", "..+..", ".....", ".....",
            ],
        },
    ),
    (
        // 法师：长杖，顶端一颗发光的宝石。
        "mage",
        ToolSpec {
            main: WOOD,
            accent: ARCANE,
            glyph: [
                ".+++.", "+++++", "+++++", ".+++.", "..##.", "..##.", "..##.", "..##.", "..##.",
                "..##.", "..##.", "..##.", "..##.", "..##.", "..##.", ".....",
            ],
        },
    ),
    (
        // 游侠：长弓，弓臂在外、弓弦在内。
        "ranger",
        ToolSpec {
            main: WOOD,
            accent: CORD,
            glyph: [
                ".##..", "#..+.", "#..+.", "#..+.", "#..+.", "#..+.", "#..+.", "#..+.", "#..+.",
                "#..+.", "#..+.", "#..+.", "#..+.", "#..+.", ".##..", ".....",
            ],
        },
    ),
    (
        // 卫兵：塔盾，盾面压一道竖脊。
        "guard",
        ToolSpec {
            main: STEEL,
            accent: (66, 106, 168),
            glyph: [
                ".....", "#####", "#+++#", "#+#+#", "#+#+#", "#+#+#", "#+#+#", "#+#+#", "#+#+#",
                "#+#+#", "#+++#", "#####", ".###.", ".###.", "..#..", ".....",
            ],
        },
    ),
    (
        // 据点管理者：一串长柄钥匙。
        "steward",
        ToolSpec {
            main: BRASS,
            accent: WOOD_DARK,
            glyph: [
                ".###.", "##.##", "##.##", ".###.", "..##.", "..##.", "..##.", "..##.", "..##.",
                "..###", "..##.", "..###", "..##.", "..##.", ".....", ".....",
            ],
        },
    ),
    (
        // 民兵：斜握的长矛，矛尖朝上。
        "militia",
        ToolSpec {
            main: WOOD,
            accent: STEEL,
            glyph: [
                "...++", "..+++", "..+++", "..++.", "..##.", "..##.", ".##..", ".##..", ".##..",
                ".##..", "##...", "##...", "##...", "##...", "##...", ".....",
            ],
        },
    ),
    (
        // 农夫：一捆立着的麦，下面是绑绳。
        "farmer",
        ToolSpec {
            main: STRAW,
            accent: LEAF,
            glyph: [
                "#.#.#", "##.##", "#####", "#####", "#####", "#####", "+++++", "#####", "#####",
                "#####", "+++++", "..#..", "..#..", ".....", ".....", ".....",
            ],
        },
    ),
    (
        // 猎户：箭袋，露出三支箭羽。
        "hunter",
        ToolSpec {
            main: WOOD_DARK,
            accent: CORD,
            glyph: [
                "+.+.+", "+.+.+", "+++++", "#####", "#####", "#####", "#####", "#####", "#####",
                "#####", "#####", "#####", "#####", ".###.", ".....", ".....",
            ],
        },
    ),
    (
        // 屠夫：宽背剁刀，刀背朝上、刃口朝下。
        "butcher",
        ToolSpec {
            main: STEEL,
            accent: FLESH,
            glyph: [
                ".....", ".....", "#####", "#####", "#####", "#####", "#####", "#####", "####.",
                "+++..", "..+..", "..+..", "..+..", "..+..", ".....", ".....",
            ],
        },
    ),
    (
        // 铁匠：单手锤，锤头方而重。
        "blacksmith",
        ToolSpec {
            main: STEEL,
            accent: WOOD,
            glyph: [
                ".....", "#####", "#####", "#####", "#####", "..++.", "..++.", "..++.", "..++.",
                "..++.", "..++.", "..++.", "..++.", "..++.", ".....", ".....",
            ],
        },
    ),
    (
        // 渔夫：撒开的渔网，网眼是镂空的。
        "fisher",
        ToolSpec {
            main: CORD,
            accent: WATER,
            glyph: [
                "#.#.#", ".#.#.", "#.#.#", ".#.#.", "#.#.#", ".#.#.", "#.#.#", ".#.#.", "#.#.#",
                ".#.#.", "#.#.#", ".+++.", "+++++", ".+++.", ".....", ".....",
            ],
        },
    ),
    (
        // 牧羊人：牧杖，顶端一个大弯钩。
        "shepherd",
        ToolSpec {
            main: WOOD,
            accent: LEAF,
            glyph: [
                ".+++.", "++.++", "++.++", ".++..", "..##.", "..##.", "..##.", "..##.", "..##.",
                "..##.", "..##.", "..##.", "..##.", "..##.", "..##.", ".....",
            ],
        },
    ),
    (
        // 石匠：凿子 + 一摞错缝的砖。
        "mason",
        ToolSpec {
            main: STONE,
            accent: STEEL,
            glyph: [
                "..+..", "..+..", "..+..", "..+..", ".+++.", ".....", "#####", "##.##", "#####",
                ".#.#.", "#####", "##.##", "#####", ".....", ".....", ".....",
            ],
        },
    ),
];

/// 本体全部合成图的条目名，种族在外层、职业在内层。
///
/// 名字形状见模块文档。返回 `Vec<String>` 而不是 `&'static [&str]`：
/// 52 个名字是两张表的笛卡尔积**现拼**的，写死成字面量等于把同一份
/// 信息抄第三遍，加种族时又多一处要同步的地方。
pub(crate) fn composite_names() -> Vec<String> {
    let mut names = Vec::with_capacity(npc::race_bodies().len() * npc::profession_badges().len());
    for (race, _) in npc::race_bodies() {
        for (class, _) in npc::profession_badges() {
            names.push(format!("{race}_{BASE_NAMESPACE}_{class}"));
        }
    }
    names
}

/// 画布尺寸——与种族身子/职业挂件同一档，合成图本来就是把那两层叠出来
/// 再加料，尺寸不一致就谈不上「回退链换一层图而画面不跳」。
pub(crate) const COMPOSITE_WIDTH: u32 = npc::NPC_WIDTH;
/// 画布高度，理由同 [`COMPOSITE_WIDTH`]。
pub(crate) const COMPOSITE_HEIGHT: u32 = npc::NPC_HEIGHT;

/// 按条目名查一份画法。认不出这个名字返回 `false`，交回
/// [`crate::draw_entry`] 继续往下试。
pub(crate) fn draw_named(image: &mut RgbaImage, name: &str, rect: EntryRect) -> bool {
    let Some((race, class)) = split_name(name) else {
        return false;
    };
    let Some((_, body)) = npc::race_bodies().iter().find(|(entry, _)| *entry == race) else {
        return false;
    };
    let Some((_, badge)) = npc::profession_badges()
        .iter()
        .find(|(entry, _)| *entry == class)
    else {
        return false;
    };
    draw_composite(image, rect, *body, *badge, tool_of(class));
    true
}

/// 把 `human_lostland_blacksmith` 拆回 `("human", "blacksmith")`。
///
/// 中间那一段必须**恰好**是 [`BASE_NAMESPACE`]：本体种族的本地名里没有
/// `_lostland_` 这种东西，因此这一步不会把别的条目名误认成合成图。认不
/// 出来返回 `None` 而不是猜——猜错会让某个条目静默画成另一张图。
fn split_name(name: &str) -> Option<(&str, &str)> {
    let separator = format!("_{BASE_NAMESPACE}_");
    let (race, class) = name.split_once(&separator)?;
    (!race.is_empty() && !class.is_empty()).then_some((race, class))
}

/// 这个职业的工具配方。
///
/// 查不到直接 panic 而不是画一张没有工具的图：职业挂件那张表与本表必须
/// 一一对应，少一条意味着某个职业的 4 张合成图全部退化成「只换了件衣服
/// 的同一个人」——那正是所有者报的「所有 NPC 长得一模一样」的小型翻版，
/// 而静默画出来没人会发现。
fn tool_of(class: &str) -> ToolSpec {
    PROFESSION_TOOLS
        .iter()
        .find(|(entry, _)| *entry == class)
        .map(|(_, spec)| *spec)
        .unwrap_or_else(|| {
            panic!("职业 '{class}' 有挂件配方却没有工具配方：请在 composite.rs 的 PROFESSION_TOOLS 里补一行")
        })
}

/// 把一个种族 + 一个职业画成一张合成图。
///
/// 层序：种族身子（原样）→ 职业衣服/帽子/袖子 → 胸口徽记 → 手里的工具。
/// 后画的盖住先画的，与运行期那条「身子 → 挂件」的绘制顺序同向。
fn draw_composite(
    image: &mut RgbaImage,
    rect: EntryRect,
    body: BodySpec,
    badge: BadgeSpec,
    tool: ToolSpec,
) {
    npc::draw_race_body(image, rect, body);
    paint_profession_garment(image, rect, body, npc::badge_plate(badge));
    npc::draw_profession_badge(image, rect, badge);
    paint_tool(image, rect, tool);
}

/// 把身子上「衣服」那几块换成职业签名色的三档明暗。
///
/// 换的是**衣服**不是身体：裤子、鞋、皮肤、头发、耳朵、胡子一个像素都
/// 不碰——那些是种族那条轴的表达手段，被职业盖掉的话「同一个职业的两个
/// 种族」就分不开了。
fn paint_profession_garment(
    image: &mut RgbaImage,
    rect: EntryRect,
    body: BodySpec,
    signature: (u8, u8, u8),
) {
    let geometry = body.geometry();
    let tunic = npc::darken_color(signature, TUNIC_DARKEN);
    let cap = npc::darken_color(signature, CAP_DARKEN);
    let sleeve = npc::darken_color(signature, SLEEVE_DARKEN);

    let (tx, ty, tw, th) = geometry.torso;
    paint_patch(image, rect, tx, ty, tw, th, tunic);
    let (hx, hy, hw, hh) = geometry.hair;
    paint_patch(image, rect, hx, hy, hw, hh, cap);
    for (ax, ay, aw, ah) in geometry.arms {
        paint_patch(image, rect, ax, ay, aw, ah.min(SLEEVE_ROWS), sleeve);
    }
}

/// 把工具那张图形画进 [`TOOL_LEFT`]/[`TOOL_TOP`] 那块区域。
fn paint_tool(image: &mut RgbaImage, rect: EntryRect, tool: ToolSpec) {
    assert!(
        tool.glyph
            .iter()
            .all(|line| line.chars().count() == TOOL_WIDTH as usize),
        "工具图形每一行必须恰好 {TOOL_WIDTH} 个字符：短了会在右侧留一条空列，长了会画到画布外"
    );
    for (row, line) in tool.glyph.iter().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let color = match ch {
                '#' => tool.main,
                '+' => tool.accent,
                _ => continue,
            };
            paint_patch(
                image,
                rect,
                TOOL_LEFT + col as u32,
                TOOL_TOP + row as u32,
                1,
                1,
                color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: COMPOSITE_WIDTH,
        height: COMPOSITE_HEIGHT,
    };

    /// 与 `crates/ll-game/tests/npc_appearance.rs` 那条端到端断言同一把
    /// 尺子：16×24 = 384，四分之一是 96。本条在**绘制函数**这一侧先拦
    /// 一道，端到端那条再验「打包进真实图集之后仍然如此」。
    const QUARTER: usize = (COMPOSITE_WIDTH * COMPOSITE_HEIGHT) as usize / 4;

    fn render(name: &str) -> RgbaImage {
        let mut image = RgbaImage::new(COMPOSITE_WIDTH, COMPOSITE_HEIGHT);
        assert!(draw_named(&mut image, name, RECT), "认不出条目名 {name}");
        image
    }

    #[test]
    fn 合成图的数量等于种族数乘职业数() {
        // Arrange & Act
        let names = composite_names();

        // Assert
        assert_eq!(
            names.len(),
            npc::race_bodies().len() * npc::profession_badges().len()
        );
        assert_eq!(names.len(), 52, "本体现有 4 个种族 × 13 个职业");
    }

    #[test]
    fn 每个条目名都拆得回一个已知的种族与一个已知的职业() {
        // 这条守的是「名字与画法对得上」：`draw_named` 认不出名字时返回
        // `false`，而 `draw_entry` 会一路落到 `panic!("不知道如何绘制")`
        // ——那是响的失败。真正危险的是名字拼错但恰好还能拆开，画出一张
        // 张冠李戴的图。
        //
        // 反例（本次开发实跑）：把 `composite_names` 里的分隔符写成
        // `"{race}_{class}"`（漏掉命名空间那一段），本条报「认不出条目
        // 名 human_blacksmith」。
        // Arrange & Act & Assert
        for name in composite_names() {
            let (race, class) = split_name(&name).unwrap_or_else(|| panic!("{name} 拆不开"));
            assert!(npc::race_bodies().iter().any(|(entry, _)| *entry == race));
            assert!(
                npc::profession_badges()
                    .iter()
                    .any(|(entry, _)| *entry == class)
            );
        }
    }

    #[test]
    fn 每个职业都有一件工具且工具本身画得够大() {
        // 工具是「职业」这条轴在远景里唯一读得出来的东西（胸口 6×6 的
        // 徽记在缩略图上基本消失）。太小的工具等于没有。
        // Arrange
        let mut smallest = usize::MAX;

        // Act & Assert
        for (class, _) in npc::profession_badges() {
            let tool = tool_of(class);
            let painted = tool
                .glyph
                .iter()
                .map(|line| line.chars().filter(|c| *c != '.').count())
                .sum::<usize>();
            // 行宽由 `paint_tool` 自己断言（画到画布外是它的事），这里
            // 只管「够不够大」。
            assert!(painted >= 30, "职业 {class} 的工具只画了 {painted} 个像素");
            smallest = smallest.min(painted);
        }
        assert!(smallest >= 30);
    }

    #[test]
    fn 五十二张合成图两两之间至少四分之一像素不同() {
        // 所有者的硬要求「要真的区分得开，不是同一张图换个色调」的可执行
        // 版本，也是任务书点名要照抄家具那批的那条判据。1326 对全比。
        //
        // 反例（本次开发实跑）：把 `paint_tool` 整个函数体注释掉、并让
        // `paint_profession_garment` 只画躯干不画帽子与袖子，本条在
        // 「哥布林」那几对上报「只有 72 个像素不同（门槛 96）」。
        // Arrange
        let rendered: Vec<(String, RgbaImage)> = composite_names()
            .into_iter()
            .map(|name| {
                let image = render(&name);
                (name, image)
            })
            .collect();

        // Act & Assert
        for (i, (name_a, image_a)) in rendered.iter().enumerate() {
            for (name_b, image_b) in &rendered[i + 1..] {
                let differing = image_a
                    .pixels()
                    .zip(image_b.pixels())
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    differing >= QUARTER,
                    "{name_a} 与 {name_b} 只有 {differing} 个像素不同（门槛 {QUARTER}）"
                );
            }
        }
    }

    #[test]
    fn 合成图留出透明四角让地形透出来() {
        // 与 `furniture.rs`/`world_marks.rs` 的同名判据一致：角色画在
        // `Layer::ENTITY`，铺满不透明底色会把这一格读成「地形变了」。
        // Arrange & Act & Assert
        for name in composite_names() {
            let image = render(&name);
            for corner in [
                (0, 0),
                (COMPOSITE_WIDTH - 1, 0),
                (0, COMPOSITE_HEIGHT - 1),
                (COMPOSITE_WIDTH - 1, COMPOSITE_HEIGHT - 1),
            ] {
                assert_eq!(
                    image.get_pixel(corner.0, corner.1).0[3],
                    0,
                    "{name} 的角落 {corner:?} 应当透明"
                );
            }
        }
    }

    #[test]
    fn 职业只换衣服不换身体() {
        // 「种族」这条轴的表达手段是肤色/发色/耳型/胡子/裤子鞋——职业那
        // 一层如果把它们盖掉，同一个职业的四个种族就分不开了，而那正是
        // `npc_appearance.rs` 的 `同一个职业下不同种族至少差四分之一张图`
        // 要拦的事。这条在绘制侧把边界钉死：脚底那三行（鞋）只由种族决定。
        // Arrange
        let boots_of = |name: &str| {
            let image = render(name);
            (0..COMPOSITE_WIDTH)
                .flat_map(|x| (21..COMPOSITE_HEIGHT).map(move |y| (x, y)))
                .map(|(x, y)| *image.get_pixel(x, y))
                .collect::<Vec<_>>()
        };

        // Act
        let smith = boots_of("dwarf_lostland_blacksmith");
        let fisher = boots_of("dwarf_lostland_fisher");

        // Assert
        assert_eq!(smith, fisher, "同一个种族的鞋不该随职业变");
    }
}
