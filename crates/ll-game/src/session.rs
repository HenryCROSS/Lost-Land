//! 一局游戏的运行期状态——**只有玩家真的进了世界之后才存在的那一
//! 束东西**。
//!
//! # 这个类型解决的是「世界尚未存在」这个新状态
//!
//! 游戏主菜单（首页）落地之前，游戏一启动就直接进世界：`run_game` 先
//! `load_or_new_game`，再把 `GameWorld` 按值交给 `crate::app::Demo::new`。
//! 于是「世界」在整个运行期恒存在，`Demo` 的相关字段全部是非可空的。
//!
//! 首页把这个前提打破了：玩家在首页上选「开始游戏 / 读取存档」之前，
//! **世界还没有被建出来**。这个状态第一次出现在运行期。
//!
//! # 为什么是一个结构体，不是五个 `Option` 字段
//!
//! `Demo` 里依赖世界的字段不止 `game_world` 一个，另外四个都是
//! `Demo::new` 从它现推出来的：
//!
//! | 字段 | 从世界的哪一部分推出来 |
//! |---|---|
//! | `camera` | 玩家坐标 + `world.size` |
//! | `engine` | `mem::take(&mut game_world.timeline)` |
//! | `continent_field` | `game_world.noise` + `game_world.params` |
//! | `world_map_view` | `continent_field` + 玩家坐标 |
//! | `npc_ai` | `content` + `game_world.world.seed` |
//!
//! **这张表被世界地图缩放批次改写过一次**：原文是四行、正文写「五样
//! 东西」。世界地图的视野（`world_map_view`）是对着 `continent_field`
//! 与玩家位置建出来的，两个输入都只在世界存在时才存在，因此它属于同一
//! 束东西，落在这里而不是留在 `Demo` 上。
//!
//! 这六样东西的「存不存在」永远同生同死——要么全在，要么一个都没有。
//! 写成六个 `Option` 就是六个可空点、六处解包，而它们表达的其实是同
//! 一件事。合成一个 [`Session`]，`Demo` 上只留一个
//! `Option<Session>`：**`None` 就是「还在首页，世界尚未存在」这个状态
//! 的唯一表示**。
//!
//! # 为什么不是把 `Demo` 拆成 `enum { Title, InGame }`
//!
//! `Demo` 还持有十几个两种状态下都要用的字段（`content`、`config`、
//! `catalog`、`resources`、`save_path`……）——首页也要画字、也要进设置
//! 屏改配置、也要 GPU。拆成枚举会让它们全部重复一份或者再套一层。
//! `Option<Session>` 把可空性收敛到恰好那一处真正会空的东西上。
//!
//! # 它不是世界状态
//!
//! 与 `crate::world::GameWorld` 的关系：本类型**持有**它，但自己不是
//! 存档的一部分。`camera`/`continent_field`/`world_map_view`/`npc_ai`
//! 全是表现层或运行期派生数据（各自的字段文档写了为什么不进存档），
//! `engine` 持有的
//! 时间轴按 `next_action_at` 随时可重建（见
//! `crate::world::rebuild_timeline`）。存档序列化的只有
//! `game_world.world`。

use ll_core::time::Tick;
use ll_mod::native_behavior::NativeBehaviorSource;
use ll_render::camera::Camera;
use ll_sim::turn::TurnEngine;
use ll_world::overview::{ContinentField, generate_continent_field};
use ll_world::world_map::WorldMapView;

use crate::content::LoadedContent;
use crate::save_slot::SaveTarget;
use crate::world::GameWorld;

/// 一局正在进行的游戏，见模块文档。
pub struct Session {
    /// 世界本体加上它的噪声源、生成参数、玩家实体号与身份。
    pub game_world: GameWorld,
    /// 跟着玩家走的摄像机。
    pub camera: Camera,
    /// 回合引擎——世界时钟推进的唯一驱动者，见
    /// `crate::app::Demo::advance` 文档「世界时钟为什么会走」一节。
    /// 时间轴的权威副本在它手里（[`Session::begin`] 用
    /// [`std::mem::take`] 从 `game_world.timeline` 接管），此后
    /// `game_world.timeline` 恒为空。
    pub engine: TurnEngine,
    /// 世界地图（M 键）用的粗粒度地形场——建局/读档后只算这一次，见
    /// `ll_world::overview::generate_continent_field` 文档「调用方应在
    /// 世界创建时调用一次并长期持有结果」一节。纯表现层缓存，不进存档：
    /// 它只依赖噪声种子与地形表，读档后按读回来的参数重新生成同一份
    /// （种子相同则逐位相同）。
    pub continent_field: ContinentField,
    /// 世界地图当前的缩放档位与视野中心——所有者要的「直接对地图做一定
    /// 的缩放」落在这里，见 `ll_world::world_map::WorldMapView`。
    ///
    /// 与 `crate::app::Demo::world_map_open` 同一条纪律：**纯表现层
    /// 状态**，不进 `GameWorld`/`WorldState`、不进存档、不参与回放。
    /// 世界不因为玩家把地图拖到哪里、放大到第几档而有任何不同。
    ///
    /// **为什么在 `Session` 上而不是在 `Demo` 上**：它借 `continent_field`
    /// 才能建（`WorldMapView::centered_on_tile` 要拿到粗粒度地形场的
    /// 尺寸），并且初值对准玩家出生点——两个输入都随世界同生同死。留在
    /// `Demo` 上就要么多一个 `Option`，要么在首页那一刻拿着一份对着不
    /// 存在的世界建出来的视野。
    ///
    /// 打开地图那一刻重新对准玩家（见 `crate::app::Demo::advance`），而
    /// 不是记住上次关掉时停在哪：玩家按 M 最常见的意图是「我现在在哪」，
    /// 每次都从自己身上开始看比恢复一个可能已经与当前位置无关的旧视野
    /// 更有用。
    pub world_map_view: WorldMapView,
    /// NPC 决策来源——引擎自带的行为树，见
    /// `crate::app::npc_behavior_source` 文档。做成字段而不是每帧现造：
    /// 它持有一份内容表快照，每帧克隆五张表是一笔白付的开销。
    pub npc_ai: NativeBehaviorSource,
    /// 这一局写到哪个存档槽位——**进世界那一刻就定下来**，手动存档、
    /// 自动存档、退出存档三条路全部写同一份，见
    /// `crate::save_slot::SaveTarget`。
    ///
    /// 它落在 `Session` 上而不是 `Demo` 上，理由与 `world_map_view`
    /// 逐字相同：世界不存在时它也不存在（首页上没有「当前槽位」这种
    /// 东西），留在 `Demo` 上就要多一个 `Option`。
    pub save_target: SaveTarget,
    /// 上一次自动存档时的世界时钟。
    ///
    /// **世界时钟，不是墙钟**——见 `crate::app::Demo::maybe_autosave`
    /// 文档「为什么必须按世界时间」一节。
    pub last_autosave: Tick,
}

impl Session {
    /// 从一局**刚建好或刚读回来**的世界推出运行期状态。
    ///
    /// 三条进世界的路径全部走这里，不写第二份：
    ///
    /// 1. `crate::app::Demo::new`——直接构造一个已经在世界里的 `Demo`
    ///    （测试与旧调用点用）；
    /// 2. 首页的「开始游戏」（`crate::title_screen` 的
    ///    `ScreenOutcome::StartNewGame`）；
    /// 3. 首页的「读取存档」（`ScreenOutcome::LoadSave`）。
    ///
    /// 下一批的角色创建 / 世界配置 / 选重生点走完之后，终点仍然是本
    /// 函数——它是「世界准备好了，开始玩」这件事唯一的入口。
    pub fn begin(
        mut game_world: GameWorld,
        content: &LoadedContent,
        save_target: SaveTarget,
    ) -> Session {
        let player_pos = game_world
            .world
            .actors
            .get(game_world.player)
            .expect("玩家刚生成或刚读档，必然存在")
            .pos;
        let camera = Camera {
            center: player_pos,
            world: game_world.world.size,
        };
        // 接管时间轴——见 `Session::engine` 字段文档。
        let engine = TurnEngine::new(std::mem::take(&mut game_world.timeline));
        // 必须在 `game_world` 被移进下方的结构体字面量之前借出
        // `&game_world.world.terrain.layout()`。
        let continent_field = generate_continent_field(
            game_world.world.terrain.layout(),
            &game_world.noise,
            &game_world.params,
            &content.terrain_ids,
        );
        // 视野必须在 `continent_field` 被移进下方的结构体字面量之前建好
        // ——它借 `&continent_field`，与上面那句借 `layout()` 是同一条
        // 顺序约束。视野先对准玩家当前所在（新游戏是出生点，读档是存档
        // 里那个位置）；每次打开地图时还会重新对准，见
        // [`Session::world_map_view`] 字段文档。
        let world_map_view = WorldMapView::centered_on_tile(&continent_field, player_pos);
        let npc_ai = crate::app::npc_behavior_source(content, game_world.world.seed);
        // 自动存档的节拍从**进世界这一刻的世界时钟**起算，不是从 0 起
        // 算：读回一份已经玩了三天的存档时，从 0 起算会让「距上次自动
        // 存档超过一小时」立刻成立，一进世界就先卡一次盘。
        let last_autosave = game_world.world.clock;
        Session {
            game_world,
            camera,
            engine,
            continent_field,
            world_map_view,
            npc_ai,
            save_target,
            last_autosave,
        }
    }
}
