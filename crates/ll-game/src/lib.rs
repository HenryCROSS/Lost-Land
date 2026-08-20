//! 迷途大陆的游戏本体：把六个验收 demo 各自证明过的零件（窗口、渲染、
//! 世界、战斗、UI、坐标）装成一台整机。
//!
//! 完整闭环：启动 → [`content::load_content`] 装载内容 →
//! [`world::build_new_world`] 建世界或 [`save::load_game`] 读档 →
//! 游玩（[`app::Demo`]，`Intent → resolve → Effect → apply`）→
//! [`save::save_game`] 存档 → 退出。
//!
//! # 为什么拆成库 + 薄二进制
//!
//! `src/main.rs` 只做两件事：声明进程级 `#[global_allocator]`（见其
//! 文档）与调用 [`run`]。其余全部逻辑（内容装载、世界搭建、存档
//! 读写）都在本库里，可以脱离真实窗口/GPU 被 `cargo test -p ll-game`
//! 直接覆盖——只有 [`app`] 模块（真正持有 GPU 资源、驱动事件循环）
//! 不可单元测试，这与本仓库其余验收 demo 的取舍一致：渲染/输入glue
//! 层薄而不可测，纯逻辑层厚而可测。

pub mod animation;
pub mod app;
pub mod content;
pub mod layout;
pub mod save;
pub mod world;

use std::path::{Path, PathBuf};

use ll_content::degrade::LoadOutcome;
use ll_platform::config::{load_or_default, save as save_config};
use ll_platform::logging::init_logging;
use ll_platform::window::{WindowConfig, run};

use app::Demo;
use content::load_content;
use world::{GameWorld, build_new_world};

/// 配置文件相对可执行文件所在目录的文件名。
const CONFIG_FILE_NAME: &str = "config.json";
/// 存档文件相对可执行文件所在目录的文件名——本体目前只有单一存档位
/// （规格 §11.2 模式2 默认单存档位，见 `ll_content::mode` 模块文档），
/// 多存档位管理属于 P7 存档界面范围。
const SAVE_FILE_NAME: &str = "save.llsave";
/// mod 根目录相对可执行文件所在目录的路径——与仓库根 `mods/` 对齐。
const MODS_DIR_NAME: &str = "mods";
/// 本体资产根目录相对可执行文件所在目录的路径——与仓库根 `assets/`
/// 对齐，内含本体自己的 `sprites/manifest.json`（见
/// `ll_mod::asset_vfs` 模块文档）。与 `mods/` 一样，发行时需要与可
/// 执行文件放在同一目录下——本体目前没有安装器，这是与 `mods/` 完全
/// 相同的既有部署假设，不是本次新增的限制。
const ASSETS_DIR_NAME: &str = "assets";

/// 新游戏使用的默认地形种子——本体目前没有开局选择种子的界面（P7），
/// 固定用一个值保证「同一份构建反复运行产出同一个世界」，便于开发期
/// 复现问题；未来开局界面接入后，这里应换成玩家输入或随机数。
const DEFAULT_SEED: u64 = 20_260_820;

/// 运行期用到的全部文件系统路径，集中一处方便测试与未来的命令行参数
/// 覆盖。
pub struct GamePaths {
    /// 配置文件路径。
    pub config: PathBuf,
    /// 存档文件路径。
    pub save: PathBuf,
    /// mod 根目录。
    pub mods_root: PathBuf,
    /// 本体资产根目录。
    pub assets_root: PathBuf,
}

impl GamePaths {
    /// 以 `base` 为根目录推出四个路径——生产环境用可执行文件所在目录，
    /// 测试用临时目录，两者走同一套推导逻辑,不需要两份实现。
    pub fn under(base: &Path) -> GamePaths {
        GamePaths {
            config: base.join(CONFIG_FILE_NAME),
            save: base.join(SAVE_FILE_NAME),
            mods_root: base.join(MODS_DIR_NAME),
            assets_root: base.join(ASSETS_DIR_NAME),
        }
    }
}

/// 可执行文件所在目录——配置、存档、mod 都相对这个目录寻址，而不是
/// 「当前工作目录」（玩家可能从任意目录双击启动或用不同的工作目录跑
/// 命令行），这样游戏在哪里启动都能找到同一份配置与存档。
fn executable_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 装载内容 → 建世界或读档：存档存在就读档，读档失败/降级为只读时
/// 记日志退回新游戏，从不 panic——存档损坏不该让玩家彻底玩不了。
fn load_or_new_game(paths: &GamePaths, content: &content::LoadedContent) -> GameWorld {
    if !paths.save.exists() {
        tracing::info!(path = %paths.save.display(), "未找到存档，开始新游戏");
        return new_game(content);
    }

    match save::load_game(&paths.save, content) {
        LoadOutcome::Playable(world) => {
            let player = world.player_entity.expect("可游玩的存档必然记录了玩家实体");
            tracing::info!(path = %paths.save.display(), "读档成功，继续游玩");
            GameWorld {
                world,
                noise: rebuild_noise(),
                params: default_params(),
                player,
            }
        }
        LoadOutcome::ReadOnly(_) => {
            tracing::warn!(
                path = %paths.save.display(),
                "存档因缺失内容降级为只读，本体二进制暂不支持只读模式游玩，改为开始新游戏"
            );
            new_game(content)
        }
        LoadOutcome::Rejected(error) => {
            tracing::error!(?error, path = %paths.save.display(), "存档读取失败，开始新游戏");
            new_game(content)
        }
    }
}

fn new_game(content: &content::LoadedContent) -> GameWorld {
    build_new_world(content, DEFAULT_SEED).expect("默认区块布局满足全部构造前置条件")
}

fn default_params() -> ll_world::generate::GenParams {
    ll_world::generate::GenParams {
        seed: DEFAULT_SEED,
        ..ll_world::generate::GenParams::default()
    }
}

/// 读档成功后重建噪声源——`WorldState` 反序列化不携带 `TileableNoise`
/// （它不是世界状态的一部分，是按 `params.seed`/世界布局可以随时重新
/// 派生出的派生数据，见 `ll_world::state::WorldState::terrain_at_streaming`
/// 文档同一取舍），流式加载继续需要它。噪声只依赖布局与种子,不需要
/// 已装载的内容,故不接收 `LoadedContent` 参数。
fn rebuild_noise() -> ll_world::noise::TileableNoise {
    let layout = world::build_zone_layout().expect("默认区块布局满足全部构造前置条件");
    ll_world::generate::build_zone_noise(&layout, &default_params())
        .expect("默认区块布局满足全部构造前置条件")
}

/// 完整启动流程：日志 → 配置 → 内容装载 → 建世界/读档 → 运行事件
/// 循环，直到窗口关闭或玩家按下取消键。
pub fn run_game() {
    init_logging(false).expect("首次初始化日志不应失败");

    let base = executable_dir();
    let paths = GamePaths::under(&base);

    let config = load_or_default(&paths.config);
    tracing::info!(
        vsync = config.display.vsync,
        scale_filter = ?config.display.scale_filter,
        zoom_default = ll_render::camera::Zoom::default().get(),
        zoom_range_min = crate::world::MIN_SAFE_ZOOM,
        zoom_range_max = crate::world::MAX_SAFE_ZOOM,
        "显示配置已装载"
    );
    // 首次启动时把默认配置写回磁盘——证明配置系统的写入路径确实被
    // 使用,不只是「有读的代码」。写入失败只记日志,不阻塞启动：配置
    // 文件系统是用户体验的一部分,不该因为一次性写入失败（例如目录
    // 只读）就让整个游戏起不来。
    if !paths.config.exists()
        && let Err(error) = save_config(&paths.config, &config)
    {
        tracing::warn!(%error, path = %paths.config.display(), "写出默认配置失败，继续使用内存中的默认值");
    }

    let content = load_content(&paths.mods_root, &paths.assets_root);
    tracing::info!(
        registered_mods = content.report.loaded_count(),
        failed_mods = content.report.failed_count(),
        "游戏内容装载完成"
    );

    let game_world = load_or_new_game(&paths, &content);
    tracing::info!(
        seed = game_world.world.seed,
        clock = game_world.world.clock.0,
        "世界就绪"
    );

    let window_config = WindowConfig {
        bindings: config.bindings,
        ..WindowConfig::default()
    };
    let demo = Demo::new(
        content,
        game_world,
        paths.save.clone(),
        "旅人".to_string(),
        config.display,
    );

    if let Err(error) = run(window_config, demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 路径推导把三个文件都放在同一个基目录下() {
        // Arrange
        let base = PathBuf::from("/tmp/lostland-test-base");

        // Act
        let paths = GamePaths::under(&base);

        // Assert
        assert_eq!(paths.config, base.join(CONFIG_FILE_NAME));
        assert_eq!(paths.save, base.join(SAVE_FILE_NAME));
        assert_eq!(paths.mods_root, base.join(MODS_DIR_NAME));
        assert_eq!(paths.assets_root, base.join(ASSETS_DIR_NAME));
    }

    #[test]
    fn 存档不存在时读档流程建出一局带玩家实体的新游戏() {
        // 端到端断言：load_or_new_game 在没有存档时确实产出一个可玩
        // 世界,而不是 panic 或产出一个没有玩家的空世界。
        // Arrange
        let base =
            std::env::temp_dir().join(format!("ll-game-lib-test-no-save-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
        let paths = GamePaths::under(&base);
        let content = load_content(&paths.mods_root, &paths.assets_root);

        // Act
        let game_world = load_or_new_game(&paths, &content);

        // Assert
        assert!(game_world.world.actors.get(game_world.player).is_some());

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn 存档存在时读档流程读回同一个玩家实体位置() {
        // Arrange：先用 new_game + save_game 产出一份真实存档。
        let base =
            std::env::temp_dir().join(format!("ll-game-lib-test-with-save-{}", std::process::id()));
        std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
        let paths = GamePaths::under(&base);
        let content = load_content(&paths.mods_root, &paths.assets_root);
        let original = new_game(&content);
        let original_pos = original
            .world
            .actors
            .get(original.player)
            .expect("刚生成必然存在")
            .pos;
        save::save_game(
            &paths.save,
            &content,
            &original,
            "测试旅人",
            "出生地",
            ll_content::mode::SaveMode::Permadeath,
        )
        .expect("写出应当成功");

        // Act
        let reloaded = load_or_new_game(&paths, &content);

        // Assert
        let reloaded_pos = reloaded
            .world
            .actors
            .get(reloaded.player)
            .expect("读档后玩家实体应当仍存在")
            .pos;
        assert_eq!(reloaded_pos, original_pos);

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }
}
