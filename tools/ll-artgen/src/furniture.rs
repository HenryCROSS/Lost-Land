//! 本体家具贴图：椅、长桌、卧铺、书柜、酒桶、铁箍箱六张。
//!
//! # 这一组图的地位与锻炉完全相同
//!
//! 它们都是**内容自己顺手带的一张图**，不是引擎的兜底记号。渲染层
//! （`ll_game::surface_draw::placed_furniture_draws`）拿这件物品的完整
//! 命名空间 ID 去图集里查同名条目，查得到就用，查不到退回
//! `world_marks::decorate_furniture_placed` 那张通用记号——引擎里没有
//! 任何一处按 `lostland:oak_chair` 分支。把本模块整个删掉，六件家具会
//! 自动变回六个一样的紫罗兰箱子，引擎一行都不用改。
//!
//! 因此**条目名必须与内容 id 的本地名逐字一致**
//! （`lostland:oak_chair` → `oak_chair`）：`ll_mod::asset_vfs` 把清单
//! 条目名与所属命名空间拼成图集条目名（`ResolvedSprite::atlas_name`），
//! 运行期真正被查的键是**带前缀的** `lostland:oak_chair`。上一批五张
//! HUD 贴图正是栽在这一步（查裸名字、图集里存的是带前缀的，五张全部
//! 静默失效、不打任何日志），所以本批的对齐由
//! `crates/ll-game/tests/atlas_coverage.rs` 里一条端到端断言钉住，不靠
//! 记性。
//!
//! # 六张图都走 `LooseOnlyEntry`，不进 `placeholder.json`
//!
//! 那份 JSON 描述的共享画布 `assets/atlas/placeholder.png` 是四张
//! **冻结像素基准**的来源（见 `main.rs` 的 `LooseOnlyEntry`
//! 文档）。往里加条目会撑大画布、把那批基准卷进来，换不到任何东西
//! ——`ll-game` 本体二进制早就不读那张图了。锻炉在里面是历史原因
//! （家具层落地时那条路径还没分岔），新的六张不跟着进去。
//!
//! # 底色留空
//!
//! 与 `world_marks.rs` 同一条：家具画在 `Layer::DECOR`，铺满不透明底色
//! 会把这一格读成「地形变了」而不是「这一格上摆着东西」。每张图都留出
//! 四角透明。
//!
//! # 配色为什么是这几个
//!
//! 硬要求只有一条：**六件之间、以及与既有那几张之间，一眼分得开**。
//! 要避开的既有颜色有三组——地形主色（绿/褐/蓝/沙黄/灰/近白）、角色色
//! （玩家钢蓝、boss 暗红、NPC 紫红）、以及 `world_marks.rs` 那三张
//! （琥珀橙的物品堆、紫罗兰的通用家具记号、锻炉的石灰 + 橙红炉火）。
//!
//! 四件木器共用一套**橡木棕**（比土地形更红、比物品堆的琥珀橙更暗），
//! 靠**轮廓**而不是颜色互相区分——椅是细的、桌是宽的、柜是高的、桶是
//! 圆的。这是刻意的：它们在设定里本来就是同一种木料做的，给每件换一种
//! 颜色只会让画面读起来像四种材质。真正靠颜色区分的是另外两件：卧铺走
//! 灰褐毛皮 + 米白亚麻（它本来就不是木头），铁箍箱在橡木上压**冷调
//! 钢色**的箍条（暖木配冷铁，是它一眼可辨的那一笔）。

use crate::EntryRect;
use crate::sprite::paint_patch;
use image::RgbaImage;

/// 橡木受光面（最亮的一档）。
const OAK_LIGHT: (u8, u8, u8) = (170, 122, 74);
/// 橡木主体色。
const OAK_BODY: (u8, u8, u8) = (134, 90, 52);
/// 橡木暗边／背光面，兼作四件木器的轮廓色。
const OAK_DARK: (u8, u8, u8) = (72, 46, 26);

/// 毛皮卧铺的皮面色（灰褐）——刻意不用橡木那套暖棕：卧铺不是木器，
/// 它与旁边的椅子桌子摆在同一间屋里也该一眼看出是两种东西。
const PELT_BODY: (u8, u8, u8) = (148, 138, 122);
/// 毛皮卧铺的褶皱暗色。
const PELT_DARK: (u8, u8, u8) = (86, 78, 66);
/// 亚麻衬里色（米白）。它同时是这张图与其余五张拉开距离的那一笔。
const LINEN: (u8, u8, u8) = (224, 214, 188);

/// 铁件冷调钢色（箱子的箍条、酒桶的桶箍）。与锻炉的石灰
/// `(96,90,86)` 刻意不同：那是暖灰的石头，这是冷调的铁。
const IRON_BAND: (u8, u8, u8) = (104, 116, 128);
/// 铁件高光。
const IRON_LIGHT: (u8, u8, u8) = (156, 168, 180);
/// 锁扣／合页的黄铜色。全组唯一一处暖金，只用在箱子上。
const BRASS: (u8, u8, u8) = (206, 168, 74);

/// 书脊三色。书柜靠它们与其余三件木器拉开距离——同样的橡木轮廓，
/// 格子里塞着彩色的东西。
const BOOK_RED: (u8, u8, u8) = (166, 62, 54);
const BOOK_BLUE: (u8, u8, u8) = (62, 92, 158);
const BOOK_GREEN: (u8, u8, u8) = (78, 132, 76);

/// 画 `oak_chair`：橡木椅。
///
/// 全组最「瘦」的一张——靠背在左、椅面居中、两条腿落地，四周留大片
/// 透明。椅子在屋里本来就占不满一格，画满反而与长桌读不开。
pub(crate) fn decorate_oak_chair(image: &mut RgbaImage, rect: EntryRect) {
    // 靠背：竖在左侧，上端两根立柱之间留一道空。
    paint_patch(image, rect, 3, 2, 2, 9, OAK_DARK);
    paint_patch(image, rect, 3, 3, 1, 7, OAK_BODY);
    paint_patch(image, rect, 3, 4, 2, 1, OAK_LIGHT);
    paint_patch(image, rect, 3, 7, 2, 1, OAK_LIGHT);
    // 椅面
    paint_patch(image, rect, 3, 9, 9, 3, OAK_DARK);
    paint_patch(image, rect, 4, 10, 7, 1, OAK_LIGHT);
    // 两条腿
    paint_patch(image, rect, 4, 12, 2, 3, OAK_BODY);
    paint_patch(image, rect, 9, 12, 2, 3, OAK_BODY);
}

/// 画 `oak_table`：橡木长桌。
///
/// 与椅子正好相反：一整条横过去的桌面加四条腿，横向撑满。「宽 vs 瘦」
/// 是这两件在同一间屋里唯一需要的区分，比换颜色可靠得多。
pub(crate) fn decorate_oak_table(image: &mut RgbaImage, rect: EntryRect) {
    // 桌面：外框暗色、里面亮色，读成一块厚板。
    paint_patch(image, rect, 1, 5, 14, 4, OAK_DARK);
    paint_patch(image, rect, 2, 6, 12, 2, OAK_BODY);
    paint_patch(image, rect, 3, 6, 10, 1, OAK_LIGHT);
    // 四条腿
    for leg_dx in [2, 6, 9, 12] {
        paint_patch(image, rect, leg_dx, 9, 2, 5, OAK_BODY);
        paint_patch(image, rect, leg_dx, 13, 2, 1, OAK_DARK);
    }
}

/// 画 `fur_bed`：毛皮卧铺。
///
/// 一卷铺开的毛皮压着一条亚麻衬里，贴地、压低——它是全组唯一一件
/// 「躺在地上」而不是「立在地上」的家具，轮廓因此刻意压在下半格，
/// 上半格整片透明。
pub(crate) fn decorate_fur_bed(image: &mut RgbaImage, rect: EntryRect) {
    // 铺开的皮面
    paint_patch(image, rect, 1, 8, 14, 7, PELT_DARK);
    paint_patch(image, rect, 2, 9, 12, 5, PELT_BODY);
    // 三道褶皱：让这块面读成「毛的」而不是一块布板。
    paint_patch(image, rect, 4, 10, 1, 4, PELT_DARK);
    paint_patch(image, rect, 8, 10, 1, 4, PELT_DARK);
    paint_patch(image, rect, 11, 10, 1, 4, PELT_DARK);
    // 枕头那一头的亚麻衬里
    paint_patch(image, rect, 2, 6, 6, 3, PELT_DARK);
    paint_patch(image, rect, 3, 7, 4, 1, LINEN);
    paint_patch(image, rect, 2, 13, 12, 1, LINEN);
}

/// 画 `oak_bookshelf`：橡木书柜。
///
/// 同一套橡木，靠**高**（顶到画布上沿）与**格子里的彩色书脊**与另外
/// 三件区分。三种书脊色也是本模块唯一一处高饱和色。
pub(crate) fn decorate_oak_bookshelf(image: &mut RgbaImage, rect: EntryRect) {
    // 柜体外框
    paint_patch(image, rect, 2, 0, 12, 16, OAK_DARK);
    paint_patch(image, rect, 3, 1, 10, 14, OAK_BODY);
    // 三层隔板
    for shelf_dy in [5, 10] {
        paint_patch(image, rect, 3, shelf_dy, 10, 1, OAK_DARK);
    }
    // 书脊：每层三本，颜色轮换。
    for (row_dy, colors) in [
        (2, [BOOK_RED, BOOK_BLUE, BOOK_GREEN]),
        (7, [BOOK_GREEN, BOOK_RED, BOOK_BLUE]),
        (12, [BOOK_BLUE, BOOK_GREEN, BOOK_RED]),
    ] {
        for (slot, color) in colors.into_iter().enumerate() {
            paint_patch(image, rect, 4 + slot as u32 * 3, row_dy, 2, 3, color);
        }
    }
}

/// 画 `oak_barrel`：橡木酒桶。
///
/// 靠**圆**与另外三件木器区分：桶身左右各削掉一列（上下窄、中间宽），
/// 再压两道冷调铁箍。铁箍同时是它与书柜的第二重区分——书柜上没有任何
/// 冷色。
pub(crate) fn decorate_oak_barrel(image: &mut RgbaImage, rect: EntryRect) {
    // 桶身：上下两端窄一圈，中段最宽，读成一个鼓肚子的桶。
    paint_patch(image, rect, 4, 1, 8, 14, OAK_DARK);
    paint_patch(image, rect, 3, 4, 10, 8, OAK_DARK);
    paint_patch(image, rect, 5, 2, 6, 12, OAK_BODY);
    paint_patch(image, rect, 4, 5, 8, 6, OAK_BODY);
    // 桶板缝：两道竖线。
    paint_patch(image, rect, 6, 3, 1, 10, OAK_LIGHT);
    paint_patch(image, rect, 9, 3, 1, 10, OAK_LIGHT);
    // 两道铁箍
    paint_patch(image, rect, 4, 5, 8, 2, IRON_BAND);
    paint_patch(image, rect, 4, 10, 8, 2, IRON_BAND);
    paint_patch(image, rect, 4, 5, 8, 1, IRON_LIGHT);
}

/// 画 `iron_bound_chest`：铁箍箱。
///
/// 橡木箱体上压两道**冷调钢箍**加一枚黄铜锁——暖木配冷铁再点一笔金，
/// 是它一眼可辨的那一笔，也是全组唯一一处黄铜。轮廓是矮而宽的箱体，
/// 与通用家具记号那个高箱子（`world_marks::decorate_furniture_placed`）
/// 刻意不同：这两张最容易被认成同一件东西，因此专门错开高度与配色。
pub(crate) fn decorate_iron_bound_chest(image: &mut RgbaImage, rect: EntryRect) {
    // 箱盖（圆拱的暗示：比箱身窄一圈）
    paint_patch(image, rect, 2, 4, 12, 4, OAK_DARK);
    paint_patch(image, rect, 3, 5, 10, 2, OAK_LIGHT);
    // 箱身
    paint_patch(image, rect, 1, 8, 14, 7, OAK_DARK);
    paint_patch(image, rect, 2, 9, 12, 5, OAK_BODY);
    // 两道竖向铁箍
    for band_dx in [4, 10] {
        paint_patch(image, rect, band_dx, 4, 2, 11, IRON_BAND);
        paint_patch(image, rect, band_dx, 4, 1, 11, IRON_LIGHT);
    }
    // 锁扣
    paint_patch(image, rect, 7, 7, 2, 3, BRASS);
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: EntryRect = EntryRect {
        x: 0,
        y: 0,
        width: 16,
        height: 16,
    };

    /// 一份「条目名 → 画法」配对，供下面几条断言遍历。
    type NamedDraw = (&'static str, fn(&mut RgbaImage, EntryRect));

    /// 本模块六张图，按 `draw_entry` 的派发名与画法配对。清单在这里
    /// 手写一次，是因为**测试要断言的正是「这六个名字各有一张不同的
    /// 图」**——从别处现查会把这条断言变成同义反复。
    fn all_furniture() -> [NamedDraw; 6] {
        [
            ("oak_chair", decorate_oak_chair),
            ("oak_table", decorate_oak_table),
            ("fur_bed", decorate_fur_bed),
            ("oak_bookshelf", decorate_oak_bookshelf),
            ("oak_barrel", decorate_oak_barrel),
            ("iron_bound_chest", decorate_iron_bound_chest),
        ]
    }

    fn render(draw: fn(&mut RgbaImage, EntryRect)) -> RgbaImage {
        let mut image = RgbaImage::new(TILE.width, TILE.height);
        draw(&mut image, TILE);
        image
    }

    #[test]
    fn 六件家具每一件都画了东西而不是一张空图() {
        // Arrange & Act & Assert
        for (name, draw) in all_furniture() {
            let image = render(draw);
            let opaque = image.pixels().filter(|p| p.0[3] == 255).count();
            assert!(
                opaque >= 40,
                "{name} 只有 {opaque} 个不透明像素，屏幕上等于没画"
            );
        }
    }

    #[test]
    fn 六件家具两两之间至少四分之一像素不同() {
        // 判据与 `crates/ll-game/tests/atlas_coverage.rs` 那条地形贴图
        // 的门槛逐字相同（16×16 = 256 像素，门槛 64）：它不是「画得好
        // 不好看」，是「有没有两张被写成几乎一样」的下界。摆在同一间
        // 屋里的两件家具必须一眼分得开。
        // Arrange
        let rendered: Vec<(&str, RgbaImage)> = all_furniture()
            .into_iter()
            .map(|(name, draw)| (name, render(draw)))
            .collect();
        let threshold = (TILE.width * TILE.height) as usize / 4;

        // Act & Assert
        for (i, (name_a, image_a)) in rendered.iter().enumerate() {
            for (name_b, image_b) in &rendered[i + 1..] {
                let differing = image_a
                    .pixels()
                    .zip(image_b.pixels())
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    differing >= threshold,
                    "{name_a} 与 {name_b} 只有 {differing} 个像素不同（门槛 {threshold}）"
                );
            }
        }
    }

    #[test]
    fn 六件家具都留出透明四角让地形透出来() {
        // 与 `world_marks.rs` 的同名判据一致：铺满不透明底色会把这一格
        // 读成「地形变了」，见模块文档「底色留空」。
        // Arrange & Act & Assert
        for (name, draw) in all_furniture() {
            let image = render(draw);
            for corner in [(0, 0), (15, 0), (0, 15), (15, 15)] {
                assert_eq!(
                    image.get_pixel(corner.0, corner.1).0[3],
                    0,
                    "{name} 的角落 {corner:?} 应当透明"
                );
            }
        }
    }

    #[test]
    fn 卧铺的上半格是空的因为它躺在地上() {
        // 六件里唯一一件不「立着」的东西，轮廓压在下半格——这条把
        // 模块文档里那句设计判据变成可执行的。
        // Arrange
        let image = render(decorate_fur_bed);

        // Act
        let top_opaque = (0..16)
            .flat_map(|x| (0..6).map(move |y| (x, y)))
            .filter(|&(x, y)| image.get_pixel(x, y).0[3] != 0)
            .count();

        // Assert
        assert_eq!(top_opaque, 0, "卧铺的上六行应当整片透明");
    }

    #[test]
    fn 只有铁箍箱用黄铜色() {
        // 黄铜是本组唯一一处暖金，专供箱子的锁扣——它同时是箱子与
        // 通用家具记号（紫罗兰箱体）最直接的区分。这条守住那个约定。
        // Arrange
        let brass = image::Rgba([BRASS.0, BRASS.1, BRASS.2, 255]);

        // Act & Assert
        for (name, draw) in all_furniture() {
            let image = render(draw);
            let has_brass = image.pixels().any(|pixel| *pixel == brass);
            assert_eq!(
                has_brass,
                name == "iron_bound_chest",
                "{name} 的黄铜用量与约定不符"
            );
        }
    }
}
