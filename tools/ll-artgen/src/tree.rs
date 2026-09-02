//! 三张树贴图：橡树、松树、棕榈。
//!
//! # 这一组图的地位与家具那六张完全相同
//!
//! 它们都是**画在世界格子上的一层记号**，16×16 铺一格、锚点与占地与地形
//! 那一档一致。渲染层（`ll_game::surface_draw::tree_draws`）拿
//! `lostland:` + `ll_world::tree::TreeSpecies::sprite_stem` 去图集里查
//! 同名条目，查不到就**不画这一格的树**（地形底图照常）。
//!
//! 因此**条目名必须与 `TreeSpecies::sprite_stem` 逐字一致**
//! （`TreeSpecies::Oak` → `tree_oak`）：`ll_mod::asset_vfs` 把清单条目名与
//! 所属命名空间拼成图集条目名，运行期真正被查的键是**带前缀的**
//! `lostland:tree_oak`。加一种树要动三处：`TreeSpecies` 一个变体、
//! [`TREE_NAMES`](crate::TREE_NAMES) 一行、`draw_entry` 一支。少了任何一处，
//! `crates/ll-game/tests/atlas_coverage.rs` 的
//! `每一种树在真实图集里都查得到条目` 与
//! `图集里不许有声明侧数不出来的树贴图` 会从两个方向各红一次。
//!
//! # 三张图都走 `LooseOnlyEntry`，不进 `placeholder.json`
//!
//! 与家具那六张同一条：那份 JSON 描述的共享画布是四张**冻结像素基准**
//! 的来源，往里加条目会撑大画布、把那批基准卷进来，换不到任何东西。
//!
//! # 底色留空
//!
//! 树画在 `Layer::DECOR`（地形之上、人之下），铺满不透明底色会把这一格
//! 读成「地形变了」而不是「这一格上长着一棵树」。三张都留出四角透明，
//! 且**树冠都不顶满**——顶满会让相邻两格的树冠连成一片色块，成排时读不
//! 出「一棵一棵」。
//!
//! # 三张靠什么互相区分
//!
//! 硬要求只有一条：**三种树一眼分得开**，而且要与既有那批图分得开。
//! 既有的绿色只有草地/森林两块地形主色，树冠若也用同一档绿，树就会
//! 「陷进」底图里——所以三张的树冠**一律比地形绿更深或更冷**。
//!
//! 三者之间**靠轮廓而不是颜色**区分（与四件橡木家具同一条判断）：
//!
//! | 树种 | 轮廓 | 一眼可辨的那一笔 |
//! |---|---|---|
//! | 橡树 | 宽而圆的冠，短粗主干 | 最宽，冠压到两侧边缘 |
//! | 松树 | 三层收窄的尖塔，细直干 | 唯一的三角轮廓 |
//! | 棕榈 | 细长弯杆，顶上四片散叶 | 唯一「杆比冠高」的一张 |
//!
//! 颜色只做辅助：橡树暖绿、松树冷墨绿、棕榈黄绿。**换掉颜色三张仍然
//! 分得开**，这是刻意的——色盲玩家与夜间色调（`tile_tint` 会整体压暗）
//! 下轮廓仍然有效，颜色未必。

use crate::EntryRect;
use crate::sprite::paint_patch;
use image::RgbaImage;

/// 橡树冠的受光面。比地形草地绿更深、更暖。
const OAK_LEAF_LIGHT: (u8, u8, u8) = (96, 142, 62);
/// 橡树冠主体。
const OAK_LEAF_BODY: (u8, u8, u8) = (68, 108, 46);
/// 橡树冠暗边，兼作轮廓色。
const OAK_LEAF_DARK: (u8, u8, u8) = (42, 70, 30);

/// 松树冠：**冷**墨绿，与橡树那套暖绿拉开一档。
const PINE_LEAF_LIGHT: (u8, u8, u8) = (62, 116, 96);
/// 松树冠主体。
const PINE_LEAF_BODY: (u8, u8, u8) = (38, 82, 68);
/// 松树冠暗边。
const PINE_LEAF_DARK: (u8, u8, u8) = (22, 52, 44);

/// 棕榈叶：偏黄的绿，三张里最亮的一档——它长在沙漠边上，本来就该显得
/// 干、显得晒。
const PALM_LEAF_LIGHT: (u8, u8, u8) = (150, 168, 70);
/// 棕榈叶主体。
const PALM_LEAF_BODY: (u8, u8, u8) = (112, 132, 50);

/// 树干色（三张共用的暖褐）。刻意**不用**家具那套橡木棕
/// （`(134,90,52)`）：那是加工过的木料，这是带皮的活树。
const TRUNK_BODY: (u8, u8, u8) = (92, 66, 42);
/// 树干暗边／轮廓。
const TRUNK_DARK: (u8, u8, u8) = (54, 38, 24);

/// 画 `tree_oak`：橡树。
///
/// 三张里**最宽**的一张：树冠横向压到两侧边缘，主干短粗。「宽 vs 尖 vs
/// 细高」是三张唯一需要的区分，比换颜色可靠得多。
pub(crate) fn decorate_oak(image: &mut RgbaImage, rect: EntryRect) {
    // 主干：短、粗，落在下三分之一。
    paint_patch(image, rect, 6, 10, 4, 6, TRUNK_DARK);
    paint_patch(image, rect, 7, 10, 2, 5, TRUNK_BODY);
    // 树冠：三段横条堆成一个圆，两侧收进 1 像素做出圆角。
    paint_patch(image, rect, 2, 2, 12, 8, OAK_LEAF_DARK);
    paint_patch(image, rect, 1, 4, 14, 4, OAK_LEAF_DARK);
    paint_patch(image, rect, 3, 3, 10, 6, OAK_LEAF_BODY);
    paint_patch(image, rect, 2, 5, 12, 2, OAK_LEAF_BODY);
    // 受光面偏左上：全组光照方向统一，否则一屏树看起来像各照各的灯。
    paint_patch(image, rect, 4, 3, 5, 2, OAK_LEAF_LIGHT);
    paint_patch(image, rect, 3, 5, 3, 2, OAK_LEAF_LIGHT);
}

/// 画 `tree_pine`：松树。
///
/// 三张里唯一的**三角轮廓**：三层由宽到窄的塔，细直干。
pub(crate) fn decorate_pine(image: &mut RgbaImage, rect: EntryRect) {
    // 细直干
    paint_patch(image, rect, 7, 12, 2, 4, TRUNK_DARK);
    paint_patch(image, rect, 7, 12, 1, 3, TRUNK_BODY);
    // 三层塔，自下而上收窄——每层都比上一层宽 2 像素。
    for (dy, dx, w, h) in [(9u32, 2u32, 12u32, 4u32), (5, 3, 10, 4), (1, 5, 6, 4)] {
        paint_patch(image, rect, dx, dy, w, h, PINE_LEAF_DARK);
        paint_patch(image, rect, dx + 1, dy, w - 2, h - 1, PINE_LEAF_BODY);
        paint_patch(image, rect, dx + 1, dy, w / 3, 1, PINE_LEAF_LIGHT);
    }
}

/// 画 `tree_palm`：棕榈。
///
/// 三张里唯一「**杆比冠高**」的一张：一根细长略弯的杆撑起顶上四片散叶，
/// 中间大片透明。它与另外两张的差别在剪影上就已经成立。
pub(crate) fn decorate_palm(image: &mut RgbaImage, rect: EntryRect) {
    // 弯杆：四段各向右偏一点，读出一条弧。
    paint_patch(image, rect, 6, 13, 2, 3, TRUNK_DARK);
    paint_patch(image, rect, 7, 10, 2, 3, TRUNK_DARK);
    paint_patch(image, rect, 7, 7, 2, 3, TRUNK_BODY);
    paint_patch(image, rect, 8, 5, 2, 2, TRUNK_BODY);
    // 四片散叶：两长两短，向四角披下去。
    //
    // **叶片之间必须留空。** 第一版把左右两片各画成一条横贯的长条，
    // 结果两条在中间接上，整张图读成「一根杆顶着一块板」——正是要与
    // 橡树那个树冠区分开的东西。改法是每片叶只占**一段**，且左右两侧
    // 各留一列透明缝；顶上两片再错开一行，四片因此互不相接。
    // 判据是 `atlas_coverage.rs` 那条「三张两两之间至少四分之一像素
    // 不同」——它咬得住「三张不一样」，咬不住「这一张读起来像什么」，
    // 后者只能靠这段注释与生成后的肉眼核对（本批做过，见计划文档十节）。
    // 左侧下垂的一片
    paint_patch(image, rect, 1, 5, 5, 1, PALM_LEAF_BODY);
    paint_patch(image, rect, 2, 4, 4, 1, PALM_LEAF_LIGHT);
    // 右侧下垂的一片
    paint_patch(image, rect, 11, 5, 4, 1, PALM_LEAF_BODY);
    paint_patch(image, rect, 11, 4, 3, 1, PALM_LEAF_LIGHT);
    // 顶上两片，错开一行，中间留一列缝
    paint_patch(image, rect, 3, 2, 4, 1, PALM_LEAF_LIGHT);
    paint_patch(image, rect, 10, 2, 4, 1, PALM_LEAF_BODY);
    // 顶芽：把四片叶收到一点上，不然它们看起来是四片各自漂着的叶子。
    // 只占 3×2，不外扩——外扩就会重新把左右两片接起来。
    paint_patch(image, rect, 7, 3, 3, 2, PALM_LEAF_BODY);
}
