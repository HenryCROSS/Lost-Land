//! `app::hud_draw`：把常驻 HUD 的几块面板喂给 `ll-ui` 并提交到这一帧。
//!
//! 本模块由 [`crate::app`] 按职责拆出（批次 16，纯搬移，没有改动任何逻辑）。
//! 拆分的依据不是行数而是「下一批要往哪里加东西」：对话批次要加一块屏、
//! UI 布局批次要改 HUD，两批原先撞在同一个文件的同两个函数上。主循环
//! （`impl AppHandler for Demo`）与 `Demo` 自身的状态仍然在 [`crate::app`]。

use ll_i18n::Catalog;
use ll_platform::window::FrameId;
use ll_render::wgpu;
use ll_sim::rule_modifier::{SubjectRegistry, agent_rule_modifiers, rule_modifier_displays};
use ll_ui::hud::character_panel::CharacterPanelData;
use ll_ui::hud::render::render_hud;
use ll_ui::hud::status_bar::StatusBarData;
use ll_ui::hud::world_map::{WorldMapPanelData, WorldMapSite};
use ll_ui::widget::state::WidgetStateTable;
use ll_world::overview::ContinentField;
use ll_world::settlement::SettlementStatus;
use ll_world::weather::Weather;
use ll_world::world_map::{WorldMapView, world_map_slice};

use crate::content::LoadedContent;
use crate::player_action::{Feedback, PlayerMenu};
use crate::world::GameWorld;

use super::gpu::GpuResources;

/// 把地表世界画到离屏目标：地形 + 玩家标记。
///
/// # 三层可见性（战争迷雾）
///
/// 项目所有者原话：「没有视野的地方就暗下来一些，有视野的地方就没
/// 问题。而没去过的地方就黑着」。三层对应到这里的三种处理：
///
/// 1. **从未探索**——完全跳过绘制，留下 `ll_render::batch` 既有的黑色
///    清屏背景（见 `crates/ll-render/src/batch.rs` 的
///    `wgpu::LoadOp::Clear(wgpu::Color::BLACK)`），不需要本函数另画
///    一层黑色。
/// 2. **探索过、当前无视野**——照常画出该格当前的地形（地形是确定性
///    噪声，参见 `ll_world::exploration` 模块文档「只存位图，不存
///    地形副本」：记忆里的样子等价于现在重新算出的样子，没有另存
///    快照的必要），但用比当前光照更暗的记忆色调。
/// 3. **当前有视野**——按 [`effective_tint`](crate::layout::effective_tint) 正常绘制。
///
/// 三层的判定表本身是 [`crate::layout::tile_tint`]（与 GPU 无关的纯
/// 函数，见其文档），本函数只负责喂参数、按结果决定画不画。
///
/// `exploration` 读自 `world.exploration`——`ExplorationMemory` 是随
/// `WorldState` 一起持久化、参与 `hash()` 的世界状态（见
/// `ll_world::state::WorldState::exploration` 字段文档），写入路径是
/// `ll_sim::resolve::resolve_move` 在玩家移动后追加的
/// `ll_sim::effect::Effect::MarkExplored`，经 `apply` 落地——本函数只
/// 读，不写。
///
/// # 缩放与可见性是两件独立的事
///
/// `zoom` 只影响**画在哪里、画多大**（`apply_zoom` 与逐精灵尺寸乘法）
/// 与**枚举多大范围**（`visible_tiles_zoomed`），完全不影响 FOV 半径
/// `radius`——视野看得多远是玩法规则（`effective_sight_radius`
/// 读的是空间属性表与时钟，两者都不知道 `zoom` 存在），缩放只是把
/// 「已经算好可见的这批格子」画得更大或更小、连带能塞进画布的格子
/// 更少或更多，从未反过来影响「哪些格子算可见」这个判定本身。
///
/// 同理，缩放也不改变上面三层的**归属**：拉远只是让更多格子进入枚举
/// 范围，每一格属于哪一层仍由 FOV 与探索记忆决定。
/// 画出只读观测 HUD（P7 第一批）：状态栏（常驻）、角色面板、背包、
/// 装备栏——四块面板全部读玩家 `Agent` 与 `world.clock` 现算,不修改
/// 任何世界状态,见 `ll_ui::hud` 模块文档「只读，不做任何交互」一节。
///
/// 拆成自由函数而非 `Demo` 的方法，理由与 [`render_surface`](super::surface::render_surface) 一致：
/// 调用点需要同时持有 `&self.game_world`/`&self.content`/`&self.catalog`
/// 与 `&mut resources`，写成 `&self` 方法会让借用检查器把两者混为一谈。
///
/// 玩家实体查不到时（不应该发生——`GameWorld::player` 恒指向一个刚
/// 生成或刚读档必然存在的实体）跳过本帧 HUD 绘制并记一条警告,不
/// panic：显示层的降级纪律与 `GpuResources::resolve_key`「整条候选链落空，
/// 跳过本次绘制」一致，不能因为一次意外的查询落空就让整个游戏崩溃。
///
/// # 世界地图（`world_map_open`/`continent_field`/`world_map_view`）
///
/// `world_map_open` 为假时 [`ll_ui::hud::render::build_hud_frame`] 收到
/// 的是 `None`，世界地图整块不参与本帧渲染——见 `ll_ui::hud::world_map`
/// 模块文档「战争迷雾」一节与 `ll_platform::input::GameKey::Map` 文档。
/// 为真时才现算一份 [`ll_world::world_map::world_map_slice`] 输出：这一步
/// 只读 `continent_field`（建局时算过一次，见 [`crate::session::Session::continent_field`]
/// 字段文档）、`world_map_view`（当前缩放档位与视野中心，见
/// [`crate::session::Session::world_map_view`]）与
/// `game_world.world.exploration`（真实探索记忆），不触发
/// 任何区块的按需生成、不修改任何世界状态——按需才算，避免地图关着的
/// 绝大多数帧白白花这份开销。
///
/// **这一段被缩放批次改写过一次**：原文说的是「现算一份
/// `ll_world::overview::continent_map` 输出」、开销是 `O(区块数)`。
/// 缩放落地之后走的是 `world_map_slice`：格子数恒等于屏上那一格阵列
/// （`cols × rows`），每格归并 `samples_per_cell²` 个采样点，于是开销
/// 是 **O(当前视野覆盖的采样点数)**——随缩放档位变化，拉到最近时远小于
/// 整个世界。
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_hud(
    game_world: &GameWorld,
    content: &LoadedContent,
    catalog: &Catalog,
    language: &str,
    resources: &mut GpuResources,
    // 见 `Demo::measurer` 字段文档：输入侧与渲染侧共用同一个测量器。
    measure: &mut dyn ll_text::MeasureText,
    view: &wgpu::TextureView,
    hud_anim: &mut WidgetStateTable,
    frame: FrameId,
    fps: f32,
    world_map_open: bool,
    continent_field: &ContinentField,
    world_map_view: &WorldMapView,
    // 玩家菜单与反馈行，见 `Demo::menu`/`Demo::feedback` 字段文档。
    menu: PlayerMenu,
    feedback: Option<Feedback>,
    // 选出生地屏那一刻的两处改写，`None` 表示正常游玩，见
    // [`SpawnPickHud`]。
    spawn_pick: Option<SpawnPickHud<'_>>,
) {
    let Some(agent) = game_world.world.actors.get(game_world.player) else {
        tracing::warn!("玩家实体查不到，本帧跳过 HUD 绘制");
        return;
    };

    // 状态栏里的天气：与 `render_surface` 各自派生一次，而不是从那边
    // 传过来。两处算出来的必然是同一个值（`Weather::derive` 是纯函数，
    // 输入只有世界种子与世界时钟，两处读的是同一个 `WorldState`），
    // 把它拎成一个跨函数参数只会在 `draw_hud` 的参数表上再加一项，换
    // 不来任何正确性——这正是「派生而不缓存」这条纪律的好处：不需要有
    // 人负责保证两处看到的天气一致。
    //
    // `weather_name_key` 必须在 `status` 之外声明：`StatusBarData` 借用
    // 它的字符串切片，与下面 `world_map_cells` 同一条既有写法。
    let weather = Weather::derive(
        game_world.world.seed,
        game_world.world.clock,
        &content.weather_table,
    );
    let weather_name_key = weather
        .kind
        .and_then(|kind| content.weather_table.display_name_key(kind))
        .map(|key| key.to_string());
    let status = StatusBarData {
        clock: game_world.world.clock,
        health: agent.health,
        mana: agent.mana,
        fps,
        weather_display_name_key: weather_name_key.as_deref(),
    };
    // 规则修正（抗性/易伤/偷袭/盘查减免/藏匿/制作产出加成/优势/劣势/
    // 重掷）——**这里是唯一的装配点**：`agent_rule_modifiers` 要同时
    // 拿到种族/职业/副职三张授予表、天赋表与物品表，`rule_modifier_displays`
    // 还要从伤害类别表/配方类别表里读出主语的 `display_name_key`，这七张
    // 表只在本函数里同时够得着（`content: &LoadedContent`）。面板层拿到的
    // 是已经按加值类型规则合并好的成品行，见
    // `ll_ui::hud::character_panel::CharacterPanelData::rule_modifiers`。
    //
    // 每帧现算一次，与上面天气「派生而不缓存」同一条纪律：修正来自
    // 天赋与**当前装备**，缓存就得有人负责在换装/损坏时让它失效。
    let rule_modifiers = rule_modifier_displays(
        &agent_rule_modifiers(
            agent,
            &content.race_table,
            &content.class_table,
            &content.subclass_table,
            &content.trait_table,
            &content.item_table,
        ),
        // 主语的显示名文案键：**读内容表声明的字段**，不按约定拼键
        // （旧做法与它的代价见 `ll_mod::damage_category` 模块文档
        // 「显示名字段」一节）。两张表在这里都够得着，这也正是本函数
        // 是唯一装配点的原因之一。
        &|registry, index| match registry {
            SubjectRegistry::DamageCategory => content
                .damage_category_table
                .get(index)
                .map(|def| def.display_name_key.clone()),
            SubjectRegistry::RecipeCategory => content
                .recipe_category_table
                .get(index)
                .map(|def| def.display_name_key.clone()),
        },
    );
    let character = CharacterPanelData {
        base_stats: agent.stats,
        active_stat_modifiers: &agent.active_stat_modifiers,
        equipment: &agent.equipment,
        level: agent.level,
        experience: agent.experience,
        xp_to_next_level: agent.xp_to_next_level,
        unspent_attribute_points: agent.unspent_attribute_points,
        unspent_skill_points: agent.unspent_skill_points,
        // 职业主属性倾向——查不到职业定义时是 None，面板整行不出现，
        // 见 `CharacterPanelData::primary_attribute` 文档。这是本仓库
        // 里 `ClassDef::primary_attribute` 的第一个真实消费者。
        primary_attribute: content
            .class_table
            .get(agent.profession)
            .map(|view| view.primary_attribute),
        now: game_world.world.clock,
        rule_modifiers: &rule_modifiers,
    };

    // 见本函数文档「世界地图」一节：`world_map_slice_data`/`world_map_sites`
    // 声明在 `if` 之外，让 `world_map_data` 借用的数据在传给 `render_hud`
    // 那一刻仍然存活。
    let world_map_slice_data;
    let world_map_sites;
    let world_map_data = if world_map_open {
        let layout = *game_world.world.terrain.layout();
        world_map_slice_data = world_map_slice(
            continent_field,
            &layout,
            // 选出生地屏传一份「全部已探索」的记忆进来，`explored` 恒为
            // 真，**同一份呈现代码**自然变成全图可见——没有 `reveal_all`
            // 标志，见 `ll_world::exploration::ExplorationMemory::fully_explored`
            // 与 `ll_ui::hud::world_map::site_marker_quads` 两处文档。
            spawn_pick
                .map(|overlay| overlay.exploration)
                .unwrap_or(&game_world.world.exploration),
            world_map_view,
        );
        // 玩家位置标记——纯呈现，由玩家坐标现算，不进 `WorldState`、
        // 不进 `OverviewCell`，见 `ll_ui::hud::world_map::WorldMapPanelData::player`
        // 字段文档。环面换算由 `WorldMapSlice::cell_of_tile` 负责（内部
        // 走 `TorusSize`），这里不手写任何取模；它与画格子用的是同一个
        // 视野原点，因此任何缩放档位、任何平移下标记都对得上。
        //
        // 不区分玩家当前在哪个 `Space`：世界地图画的是大陆平面，玩家
        // 下到地下时他在大陆上的**横向**位置没变，标记仍然应该指在那
        // 里——藏起来只会让玩家在地下彻底失去方位感。
        // 正常游玩时这是「我在哪」；**选出生地屏上它是「我将在哪」**
        // ——同一个橙色标记，同一段呈现代码。这不是把字段挪作他用：
        // `WorldMapPanelData::player` 的语义本来就是「玩家落在哪一格」，
        // 而选点期间玩家落在哪，答案就是光标那一格。
        let player = match spawn_pick {
            Some(overlay) => Some(overlay.cursor_cell),
            None => world_map_slice_data.cell_of_tile(agent.pos),
        };

        // 据点标记——所有者要「显示多点细节，好让玩家决定选哪里」，而
        // 「哪里有村子、哪里只剩废墟」大概率是那个决定里最重的一条。
        //
        // 数据来自编年史的 `sites()`：一个**已按区块光栅序排好的切片**
        // （见 `ll_world::chronicle::WorldChronicle::sites` 文档），顺序
        // 因此是世界数据自身的确定性顺序，不是任何哈希容器的桶序
        // （约束 C5）。默认世界二百多座，每帧一次线性遍历，与同一帧已经
        // 在跑的整屏归并相比可以忽略。
        //
        // 编年史拿不到时（`chronicle_handle` 为 `None`）就不画据点——
        // 与 `GpuResources::resolve_key`「整条候选链落空，跳过本次绘制」同一条
        // 显示层降级纪律，不 panic。
        //
        // **资源点没有画**：`SettlementSite` 只存了这座据点靠什么吃饭的
        // 两种资源（那是据点的属性），世界里真正的资源点分布没有一份
        // 可供概览查询的索引，要现算就得逐区块跑资源采样——那正是
        // `ContinentField` 存在的理由所要避免的开销。如实不做，不硬接。
        let chronicle = game_world.world.terrain.chronicle_handle();
        world_map_sites = chronicle
            .as_ref()
            .map(|chronicle| {
                chronicle
                    .sites()
                    .iter()
                    .filter_map(|site| {
                        world_map_slice_data
                            .cell_of_tile(site.anchor)
                            .map(|cell| WorldMapSite {
                                cell,
                                inhabited: matches!(site.status, SettlementStatus::Inhabited),
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Some(WorldMapPanelData {
            cells: &world_map_slice_data.cells,
            cols: world_map_slice_data.cols,
            rows: world_map_slice_data.rows,
            terrain_ids: &content.terrain_ids,
            player,
            sites: &world_map_sites,
            tiles_per_cell: world_map_view.tiles_per_cell(continent_field),
        })
    } else {
        None
    };

    // 菜单这一帧的行：与 `player_command` 各自独立重建一次，理由见
    // `crate::player_action::menu_rows` 文档。`menu_rows` 必须在
    // `menu_data` 之前声明——后者借用前者产出的字符串。
    let menu_rows = crate::player_action::menu_rows(
        menu,
        &game_world.world,
        game_world.player,
        &content.recipe_table,
        &content.item_table,
        catalog,
        language,
    );
    let menu_data = crate::player_action::menu_data(menu, &menu_rows);
    let feedback_text = feedback.map(|feedback| catalog.resolve(language, feedback.i18n_key()));

    render_hud(
        &mut resources.quad_renderer,
        &mut resources.textured_quad_renderer,
        &mut resources.text_renderer,
        measure,
        resources.gpu.device(),
        resources.gpu.queue(),
        view,
        resources.window_size.width,
        resources.window_size.height,
        &status,
        &character,
        &agent.inventory,
        &agent.equipment,
        // 观察者是玩家自己——未鉴定的东西在背包/装备两块面板上显示成
        // 「未鉴定的物品」，见 `ll_ui::hud::item_display_name`。
        &agent.identified_items,
        &content.item_table,
        &content.item_table,
        catalog,
        language,
        &resources.skin,
        hud_anim,
        frame.0,
        world_map_data.as_ref(),
        menu_data.as_ref(),
        feedback_text.as_deref(),
    );
}

/// 选出生地屏对世界地图 HUD 的两处改写。
///
/// # 为什么是「改写既有 HUD」而不是另画一块屏
///
/// 玩家在选点屏上看到的地图，与他进游戏之后按 M 看到的**必须是同一张**
/// ——同一套配色、同一个缩放档位表、同一份据点标记。另写一份呈现代码
/// 等于把「世界地图长什么样」变成两个真相源，两边一旦漂移，玩家会在
/// 「我按着地图选的地方」和「我进去之后看到的地方」之间对不上号。
///
/// 因此这里只改**两样东西**：探索记忆（换成全部已探索）与玩家标记落在
/// 哪一格（换成光标那一格）。其余一行不动。
#[derive(Clone, Copy)]
pub struct SpawnPickHud<'a> {
    /// 一份「全部已探索」的记忆，见
    /// `ll_world::exploration::ExplorationMemory::fully_explored`。
    pub exploration: &'a ll_world::exploration::ExplorationMemory,
    /// 选点光标落在地图的哪一格（列, 行）。
    pub cursor_cell: (u32, u32),
}
