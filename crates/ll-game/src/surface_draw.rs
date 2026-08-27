//! 地表世界内容 → 绘制指令的**纯计算**：地面物品堆、放置家具、NPC 三
//! 类内容各该用哪个图集键、落在 [`Layer`] 的哪一层、同层内部按什么顺序
//! 排。与 GPU 无关，拆出来的理由与 [`crate::layout`] 一致。
//!
//! # 这个模块补的是哪个洞
//!
//! `crate::app::render_surface` 此前只 push 两样东西：地形瓦片与玩家
//! 标记。地面物品（[`ll_world::state::WorldState::ground_items`]）、
//! 放置家具（同一份数据里 `placed == true` 的那些）、NPC
//! （[`ll_world::state::WorldState::actors`] 里除玩家之外的全部）在引擎
//! 里都存在、能交互、有测试，**但从来没有任何一条渲染路径读过它们**
//! ——玩家看不见脚下有东西。本模块与
//! `crate::app::render_surface` 里对它的调用，就是把这三类内容接上屏幕
//! 的那一步。
//!
//! # 项目所有者的裁定：地面物品统一用一个「团」
//!
//! > 当物品丢在地上，无论是一个还是N个，交互的时候都统一以列表显示，
//! > 并且统一用一个团表示哪一个地方有东西
//!
//! [`ground_pile_draws`] 因此**按坐标去重**：一格上躺着一件还是二十件
//! 东西，产出的都是恰好一条指令、恒定指向同一个图集键
//! [`GROUND_PILE_SPRITE`]。件数、种类、是不是尸体，全都不影响这一条
//! 指令——那些是交互列表的事。
//!
//! # 引擎不认识任何一件具体内容（ADR 0021）
//!
//! 「已放置的家具画成什么样」「这个 NPC 画成什么样」**不在 Rust 里按
//! 内容 id 分支**。规矩是两级：
//!
//! 1. 先拿这条内容在 [`Registry`] 里的**完整命名空间 ID** 当图集键去查
//!    （家具查物品 ID，NPC 的身子查种族 ID、挂件查职业 ID）。这不是本
//!    模块发明的新约定——
//!    `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 规定「任何精灵的
//!    图集条目名恒等于它的完整命名空间 ID」，`crate::layout::terrain_atlas_key`
//!    的 mod 地形回退路径用的就是同一条约定。**内容因此已经有办法声明
//!    自己的精灵键了：往自己的 `assets/sprites/` 里放一张与本地名同名的
//!    图即可**，不需要在 `items.json5`/`races.json5`/`classes.json5` 里
//!    新增字段，也就不需要动 `CONTENT_HASH_ALGORITHM_VERSION`。
//! 2. 查不到就退化到一张通用记号（[`PLACED_FURNITURE_SPRITE`]/
//!    [`NPC_SPRITE`]），或者——当这一层本来就是可选的叠加层时——
//!    **什么都不画**（[`SurfaceDraw::fallback_key`] 为 `None`）。
//!
//! # NPC 为什么是两条指令而不是一张「种族×职业」的图
//!
//! 所有者要求「npc 根据职业种族做出区别」。本体现有 4 个种族 × 13 个
//! 职业 = 52 种组合，所有者已说还要再加 5 个种族（9 × 13 = 117）——
//! 逐个组合备一张图，加第 10 个种族要补 13 张，加第 14 个职业要补 9 张，
//! 是乘法级的负担。
//!
//! 本模块因此把一个 NPC 拆成**两条绘制指令、两张图**：
//!
//! - **身子**：查种族 ID（`lostland:dwarf`），查不到退回 [`NPC_SPRITE`]。
//!   决定体型、肤色、耳朵、胡子这些「他是什么」的东西。
//! - **挂件**：查职业 ID（`lostland:blacksmith`），**查不到就不画**。
//!   一张四周透明、只在胸口有图案的同尺寸贴图，叠在身子上，决定「他
//!   干什么」。
//!
//! 于是资产量从 `种族数 × 职业数` 降到 `种族数 + 职业数`：加第 10 个
//! 种族只要多一张身子图，加第 14 个职业只要多一张挂件图，**另一侧一张
//! 都不用补，引擎一行都不用改**。
//!
//! 这条抽象的正当理由仍然是 ADR 0021 说的「有算法要共用」而不是「看起来
//! 该对称」：身子与挂件共用同一条「优先键 → 兜底」的查图次序、同一套
//! 锚点/缩放换算、同一个消费点（`render_surface` 里的 `push_surface_draw`）。
//! 它同时也是那条纪律的**另一侧**——把同一张人形抄 52 遍同样没有正当
//! 理由，因为那 52 份之间没有任何算法差异，只有一个查表键的差异。
//!
//! 「身子按种族、挂件按职业」这条分工是本批次的判断，不是所有者原话；
//! 所有者只说了「根据职业种族做出区别」。真要改成「职业也换体型」或者
//! 「装备也画上去」，是往这条规则上再加层，不是推翻它。
//!
//! 抽象在这里的正当理由是**有算法要共用**（ADR 0021）：三类内容共用
//! 同一条「优先键 → 兜底键」的查图次序与同一条确定性排序规则，写成
//! [`SurfaceDraw`] 这一种指令 + 一个消费点（`render_surface` 里的
//! `push_surface_draw`），才不至于把同一段查图逻辑抄三遍。反过来，本
//! 模块**不**把「地面物品堆」也做成可被内容覆盖的键——那不是共用算法，
//! 那是所有者明确裁定过的「统一一个团」。
//!
//! # 确定性（约束 C5）
//!
//! 同一格上可能同时有多样东西，绘制顺序必须逐帧、逐进程恒定：
//!
//! - 地面物品堆按 `(y, x)` 收进 [`std::collections::BTreeSet`]（**不是** `HashSet`）去重，
//!   产出顺序是行主序。
//! - 放置家具按 `ground_items` 这个 [`Vec`] 的下标排。
//! - NPC 按 `ll_world::entity::Arena` 的槽位下标排（`iter_with_id`
//!   本身就是 `Vec` 顺序）。
//!
//! 三者的绘制序号（[`SurfaceDraw::entity`]，[`ll_render::sprite::DrawOrder`] 的最后一级
//! 比较键）落在互不重叠的号段里，见本模块的四个 `*_ENTITY_BASE` 常量。

use std::collections::BTreeSet;

use ll_core::torus::TorusPos;
use ll_mod::registry::Registry;
use ll_render::sprite::Layer;
use ll_world::entity::EntityId;
use ll_world::state::WorldState;

/// 玩家标记在绘制顺序里固定的实体号（[`Layer::ENTITY`] 层）。
///
/// 与 [`NPC_ENTITY_BASE`] 同住 [`Layer::ENTITY`]，两者必须互不相撞——
/// 这正是这个常量从 `crate::app` 搬到这里的理由：玩家号与 NPC 号段是
/// **同一个决定的两面**，分居两个文件时，改动其中一个的人看不见另一个。
pub const PLAYER_ENTITY: u64 = 0;

/// 地形瓦片绘制顺序号的起始偏移（[`Layer::TERRAIN`] 层）。
///
/// 与其余号段不同层，因此与它们不可能相撞（[`ll_render::sprite::DrawOrder`] 先比层），
/// 放在这里只是为了让「绘制序号一共分了哪几段」有唯一一处清单。
pub const TERRAIN_ENTITY_BASE: u64 = 1;

/// 地面物品堆绘制顺序号的起始偏移（[`Layer::DECOR`] 层）。
///
/// 号段是 `[0, 世界格数)`——每格至多一堆（按坐标去重），序号取
/// `y * 世界宽 + x`，与地形用的是同一套行主序编号。
pub const GROUND_PILE_ENTITY_BASE: u64 = 0;

/// 放置家具绘制顺序号的起始偏移（[`Layer::DECOR`] 层）。
///
/// 取 `1 << 63` 而不是「世界格数」这类随世界大小变化的值：号段起点必须
/// 是编译期常量，否则「两个号段有没有可能重叠」这个问题的答案会依赖
/// 运行期的世界尺寸，没法在这里一眼断言。`ll_world::core::TorusSize::MAX_EXTENT`
/// 是 `i32::MAX / 2`，最大世界的格数上限约 `2^60`，仍远低于 `2^63`。
pub const PLACED_FURNITURE_ENTITY_BASE: u64 = 1 << 63;

/// NPC 绘制顺序号的起始偏移（[`Layer::ENTITY`] 层）。
///
/// 从 1 起，把 0 让给 [`PLAYER_ENTITY`]：NPC 的序号取
/// `NPC_ENTITY_BASE + 槽位下标`，槽位下标从 0 开始，因此 NPC 永远不会
/// 拿到玩家那个号。
pub const NPC_ENTITY_BASE: u64 = 1;

/// 地面物品堆那一个「团」的图集键。**恒定这一个**，见模块文档所有者
/// 裁定一节。
pub const GROUND_PILE_SPRITE: &str = "lostland:ground_pile";

/// 放置家具查不到内容自带贴图时的通用记号。
pub const PLACED_FURNITURE_SPRITE: &str = "lostland:furniture_placed";

/// NPC 查不到种族自带贴图时的通用记号。
pub const NPC_SPRITE: &str = "lostland:npc_idle_0";

/// NPC 职业挂件绘制顺序号的起始偏移（[`Layer::ENTITY`] 层）。
///
/// 取 `1 << 63` 的理由与 [`PLACED_FURNITURE_ENTITY_BASE`] 逐字相同
/// （号段起点必须是编译期常量），另外还多担一件事：**挂件必须画在身子
/// 之上**。同层同脚底纵坐标时 [`ll_render::sprite::DrawOrder`] 比的正是
/// 这个号，号大的后画、后画的盖在上面；身子的号是
/// `NPC_ENTITY_BASE + 槽位下标`，槽位下标上限远低于 `2^60`（见
/// [`PLACED_FURNITURE_ENTITY_BASE`] 的同一段推导），因此任何挂件的号都
/// 严格大于任何身子的号。
pub const NPC_BADGE_ENTITY_BASE: u64 = 1 << 63;

/// 一条地表内容的绘制指令。
///
/// 刻意**不含屏幕坐标、不含 tint、不含 zoom**：那些要么依赖相机、要么
/// 依赖当前光照，都属于 `render_surface` 那一侧的事。本类型只回答三个
/// 与 GPU 无关的问题——画在世界的哪一格、用哪个图集键（含兜底）、在
/// 绘制顺序里排第几。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDraw {
    /// 画在世界的哪一格。
    pub pos: TorusPos,
    /// 落在哪一层。
    pub layer: Layer,
    /// [`ll_render::sprite::DrawOrder`] 的最后一级比较键，号段见模块
    /// 文档「确定性」一节。
    pub entity: u64,
    /// 内容自己声明的图集键——内容的完整命名空间 ID。`None` 表示这类
    /// 内容**不允许**被内容覆盖（目前只有地面物品堆，见模块文档）。
    pub preferred_key: Option<String>,
    /// 优先键查不到时的通用记号。`None` 表示这一层**本来就是可选的**
    /// ——查不到就整条指令不画，而不是退到某张兜底图。目前只有 NPC 的
    /// 职业挂件是这一种：没有为某个职业准备挂件贴图是正常状态（mod 新
    /// 注册一个职业时的默认状态就是这样），画一张「通用职业记号」反而
    /// 会让所有没画过的职业看起来是同一个职业。
    pub fallback_key: Option<&'static str>,
}

impl SurfaceDraw {
    /// 按优先级列出该依次尝试的图集键。
    ///
    /// 消费方（`render_surface`）应当取**第一个在图集里查得到**的那个。
    /// 把「次序」收在这里而不是让每个消费方自己写 `match`，是模块文档
    /// 「不许把同一段查图逻辑抄三遍」那条的落点。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.preferred_key
            .as_deref()
            .into_iter()
            .chain(self.fallback_key)
    }
}

/// 本帧地表上除地形与玩家之外该画的全部东西，已按 `(层, 号段)` 排好。
///
/// 返回的次序就是**建议的 push 次序**：地面物品堆 → 放置家具 → NPC。
/// 真正的遮挡排序由 [`ll_render::sprite::DrawOrder`] 在批次里做（先比
/// 层、再比脚底屏幕纵坐标、最后比 [`SurfaceDraw::entity`]），本函数的
/// 次序只保证「同样的世界状态产出同样的序列」。
///
/// `player` 是玩家实体——它由 `render_surface` 单独画（要走当前动画帧，
/// 见 `crate::animation`），在这里必须跳过，否则玩家会被画两次：一次
/// 是动画帧，一次是通用 NPC 记号。
pub fn surface_draws(
    world: &WorldState,
    registry: &Registry,
    player: EntityId,
) -> Vec<SurfaceDraw> {
    let mut draws = ground_pile_draws(world);
    draws.extend(placed_furniture_draws(world, registry));
    draws.extend(npc_draws(world, registry, player));
    draws
}

/// 每一格「躺着东西」的地方产出恰好一条指令。
///
/// 「躺着」= [`ll_world::item::GroundItemStack::placed`] 为假。立着的
/// 那些是家具，走 [`placed_furniture_draws`]。
pub fn ground_pile_draws(world: &WorldState) -> Vec<SurfaceDraw> {
    let width = world.size.width() as u64;
    // BTreeSet 而非 HashSet：约束 C5 禁止逻辑（这里是绘制顺序）依赖
    // 哈希容器的迭代顺序。键写成 `(y, x)` 让迭代顺序恰好是行主序，与
    // 地形瓦片的编号方式一致。
    let occupied: BTreeSet<(i32, i32)> = world
        .ground_items
        .iter()
        .filter(|ground| !ground.placed)
        .map(|ground| (ground.pos.y(), ground.pos.x()))
        .collect();

    occupied
        .into_iter()
        .map(|(y, x)| SurfaceDraw {
            pos: world.size.wrap(x, y),
            layer: Layer::DECOR,
            entity: GROUND_PILE_ENTITY_BASE + y as u64 * width + x as u64,
            // 恒定 `None`：所有者裁定「统一用一个团」，内容不得为
            // 「地上躺着的东西」声明自己的样子。
            preferred_key: None,
            fallback_key: Some(GROUND_PILE_SPRITE),
        })
        .collect()
}

/// 每一件立着的家具产出一条指令，优先用这件物品自己的贴图。
pub fn placed_furniture_draws(world: &WorldState, registry: &Registry) -> Vec<SurfaceDraw> {
    world
        .ground_items
        .iter()
        .enumerate()
        .filter(|(_, ground)| ground.placed)
        .map(|(index, ground)| SurfaceDraw {
            pos: ground.pos,
            layer: Layer::DECOR,
            entity: PLACED_FURNITURE_ENTITY_BASE + index as u64,
            preferred_key: registry.resolve(ground.stack.def).map(|id| id.to_string()),
            fallback_key: Some(PLACED_FURNITURE_SPRITE),
        })
        .collect()
}

/// 除玩家之外的每个存活角色产出**两条**指令：先身子（查种族 ID，兜底
/// [`NPC_SPRITE`]），再职业挂件（查职业 ID，查不到就不画）。
///
/// 为什么是两条而不是一张「种族×职业」的合成图，见模块文档
/// 「NPC 为什么是两条指令」一节。
///
/// # 两个字段各自的真相源
///
/// 身子查的是 `ll_world::entity::Agent::race`，挂件查的是
/// `Agent::profession`——后者是所有者裁定的职业唯一真相源。两者都已经
/// 在 `Agent` 上，本函数不需要任何新的数据通路：`WorldState::actors`
/// 里的 `Agent` 本来就是整只结构体，渲染层拿得到 `race` 就同样拿得到
/// `profession`。
///
/// # 引擎里没有任何一处按种族/职业 id 分支
///
/// 两条指令的键都只是「把 [`ContentIndex`] 翻回它注册时的完整命名空间
/// 字符串」（[`Registry::resolve`]），没有 `match "lostland:dwarf"` 这
/// 种东西。新增一个种族或职业，本文件一个字都不用改——内容声明它，
/// 往自己的 `assets/sprites/` 里放一张同名图，就画出来了。
///
/// [`ContentIndex`]: ll_core::ident::ContentIndex
/// [`Registry::resolve`]: ll_mod::registry::Registry::resolve
pub fn npc_draws(world: &WorldState, registry: &Registry, player: EntityId) -> Vec<SurfaceDraw> {
    world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != player)
        .flat_map(|(id, agent)| {
            let slot = id.index() as u64;
            [
                SurfaceDraw {
                    pos: agent.pos,
                    layer: Layer::ENTITY,
                    entity: NPC_ENTITY_BASE + slot,
                    preferred_key: registry.resolve(agent.race).map(|id| id.to_string()),
                    fallback_key: Some(NPC_SPRITE),
                },
                SurfaceDraw {
                    pos: agent.pos,
                    layer: Layer::ENTITY,
                    entity: NPC_BADGE_ENTITY_BASE + slot,
                    preferred_key: registry.resolve(agent.profession).map(|id| id.to_string()),
                    // 没有挂件贴图就不画这一层，见 `fallback_key` 字段
                    // 文档。
                    fallback_key: None,
                },
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{ContentIndex, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_sim::item::ItemStack;
    use ll_world::generate::GenParams;
    use ll_world::item::GroundItemStack;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    /// 一个只够本模块用的最小世界：构造方式逐字取自
    /// `ll_world::state` 的 `test_world`，本模块只往里塞 `ground_items`
    /// 与 `actors`，其余字段一概不碰。
    fn empty_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(5, 5);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn at(world: &WorldState, x: i32, y: i32) -> TorusPos {
        world.size.wrap(x, y)
    }

    /// 注册一条内容并拿到它的索引——`ContentIndex` 没有公开构造函数
    /// （索引只能来自 [`Registry`] 的 intern，见其类型文档），因此测试
    /// 也走同一条路。
    fn intern(registry: &mut Registry, id: &str) -> ContentIndex {
        registry.intern(NamespacedId::parse(id).expect("字面量合法"))
    }

    fn ground(pos: TorusPos, def: ContentIndex, placed: bool) -> GroundItemStack {
        GroundItemStack {
            pos,
            stack: ItemStack::new(def, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed,
        }
    }

    #[test]
    fn 同一格躺着多件东西只画一个团() {
        // Arrange：一格上堆三件不同的东西。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let pos = at(&world, 3, 4);
        for name in ["testmod:a", "testmod:b", "testmod:c"] {
            let def = intern(&mut registry, name);
            world.ground_items.push(ground(pos, def, false));
        }

        // Act
        let draws = ground_pile_draws(&world);

        // Assert：所有者裁定「无论是一个还是N个都统一用一个团」。
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].pos, pos);
        assert_eq!(
            draws[0].keys().collect::<Vec<_>>(),
            vec![GROUND_PILE_SPRITE]
        );
    }

    #[test]
    fn 立着的家具不算进地面物品堆() {
        // Arrange：同一格上一件立着的家具，别无他物。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let def = intern(&mut registry, "testmod:anvil");
        let pos = at(&world, 5, 6);
        world.ground_items.push(ground(pos, def, true));

        // Act
        let piles = ground_pile_draws(&world);

        // Assert：立着的东西是家具，不该同时又画一个「地上躺着东西」的团。
        assert!(piles.is_empty(), "立着的家具不该产出地面物品堆指令");
    }

    #[test]
    fn 地面物品堆按行主序排且与推入顺序无关() {
        // Arrange：刻意按「后面的行先塞」的顺序推进去。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let far = at(&world, 1, 9);
        let near = at(&world, 7, 2);
        let a = intern(&mut registry, "testmod:a");
        let b = intern(&mut registry, "testmod:b");
        world.ground_items.push(ground(far, a, false));
        world.ground_items.push(ground(near, b, false));

        // Act
        let draws = ground_pile_draws(&world);

        // Assert：行主序（y 小的在前），与推入顺序无关——约束 C5。
        let positions: Vec<TorusPos> = draws.iter().map(|draw| draw.pos).collect();
        assert_eq!(positions, vec![near, far]);
        assert!(draws[0].entity < draws[1].entity);
    }

    #[test]
    fn 家具优先用物品自己的完整id当图集键兜底才是通用记号() {
        // Arrange：注册一件物品，摆一件立着的。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let def = intern(&mut registry, "testmod:anvil");
        let pos = at(&world, 2, 2);
        world.ground_items.push(ground(pos, def, true));

        // Act
        let draws = placed_furniture_draws(&world, &registry);

        // Assert：先试内容自己的 ID，查不到才退到通用记号——引擎里
        // 没有任何一处按 id 分支。
        assert_eq!(draws.len(), 1);
        assert_eq!(
            draws[0].keys().collect::<Vec<_>>(),
            vec!["testmod:anvil", PLACED_FURNITURE_SPRITE]
        );
    }

    #[test]
    fn 同一件物品躺着时不给内容留覆盖余地() {
        // Arrange：与上一条同一件物品，但这次是躺着的。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let def = intern(&mut registry, "testmod:anvil");
        let pos = at(&world, 2, 2);
        world.ground_items.push(ground(pos, def, false));

        // Act
        let draws = ground_pile_draws(&world);

        // Assert：与上一条形成对照——同一件物品，立着时用自己的图，
        // 躺着时只能是那个团。这正是所有者裁定的两侧。
        assert_eq!(draws[0].preferred_key, None);
        assert_eq!(
            draws[0].keys().collect::<Vec<_>>(),
            vec![GROUND_PILE_SPRITE]
        );
    }

    #[test]
    fn 地面物品堆与家具的绘制序号号段不重叠() {
        // Arrange：一格躺着东西、一格立着东西，两者同在 DECOR 层。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let a = intern(&mut registry, "testmod:a");
        let b = intern(&mut registry, "testmod:b");
        let far = at(&world, 63, 63);
        let near = at(&world, 0, 0);
        world.ground_items.push(ground(far, a, false));
        world.ground_items.push(ground(near, b, true));

        // Act
        let piles = ground_pile_draws(&world);
        let furniture = placed_furniture_draws(&world, &registry);

        // Assert：同层内部序号必须互不相撞，否则同一格上「团」与「家具」
        // 的前后顺序会变成未定义的。
        assert_eq!(piles[0].layer, furniture[0].layer);
        assert!(piles[0].entity < furniture[0].entity);
    }

    #[test]
    fn 三类指令各自落在预期的图层() {
        // Arrange：地上躺一堆、立一件家具。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let a = intern(&mut registry, "testmod:a");
        let b = intern(&mut registry, "testmod:b");
        world.ground_items.push(ground(at(&world, 1, 1), a, false));
        world.ground_items.push(ground(at(&world, 2, 1), b, true));

        // Act
        let piles = ground_pile_draws(&world);
        let furniture = placed_furniture_draws(&world, &registry);

        // Assert：地面物品与家具都在地形之上、角色之下。
        assert_eq!(piles[0].layer, Layer::DECOR);
        assert_eq!(furniture[0].layer, Layer::DECOR);
        assert!(Layer::TERRAIN < Layer::DECOR);
        assert!(Layer::DECOR < Layer::ENTITY);
    }
}
