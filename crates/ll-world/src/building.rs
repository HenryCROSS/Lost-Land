//! 建筑类型：一座据点里的屋子**不再全是同一款**。
//!
//! # 所有者原话
//!
//! > 「聚居地的建筑靠这么近，而且只有款式一样的房子，这不像是一个能
//! > 正常运作的聚居地。」
//! >
//! > 「建筑需要根据他的类型填入不同的家具，例如箱子，椅子，床，书柜等。」
//!
//! 本模块回答后一句：**建筑类型是内容，不是 Rust 枚举**。一份文化声明
//! 它有哪几类屋子、各占多少权重、每类屋里摆什么家具
//! （[`crate::culture::CultureAttrs::buildings`]），于是**加一份
//! `cultures.json5` 就有自己的城镇形态**——这条判据与
//! [`crate::culture::CultureAttrs::wall_terrain`]（矮人矿城石头砌、
//! 哥布林营地木头搭）走的是同一条已经验证过的路。
//!
//! # 为什么类型没有 id、也没有展示名
//!
//! [`BuildingTemplate`] 只有「权重」和「摆什么」两项。今天没有任何消费者
//! 需要「按名字找一栋酒馆」——加一个 `id`/`display_name_key` 就是再造一个
//! `SpaceProfile::buildable`（声明了、没人读）。类型的身份就是文化声明里的
//! **那一条模板**，人读的名字写在 `mods/lostland/cultures.json5` 的注释里。
//!
//! 这是一次**可反转的收窄**：真需要按名字找建筑（找酒馆的任务、地图标注）
//! 时，加一个字段即可，不动本模块任何结构。
//!
//! # 为什么摆家具的**计划**在 `ll-world`，而**写入**在 `ll-game`
//!
//! [`settlement_furnishing`] 是一个纯函数：`f(据点, 文化表, 种子) → 一串
//! (坐标, 物品索引)`。它不需要 `WorldState`、不需要地形、不需要注册表，
//! 因此它属于「世界长什么样」这一层，和 [`crate::settlement`] 铺墙铺门
//! 同一档。
//!
//! 真正写进世界的那一步（查这一格是不是已经铺好的木地板、有没有人站着、
//! 构造带主人的 [`crate::item::GroundItemStack`]）需要 `WorldState` 与
//! 物品表，住在 `ll_game::settlement_spawn`——那里正是 NPC 物化的同一趟。
//!
//! **这个拆分有一个可测的好处**：一份 `cultures.json5` 的文本经真实解析
//! 路径变成一张 [`crate::culture::CultureTable`] 之后，本模块就能直接答出
//! 「这份文化的城镇里家具怎么摆」，不必先造一个世界。

use ll_core::ident::ContentIndex;
use ll_core::rng::DetRng;
use ll_core::torus::{TorusPos, TorusSize};

use crate::culture::CultureTable;
use crate::settlement::{
    BUILDING_SPAN, MAX_BUILDINGS, SettlementSite, SettlementStatus, building_origin,
};

/// 摆家具所用的随机流编号——与
/// [`crate::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]（门窗朝向）**分开**：
/// 改家具不会连带把每栋屋子的门挪个位置，反之亦然。
///
/// 形状照抄那一条：一个固定的流编号 + 一个「第几号事物」的计数，喂给
/// [`DetRng::for_entity`]（约束 C3）。
pub const SETTLEMENT_FURNISH_STREAM_ID: u64 = 0x0053_5445_4144_0002;

/// 一栋屋子最多摆几件家具。
///
/// # 这个数不是拍脑袋，是几何推出来的
///
/// 5×5 的外廓（[`BUILDING_SPAN`]）内部是 3×3 = 9 格。**正中那一格永远
/// 不摆**，因此上界是 8。
///
/// # 为什么正中一定要空着
///
/// 放置的家具（[`crate::item::GroundItemStack::placed`]）**独占那一格**：
/// 别的东西丢不进来。屋子若被填满，这栋屋子就没有一格干净地板——NPC 站
/// 进去、玩家走进去、将来 NPC 自己造东西，全都没有落脚点。
///
/// 留白可以有两种做法：**摆完之后数一数还剩几格**（概率保证），或者
/// **结构上就有一格永远不参与**（本模块选的这条）。后者让「一栋屋子至少
/// 有一格空地」成为一条**恒真**的性质，而不是一条需要每次校验的约束——
/// 与「每格至多站一人」由结算层前置维持是相反方向、但同一条取舍精神：
/// 能靠结构保证的，不要靠检查保证。
pub const MAX_FURNITURE_PER_BUILDING: usize = 8;

/// 一种建筑类型：一份文化声明的「这种屋子占多大比例、里面摆什么」。
///
/// 字段只有两项，理由见模块文档「为什么类型没有 id、也没有展示名」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingTemplate {
    /// 抽取权重。0 表示这一档不参与抽取（与
    /// [`crate::culture::CultureAttrs::founder_races`] 的权重同一条语义）。
    pub weight: u32,
    /// 屋里摆哪些家具：**物品**内容索引 + 件数，按声明顺序占用内壁的格子。
    ///
    /// 件数合计不得超过 [`MAX_FURNITURE_PER_BUILDING`]，注册期就拒
    /// （[`crate::culture::CultureError::TooMuchFurniture`]，ADR 0017）——
    /// 静默丢弃超出的部分会让「我明明声明了十件，屋里只有八件」变成一个
    /// 查不出来的问题。
    ///
    /// 索引指向 `ll_mod::item::ItemTable` 的条目，而**本 crate 不认识
    /// 物品表**（依赖方向不允许）。「这些索引真的是已定义的、且
    /// `furniture: true` 的物品」这条跨表检查因此落在
    /// `ll_mod::content_audit`，与既有的跨表引用检查同一处。
    pub furniture: Vec<(ContentIndex, u32)>,
}

impl BuildingTemplate {
    /// 这一类屋子一共要摆几件家具（各条 `count` 之和，饱和加法）。
    pub fn furniture_count(&self) -> u32 {
        self.furniture
            .iter()
            .fold(0u32, |sum, (_, count)| sum.saturating_add(*count))
    }
}

/// 内壁那一圈八格在 5×5 外廓里的局部偏移，**行主序，跳过正中**。
///
/// 顺序固定写死在数组字面量里，不经任何迭代顺序（约束 C5）。
/// `BUILDING_SPAN` 若改动，本函数的断言会在单测里当场失败——不是靠人
/// 记得同步。
pub const fn interior_offsets() -> [(i32, i32); MAX_FURNITURE_PER_BUILDING] {
    [
        (1, 1),
        (2, 1),
        (3, 1),
        (1, 2),
        (3, 2),
        (1, 3),
        (2, 3),
        (3, 3),
    ]
}

/// 内壁那八格的坐标是**照 5×5 外廓写死的**，所以这条编译期断言必须成立。
///
/// 不写成运行期检查：`BUILDING_SPAN` 是编译期常量，把「两处几何必须对得
/// 上」这件事留到运行期才发现，等于把一条能在编译时抓住的分歧放走。
const _: () = assert!(
    BUILDING_SPAN == 5,
    "BUILDING_SPAN 变了，interior_offsets 的八个坐标要跟着重写"
);

/// 一件要摆下去的家具：摆在哪、摆什么、属于第几栋屋子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FurniturePlacement {
    /// 世界瓦片坐标（已环绕）。
    pub pos: TorusPos,
    /// 物品内容索引。
    pub item: ContentIndex,
    /// 第几栋建筑——调用方要按建筑归并时用得上，也是调试时把一件家具
    /// 追回到它那栋屋子的唯一线索。
    pub building: u32,
}

/// 这座据点该摆哪些家具：**纯函数**，与「谁在铺、铺到第几个区块」无关。
///
/// 返回顺序恒为「按建筑序号，栋内按 [`interior_offsets`] 的行主序」，
/// 不依赖任何哈希容器（约束 C5）。同一份输入恒给同一串输出。
///
/// # 三种情形返回空表，都不是错误
///
/// 1. **废墟**（[`SettlementStatus::Ruined`]）：没人住的地方没有人的东西。
/// 2. **据点没有文化**（一条文化都没装载的世界）。
/// 3. **这份文化一条建筑类型都没声明**——注册期本来就拒了
///    （[`crate::culture::CultureError::NoBuildingTemplate`]），能走到这里
///    只可能是调用方递进来的文化表与产出这份据点快照的不是同一张
///    （测试夹具、或读档时内容变了），与
///    [`crate::settlement`] 的 `wall_terrain` 退回引擎默认同一种处理。
///
/// **这条性质是黄金基准「把改动关掉」那一步依赖的**：空文化表下本函数
/// 恒返回空表，世界里一件家具都不多。
pub fn settlement_furnishing(
    site: &SettlementSite,
    cultures: &CultureTable,
    world_seed: u64,
    tile_size: TorusSize,
) -> Vec<FurniturePlacement> {
    if site.status != SettlementStatus::Inhabited {
        return Vec::new();
    }
    let Some(culture) = site.culture else {
        return Vec::new();
    };
    let templates = cultures.buildings(culture);
    if templates.is_empty() {
        return Vec::new();
    }

    let offsets = interior_offsets();
    let mut plan = Vec::new();
    for building in 0..site.building_count.min(MAX_BUILDINGS) {
        let Some(template) = pick_template(site, building, templates, world_seed) else {
            continue;
        };
        let (left, top) = building_origin(site, building);
        let mut slot = 0usize;
        for (item, count) in &template.furniture {
            for _ in 0..*count {
                if slot >= offsets.len() {
                    break;
                }
                let (dx, dy) = offsets[slot];
                plan.push(FurniturePlacement {
                    pos: tile_size.wrap(left + dx, top + dy),
                    item: *item,
                    building,
                });
                slot += 1;
            }
        }
    }
    plan
}

/// 第 `building` 栋屋子是哪一类——按权重抽一次。
///
/// 走 [`DetRng::for_entity`]（约束 C3），实体号取
/// `据点号 × MAX_BUILDINGS + 栋号`，与
/// [`crate::settlement::stamp_settlement`] 给门窗用的那个键**同一个公式、
/// 不同的流**：两栋不同的屋子恒抽不同的一次，而改这一支不会动到门窗。
///
/// 全部权重为 0 时返回 `None`（注册期已经拒了这种文化，这里是防御性的
/// 那一半）。
fn pick_template<'a>(
    site: &SettlementSite,
    building: u32,
    templates: &'a [BuildingTemplate],
    world_seed: u64,
) -> Option<&'a BuildingTemplate> {
    let total: u32 = templates
        .iter()
        .fold(0u32, |sum, t| sum.saturating_add(t.weight));
    if total == 0 {
        return None;
    }
    let mut rng = DetRng::for_entity(
        world_seed,
        SETTLEMENT_FURNISH_STREAM_ID,
        u64::from(site.id.get()) * u64::from(MAX_BUILDINGS) + u64::from(building),
    );
    let mut roll = rng.gen_range(u64::from(total));
    for template in templates {
        if roll < u64::from(template.weight) {
            return Some(template);
        }
        roll -= u64::from(template.weight);
    }
    // 理论不可达：roll < total = 全部权重之和。
    templates.iter().find(|t| t.weight > 0)
}

/// 夹具用的一份最小建筑声明：**一类屋子、权重 1、不摆任何家具**。
///
/// 与 [`crate::terrain::base_terrain_fixture`]/
/// [`crate::culture::base_culture_fixture`] 同一条既有惯例——测试要造一张
/// 「像那么回事」的文化表，但绝大多数用例（战争推演、敌对判定、名册）
/// 与家具毫无关系。
///
/// # 为什么不让 `buildings` 干脆允许为空
///
/// 因为「一份文化一条建筑类型都没有」的症状是**它的城镇里每一栋屋子
/// 都是空壳**——那正是本批次要消灭的东西，让它靠「忘了写」重新出现
/// 说不过去（ADR 0017「注册期完整校验」）。代价是每个夹具都要写一行，
/// 本函数就是那一行；收益是生产内容里漏写当场报错。
pub fn bare_building_fixture() -> Vec<BuildingTemplate> {
    vec![BuildingTemplate {
        weight: 1,
        furniture: Vec::new(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 内壁偏移恰好是三乘三去掉正中() {
        // Arrange：5×5 外廓的内部是 1..=3 两轴。
        let mid = BUILDING_SPAN / 2;
        let offsets = interior_offsets();

        // Assert
        assert_eq!(offsets.len(), MAX_FURNITURE_PER_BUILDING);
        for (dx, dy) in offsets {
            assert!(
                (1..BUILDING_SPAN - 1).contains(&dx) && (1..BUILDING_SPAN - 1).contains(&dy),
                "({dx},{dy}) 落到墙上或墙外了"
            );
            assert_ne!((dx, dy), (mid, mid), "正中那一格必须留空");
        }
        let mut sorted = offsets;
        sorted.sort_unstable();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), offsets.len(), "八个格位不得重复");
    }

    #[test]
    fn 家具件数合计是各条计数之和() {
        // Arrange
        let mut interner = ll_core::ident::Interner::new();
        let a = interner.intern(ll_core::ident::NamespacedId::parse("test:chair").expect("合法"));
        let b = interner.intern(ll_core::ident::NamespacedId::parse("test:bed").expect("合法"));
        let template = BuildingTemplate {
            weight: 1,
            furniture: vec![(a, 3), (b, 2)],
        };

        // Act & Assert
        assert_eq!(template.furniture_count(), 5);
    }
}
