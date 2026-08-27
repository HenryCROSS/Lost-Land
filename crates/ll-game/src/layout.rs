//! 与 GPU 无关的纯计算：地形 → 图集条目名映射、光照 → 视野半径换算。
//!
//! 拆成独立文件、脱离窗口/GPU 也能被 `cargo test --workspace` 覆盖的
//! 理由，与 `ll-sim` 的 `p5_coordinate_acceptance::layout` 一致，见其
//! 模块文档。本文件的 [`terrain_entry_name`]/[`effective_sight_radius`]/
//! [`effective_tint`] 是同一套换算的独立实现（不是同一份代码的引用
//! ——`p5_coordinate_acceptance` 是 `ll-sim` 的一个 `examples/` 目录，
//! 不是可供下游 crate 依赖的库 API，见 Cargo 对 `examples/` 的可见性
//! 规则），保持逻辑一致但物理上各自独立。
//!
//! 「逻辑一致」曾经短暂地不成立：据点建筑地形补图那一批只改了本文件的
//! [`terrain_entry_name`]，p5 那份仍按更早的借用关系画石地板/石墙，
//! 两张表就此分叉。项目所有者的裁定是统一（原话「第三条的话先统一了
//! 吧，避免以后有什么问题」），p5 那张表已经改回与本文件同一张贴图。
//! 恢复的方向是**把 p5 对齐到本文件**而不是反过来：本文件是生产渲染
//! 路径，p5 是验收 demo。守住这条一致性的是 p5 自己的
//! `十种地形两两不共用同一个图集条目`——任何一支改回借用它立刻变红。

use ll_core::time::Tick;
use ll_mod::registry::Registry;
use ll_world::light::sight_radius_under_weather;
use ll_world::space_profile::{SpaceProfile, effective_ambient_light, effective_weather};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};
use ll_world::weather::Weather;

/// 地表视野基准半径（格），随光照缩放。
pub const BASE_SIGHT_RADIUS: u32 = 12;

/// 把地形种类映射到图集条目名——覆盖本体 `define_base` 注册的**全部
/// 17 种**地形，一种不漏。
///
/// 返回值带 `lostland:` 前缀：图集条目名统一用完整命名空间字符串（见
/// `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 文档），这张表本身
/// 是硬编码字面量，不经过 [`Registry`]，因此前缀直接写死在表里，而不是
/// 运行期拼接。
///
/// 这张表的本地部分与地形在 [`Registry`] 里注册的内容 ID 本地部分并不
/// 相同（例如 `ids.grass` 对应的注册 ID 是 `lostland:grass`，这里查出
/// 的图集条目名却是 `lostland:terrain_grass`）——图集条目名描述的是
/// 「贴图长什么样」，注册 ID 描述的是「这是哪种地形」，两者是两套独立
/// 的字符串空间，只是恰好共享同一个本体命名空间前缀。
///
/// # 「一种不漏」这条为什么要单独写出来
///
/// 这张表此前只覆盖 10 种：8 种自然地形，加上 `floor_stone`/`wall_stone`
/// **借用** `terrain_dirt`/`terrain_mountain` 两张自然地形图。剩下 7 种
/// 建筑地形（`floor_wood`/`wall_wood`/`door_closed`/`door_open`/
/// `window`/`stairs_up`/`stairs_down`）在这里返回 `None`，落到
/// [`terrain_atlas_key`] 的 [`Registry`] 回退路径上，拿注册 ID
/// （`lostland:wall_wood`）当图集键去查——那条回退路径本来是给 mod 地形
/// 用的，本体地形的注册 ID 与图集条目名根本不是同一个字符串空间（见上
/// 一段），必然查不到。后果是玩家一走进据点，每帧每格刷一条「图集条目
/// 缺失，跳过本次绘制」的 ERROR，据点/建筑/室内一格都画不出来。
///
/// 守住「一种不漏」的是本文件的
/// `全部十七种本体地形都能查到图集条目` 与
/// `crates/ll-game/tests/atlas_coverage.rs`：前者钉这张表返回 `Some`，
/// 后者钉那个字符串在真实图集里查得到、且对应矩形里真的有像素。此前
/// 只有一条覆盖 8 种自然地形的测试，缺口正落在它的盲区里。
///
/// # 两处「借用」已经解除
///
/// `floor_stone`/`wall_stone` 现在各有专属贴图
/// （`terrain_floor_stone`/`terrain_wall_stone`），不再借用泥土/山体。
/// 理由是木质建筑地形一并有了图之后，暖褐的木地板会和同样暖褐的
/// `terrain_dirt` 糊在一起——所有者的验收方式是「走进据点看一眼」，
/// 木/石地板必须一眼可分。这是本批次的判断，不是所有者原话。
///
/// `p5_coordinate_acceptance` 里的同一处借用也已经解除，理由见本模块
/// 文档开头「逻辑一致曾经短暂地不成立」一段。
///
/// `terrain_dirt` 本身**没有**因此变成孤儿图：
/// `crates/ll-render/examples/p1_acceptance` 拿它铺棋盘格、
/// `crates/ll-game/src/content.rs` 的 mod 资产覆盖验收拿它当被覆盖的
/// 目标，两处都还在用。这两处**不是**借用——那里的 `terrain_dirt` 就是
/// 泥土本身，与「拿泥土冒充石地板」是两回事，不在这次统一的范围内。
pub fn terrain_entry_name(kind: TerrainKind, ids: &BaseTerrainIds) -> Option<&'static str> {
    if kind == ids.deep_water {
        Some("lostland:terrain_deep_water")
    } else if kind == ids.shallow_water {
        Some("lostland:terrain_shallow_water")
    } else if kind == ids.sand {
        Some("lostland:terrain_sand")
    } else if kind == ids.grass {
        Some("lostland:terrain_grass")
    } else if kind == ids.forest {
        Some("lostland:terrain_forest")
    } else if kind == ids.hill {
        Some("lostland:terrain_hill")
    } else if kind == ids.mountain {
        Some("lostland:terrain_mountain")
    } else if kind == ids.snow {
        Some("lostland:terrain_snow")
    } else if kind == ids.floor_wood {
        Some("lostland:terrain_floor_wood")
    } else if kind == ids.floor_stone {
        Some("lostland:terrain_floor_stone")
    } else if kind == ids.wall_wood {
        Some("lostland:terrain_wall_wood")
    } else if kind == ids.wall_stone {
        Some("lostland:terrain_wall_stone")
    } else if kind == ids.door_closed {
        Some("lostland:terrain_door_closed")
    } else if kind == ids.door_open {
        Some("lostland:terrain_door_open")
    } else if kind == ids.window {
        Some("lostland:terrain_window")
    } else if kind == ids.stairs_up {
        Some("lostland:terrain_stairs_up")
    } else if kind == ids.stairs_down {
        Some("lostland:terrain_stairs_down")
    } else {
        None
    }
}

/// 把地形种类映射到图集条目名，覆盖本体注册的自然地形**与** mod 注册
/// 的自定义地形（例如 `mods/example_mod` 的 `examplemod:lava_floor`）。
///
/// # 为什么需要这个回退，而不是只用 [`terrain_entry_name`]
///
/// [`terrain_entry_name`] 是一张写死的静态映射表，只认识本体注册的
/// 那几种基础地形——mod 通过 `register-terrain` 注册的新地形种类，
/// 这张表天然查不到（它压根不知道这些地形的存在），此前 mod 自定义
/// 地形因此永远画不出来，只能靠 [`tile_tint`] 之外没有任何降级路径，
/// 直接在 [`terrain_entry_name`] 返回 `None` 时被跳过——这正是「mod
/// 能注册一把剑，却给不了它一张图」这条真实瓶颈在地形渲染上的具体
/// 体现。
///
/// 回退路径反查 [`Registry::resolve`] 拿到这个地形种类的完整命名空间
/// ID（例如 `"examplemod:lava_floor"`），直接把这个字符串当图集查找
/// 键——`ll_mod::asset_vfs::ResolvedSprite::atlas_name` 对任意命名空间
/// 的精灵，图集条目名恒定就是这个完整 ID 字符串（本体与 mod 统一，见
/// 其文档），两边约定完全对齐，不需要额外的映射表。
///
/// 这条回退路径只对 mod 注册的地形成立——本体注册的自然地形已经被
/// [`terrain_entry_name`] 挡在前面提前返回，走不到这里；[`Registry`]
/// 里本体地形的注册 ID（本地部分是 `grass`/`mountain` 这类简称）与图集
/// 条目名（本地部分是 `terrain_grass`/`terrain_mountain`）本就不是同一
/// 个字符串，`registry.resolve` 直接查也查不出正确的图集键——这正是
/// [`terrain_entry_name`] 这张表不能被这条回退路径整个取代的原因。
///
/// 与 GPU 无关的纯函数：[`Registry`] 是普通数据，不需要真实图集就能
/// 单测覆盖「查到了哪个字符串」这层逻辑；「这个字符串在图集里查不查
/// 得到条目」是下一步 `GpuResources::lookup` 的职责，不在本函数范围。
pub fn terrain_atlas_key(
    kind: TerrainKind,
    ids: &BaseTerrainIds,
    registry: &Registry,
) -> Option<String> {
    if let Some(bare) = terrain_entry_name(kind, ids) {
        return Some(bare.to_string());
    }
    registry.resolve(kind.index()).map(|id| id.to_string())
}

/// 给定空间在某一世界时刻、某种天气下的环境光换算出的视野半径。
///
/// 不叠加任何种族暗视（夜间下限恒取未声明时的默认值）——本函数留给不知道「谁在看」的调用方（例如本
/// 文件与 `ll-sim` p5 验收 demo 里只关心「这个空间本身多亮」的测试）。
/// 真正的玩家渲染路径需要暗视时用 [`effective_sight_radius_for_race`]。
///
/// # 天气在这里进来两次，不是重复
///
/// 天气有两个独立的乘数（见 `ll_world::weather::WeatherDef::sight_scale`）：
/// `light_scale` 经 [`effective_ambient_light`] 折进光照，`sight_scale`
/// 由 [`sight_radius_under_weather`] 在光照换算**之后**单独再乘一次。
/// 两次都必须先过 [`effective_weather`]——那是「洞窟不受天气影响」这条
/// 判断的唯一真相源，`effective_ambient_light` 内部也走它，两个消费者
/// 因此不可能对同一个空间给出相反的结论。
pub fn effective_sight_radius(profile: &SpaceProfile, clock: Tick, weather: Weather) -> u32 {
    let light = effective_ambient_light(profile, clock, weather);
    sight_radius_under_weather(
        BASE_SIGHT_RADIUS,
        light,
        effective_weather(profile, weather),
        NO_DARKVISION,
    )
}

/// 「这个调用方不知道谁在看」时传给暗视参数的取值。
///
/// `0` 在 [`ll_world::light::sight_radius_at`] 里被解读成**未声明**
/// 暗视，落回 [`ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS`]——与
/// [`effective_sight_radius`] 长出这个参数之前的行为逐格相同。写成
/// 具名常量而不是散落的字面量 `0`，是为了让「这里传 0 是因为没有
/// 观察者」与「某个种族真的声明了 0」在读代码时不会混淆（后者不可能
/// 出现——0 恒被解读成未声明）。
const NO_DARKVISION: u32 = 0;

/// 给定空间在某一世界时刻的有效光照，叠加某个种族声明的**夜间视野
/// 格数下限**后换算出的视野半径——`race-system.md`「五、暗视」一节的
/// 渲染侧接线点。
///
/// # 暗视只改「看多远」，不改「看多清」
///
/// 项目所有者裁定暗视只买视野格数：本函数的返回值只喂给 FOV，画面
/// 亮度那一路（[`effective_tint`]）读的是环境光本身，与暗视无关——
/// 夜视好的种族在黑暗里看得**更远**，不是让整个世界对它变亮。这也是
/// 为什么本函数不再先算一个「有效光照」再交给半径换算：暗视根本不
/// 经过光照这个量。
///
/// # 为什么接在这一步，不是更早或更晚
///
/// 现有链路是 `season_light_scale → ambient_light →
/// effective_ambient_light → effective_sight_radius`——`ambient_light`/
/// `effective_ambient_light` 只描述「这个世界时刻、这个空间本身多亮」，
/// 与「谁在看」完全无关（同一个地下城任何种族站进去，`ambient_light_floor`
/// 都一样），暗视是**观察者的属性**，不该往上游这两步塞：往
/// `ambient_light` 塞会让同一个空间对所有种族都变亮（错——暗视应该是
/// 「这个种族看得见，其余种族看不见」，不是「这个地方变亮了」），
/// 往 `effective_ambient_light` 塞同理，且两者都定义在 `ll-world`，
/// 而 `darkvision_cells` 在下游 `ll-mod::race`（依赖方向不允许
/// `ll-world` 认识它）。唯一合适的落点是 `effective_ambient_light` 算
/// 出「这个空间这一刻本身多亮」**之后**、喂给视野半径换算
/// **之前**——[`ll_sim::vision::sight_radius_for_race`] 正是卡在这两步
/// 中间，见其模块文档「为什么定义在 `ll-sim`」一节。
///
/// # 依赖方向：`RaceDarkvisionSource` 由调用方传入，不是本函数去查
///
/// `ll-game` 依赖 `ll-mod`/`ll-sim`（见 `Cargo.toml`），可以直接认识
/// `ll_mod::race::RaceTable`，本可以在这里直接要一个 `&RaceTable`——
/// 但 [`ll_sim::vision::sight_radius_for_race`] 的签名是
/// `&dyn RaceDarkvisionSource`（依赖倒置接口，定义在 `ll-sim`），这里
/// 沿用同一个接口类型而不是收窄成具体的 `RaceTable`，理由与
/// `ll_game::world::build_player_agent` 调用
/// `ll_sim::character::bake_race_stat_modifiers` 时把
/// `&content.race_table` 当 `&dyn RaceStatModifierSource` 传入完全
/// 一致：调用方是唯一同时持有真实 `RaceTable` 与真实空间/时钟的地方，
/// 但真正做换算的函数不需要认识 `RaceTable` 这个具体类型，只需要认识
/// 接口。
pub fn effective_sight_radius_for_race(
    profile: &SpaceProfile,
    clock: Tick,
    weather: Weather,
    race: ll_core::ident::ContentIndex,
    darkvision: &dyn ll_sim::vision::RaceDarkvisionSource,
) -> u32 {
    let light = effective_ambient_light(profile, clock, weather);
    // 暗视是**夜间视野格数的下限**，天气的 sight_scale 是一个乘数——
    // `sight_radius_for_race` 内部把下限应用在乘数**之前和之后**各一
    // 次，因此雾雪削得掉光照换算出来的那部分视野，削不掉暗视这条底线
    // （`ll_world::light::sight_radius_under_weather` 文档「夜间下限在
    // 这里第二次应用」一节）。这一步不能拆成「先算半径、再乘天气」两
    // 句写在本函数里——那正是暗视会被恶劣天气吃掉的写法。
    ll_sim::vision::sight_radius_for_race(
        BASE_SIGHT_RADIUS,
        light,
        effective_weather(profile, weather),
        race,
        darkvision,
    )
}

/// 画面整体亮度的下限——再暗的夜晚也不会低于这个值。
///
/// 项目所有者的要求是「黑夜要有一个还算能看的亮度」。原先午夜的调制
/// 系数是 0.1，连当前视野内的格子都被压得几乎看不出地形。
///
/// 这条是**纯表现层**决策（ADR 0020 甲区：结果只变成像素），与视野半径
/// 的下限 [`ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS`] 分属两件事——一个管
/// 「看得清不清」，一个管「看得到多远」。前者可以自由用浮点、不进
/// `WorldState`、不参与 `hash()`；后者会经 FOV 影响探索记忆，是世界状态。
///
/// 取 0.4 而不是更高：夜晚仍要明显暗于白天（正午为 1.0），否则昼夜就
/// 只剩计时意义。已探索但当前无视野的格子还会再乘
/// [`EXPLORED_MEMORY_DIM_FACTOR`]，因此夜里的记忆层约为 0.14——能看出
/// 轮廓，但一眼能和当前视野区分开。
pub const MIN_VISIBLE_TINT: f32 = 0.4;

/// 画面整体亮度调制（灰阶），下限为 [`MIN_VISIBLE_TINT`]。
///
/// 天气只经 `light_scale` 影响这里——`sight_scale`（雾）**不**参与画面
/// 亮度：雾让人看不远，不让人看不清脚下这一格，把它折进色调会让雾变成
/// 「又暗又看不远」的第二种阴天，见 `ll_world::light::sight_radius_under_weather`
/// 文档「为什么是第二个乘数」一节。
pub fn effective_tint(profile: &SpaceProfile, clock: Tick, weather: Weather) -> [f32; 4] {
    let light = effective_ambient_light(profile, clock, weather)
        .0
        .clamp(0, 1000) as f32
        / 1000.0;
    let light = light.max(MIN_VISIBLE_TINT);
    [light, light, light, 1.0]
}

/// 已探索但当前无视野的格子（战争迷雾「记忆」层）在 [`effective_tint`]
/// 基础上再压暗的系数。
///
/// 只影响像素颜色，不进 [`ll_world::state::WorldState`]——世界状态禁止
/// 浮点（约束见 `ll_world::exploration` 模块文档「只存位图」一节：
/// `ExplorationMemory` 只记「看没看过」这一个 bit，暗化多少是纯表现层
/// 决策，不该反过来污染世界状态）。取值小于 1 让记忆层比当前视野暗、
/// 大于零让它比「从未探索」（完全不画、留黑）更亮——三层可见性
/// （项目所有者原话：「没有视野的地方就暗下来一些……没去过的地方就
/// 黑着」）因此不是三个离散色阶,而是「不画」与「按此系数压暗」两种
/// 处理叠加在同一套 `effective_tint` 光照调制之上。
const EXPLORED_MEMORY_DIM_FACTOR: f32 = 0.35;

/// 把当前光照色调换算成「已探索但当前无视野」格子应使用的记忆色调。
///
/// 见 [`EXPLORED_MEMORY_DIM_FACTOR`] 文档：只压暗 RGB，不动 alpha——
/// 记忆层格子仍需完全不透明地画出来，只是比当前视野内的格子暗。
pub fn memory_tint(tint: [f32; 4]) -> [f32; 4] {
    [
        tint[0] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[1] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[2] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[3],
    ]
}

/// 三层可见性判定：给定一格「当前是否在玩家视野内」与「是否已被探索
/// 过」，返回这一帧该不该画这一格、画的话用哪种色调。
///
/// 从 [`crate::app`] 的 `render_surface` 抽成与 GPU 无关的纯函数——三层
/// 可见性本身只是一张判定表（项目所有者原话：「没有视野的地方就暗
/// 下来一些，有视野的地方就没问题。而没去过的地方就黑着」），不需要
/// 靠跑起整条渲染管线才能验证：
///
/// - 当前有视野 → 画，用 `tint`（当前光照色调）。
/// - 当前无视野但已探索过 → 画，用 [`memory_tint`]（记忆层，比当前
///   光照暗）。
/// - 既无视野也没探索过 → 不画（`None`），调用方应当跳过这一格，让
///   `ll-render` 的黑色清屏背景顶替「从未探索」的黑。
pub fn tile_tint(currently_visible: bool, explored: bool, tint: [f32; 4]) -> Option<[f32; 4]> {
    if currently_visible {
        Some(tint)
    } else if explored {
        Some(memory_tint(tint))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个仅在测试里现造的露天地表 profile——本文件多条断言都需要
    /// 「露天、没有地板光、其余字段无所谓」这同一个形状，抽成函数避免
    /// 每条各拼一遍（既有的几条测试各自内联构造，改动它们不属于本批次
    /// 范围，新增的几条用这个帮手）。
    /// 本体矮人在 `mods/lostland/races.json5` 里声明的暗视格数。
    ///
    /// 本文件的断言只需要「一个高于默认值的声明」这条性质，取本体真实
    /// 数值而不是另编一个，是为了让这里失败时能直接对上内容里的那一行
    /// ——端到端那一侧（`ll-mod/tests/base_mod_darkvision.rs`）钉的是
    /// 同一个数字经真实 `mods/` 装载之后的结果。
    const DWARF_DARKVISION_CELLS: u32 = 7;

    fn surface_profile() -> SpaceProfile {
        SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        }
    }

    #[test]
    fn 本体地形直接查到带命名空间前缀的图集条目而不需要回退到registry() {
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let registry = Registry::new();

        // Act
        let key = terrain_atlas_key(ids.grass, &ids, &registry);

        // Assert
        assert_eq!(key.as_deref(), Some("lostland:terrain_grass"));
    }

    #[test]
    fn mod注册的地形回退到registry查出完整命名空间字符串() {
        // 这条测试直接对应「mod 能注册一把剑，却给不了它一张图」这条
        // 瓶颈在地形渲染上的修复：examplemod 注册的 lava_floor 不在
        // BaseTerrainIds 这张静态表里，terrain_atlas_key 必须回退到
        // Registry 反查出完整命名空间字符串，而不是直接判定「查不到」。
        // Arrange：地形索引与 mod 地形索引必须来自同一个 Registry——
        // 与真实装载流程一致（本体先注册、mod 后 intern，见
        // `ll_mod::pipeline` 模块文档「本体内容不经过这条管线」一节）。
        // 若各用一个独立 `Registry::new()`，两边的索引计数器各自从零
        // 开始，数值可能巧合重叠，`terrain_entry_name` 会在真正测试
        // 回退逻辑之前就已经因为索引数值碰巧相等而误判命中。
        let mut registry = Registry::new();
        let (ids, _table) = ll_mod::base_terrain::register_base_terrain(&mut registry)
            .expect("本体地形声明表内部一致，注册恒不失败");
        let mod_id = ll_core::ident::NamespacedId::parse("examplemod:lava_floor")
            .expect("测试用命名空间恒合法");
        let index = registry.intern(mod_id);
        let mod_terrain = ll_world::terrain::TerrainKind::from_index(index);

        // Act
        let key = terrain_atlas_key(mod_terrain, &ids, &registry);

        // Assert
        assert_eq!(key.as_deref(), Some("examplemod:lava_floor"));
    }

    /// `define_base` 注册的全部 17 种本体地形，与
    /// `ll_world::terrain` 里那张注册表逐条对应。
    ///
    /// 写成一张具名表而不是就地展开，是因为下面两条测试都要遍历它：
    /// 一条断言「每种都查得到条目名」，一条断言「17 种查出来的名字
    /// 两两不同」。
    fn all_base_kinds(ids: &BaseTerrainIds) -> [TerrainKind; 17] {
        [
            ids.deep_water,
            ids.shallow_water,
            ids.sand,
            ids.grass,
            ids.forest,
            ids.hill,
            ids.mountain,
            ids.snow,
            ids.floor_wood,
            ids.floor_stone,
            ids.wall_wood,
            ids.wall_stone,
            ids.door_closed,
            ids.door_open,
            ids.window,
            ids.stairs_up,
            ids.stairs_down,
        ]
    }

    #[test]
    fn 全部十七种本体地形都能查到图集条目() {
        // 此前这条只覆盖 8 种自然地形，7 种建筑地形整个落在盲区里——
        // 见 `terrain_entry_name` 文档「一种不漏」一节。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();

        // Act & Assert
        for kind in all_base_kinds(&ids) {
            assert!(
                terrain_entry_name(kind, &ids).is_some(),
                "地形索引 {:?} 查不到图集条目名",
                kind.index()
            );
        }
    }

    #[test]
    fn 十七种本体地形的图集条目名两两不同() {
        // 「都查得到」不等于「查到的不是同一张图」：此前 `wall_stone`
        // 与 `mountain` 就共用 `terrain_mountain`，两条都是 Some，屏幕
        // 上却分不出哪格是山、哪格是石墙。这条钉的是那种失效方式。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();

        // Act
        let names: Vec<&str> = all_base_kinds(&ids)
            .into_iter()
            .map(|kind| terrain_entry_name(kind, &ids).expect("上一条测试已保证恒为 Some"))
            .collect();

        // Assert：BTreeSet 而非 HashSet——约束 C5 禁止逻辑依赖哈希
        // 容器迭代顺序，这里虽然只数个数，仍统一用有序容器。
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "17 种地形只查出 {} 个不同的图集条目名：{names:?}",
            unique.len()
        );
    }

    #[test]
    fn 光照全灭时视野半径缩小到基准值以下() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_dark").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: false,
            base_temperature: 0,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        };

        // Act
        let radius = effective_sight_radius(&profile, Tick(0), Weather::CLEAR);

        // Assert
        assert!(radius < BASE_SIGHT_RADIUS);
    }

    /// 测试用的最小 `RaceDarkvisionSource`——固定返回同一个暗视格数，
    /// 不依赖 `ll_mod::race::RaceTable`，只用来隔离验证
    /// [`effective_sight_radius_for_race`] 这一步换算本身的行为，见其
    /// 文档「依赖方向」一节。
    ///
    /// 取值直接用本体矮人声明的 7 格：暗视改成「夜间视野格数下限」
    /// 之后，测试用的数字与 `mods/lostland/races.json5` 里的数字终于是
    /// 同一个量纲。旧形态（暗视是光照千分比下限）下这个夹具必须写成
    /// `FixedDarkvision(DWARF_DARKVISION_CELLS)`——把本体矮人实际声明的 4 放大 150 倍才能
    /// 让功能表现出可观测差异，那本身就是「机制对、数值错」的自白，
    /// 见 `ll_sim::vision` 模块文档「缺口是什么」一节。
    struct FixedDarkvision(u32);

    impl ll_sim::vision::RaceDarkvisionSource for FixedDarkvision {
        fn darkvision_cells(&self, _race: ll_core::ident::ContentIndex) -> u32 {
            self.0
        }
    }

    #[test]
    fn 暗视种族夜间视野大于无暗视种族() {
        // 同一时刻、同一地点，唯一变量是种族声明的暗视格数——直接
        // 对应 `effective_sight_radius_for_race` 文档「为什么接在这一
        // 步」一节要接线的效果。手工验证：把
        // `ll_sim::vision::sight_radius_for_race` 改成恒传 0（不查种族
        // 声明），这条测试会失败——两者都落回
        // `DEFAULT_NIGHT_SIGHT_RADIUS`，断言 `>` 不再成立。
        //
        // **旧公式下这条断言是假的**：暗视还是「光照千分比下限」时，
        // 本体矮人的 4 连午夜环境光 100 都抬不动，矮人与人类的夜间视野
        // 完全相同（都撞在 4 格下限上），只有把夹具放大到 600 才测得
        // 出差异。现在夹具用的就是矮人真实声明的 7 格。
        // Arrange：地表深夜（`Tick(0)`，午夜光照按昼夜曲线不为零但很低）。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };
        let midnight = Tick(0);
        let race = ll_core::ident::ContentIndex::default();
        let darkvision = FixedDarkvision(DWARF_DARKVISION_CELLS);
        let no_darkvision = FixedDarkvision(0);

        // Act
        let with_darkvision =
            effective_sight_radius_for_race(&profile, midnight, Weather::CLEAR, race, &darkvision);
        let without_darkvision = effective_sight_radius_for_race(
            &profile,
            midnight,
            Weather::CLEAR,
            race,
            &no_darkvision,
        );

        // Assert
        assert!(with_darkvision > without_darkvision);
    }

    #[test]
    fn 白天暗视种族与无暗视种族视野相同() {
        // 正午满光照（1000）下基准半径 12 格本就远高于任何种族声明的
        // 暗视格数——夜间下限在这种输入下根本不参与取值，证明暗视只在
        // 暗处起作用，不是无脑加成。
        // Arrange：地表正午（与 `ll_world::light` 「正午光照最强」测试
        // 同一个采样点：夏季第 30 天正午，季节缩放不折损）。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };
        let noon = Tick(30 * ll_core::time::TICKS_PER_DAY + 12 * ll_core::time::TICKS_PER_HOUR);
        let race = ll_core::ident::ContentIndex::default();
        let darkvision = FixedDarkvision(DWARF_DARKVISION_CELLS);
        let no_darkvision = FixedDarkvision(0);

        // Act
        let with_darkvision =
            effective_sight_radius_for_race(&profile, noon, Weather::CLEAR, race, &darkvision);
        let without_darkvision =
            effective_sight_radius_for_race(&profile, noon, Weather::CLEAR, race, &no_darkvision);

        // Assert
        assert_eq!(with_darkvision, without_darkvision);
    }

    /// 开局那一刻玩家到底看得见什么——这条是**组合断言**。
    ///
    /// 午夜环境光（千分之一百）、`sight_radius_at` 的缩放、`effective_tint`
    /// 的整体调制、以及三层可见性里「从未探索就不画」，四条规则各自都
    /// 正确、各自都有测试守着，叠在一起却让 `Tick(0)` 开局变成纯黑屏加
    /// 正中央五个格子——项目所有者实测报告了这个现象。缺的从来不是某
    /// 一块的测试，而是「这些块凑在一起时开局长什么样」这一条。
    #[test]
    fn 新游戏起始时刻的地表视野远大于最小半径() {
        // Arrange：露天地表，没有额外的环境光下限加成。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let radius =
            effective_sight_radius(&profile, crate::world::NEW_GAME_START_TICK, Weather::CLEAR);

        // Assert：至少要有基准半径的一半，否则开局仍然近乎瞎。
        assert!(radius >= BASE_SIGHT_RADIUS / 2);
    }

    /// 与上一条配套：起始时刻的画面整体亮度不能低到把可见格子也压黑。
    #[test]
    fn 新游戏起始时刻的画面亮度过半() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let tint = effective_tint(&profile, crate::world::NEW_GAME_START_TICK, Weather::CLEAR);

        // Assert
        assert!(tint[0] > 0.5);
    }

    /// 项目所有者的要求：「让黑夜有个最低视野范围以及一个还算能看的
    /// 亮度」。这条锁住亮度那一半，视野那一半由
    /// `ll_world::light` 的 `午夜视野不低于最小半径` 锁住。
    #[test]
    fn 午夜的画面亮度不低于可见下限() {
        // Arrange：露天地表，午夜，没有任何额外光源。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let tint = effective_tint(&profile, Tick(0), Weather::CLEAR);

        // Assert
        assert!(tint[0] >= MIN_VISIBLE_TINT);
    }

    /// 但夜晚仍必须明显暗于正午，否则昼夜只剩计时意义。
    #[test]
    fn 午夜画面明显暗于正午() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let midnight = effective_tint(&profile, Tick(0), Weather::CLEAR);
        let noon = effective_tint(
            &profile,
            Tick(12 * ll_core::time::TICKS_PER_HOUR),
            Weather::CLEAR,
        );

        // Assert
        assert!(midnight[0] < noon[0]);
    }

    /// 生产渲染路径上的天气组合断言——`ll_world::light` 那一侧已经钉住
    /// 了「换算本身不会把视野压到不可玩」，这里钉的是**接线**：
    /// `effective_sight_radius`/`effective_tint` 这两个每帧被
    /// `crate::app::render_surface` 调用的函数真的会因为天气不同而给出
    /// 不同答案。链路断在 `ll-game` 这一节的话，上游断言全绿、玩家却
    /// 看不出任何区别，正是本项目反复吃亏的那类缺口。
    #[test]
    fn 露天空间的视野半径与画面亮度都随天气变化() {
        // Arrange：夏季正午，露天地表——排除昼夜与季节的干扰。
        let profile = surface_profile();
        let noon = Tick(30 * ll_core::time::TICKS_PER_DAY + 12 * ll_core::time::TICKS_PER_HOUR);
        let (ids, table) = ll_world::weather::base_weather_fixture();
        let foggy = Weather {
            kind: Some(ids.fog),
            light_scale: table.light_scale(ids.fog),
            sight_scale: table.sight_scale(ids.fog),
            temperature_offset: 0,
        };

        // Act
        let clear_radius = effective_sight_radius(&profile, noon, Weather::CLEAR);
        let foggy_radius = effective_sight_radius(&profile, noon, foggy);
        let clear_tint = effective_tint(&profile, noon, Weather::CLEAR);
        let foggy_tint = effective_tint(&profile, noon, foggy);

        // Assert
        assert!(foggy_radius < clear_radius, "雾必须真的缩短实机视野半径");
        assert!(foggy_tint[0] < clear_tint[0], "雾必须真的压暗画面");
    }

    #[test]
    fn 非露天空间的视野半径与画面亮度都不随天气变化() {
        // 「洞窟不受天气影响」这条语义在生产渲染路径上的落点——两个
        // 乘数都必须被 effective_weather 中和掉，不能只中和一个。
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_cave").expect("字面量恒合法"),
            ambient_light_floor: 200,
            exposed_to_sky: false,
            base_temperature: 0,
            diggable: true,
            buildable: false,
            reverb_tag: None,
        };
        let noon = Tick(30 * ll_core::time::TICKS_PER_DAY + 12 * ll_core::time::TICKS_PER_HOUR);
        let storm = Weather {
            kind: None,
            light_scale: 100,
            sight_scale: 100,
            temperature_offset: 0,
        };

        // Act & Assert
        assert_eq!(
            effective_sight_radius(&profile, noon, Weather::CLEAR),
            effective_sight_radius(&profile, noon, storm)
        );
        assert_eq!(
            effective_tint(&profile, noon, Weather::CLEAR),
            effective_tint(&profile, noon, storm)
        );
    }

    #[test]
    fn 开局那一刻可能出现的任何天气下视野都不低于基准半径的一半() {
        // 既有断言「开局视野至少要有基准半径的一半，否则开局仍然近乎
        // 瞎」在加进天气之后仍须成立——这是天气最容易破坏的一条既有
        // 保证（两个乘数相乘很容易把开局压穿；本批次实测雾的视野乘数
        // 取 650 时开局会掉到 5，因此本体表把它改成了 700，见
        // `ll_world::weather::materialize_base_weathers` 文档第 3 条）。
        //
        // 只遍历**开局那一季真的可能出现**的天气：雪在春季权重为 0，
        // 新游戏（春季早八点）永远不会开在雪天，把它算进来是在给一个
        // 不可能发生的组合立断言，会逼着未来的人为了让测试变绿去改一个
        // 与开局无关的数值。
        // Arrange
        let profile = surface_profile();
        let start = crate::world::NEW_GAME_START_TICK;
        let (_ids, table) = ll_world::weather::base_weather_fixture();
        let slot = ll_world::weather::season_slot(start.season());

        // Act & Assert
        let mut checked = 0;
        for index in table.registered() {
            if table.season_weights(*index)[slot] == 0 {
                continue;
            }
            checked += 1;
            let weather = Weather {
                kind: Some(*index),
                light_scale: table.light_scale(*index),
                sight_scale: table.sight_scale(*index),
                temperature_offset: 0,
            };
            let radius = effective_sight_radius(&profile, start, weather);
            assert!(
                radius >= BASE_SIGHT_RADIUS / 2,
                "开局视野半径 {radius} 低于基准半径的一半"
            );
        }
        assert!(
            checked >= 2,
            "开局那一季只有 {checked} 种可能的天气，这条断言几乎没检查到东西"
        );
    }

    #[test]
    fn 记忆色调比原始光照色调暗() {
        // Arrange
        let tint = [1.0, 1.0, 1.0, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert!(dimmed[0] < tint[0]);
    }

    #[test]
    fn 记忆色调不改变透明度() {
        // Arrange
        let tint = [0.6, 0.6, 0.6, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert_eq!(dimmed[3], tint[3]);
    }

    #[test]
    fn 全黑光照下记忆色调仍是全黑() {
        // Arrange：夜间/无光照场景，压暗系数不该把零变成非零。
        let tint = [0.0, 0.0, 0.0, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert_eq!(dimmed, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn 当前有视野的格子用原始色调绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(true, false, tint);

        // Assert
        assert_eq!(result, Some(tint));
    }

    #[test]
    fn 探索过但当前无视野的格子用记忆色调绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(false, true, tint);

        // Assert
        assert_eq!(result, Some(memory_tint(tint)));
    }

    #[test]
    fn 从未探索且当前无视野的格子不绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(false, false, tint);

        // Assert
        assert_eq!(result, None);
    }
}
