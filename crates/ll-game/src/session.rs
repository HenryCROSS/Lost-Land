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
//! | `npc_ai` | `content` + `game_world.world.seed` |
//!
//! 这五样东西的「存不存在」永远同生同死——要么全在，要么一个都没有。
//! 写成五个 `Option` 就是五个可空点、五处解包，而它们表达的其实是同
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
//! 存档的一部分。`camera`/`continent_field`/`npc_ai` 全是表现层或运行
//! 期派生数据（各自的字段文档写了为什么不进存档），`engine` 持有的
//! 时间轴按 `next_action_at` 随时可重建（见
//! `crate::world::rebuild_timeline`）。存档序列化的只有
//! `game_world.world`。

use ll_mod::native_behavior::NativeBehaviorSource;
use ll_render::camera::Camera;
use ll_sim::turn::TurnEngine;
use ll_world::overview::{ContinentField, generate_continent_field};

use crate::content::LoadedContent;
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
    /// NPC 决策来源——引擎自带的行为树，见
    /// `crate::app::npc_behavior_source` 文档。做成字段而不是每帧现造：
    /// 它持有一份内容表快照，每帧克隆五张表是一笔白付的开销。
    pub npc_ai: NativeBehaviorSource,
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
    pub fn begin(mut game_world: GameWorld, content: &LoadedContent) -> Session {
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
        let npc_ai = crate::app::npc_behavior_source(content, game_world.world.seed);
        Session {
            game_world,
            camera,
            engine,
            continent_field,
            npc_ai,
        }
    }
}
