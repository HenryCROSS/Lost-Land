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
pub mod menu_screen;
pub mod player_action;
pub mod save;
pub mod settings_view;
pub mod surface_draw;
#[cfg(test)]
mod test_support;
pub mod world;
pub mod worldgen;

use std::path::{Path, PathBuf};

use crate::save::LoadedGame;
use ll_i18n::Catalog;
use ll_platform::config::{load_or_default, save as save_config};
use ll_platform::logging::init_logging;
use ll_platform::window::{WindowConfig, run};

use app::Demo;
use content::load_content;
use world::{GameWorld, build_new_world, rebuild_timeline};

/// 配置文件相对可执行文件所在目录的文件名。格式是 JSON5，不是
/// JSON——项目所有者 2026-08-20 裁定手写配置统一 JSON5，见
/// `ll_platform::config` 模块文档「格式：JSON5，读写不对称」一节：
/// 玩家手改这份文件时可以加注释、留尾逗号。
const CONFIG_FILE_NAME: &str = "config.json5";
/// 存档文件相对可执行文件所在目录的文件名——本体目前只有单一存档位
/// （规格 §11.2 模式2 默认单存档位，见 `ll_content::mode` 模块文档），
/// 多存档位管理属于 P7 存档界面范围。
const SAVE_FILE_NAME: &str = "save.llsave";
/// mod 根目录相对可执行文件所在目录的路径——与仓库根 `mods/` 对齐。
const MODS_DIR_NAME: &str = "mods";
/// 本体资产根目录相对可执行文件所在目录的路径——与仓库根 `assets/`
/// 对齐，内含本体自己的 `sprites/manifest.json5`（见
/// `ll_mod::asset_vfs` 模块文档）。与 `mods/` 一样，发行时需要与可
/// 执行文件放在同一目录下——本体目前没有安装器，这是与 `mods/` 完全
/// 相同的既有部署假设，不是本次新增的限制。
const ASSETS_DIR_NAME: &str = "assets";
/// 本体本地化文件目录，相对可执行文件所在目录——与
/// `knowledge/design/mod-package-structure.md`「本地化文件」一节
/// `locales/<语言标签>.ftl` 的固定目录名约定一致，本体（命名空间
/// `lostland`）的这一份就放在资产根目录下，与任何 mod 的 `locales/`
/// 是同一套查找规则,不需要另开一条特殊路径。
const LOCALES_DIR_NAME: &str = "locales";

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
    /// 本体本地化文件目录（`assets_root` 下的 `locales/`），见
    /// [`LOCALES_DIR_NAME`]。
    pub locales_root: PathBuf,
}

impl GamePaths {
    /// 以 `base` 为根目录推出五个路径——生产环境用可执行文件所在目录，
    /// 测试用临时目录，两者走同一套推导逻辑,不需要两份实现。
    pub fn under(base: &Path) -> GamePaths {
        let assets_root = base.join(ASSETS_DIR_NAME);
        GamePaths {
            config: base.join(CONFIG_FILE_NAME),
            save: base.join(SAVE_FILE_NAME),
            mods_root: base.join(MODS_DIR_NAME),
            locales_root: assets_root.join(LOCALES_DIR_NAME),
            assets_root,
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

/// 显式覆盖数据目录（`assets_root`/`mods_root` 的共同上级）的环境变量
/// ——见 [`resolve_data_dir`] 文档「三级查找顺序」一节，设置了就跳过
/// 后面两条自动探测，方便测试与将来打包（例如 CI 跑一份临时装配好的
/// 数据目录，或者未来的安装器把数据目录放在与可执行文件不同的位置）。
pub const DATA_DIR_ENV_VAR: &str = "LOSTLAND_DATA_DIR";

/// 定位不到任何含 `assets/` 目录的候选路径——[`resolve_data_dir`] 三级
/// 查找全部失败时返回的错误,取代此前「找不到就静默回退成 `exe_dir`
/// 本身继续跑」的旧行为。
///
/// # 为什么静默回退是真实缺陷，不是可接受的降级
///
/// 旧行为在 `cargo run`（exe 位于 `target/{debug,release}/`，资产仍在
/// 仓库根）下会让 `assets_root`/`mods_root` 都指向一个不存在的目录，
/// `ll_mod::asset_vfs::build` 对不存在的目录只会产出一份空 VFS
/// （`sprites` 长度为零），装载阶段本身不报任何错误——直到每一帧绘制
/// 都要按精灵名查图集，才在 `crate::app::Demo` 里刷出一屏
/// 「图集条目缺失，跳过本次绘制」的 ERROR。同一个根因（数据目录解析
/// 错误）被推迟到渲染阶段才第一次表现出来，而且是刷屏而不是一条,诊断
/// 起来比在启动那一刻直接失败困难得多——这正是本类型要修的坑。
#[derive(Debug)]
pub struct DataDirNotFound {
    searched_from: PathBuf,
}

impl std::fmt::Display for DataDirNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "从 {} 开始逐级向上查找，都找不到一个含 {ASSETS_DIR_NAME}/ 子目录的祖先目录；\
请确认可执行文件旁边有 {ASSETS_DIR_NAME}/ 与 {MODS_DIR_NAME}/（发布布局），\
或设置 {DATA_DIR_ENV_VAR} 环境变量显式指定数据目录",
            self.searched_from.display()
        )
    }
}

impl std::error::Error for DataDirNotFound {}

/// 从 `start` 出发，先看 `start` 自己是否含 `assets/` 子目录，找不到就
/// 逐级向上查找第一个含 `assets/` 子目录的祖先——发布布局（exe 与
/// `assets/`/`mods/` 同目录）在第一步就命中；开发布局（`cargo run`
/// 产出的 exe 深埋在 `target/{debug,release}/` 下，资产在仓库根）天然
/// 靠向上查找命中：`target/release/` 的上一级是 `target/`，再上一级
/// 就是仓库根。
///
/// 拆成独立于 [`resolve_data_dir`] 的纯函数，是为了不依赖
/// `std::env::current_exe()`（该函数在测试环境下返回的是测试二进制
/// 自己的真实路径，测试无法控制它指向哪里）——查找算法本身只依赖一个
/// 传入的起点路径，可以直接用临时目录测试两种布局与「两处都找不到」
/// 的失败路径。
fn find_data_dir_from(start: &Path) -> Result<PathBuf, DataDirNotFound> {
    if start.join(ASSETS_DIR_NAME).is_dir() {
        return Ok(start.to_path_buf());
    }
    let mut current = start;
    while let Some(parent) = current.parent() {
        if parent.join(ASSETS_DIR_NAME).is_dir() {
            return Ok(parent.to_path_buf());
        }
        current = parent;
    }
    Err(DataDirNotFound {
        searched_from: start.to_path_buf(),
    })
}

/// 三级查找顺序推出真正要用的数据目录（`assets_root`/`mods_root` 的
/// 共同上级）：
///
/// 1. [`DATA_DIR_ENV_VAR`] 环境变量——显式覆盖，优先级最高，设置了就
///    不再走后面两条自动探测。
/// 2. 可执行文件所在目录本身——发布布局，**必须保住的既有行为**。
/// 3. 从可执行文件目录逐级向上查找（[`find_data_dir_from`]）——开发
///    布局，`cargo run`/`cargo test` 场景。
///
/// 三级都找不到时返回 `Err`——见 [`DataDirNotFound`] 文档「为什么静默
/// 回退是真实缺陷」一节：调用方（[`run_game`]）应当在这里直接终止
/// 启动，而不是拿一个不存在的目录继续往下跑。
fn resolve_data_dir() -> Result<PathBuf, DataDirNotFound> {
    resolve_data_dir_with(std::env::var_os(DATA_DIR_ENV_VAR), &executable_dir())
}

/// [`resolve_data_dir`] 的可测试版本——环境变量的值与「可执行文件所在
/// 目录」都作为参数传入，而不是分别读取真实的进程环境变量与
/// `std::env::current_exe()`，理由同 [`find_data_dir_from`] 文档：两者
/// 在测试环境下都不受测试控制。
fn resolve_data_dir_with(
    env_override: Option<std::ffi::OsString>,
    exe_dir: &Path,
) -> Result<PathBuf, DataDirNotFound> {
    if let Some(over) = env_override {
        return Ok(PathBuf::from(over));
    }
    find_data_dir_from(exe_dir)
}

/// 装载内容 → 建世界或读档：存档存在就读档，读档失败/降级为只读时
/// 记日志退回新游戏，从不 panic——存档损坏不该让玩家彻底玩不了。
fn load_or_new_game(
    paths: &GamePaths,
    content: &content::LoadedContent,
    new_game_config: &ll_platform::config::NewGameConfig,
) -> GameWorld {
    if !paths.save.exists() {
        tracing::info!(path = %paths.save.display(), "未找到存档，开始新游戏");
        return new_game(content, new_game_config);
    }

    match save::load_game(&paths.save, content) {
        LoadedGame::Playable {
            mut world,
            identity,
        } => {
            let player = world.player_entity.expect("可游玩的存档必然记录了玩家实体");
            // 编年史与 noise 同一类「按种子随时能重新派生」的运行期数据，
            // 不随 `WorldState` 序列化（ADR 0009，见
            // `ll_world::surface_store::SurfaceStore` 的 `chronicle`
            // 字段文档）。这里只 `attach`，不 `install`——存档里的常驻
            // 区块早就带着据点，而且可能已经被玩家改过，绝不能重铺。
            world
                .terrain
                .attach_chronicle(std::sync::Arc::new(rebuild_chronicle(
                    content,
                    &world.gen_params(),
                )));
            tracing::info!(path = %paths.save.display(), "读档成功，继续游玩");
            // 时间轴与 noise 同一类「运行期派生数据」，不随
            // `WorldState` 序列化——按每个存活实体已持久化的
            // `next_action_at` 重建即可，见
            // `crate::world::rebuild_timeline` 文档「为什么时间轴不进
            // 存档」一节。
            let timeline = rebuild_timeline(&world);
            // 生成参数取**存档里记着的那一组**，不是配置文件里的、更
            // 不是一份默认值——见 `WorldState::gen_params` 文档「为什么
            // 读档后必须走这里」一节：读档路径此前重建噪声源时取的是
            // 本体默认种子加默认形态，玩家一旦能自己选，往前走一步地形
            // 就会换一张图。
            let params = world.gen_params();
            GameWorld {
                world,
                noise: rebuild_noise(&params),
                params,
                player,
                timeline,
                // 世界身份取**存档头里记着的那一份**，不是按当前会话
                // 重新算一份——生成期 mod 集合只存在于存档头，重算等于
                // 用「玩家现在开着哪些 mod」覆盖掉「这个世界当初是用
                // 哪些 mod 生成的」，见 `crate::save` 模块文档第一节。
                identity,
            }
        }
        LoadedGame::ReadOnly(_) => {
            tracing::warn!(
                path = %paths.save.display(),
                "存档因缺失内容降级为只读，本体二进制暂不支持只读模式游玩，改为开始新游戏"
            );
            new_game(content, new_game_config)
        }
        LoadedGame::Rejected(error) => {
            tracing::error!(?error, path = %paths.save.display(), "存档读取失败，开始新游戏");
            new_game(content, new_game_config)
        }
    }
}

/// 按玩家的新游戏配置建一局新世界——`config.json5` 的
/// `new_game` 段在这里、也只在这里，真正变成一张地图。
fn new_game(
    content: &content::LoadedContent,
    new_game_config: &ll_platform::config::NewGameConfig,
) -> GameWorld {
    let params = worldgen::resolve_gen_params(new_game_config);
    build_new_world(content, params).expect("默认区块布局满足全部构造前置条件")
}

/// 读档成功后重建噪声源——`WorldState` 反序列化不携带 `TileableNoise`
/// （它不是世界状态的一部分，是按 `params.seed`/世界布局可以随时重新
/// 派生出的派生数据，见 `ll_world::state::WorldState::terrain_at_streaming`
/// 文档同一取舍），流式加载继续需要它。噪声只依赖布局与种子,不需要
/// 已装载的内容,故不接收 `LoadedContent` 参数。
fn rebuild_noise(params: &ll_world::generate::GenParams) -> ll_world::noise::TileableNoise {
    let layout = world::build_zone_layout().expect("默认区块布局满足全部构造前置条件");
    ll_world::generate::build_zone_noise(&layout, params).expect("默认区块布局满足全部构造前置条件")
}

/// 读档成功后重新派生世界编年史——与 [`rebuild_noise`] 同一条纪律，
/// 见 `ll_world::chronicle` 模块文档「为什么编年史不进存档」。
///
/// 与 `rebuild_noise` 不同的是本函数需要 `LoadedContent`：判断「哪个
/// 区块能住人」要读地形属性表（`blocks_move`）、「这里有什么资源」要读
/// 资源表，而两者的索引都依赖当前会话的注册结果。
fn rebuild_chronicle(
    content: &content::LoadedContent,
    params: &ll_world::generate::GenParams,
) -> ll_world::chronicle::WorldChronicle {
    let layout = world::build_zone_layout().expect("默认区块布局满足全部构造前置条件");
    let noise = ll_world::generate::build_zone_noise(&layout, params)
        .expect("默认区块布局满足全部构造前置条件");
    ll_world::chronicle::WorldChronicle::generate(
        &ll_world::chronicle::ChronicleInput {
            layout: &layout,
            noise: &noise,
            params,
            terrain_ids: &content.terrain_ids,
            terrain_table: &content.terrain_table,
            resources: &content.resource_table,
            cultures: &content.culture_table,
        },
        ll_world::chronicle::ChronicleParams::default(),
    )
}

/// 用 `catalog` 把 `title_key` 解析成 `language` 下的真实显示文本。
///
/// 单独拆出这个一行函数，是为了给「键 → 加载器 → 实际渲染文字」这条
/// 链路留一个不需要真开窗口就能单元测试的接缝——[`run_game`] 本身因为
/// 会驱动 winit 事件循环，不可单元测试（见模块文档「为什么拆成库 + 薄
/// 二进制」一节），但标题解析这一步的正确性不应该因此就测不到。
fn resolve_window_title(catalog: &Catalog, language: &str, title_key: &'static str) -> String {
    catalog.resolve(language, title_key)
}

/// 完整启动流程：日志 → 配置 → 内容装载 → 建世界/读档 → 运行事件
/// 循环，直到窗口关闭或玩家按下取消键。
pub fn run_game() {
    init_logging(false).expect("首次初始化日志不应失败");

    let base = resolve_data_dir().unwrap_or_else(|error| {
        // 找不到数据目录本身就是一条无法继续的启动期错误——不是回退
        // 到某个猜测目录静默往下跑（那正是本次要修的旧缺陷，见
        // `DataDirNotFound` 文档），直接终止，把可诊断的原因打进日志
        // 后 panic。
        tracing::error!(%error, "无法定位数据目录，游戏无法继续启动");
        panic!("{error}");
    });
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

    // 内容装载失败是一条启动期硬错误，与下面「图集为空」同一条纪律。
    // 两种失败原因（见 `crate::content::ContentLoadError`）：本体内容
    // 契约没解析成功——本体内容（当前是三个种族）现在住在
    // `mods/lostland/` 的脚本里，误删/改名那个目录会让
    // `content.race_ids.human` 背后没有任何真实内容；或者跨表引用
    // 完整性没通过——某个 mod（也可能是本体自己）声明了一条指向不存在
    // 内容的引用。与其让玩家进到一个建不出角色、或者某件武器算不出
    // 伤害的残破会话里自己猜原因，不如在启动那一刻就点名。
    // 错误文案本身已经逐条列出全部明细，这里补上它不知道的那一半：
    // 本次会话的 mods_root 究竟指向哪里。
    let content = load_content(&paths.mods_root, &paths.assets_root).unwrap_or_else(|error| {
        tracing::error!(
            mods_root = %paths.mods_root.display(),
            %error,
            "内容装载校验失败，游戏无法继续启动：请确认 mods_root 下的本体内容目录              lostland/ 完整存在且未被改名或删除，并检查已装载 mod 的跨表引用"
        );
        panic!(
            "内容装载校验失败（mods_root={}）：
{error}",
            paths.mods_root.display()
        );
    });
    tracing::info!(
        registered_mods = content.report.loaded_count(),
        failed_mods = content.report.failed_count(),
        sprites = content.asset_vfs.sprites.len(),
        "游戏内容装载完成"
    );

    // 图集为空是一条启动期硬错误，不是等到每帧绘制时才暴露的降级——
    // 见 `DataDirNotFound` 文档同一个教训：根因（数据目录解析错误，或
    // assets_root 下确实没有任何精灵声明）被推迟到渲染阶段才第一次
    // 表现出来，会在 `crate::app::Demo` 里刷出一整屏「图集条目缺失」
    // 的 ERROR，而不是一条能直接定位原因的失败。这里选择直接终止启动
    // （而不是打一条 ERROR 后继续跑）：一个画不出任何精灵的会话对玩家
    // 没有可玩性，与其让他们面对空白画面 + 刷屏日志自己猜原因，不如
    // 在启动那一刻就给出「去检查 assets_root/mods_root 是否指对了
    // 地方」这个明确、可行动的结论。
    if content.asset_vfs.sprites.is_empty() {
        tracing::error!(
            assets_root = %paths.assets_root.display(),
            mods_root = %paths.mods_root.display(),
            "图集为空（sprites=0）：assets_root/mods_root 下没有解析到任何精灵声明，\
        游戏画面将完全空白；多半是数据目录解析到了错误的位置，而不是内容本身缺失贴图"
        );
        panic!(
            "图集为空（sprites=0），拒绝继续启动：assets_root={}, mods_root={}",
            paths.assets_root.display(),
            paths.mods_root.display()
        );
    }

    let game_world = load_or_new_game(&paths, &content, &config.new_game);
    tracing::info!(
        seed = game_world.world.seed,
        clock = game_world.world.clock.0,
        "世界就绪"
    );

    // 装载本地化：真正的消费点在下面——用当前配置里的 `language`
    // 把 `title_key` 解析成实际显示文本，而不是让 winit 直接拿键名当
    // 标题（那是本地化系统落地之前的临时占位行为，见
    // `ll_platform::window::WindowConfig::title_key` 文档）。
    let catalog = Catalog::load_dir(&paths.locales_root);
    tracing::info!(
        language = %config.language,
        loaded_language_count = catalog.loaded_language_count(),
        locales_root = %paths.locales_root.display(),
        "本地化目录已装载"
    );

    // 克隆而不是移动：平台层查的是它自己那一份（`WindowConfig::bindings`），
    // 设置界面改的是 `Demo` 那一份，两者经
    // `ll_platform::window::AppHandler::take_rebound_keys` 同步。移动
    // 会让 `Demo` 拿不到初始键位表，设置界面第一屏就是空的。
    let mut window_config = WindowConfig {
        bindings: config.bindings.clone(),
        ..WindowConfig::default()
    };
    window_config.resolved_title =
        resolve_window_title(&catalog, &config.language, window_config.title_key);
    tracing::info!(
        title_key = window_config.title_key,
        resolved_title = %window_config.resolved_title,
        language = %config.language,
        "窗口标题已解析"
    );
    let demo = Demo::new(
        content,
        game_world,
        paths.save.clone(),
        "旅人".to_string(),
        config,
        paths.config.clone(),
        catalog,
    );

    if let Err(error) = run(window_config, demo) {
        tracing::error!(%error, "event loop terminated with error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 仓库根目录下真实的 `assets/locales/`——`ll-game` 位于
    /// `crates/ll-game`，向上两级到根。用真实资产而不是临时 fixture，
    /// 是因为这条测试要证明的正是「真实消费点接的是真实内容」，不是
    /// 「查表逻辑本身正确」（那部分已由 `ll_i18n::Catalog` 自己的单元
    /// 测试覆盖）。
    fn real_locales_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets")
            .join("locales")
    }

    #[test]
    fn 窗口标题键在中文配置下解析出中文标题() {
        // 端到端验证 run_game 里真正会执行的那条链路：
        // WindowConfig::default().title_key → Catalog::resolve → 中文
        // 标题文本，用的是仓库里真实的 assets/locales/zh-CN.ftl。
        // Arrange
        let catalog = Catalog::load_dir(&real_locales_dir());
        let title_key = WindowConfig::default().title_key;

        // Act
        let title = resolve_window_title(&catalog, "zh-CN", title_key);

        // Assert
        assert_eq!(title, "迷途大陆");
    }

    #[test]
    fn 窗口标题键切换到英文配置后解析出不同的标题() {
        // 这是「本地化真的能切换」在真实消费点上的证据，与
        // `ll_i18n::Catalog` 自己那条同名断言的测试是两个不同层次：
        // 那条测的是查表器本身，这条测的是本体二进制实际会用到的键
        // （`WindowConfig::title_key`）在切换语言后确实产出不同文本。
        // Arrange
        let catalog = Catalog::load_dir(&real_locales_dir());
        let title_key = WindowConfig::default().title_key;

        // Act
        let zh_title = resolve_window_title(&catalog, "zh-CN", title_key);
        let en_title = resolve_window_title(&catalog, "en", title_key);

        // Assert
        assert_ne!(zh_title, en_title);
        assert_eq!(en_title, "Lost Land");
    }

    #[test]
    fn 路径推导把全部文件都放在同一个基目录下() {
        // Arrange
        let base = PathBuf::from("/tmp/lostland-test-base");

        // Act
        let paths = GamePaths::under(&base);

        // Assert
        assert_eq!(paths.config, base.join(CONFIG_FILE_NAME));
        assert_eq!(paths.save, base.join(SAVE_FILE_NAME));
        assert_eq!(paths.mods_root, base.join(MODS_DIR_NAME));
        assert_eq!(paths.assets_root, base.join(ASSETS_DIR_NAME));
        assert_eq!(
            paths.locales_root,
            base.join(ASSETS_DIR_NAME).join(LOCALES_DIR_NAME)
        );
    }

    #[test]
    fn 存档不存在时读档流程建出一局带玩家实体的新游戏() {
        // 端到端断言：load_or_new_game 在没有存档时确实产出一个可玩
        // 世界,而不是 panic 或产出一个没有玩家的空世界。
        // Arrange
        let base = crate::test_support::unique_temp_path("ll-game-lib-test-no-save");
        std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
        let paths = GamePaths::under(&base);
        // mods_root 指向仓库真实的 mods/ 目录：本体内容（种族）现在
        // 住在 mods/lostland/ 里，临时目录下没有它，契约解析会（正确
        // 地）失败——见 `content::load_content` 文档。
        let content = load_content(&crate::test_support::repo_mods_dir(), &paths.assets_root)
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");

        // Act
        let game_world = load_or_new_game(
            &paths,
            &content,
            &ll_platform::config::NewGameConfig::default(),
        );

        // Assert
        assert!(game_world.world.actors.get(game_world.player).is_some());

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn 发布布局下exe所在目录本身含assets时直接解析到该目录() {
        // 发布布局：exe 旁边就有 assets/（与 mods/），第一级探测就该
        // 命中，不需要向上找。
        // Arrange
        let exe_dir = crate::test_support::unique_temp_path("ll-game-lib-test-release-layout");
        std::fs::create_dir_all(exe_dir.join(ASSETS_DIR_NAME)).expect("创建测试目录应当成功");

        // Act
        let resolved = find_data_dir_from(&exe_dir).expect("exe 目录本身含 assets/ 应当直接命中");

        // Assert
        assert_eq!(resolved, exe_dir);

        // Cleanup
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    #[test]
    fn 开发布局下exe在深层目录时向上找到含assets的祖先目录() {
        // 开发布局：cargo run 产出的 exe 深埋在 target/{debug,release}/
        // 下（这里用 a/b/c/ 模拟），assets/ 在最外层的 a/。
        // Arrange
        let root = crate::test_support::unique_temp_path("ll-game-lib-test-dev-layout");
        let exe_dir = root.join("b").join("c");
        std::fs::create_dir_all(&exe_dir).expect("创建测试目录应当成功");
        std::fs::create_dir_all(root.join(ASSETS_DIR_NAME)).expect("创建测试目录应当成功");

        // Act
        let resolved = find_data_dir_from(&exe_dir).expect("应当向上找到含 assets/ 的祖先目录");

        // Assert
        assert_eq!(resolved, root);

        // Cleanup
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn 逐级向上都找不到assets目录时返回错误而非静默回退() {
        // 找不到时必须是明确的 Err，不是回退到 exe_dir 本身继续跑
        // （旧行为，正是「静默用空目录继续跑」这个缺陷的根源）。
        // Arrange：一棵全新、确定不含 assets/ 的临时目录树；
        // std::env::temp_dir() 本身及其全部祖先目录都不含名为
        // assets 的子目录（真实开发机上的常规假设，本仓库的
        // assets/ 只存在于仓库根，不在临时目录所在的路径链上）。
        let exe_dir = crate::test_support::unique_temp_path("ll-game-lib-test-not-found");
        std::fs::create_dir_all(&exe_dir).expect("创建测试目录应当成功");

        // Act
        let result = find_data_dir_from(&exe_dir);

        // Assert
        assert!(result.is_err());

        // Cleanup
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    #[test]
    fn 数据目录环境变量覆盖优先于exe目录自动探测() {
        // resolve_data_dir_with 收到非空 env_override 时，即使
        // exe_dir 本身就含 assets/，也应当直接采用环境变量的值，
        // 不做任何自动探测——env 覆盖的语义是显式指定，不是"探测不到
        // 才用的兜底"。
        // Arrange
        let exe_dir = crate::test_support::unique_temp_path("ll-game-lib-test-env-override-exe");
        std::fs::create_dir_all(exe_dir.join(ASSETS_DIR_NAME)).expect("创建测试目录应当成功");
        let override_dir =
            crate::test_support::unique_temp_path("ll-game-lib-test-env-override-target");

        // Act
        let resolved = resolve_data_dir_with(Some(override_dir.clone().into_os_string()), &exe_dir)
            .expect("显式覆盖不应失败");

        // Assert
        assert_eq!(resolved, override_dir);

        // Cleanup
        let _ = std::fs::remove_dir_all(&exe_dir);
    }

    #[test]
    fn 存档存在时读档流程读回同一个玩家实体位置() {
        // Arrange：先用 new_game + save_game 产出一份真实存档。
        let base = crate::test_support::unique_temp_path("ll-game-lib-test-with-save");
        std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
        let paths = GamePaths::under(&base);
        let content = load_content(&crate::test_support::repo_mods_dir(), &paths.assets_root)
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let original = new_game(&content, &ll_platform::config::NewGameConfig::default());
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
        let reloaded = load_or_new_game(
            &paths,
            &content,
            &ll_platform::config::NewGameConfig::default(),
        );

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

    #[test]
    fn 改动新游戏配置不影响已存在存档读回后的世界摘要() {
        // `ll_platform::config` 模块文档「一个类别不同的字段」一节承诺
        // 的那条不变式：`new_game` 段是**建档期初值**，世界一旦建成就
        // 由存档接管，此后改配置文件不会改变任何一个已存在存档的重放
        // 结果。这条测试是那个承诺的唯一保证。
        //
        // 反例（已实跑验证会红）：把 `load_or_new_game` 读档分支里的
        // `world.gen_params()` 换回配置解析结果
        // （`worldgen::resolve_gen_params(new_game_config)`），
        // 第二条断言当场红——两次读回来的 `params` 会跟着配置一起变。
        // Arrange：用**默认**配置建一份存档。
        let base = crate::test_support::unique_temp_path("ll-game-lib-test-config-isolation");
        std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
        let paths = GamePaths::under(&base);
        let content = load_content(&crate::test_support::repo_mods_dir(), &paths.assets_root)
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let default_config = ll_platform::config::NewGameConfig::default();
        let original = new_game(&content, &default_config);
        save::save_game(
            &paths.save,
            &content,
            &original,
            "测试旅人",
            "出生地",
            ll_content::mode::SaveMode::Permadeath,
        )
        .expect("写出应当成功");
        let hash_before = original.world.hash();
        let params_before = original.world.gen_params();

        // Act：把配置改成完全不同的一档（群岛 + 另一个种子），再读同一
        // 份存档。
        let changed_config = ll_platform::config::NewGameConfig {
            terrain_preset: "archipelago".to_string(),
            seed: Some(999_999),
            ..ll_platform::config::NewGameConfig::default()
        };
        assert_ne!(
            worldgen::resolve_gen_params(&changed_config),
            worldgen::resolve_gen_params(&default_config),
            "两份配置必须真的解析出不同的参数，否则这条测试什么也没验证"
        );
        let reloaded = load_or_new_game(&paths, &content, &changed_config);

        // Assert
        assert_eq!(reloaded.world.hash(), hash_before);
        assert_eq!(reloaded.params, params_before);

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn 没有存档时新游戏真的按配置里的预设建世界() {
        // 另一半：配置在**该起作用**的时候必须真的起作用——否则「接线」
        // 只是把参数搬进了一个没人读的字段。
        // Arrange
        let base = crate::test_support::unique_temp_path("ll-game-lib-test-config-applies");
        std::fs::create_dir_all(&base).expect("创建测试目录应当成功");
        let paths = GamePaths::under(&base);
        let content = load_content(&crate::test_support::repo_mods_dir(), &paths.assets_root)
            .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
        let config = ll_platform::config::NewGameConfig {
            terrain_preset: "archipelago".to_string(),
            ..ll_platform::config::NewGameConfig::default()
        };
        let expected_shape = ll_content::world_identity::terrain_preset("archipelago")
            .expect("预设表里有群岛这一档")
            .shape;

        // Act：paths.save 不存在，走的是新游戏分支。
        let game_world = load_or_new_game(&paths, &content, &config);

        // Assert
        assert_eq!(game_world.world.terrain_shape, expected_shape);
        assert_eq!(game_world.params.shape, expected_shape);
        assert_ne!(
            expected_shape,
            ll_world::generate::TerrainShape::default(),
            "群岛预设与默认形态必须真的不同，否则这条测试什么也没验证"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&base);
    }
}
