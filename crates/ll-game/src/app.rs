//! 游戏本体的 [`AppHandler`] 实现：把 [`crate::content`]/[`crate::world`]/
//! [`crate::save`] 接到窗口事件循环上——启动 → 装载内容 → 建世界/读档
//! 已经在 [`crate::run_game`] 完成，本模块只负责「每帧输入 → 世界推进 →
//! 渲染」与「退出前存档」。
//!
//! 渲染管线（图集加载、精灵批、相机、FOV 裁剪可见格）直接复用
//! `ll-render` 已经交付的部件，取舍与 `ll-sim` 的
//! `p5_coordinate_acceptance` 完全一致（同一批零件，包括玩家精灵的
//! 行走/待机动画状态机——见 [`crate::animation`] 模块文档「这是『声明
//! 了但从没接线』的第十处修复」），差异只在本模块更薄——不做 Interior
//! 出入、不画小地图（规格 §15 把这类打磨排在 P7，见任务顶层说明「不是
//! 做 UI 项目」），聚焦「能玩、能存」这条最小闭环本身。

// 按职责拆开的五个子模块。主循环与 Demo 的状态仍在本文件，
// 每块子模块负责一件事，见各自的模块文档。
mod gpu;
mod hud_draw;
mod save_flow;
mod screen_flow;
mod surface;

// 搬出去的项在这里重新引进本模块的作用域：对外的公开路径
// （`ll_game::app::load_sprite_sources`）与 `mod tests` 里的调用因此一个字都不用改。
use self::gpu::GpuResources;
pub use self::gpu::load_sprite_sources;
use self::hud_draw::{SpawnPickHud, draw_hud};
use self::save_flow::write_save;
use self::screen_flow::{draw_screen, screen_row_texts};
use self::surface::render_surface;
use std::path::PathBuf;
use std::sync::Arc;

use ll_i18n::Catalog;
use ll_mod::native_behavior::{BehaviorRuleCatalogs, NativeBehaviorSource, NativeBehaviorTree};
use ll_mod::roster::SettlementRoles;
use ll_platform::config::GameConfig;
use ll_platform::fps::FpsCounter;
use ll_platform::input::{GameKey, InputState};
use ll_platform::keybind::InputContext;
use ll_platform::keybind::KeyBindings;
use ll_platform::window::{AppHandler, FrameId, FrameOutcome, PhysicalSize, Window};
use ll_render::anim::{AnimStateMachine, current_sprite_name};
use ll_render::camera::Zoom;
use ll_sim::effect::Effect;
use ll_sim::turn::PlayerTurnOutcome;
use ll_ui::widget::state::WidgetStateTable;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::world_map::WorldMapView;

use crate::animation::{self, FALLBACK_SPRITE};
use crate::content::{LoadedContent, RuntimeCatalogs};
use crate::menu_screen::{ScreenNotice, ScreenState};
use crate::player_action::{Feedback, PlayerCommand, PlayerMenu, player_command};
use crate::session::Session;
use crate::world::{GameWorld, MAX_SAFE_ZOOM, MIN_SAFE_ZOOM, STREAM_RADIUS_ZONES};

/// 本体二进制的 NPC 决策来源：**按职业选行为树**。
///
/// # 这里此前是什么，为什么必须改
///
/// NPC 生成批次落地时，这里硬选了**卫兵那棵树**发给全部物化出来的
/// NPC，当时的文档如实记着代价：「一整座村子的居民都会朝玩家走过来
/// ——这是分支二的直接后果，不是缺陷修不了，而是『按职业选一棵树』这
/// 条内容绑定还不存在。」
///
/// 那条内容绑定现在存在了：[`ll_mod::behavior_binding::ClassBehaviorBindings`]
/// ——`mods/lostland/classes.json5` 每条职业上的一个 `behavior` 字段，
/// 落进一张**不产生新 `ContentIndex`** 的旁表（形状照抄
/// `XpCurveBindings`）。本函数把它交给决策来源，于是：
///
/// - 卫兵 / 民兵 → 守卫型（走向视野内的人；盘查仍只有卫兵做）
/// - 据点管理者 / 农夫 / 猎户 / 屠夫 / 铁匠 / 渔夫 / 牧羊人 / 石匠
///   → 平民型（**连一次目标查询都不做**，因此不可能朝玩家走过来）
///
/// # 兜底为什么是平民，不是卫兵
///
/// 没有绑定的职业（第三方 mod 的新职业没写 `behavior`、或者实体压根
/// 没有职业）落在
/// [`NativeBehaviorSource::fallback`](ll_mod::native_behavior::NativeBehaviorSource)
/// 上。选平民那棵：**兜底应当是伤害最小的那一个**。选错成平民的代价
/// 是「这个 NPC 站着不动」，选错成卫兵的代价正是本批次要修的那个缺陷
/// ——一个没写绑定的 mod 职业会让整座村子重新朝玩家走过来。
///
/// # 哥布林那棵树在生产路径上仍然没有调用点
///
/// 野兽型原型已经接好（`behavior: "beast"` 就能绑上去），但本体内容
/// 里**没有任何一条职业绑它**——本体至今不生成怪物，`examplemod:frostbolt`
/// 那条技能也只在示例 mod 里。如实标注：这一支在生产装载下恒不成立，
/// 它的证据在 `crates/ll-mod/tests/` 那批用示例 mod 的集成测试里。
pub(crate) fn npc_behavior_source(
    content: &LoadedContent,
    world_seed: u64,
) -> NativeBehaviorSource {
    NativeBehaviorSource::new(
        NativeBehaviorTree::townsfolk(),
        BehaviorRuleCatalogs::snapshot(
            &content.race_table,
            &content.class_table,
            &content.subclass_table,
            &content.trait_table,
            &content.item_table,
        ),
        world_seed,
    )
    .with_class_bindings(content.class_behavior_bindings.clone(), &content.registry)
}

/// 每次「放大/缩小」动作激活时，缩放倍率的调整步长。
///
/// 取一个小到不会让画面一步跳变太多、又大到几次按键/滚动就能感受到
/// 明显差异的值——纯粹的手感取舍，不影响正确性，任意正数都不会破坏
/// `Zoom::new`/`MIN_SAFE_ZOOM`/`MAX_SAFE_ZOOM` 的钳制。
const ZOOM_STEP: f32 = 0.1;

/// 自动存档的间隔，按**世界时间**计（tick）。
///
/// 取既有常量 `TICKS_PER_HOUR`（36 000 tick = 游戏内一小时）而不是新造
/// 一个魔数。为什么必须是世界时间而不是墙钟，见
/// [`Demo::maybe_autosave`] 文档。
const AUTOSAVE_INTERVAL_TICKS: i64 = ll_core::time::TICKS_PER_HOUR;

/// 游戏本体的完整运行期状态。
pub struct Demo {
    content: LoadedContent,
    /// 正在进行的那一局——**`None` 表示世界尚未存在**（玩家还停在首页，
    /// 一次都没选过「开始游戏 / 读取存档」）。
    ///
    /// 它是本项目里「有没有世界」这个问题的唯一真相源，见
    /// `crate::session` 模块文档：世界、摄像机、回合引擎、粗粒度地形场、
    /// NPC 决策源这五样东西的存在性永远同生同死，因此合成**一个**
    /// `Option`，而不是五个各自可空的字段。
    ///
    /// 为 `None` 时有三处早退：[`Demo::advance`]（世界一个字节都不动）、
    /// [`Demo::maintain_streaming`]、`on_frame` 的渲染段（不画世界层、
    /// 不画 HUD，只画屏）。`on_exit` 也读它——没有世界就没有东西可存，
    /// 见那里的说明。
    session: Option<Session>,
    /// 当前画面缩放倍率——ADR 0020 甲区（渲染层浮点，结果只变成
    /// 像素，见 `ll_render::camera::Zoom` 文档），钳制在
    /// `[MIN_SAFE_ZOOM, MAX_SAFE_ZOOM]`（不是 `Zoom` 的通用上下限，
    /// 那两个常量的推导见 `crate::world` 模块文档「常驻区块集合完全
    /// 解耦」——本字段绝不进 `GameWorld`/`WorldState`，只是 `Demo`
    /// 自己的运行期渲染状态。
    zoom: Zoom,
    /// 存档目录（`<数据目录>/saves/`）——多槽位之后这里不再是单个
    /// 文件路径，见 `crate::save_slot` 模块文档。
    saves_dir: PathBuf,
    character_name: String,
    /// 玩家配置的**唯一真相源**：键位 + 显示 + 语言 + 刻意解绑清单。
    ///
    /// 此前本结构体只留了 `display`/`language` 两份拷贝，改不动、也存
    /// 不回。设置界面要能改这三样并显式写盘，就必须整份持有——两份
    /// 拷贝在设置界面落地那一刻会立刻变成两个会漂移的真相源。
    ///
    /// **仍然不是世界状态**：`ll_platform::config` 模块文档「配置不是
    /// 世界状态」一节那条约束原样成立，本字段绝不进 `GameWorld`/
    /// `WorldState`、不参与 `hash()`、不影响确定性重放；`ll-platform`
    /// 依赖不到 `ll-world` 这条依赖方向就是结构性保证。
    config: GameConfig,
    /// 配置文件路径——设置界面按下「保存」时写到这里。
    config_path: PathBuf,
    /// 玩家行走剪辑在 `content.clip_table` 里的下标——装载期由
    /// [`ll_mod::base_clip::register_base_clips`] 分配，见
    /// `LoadedContent::clip_ids`。
    walk_clip: usize,
    /// 玩家待机剪辑在 `content.clip_table` 里的下标，理由同
    /// `walk_clip` 字段文档。
    idle_clip: usize,
    /// 玩家精灵行走/待机动画状态的生命周期管理：电平驱动
    /// （[`AnimStateMachine::set_level`]），每帧由
    /// [`animation::update_player_animation`] 算出「现在该播放哪个
    /// 状态」——与 `ll-sim` 的 `p5_coordinate_acceptance::Demo::anim`
    /// 同一套接线方式，只是本体二进制这一份是独立的运行期实例。
    anim: AnimStateMachine,
    resources: Option<GpuResources>,
    /// 文本测量器——「这一行画出来多宽、断成几行」的唯一来源。
    ///
    /// # 为什么它在这里，不在 [`GpuResources`] 里
    ///
    /// 测量是纯 CPU 的（`ll_text::TextMeasurer`，见其模块文档），而
    /// **需要它的不只有渲染**：模态屏的鼠标命中要先算出每一行的矩形，
    /// 而行高现在按渲染出的行数走（规格 W2），于是输入这一侧也要测量。
    /// 把它放进 `GpuResources` 会让「没建窗口就点不了任何一行」——那正
    /// 是本字段落地时 `app_navigation_tests` 两条测试抓到的实际后果。
    ///
    /// 一个测量器服务输入与渲染两条路，两条路因此**不可能**对同一行算
    /// 出不同的高度——这正是「行矩形与行文字必须同一个产出点」那条
    /// 纪律（批次 15）在测量这一层的延续。
    measurer: ll_text::TextMeasurer,
    /// 本地化目录（P7 第一批：只读观测 HUD）——状态栏/角色面板/背包/
    /// 装备栏的全部标签、属性名、槽位名、物品名都经它解析，见
    /// `ll_ui::hud` 模块文档「三、所有文本必须走 i18n」一节对应的
    /// 任务书要求。由 [`crate::run_game`] 装载后移交给本类型持有——
    /// `run_game` 已经装载过一次用于解析窗口标题，本字段是同一份
    /// `Catalog`，不重复装载第二份。
    catalog: Catalog,
    /// HUD 条形动画的持久状态（P7 追加：血条/经验条动画）——按控件 id
    /// 索引的旁表,见 `ll_ui::widget::state` 模块文档「为什么是旁表」
    /// 一节：结构上不可能污染 `WorldState`,只影响画面。
    hud_anim: WidgetStateTable,
    /// 上一次玩家操作留下的反馈（`None` 表示没有话要说）——它是
    /// 「静默作废对玩家不成立」这条的落点，见
    /// `ll_sim::turn::PlayerTurnOutcome` 文档。
    ///
    /// # 为什么留到下一次操作，不做成定时淡出
    ///
    /// 定时淡出要一个墙钟或帧计数器，也就要回答「暂停时算不算」「掉帧
    /// 时补不补」这类与本批次无关的问题。留到下一次操作是更简单也更
    /// 诚实的语义：屏幕上那句话恒等于「你最近这一下按出了什么结果」，
    /// 玩家再按一次它就被换掉。
    feedback: Option<Feedback>,
    /// 据点职业名册解析结果——[`crate::world::materialize_nearby_settlements`]
    /// 每次物化都要用，同样只在建局/读档后解析一次（`SettlementRoles::resolve`
    /// 只是几次注册表查询，但它的输入——注册表——装载后不再变化）。
    settlement_roles: SettlementRoles,
    /// 状态栏帧率读数的墙钟计数器——见 `ll_platform::fps` 模块文档「为
    /// 什么用墙钟，不用帧计数」一节：只活在表现层，每帧调用一次
    /// [`FpsCounter::record_frame`]，产出的浮点数只用来拼状态栏文本。
    fps_counter: FpsCounter,
    /// **「现在盖着屏幕的是哪一层」的唯一真相源**——模态栈 + 模态屏 +
    /// 玩家菜单 + 世界地图，四样东西封在一个类型里，见
    /// [`crate::modal::Modal`] 模块文档。
    ///
    /// # 这个字段此前是四个字段
    ///
    /// `ui_modes` / `screen` / `menu` / `world_map_open` 曾经各自独立，
    /// 而后三者里只有 `screen` 会去压那个栈。规格
    /// `knowledge/design/ui-and-navigation.md` 第〇节把后果逐条列了出来
    /// （Esc 实现了两遍、地图那套一遍都没有、方向键被两处同时吃）。
    /// 合成一个字段之后，「两套模态各记各的」在 `crate::modal` 之外
    /// **写不出来**——那四样东西是私有的。
    ///
    /// 栈非空 ⇔ 有东西盖着屏幕 ⇔ 取消键该退一层而不是开主菜单
    /// （规格 N2）。**注意这不再等价于「`advance` 整段早退」**：玩家
    /// 菜单与世界地图现在也在栈里，而它们各自的早退判据仍然是各自
    /// 那一条，见 [`Demo::advance`]。
    modal: crate::modal::Modal,
    /// 磁盘上那份存档在**本次启动那一刻**存不存在——首页的「读取存档」
    /// 那一行能不能按，由它决定。
    ///
    /// 只在构造时算一次，不每帧 stat 一次文件系统：首页停留期间没有
    /// 任何路径能改变它（本批次不做「回到主菜单」，进了世界就回不到
    /// 首页，见本批次计划文档第八节第 3 条）。
    /// 磁盘上现有的存档槽位，按「最近存过的排在最前」排好。
    ///
    /// # 为什么缓存而不是每帧扫目录
    ///
    /// 首页与存档列表屏都要用它，而每帧 `read_dir` 加一次「逐份读头部」
    /// 是白付的开销（每份存档一次文件打开）。它在两个时刻刷新：构造
    /// `Demo` 时，以及每次**进入存档列表屏**时——玩家在游戏里存过一次
    /// 再回主菜单，列表必须是新的。
    save_slots: Vec<crate::save_slot::SaveSlot>,
    /// 窗口这一刻多大（物理像素），`None` = 窗口还没建出来。
    ///
    /// # 为什么它不住在 `GpuResources` 里
    ///
    /// `GpuResources::window_size` 也有一份，但那一份要等 GPU 资源建好
    /// 才有。而**窗口尺寸是窗口的事实，不是 GPU 的**：模态屏的行矩形
    /// （鼠标点在第几行）只需要窗口多大，不需要任何 GPU 对象。
    /// [`AppHandler::on_resume`]/[`AppHandler::on_resize`] 因此**先**记
    /// 这个字段、再去碰 `resources`。
    ///
    /// `None` 时鼠标一律不生效——与 [`Demo::clicked_spawn_zone`] 那条
    /// 既有降级同一条：没有窗口就没有窗口坐标可言。
    viewport: Option<(f32, f32)>,
    /// 指针在模态屏行列表上的跨帧状态（这次按下是从哪一行开始的、这一
    /// 刻悬停在哪一行），见 [`crate::pointer`] 模块文档。
    pointer: crate::pointer::PointerState,
    /// 菜单屏三条选项的焦点表——[`ll_ui::widget::focus`] 读写它。
    ///
    /// 与 `hud_anim` 同一条纪律（`ll_ui::widget::state` 模块文档「为
    /// 什么是旁表」）：结构上不可能污染 `WorldState`。
    screen_focus: WidgetStateTable,
    /// 设置界面这一帧要说的一句话（键位冲突、已保存等），`None` 表示
    /// 没有话要说。与 `feedback` 同一条「留到下一次操作」的语义。
    screen_notice: Option<ScreenNotice>,
    /// 本帧被设置界面改过、尚未交给平台层的键位表，见
    /// [`AppHandler::take_rebound_keys`]。
    pending_bindings: Option<KeyBindings>,
    /// 一局**尚未开始**的游戏：角色创建 / 世界配置 / 选出生地这三块屏
    /// 期间的全部状态，见 [`crate::chargen::NewGameDraft`]。
    ///
    /// # 为什么它与 [`Demo::session`] 是两个字段，而不是一个
    ///
    /// 两者表达的是**先后**而不是二选一：草稿在前（玩家还在挑），
    /// `Session` 在后（玩家进世界了）。选出生地那一刻草稿里已经有一局
    /// 真实的 `GameWorld`（不生成就没有地图可看），但那时**还不该有**
    /// `Session`——`Session::begin` 的语义是「世界准备好了，开始玩」，
    /// 而玩家还没决定在哪出生。
    ///
    /// 这样分还顺带保住了一条既有不变式：`session` 为 `Some` ⇔ 玩家真
    /// 的在世界里，因此 `save_on_exit`、`advance`、`draw_hud` 三处的
    /// 判据一个字都不用改——选点期间退出游戏不会写出一份「玩家还没选
    /// 好出生地」的存档。
    new_game_draft: Option<crate::chargen::NewGameDraft>,
}

impl Demo {
    /// 用已经装载好的内容与已经建好（新游戏或读档得来）的世界构造
    /// 运行期状态——两者都由 [`crate::run_game`] 在事件循环启动前准备好，
    /// 本类型不负责「建世界还是读档」这个决定本身。
    // 八个参数：全部是不同类型的具名值（内容、世界、路径、名字、
    // 显示配置、本地化目录、语言标签、事件分发器），调用点只有两处
    // （`crate::run_game` 与本模块的测试帮手），编译器对每一个都做
    // 类型检查——这里没有 `register-race` 那种「13 个裸整数靠数位置」
    // 的风险。真要收拢，正确形状是把「显示配置 + 本地化目录 + 语言」
    // 三个表现层参数打包成一个类型，那是一次独立的重构，不夹带在
    // 事件监听接线里。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        content: LoadedContent,
        game_world: GameWorld,
        saves_dir: PathBuf,
        character_name: String,
        config: GameConfig,
        config_path: PathBuf,
        catalog: Catalog,
    ) -> Demo {
        // 这条入口（测试与旧调用点）没有经过命名屏，就在存档目录里开一
        // 个以角色名命名的槽位——与首页那条路走同一个构造器，不另开
        // 「没有槽位的会话」这种状态。
        let target = crate::save_slot::SaveTarget::create_in(
            &saves_dir,
            &character_name,
            crate::save::now_unix_seconds(),
        );
        let session = Session::begin(game_world, &content, target);
        Demo::assemble(
            content,
            Some(session),
            saves_dir,
            character_name,
            config,
            config_path,
            catalog,
        )
    }

    /// 构造一个**停在游戏主菜单（首页）上**的运行期状态——世界尚未
    /// 存在，由玩家在首页上选「开始游戏」或「读取存档」之后才建出来。
    ///
    /// 这是 [`crate::run_game`] 现在唯一的入口。[`Demo::new`] 保留给
    /// 「直接构造一个已经在世界里的 `Demo`」那些调用点（本模块十几条
    /// 测试全部依赖它）：让那些测试先过一遍首页，只会让它们的主题
    /// （时钟推进、buff 到期、背包）被无关的 UI 状态污染。
    #[allow(clippy::too_many_arguments)]
    pub fn at_title(
        content: LoadedContent,
        saves_dir: PathBuf,
        character_name: String,
        config: GameConfig,
        config_path: PathBuf,
        catalog: Catalog,
    ) -> Demo {
        Demo::assemble(
            content,
            None,
            saves_dir,
            character_name,
            config,
            config_path,
            catalog,
        )
    }

    /// [`Demo::new`] 与 [`Demo::at_title`] 共用的装配步骤。
    ///
    /// **屏与模态栈的初值完全由 `session` 决定**，两者不是各自传进来的
    /// 参数：世界已经建好就直接进世界（没有屏、栈空、按 `Gameplay` 表
    /// 解析），世界尚未存在就停在首页（屏是 `Title`、栈压着一层、按
    /// `Menu` 表解析）。写成两个独立参数就允许出现「停在首页但栈是空的」
    /// 这种自相矛盾的组合。
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        content: LoadedContent,
        session: Option<Session>,
        saves_dir: PathBuf,
        character_name: String,
        config: GameConfig,
        config_path: PathBuf,
        catalog: Catalog,
    ) -> Demo {
        let at_title = session.is_none();
        let walk_clip = content.clip_ids.hero_walk.get() as usize;
        let idle_clip = content.clip_ids.hero_idle.get() as usize;
        tracing::info!(
            clip_count = content.clip_table.as_clips().len(),
            walk_clip,
            idle_clip,
            "玩家动画状态机已装载"
        );
        let settlement_roles = SettlementRoles::resolve(
            &content.registry,
            &content.class_table,
            &content.resource_table,
            &content.culture_table,
        );
        Demo {
            content,
            // 走 `Demo::new` 这条路的调用方给的是一局**已经建好**的世界
            // ——首页不在这条路上（那条路是 `Demo::at_title`）。
            session,
            save_slots: crate::save_slot::list_slots(&saves_dir),
            zoom: Zoom::default(),
            saves_dir,
            character_name,
            config,
            config_path,
            walk_clip,
            idle_clip,
            anim: AnimStateMachine::new(idle_clip, FrameId(0)),
            resources: None,
            // 建不出测量器意味着内置字体资产坏了——那种情况下整个 UI
            // 都画不出来，没有可降级的路径，与 `TextRenderer::new` 失败
            // 时同一条处理。
            measurer: ll_text::TextMeasurer::new()
                .expect("内置字体资产应能正常解析（与 TextRenderer::new 同一条来源）"),
            catalog,
            hud_anim: WidgetStateTable::new(),
            feedback: None,
            settlement_roles,
            fps_counter: FpsCounter::new(),
            // 首页在**第一帧之前**就已经开着，见
            // `crate::modal::Modal::at_title`。
            modal: if at_title {
                crate::modal::Modal::at_title()
            } else {
                crate::modal::Modal::in_world()
            },
            // **首页的第一项预先选中**——所有者 2026-08-29 裁定（交接
            // 文档第〇之二节第 1 条），规格 N10。见 [`Demo::open_menu`]
            // 上面那一段对被推翻的原论证的复盘。
            viewport: None,
            pointer: crate::pointer::PointerState::default(),
            screen_focus: if at_title {
                crate::menu_screen::preselected_focus(&crate::title_screen::TITLE_ITEM_IDS)
            } else {
                WidgetStateTable::new()
            },
            screen_notice: None,
            pending_bindings: None,
            new_game_draft: None,
        }
    }

    /// 每帧输入处理：先维护流式邻域（必须排在移动之前，见
    /// `ll_world::surface_store::SurfaceStore::stream_neighborhood`
    /// 文档），再处理缩放、动画与移动——动画判定只读 `input`（按住
    /// 状态），不依赖本帧是否真的产生了移动意图或移动是否成功（见
    /// [`animation::update_player_animation`] 文档），因此与缩放、移动
    /// 结算互不依赖，顺序先后不影响正确性，这里的排列只是让「本帧
    /// 输入」的处理顺序读起来更顺。
    ///
    /// # 世界时钟为什么会走
    ///
    /// 本方法此前直接 `intent_from_input` → `resolve` → `apply`,完全
    /// 绕开时间轴——`world.clock` 只在 `crate::world::build_new_world`
    /// 建局那一刻被赋值一次,此后再没有任何生产代码推进它。真实游玩
    /// 时,昼夜循环、buff 到期、技能冷却、地面物品老化全部靠这个会走
    /// 的时钟,而它从未走过,是本项目当时最严重的缺陷。
    ///
    /// 现在改由 [`ll_sim::turn::TurnEngine::advance_ai`]/[`ll_sim::turn::TurnEngine::try_player_turn`]
    /// 驱动：先结算排在玩家之前的非受控实体回合（本体二进制目前没有
    /// NPC——NPC 生成批次之后**不再恒是空操作**，见
    /// [`npc_behavior_source`] 文档),再尝试用本帧输入
    /// 结算玩家一次行动——`try_player_turn` 内部才会真正
    /// `world.clock = entry.at`。**这是本仓库回合制的核心手感：玩家不
    /// 行动,时间就不走**（详见 `ll_sim::timeline` 模块文档「为什么不是
    /// 『每个实体一轮』的传统回合制」与 `Intent::Wait` 的存在本身——
    /// 「等待一回合」在纯实时游戏里没有意义,只有回合制才需要一个显式
    /// 「什么都不做但仍然让时间前进」的意图）。没有按任何方向/等待键
    /// 的这一帧,`try_player_turn` 直接返回假,时钟原地不动。
    // `input` 是 `&mut`：本方法末尾的死亡处理要切输入上下文（压一层
    // 模态栈），而上下文切换按设计必须清空按键状态——玩家死的那一刻可能
    // 正按着方向键，见 `ll_ui::widget::ui_mode::UiModeStack::push`。
    fn advance(&mut self, input: &mut InputState, frame: FrameId) {
        // 模态屏盖着的时候，世界一个字节都不动——不跑流式维护、不跑
        // AI、不跑玩家指令，见 `crate::menu_screen` 模块文档「世界在
        // 这块屏底下不动」一节。
        //
        // 这条比「回合制本来就是玩家不动世界就不走」更强，也必须更强：
        // 后者只保证**时钟**不前进，但方向键仍然会被 `player_command`
        // 读成移动意图。整段早退是最保守、也最容易向玩家解释的语义。
        //
        // # 这道闸门此前排在地图那道**后面**（规格 D3）
        //
        // 结果是暂停菜单盖在世界地图上的那一帧里，方向键**同时**被
        // `update_screen`（菜单光标）与 `pan_and_zoom_world_map`（地图
        // 平移）消费：玩家按一下「下」，菜单光标动一格，地图也跟着滚
        // 一格。规格 N8 的落点之一就是把两道闸门换个次序。
        if self.modal.screen().is_some() {
            return;
        }
        // 世界尚未存在（玩家还停在首页）——同样整段早退。
        //
        // **两道闸门都要**，不是重复：上一道守的是「有一块屏盖着」，
        // 这一道守的是「根本没有世界可推进」。首页两者同时成立，但它们
        // 是两件事——将来任何一条绕过屏的路径（例如死亡后回到首页）
        // 仍然会被这一道挡住。
        if self.session.is_none() {
            return;
        }
        // 世界地图那一层：开关、取消、平移三件事全在里面，吃掉这一帧
        // 就返回。
        if self.handle_world_map(input) {
            return;
        }
        self.maintain_streaming();
        self.update_zoom(input);
        animation::update_player_animation(
            &mut self.anim,
            input,
            frame,
            self.walk_clip,
            self.idle_clip,
        );
        self.run_turn(input);
        // 回合结算之后、下一帧之前：先看玩家还活着没有，再决定要不要
        // 自动存一次。次序不能反——玩家刚死的那一帧要存的是「模式已经
        // 转成普通」的那份，不是死之前那份。
        self.handle_player_death(input);
        self.maybe_autosave();
    }

    /// 世界地图那一层这一帧的全部输入：取消键关掉它、地图键开关它、
    /// 开着时方向键与缩放键**只作用于地图**。返回真表示这一帧到此为止，
    /// 世界不再推进。
    ///
    /// # 取消键关地图，是规格 N2 在这一层的落点
    ///
    /// 此前地图**没有任何取消处理**：`world_map_open` 是 `Demo` 上一个
    /// 裸 `bool`，既不是 `ScreenState` 也不是 `PlayerMenu`，于是
    /// `on_frame` 那条顶层取消判据在地图开着时成立——按 Esc 不关地图，
    /// 而是在地图上面盖一层暂停菜单（规格 D3）。现在地图进了栈，那条
    /// 顶层判据只在 `modal.is_empty()` 时才成立，取消键因此落到这里。
    ///
    /// # 地图开着时世界完全静止
    ///
    /// 早退顺带保证了 NPC 不动、流式加载不跑、时钟不走。玩家盯着地图
    /// 看多久都不会被咬。
    fn handle_world_map(&mut self, input: &mut InputState) -> bool {
        if self.modal.world_map_open() && input.was_just_pressed(GameKey::Cancel) {
            self.modal.close_world_map(input);
            return true;
        }
        // 地图开关——一次性动作，`was_just_pressed` 而非 `was_activated`：
        // 与 `GameKey::Screenshot`/`GameKey::Menu` 同一类键
        // （`GameKey::is_repeatable` 没有把 `Map` 收进去），长按不该反复
        // 切换。
        if input.was_just_pressed(GameKey::Map) && self.modal.toggle_world_map(input) {
            self.recenter_world_map();
        }
        if self.modal.world_map_open() {
            self.pan_and_zoom_world_map(input);
            return true;
        }
        false
    }

    /// 每次打开地图都重新对准玩家，见
    /// `crate::session::Session::world_map_view` 字段文档。
    fn recenter_world_map(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let player_pos = session
            .game_world
            .world
            .actors
            .get(session.game_world.player)
            .map(|agent| agent.pos);
        if let Some(pos) = player_pos {
            session.world_map_view = WorldMapView::centered_on_tile(&session.continent_field, pos);
        }
    }

    /// 玩家死了：模式从肉鸽单向降级为普通，存一次，然后回角色创建屏。
    ///
    /// # 所有者的裁定（含那次修正）
    ///
    /// > 「肉鸽模式是只有自动保存的，并且死亡就删除存档。」
    /// > **（追问后的修正）**「死亡后变成一般模式，可以再创建角色然后
    /// > 选择在某个地方出生。」
    ///
    /// 所以**不删档**：世界比角色活得长（`crate::save_slot` 模块文档）。
    ///
    /// # 复用批次 8 留的那三处接缝，不抄第三份
    ///
    /// 1. [`crate::draft_world::DraftWorld`]——转生那条路的世界与槽位绑
    ///    在一个类型里，状态机因此跳过世界配置屏，而「重新生成世界」在
    ///    那个类型上**写不出来**（重新生成等于把这局玩过的一切抹掉）；
    /// 2. `crate::world::apply_character_choice`——把新选的种族/性别/职业
    ///    落到玩家实体上；
    /// 3. `crate::world::move_player_to`——把他挪到新选的出生地。
    ///
    /// 后两处在 [`Demo::generate_draft_world`] 与
    /// [`Demo::finish_entering_world`] 里，与开局那条路**共用同一段代码**。
    fn handle_player_death(&mut self, input: &mut InputState) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        if session
            .game_world
            .world
            .actors
            .get(session.game_world.player)
            .is_some()
        {
            return;
        }
        // 实体已经从 arena 里消失了（`ll_sim::apply` 的 `Despawn`），
        // 这就是「玩家死了」在本仓库里的表示。
        let Some(mut session) = self.session.take() else {
            return;
        };
        let relaxed = session.game_world.identity.downgrade_mode();
        tracing::info!(
            slot = session.save_target.id.as_str(),
            mode_relaxed = relaxed,
            "玩家死亡：存档保留，回到角色创建"
        );
        // 先存一次，把模式变化落盘——不存的话玩家一关游戏，这次降级就
        // 白降了，下次读回来还是肉鸽档。
        if let Err(error) = write_save(&self.content, &session, &self.character_name) {
            tracing::error!(%error, "死亡后存档失败，模式变化可能没有落盘");
        }
        let target = session.save_target.clone();
        // `Session::begin` 当初用 `mem::take` 把时间轴接管走了，现在要把
        // 世界交回给草稿，得先按 `Agent::next_action_at` 重建一条——见
        // `crate::world::rebuild_timeline` 文档。
        session.game_world.timeline = crate::world::rebuild_timeline(&session.game_world.world);
        self.new_game_draft = Some(crate::chargen::NewGameDraft::for_reincarnation(
            &self.content,
            session.game_world,
            target,
        ));
        self.save_slots = crate::save_slot::list_slots(&self.saves_dir);
        // 角色创建是一块模态屏，输入上下文要切到 `Menu`；玩家死的那一刻
        // 可能正按着方向键，一并视为松开——两件事都由
        // `crate::modal::Modal::set_screen` 一起做完。
        self.modal
            .set_screen(Some(ScreenState::CharacterCreation { cursor: 0 }), input);
        self.screen_notice = Some(ScreenNotice::PlayerDied);
        self.screen_focus = WidgetStateTable::default();
    }

    /// 本帧的一次回合结算：清理老化地面物品 → 结算排在玩家之前的
    /// NPC 回合 → 尝试用本帧输入结算玩家一次行动 → 摄像机跟人。
    ///
    /// 从 [`Demo::advance`] 里拆出来，原因是借用检查：本方法全程持着
    /// 一个 `&mut Session`，而 `maintain_streaming`/`update_zoom` 是
    /// `&mut self` 的方法，两者不能同时活着。拆开之后 `advance` 只剩
    /// 「闸门 + 每帧杂务 + 调用本方法」，也顺带把那个早已越过 50 行
    /// 上限的函数切小了一截。
    fn run_turn(&mut self, input: &mut InputState) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        // 地面物品老化清理（NPC 生命周期批次）——见
        // `crate::world::cleanup_aged_ground_items` 文档「为什么挂在
        // 这里」一节：与 `maintain_streaming` 并列，是当前代码库里
        // 已经存在、每帧真正跑一遍的位置。
        crate::world::cleanup_aged_ground_items(&mut session.game_world.world);
        let player = session.game_world.player;
        // 本帧的结算目录束——每帧现借一次，不长期持有：`RuntimeCatalogs`
        // 只借用 `self.content`（装载期产物，建局后不再变化），构造成本
        // 是几个引用的复制，不是查表，与 ADR 0016/0017 的性能分级无关
        // （它不跨脚本边界，也不进结算热路径的内层循环）。之所以是局部
        // 变量而不是 `Demo` 的字段：`RuntimeCatalogs<'a>` 借着
        // `self.content`，做成字段就是自引用结构体。
        //
        // 这一束是「天赋在真实游戏里生效」的唯一通道：本方法此前把
        // `TurnEngine` 接上了时间轴（见上文「世界时钟为什么会走」），
        // 但 `TurnEngine::perform` 当时调的是不带任何目录的 `resolve`,
        // 于是种族/职业天赋、抗性、偷袭规则、资源池容量在真正能跑的
        // 游戏里全都是死的——同一处接线缺口的第二层。
        let runtime_catalogs = RuntimeCatalogs::new(&self.content);
        let catalogs = runtime_catalogs.as_resolve_catalogs();
        // 本体二进制不渲染伤害飘字（`p3_acceptance` 才有，那是纯呈现层
        // 的验收效果，见 `ll_sim::turn` 模块文档），因此这条回调在这里
        // 是空操作。
        //
        // 它曾经是 mod 事件监听的落点；脚本系统拆除之后那条通道没有了
        // （判据与论证见本批次提交信息）。回调本身**保留**：它是
        // 「一条效果在呈现层意味着什么」这个问题唯一的接缝，`ll-sim`
        // 不知道调用方在不在渲染。
        let mut on_effect = |_world: &WorldState, _effect: &Effect| {};
        // 行为树真的驱动回合推进这条链路的唯一标准接法，见
        // `ll_sim::behavior::behavior_ai_intent` 文档。`session.npc_ai`
        // 与 `session.game_world.world` 是同一个 `Session` 上的两个不同
        // 字段，借用检查器分得开，不需要把决策来源搬出去。
        let mut ai_intent = ll_sim::behavior::behavior_ai_intent(&mut session.npc_ai);
        session.engine.advance_ai(
            &mut session.game_world.world,
            player,
            &mut ai_intent,
            &catalogs,
            &mut on_effect,
        );
        drop(ai_intent);
        // 玩家这一回合提交什么，由 `crate::player_action` 决定——它是
        // 物品链那六个意图（`PickUp`/`Drop`/`Equip`/`Unequip`/`Use`/
        // `Craft`）唯一的键位产出者，见该模块文档「这个模块补的是哪条
        // 断线」一节。此前这里调的是 `TurnEngine::try_player_turn`，
        // 它内部只认 `intent_from_input` 的 `Move`/`Wait` 两种，于是
        // 那六个意图在真实游戏里一个都提交不出来。
        //
        // 查不到玩家实体时跳过（与 `draw_hud` 同一条降级纪律）：菜单
        // 要读它的背包与装备。
        //
        // 菜单开关与模态栈的配对由 `crate::modal::Modal::with_player_menu`
        // 负责——那是本模块**唯一**能拿到 `&mut PlayerMenu` 的路径，
        // 见 `crate::modal` 模块文档最后一节。
        let content = &self.content;
        let world = &session.game_world.world;
        // 这一帧要不要开一块会话屏（`None` = 不开）。**先记下来，等
        // `session` 的可变借用结束之后再开**：开屏要 `&mut self.modal`
        // 与 `&mut InputState`，而这一段全程持着 `&mut self.session`。
        let mut open_dialogue: Option<ll_core::ident::ContentIndex> = None;
        let command = self.modal.with_player_menu(input, |menu, input| {
            player_command(
                menu,
                input,
                world,
                player,
                &content.recipe_table,
                crate::player_action::TalkLookup {
                    dialogues: &content.dialogue_table,
                    cultures: Some(&content.culture_table),
                },
            )
        });
        match command {
            PlayerCommand::Idle => {}
            PlayerCommand::Rejected(feedback) => self.feedback = Some(feedback),
            PlayerCommand::Submit(intent) => {
                let outcome = session.engine.try_player_intent(
                    &mut session.game_world.world,
                    player,
                    intent,
                    &catalogs,
                    &mut on_effect,
                );
                // 「按了键但屏幕纹丝不动」这一刻必须说话，见
                // `Demo::feedback` 字段文档与
                // `ll_sim::turn::PlayerTurnOutcome` 文档。还没轮到玩家
                // （`NotYet`）不算按空——这次输入压根没被消费，下一帧
                // 原样重试，说话反而是噪音。
                self.feedback = match outcome {
                    PlayerTurnOutcome::Nothing => Some(Feedback::NothingHappened),
                    PlayerTurnOutcome::Acted => None,
                    PlayerTurnOutcome::NotYet => self.feedback,
                };
            }
            // 跟人说话：开一块模态屏，**不提交任何意图、不推进世界**
            // （规格七节 7.1：会话内的位置是 UI 状态）。起始节点由这段
            // 会话的 `root` 查出来——查不到（内容被换掉）就什么都不做，
            // 与本模块其余降级路径一致。
            PlayerCommand::OpenDialogue { dialogue } => {
                open_dialogue = content.dialogue_table.get(dialogue).map(|view| view.root);
            }
        }

        if let Some(agent) = session.game_world.world.actors.get(player)
            && matches!(agent.current_space, Space::Surface { .. })
        {
            session.camera.center = agent.pos;
        }
        // 会话屏在这里才真正压栈——见上面 `open_dialogue` 的注释。
        // `set_screen` 同时把输入上下文切到 `InputContext::Menu` 并把
        // 这一刻按住的键视为全部松开（`crate::modal::Modal::set_screen`），
        // 玩家因此不会「按着交互键开了会话屏、松手时又触发一次确认」。
        if let Some(node) = open_dialogue {
            self.modal
                .set_screen(Some(ScreenState::Dialogue { node, cursor: 0 }), input);
            self.screen_notice = None;
        }
    }

    /// 把当前世界层画面存成一张 PNG（`GameKey::Screenshot`，默认 F2）。
    ///
    /// # 这是交接文档第四节第 16 条的接线点
    ///
    /// `GameKey::Screenshot` 此前在 `ll-game` 里**零消费点**——真实
    /// 消费点只在五个验收 demo 里，本体按 F2 没反应。
    ///
    /// **与冻结基准那一侧的语义区别，必须写清楚**：
    /// `crates/*/tests/visual/` 下的 PNG 是**视觉回归基准**。本体这一侧
    /// 存的是**玩家截图**，落在数据目录的 `screenshots/` 下、按帧号
    /// 编名、绝不覆盖既有文件，与那些基准无关。
    ///
    /// 〔2026-08-29〕那些基准原先各由一个 `examples/` 验收 demo 产出，
    /// 那些 demo 已随所有者裁定删除（ADR 0030）；基准 PNG 全部保留，
    /// 但暂时没有生产者，见各 `tests/visual/README.md`。**本方法不是
    /// 它们的替代**——它存的是玩家截图，不是基准。
    fn take_screenshot(&self, frame: FrameId) {
        let Some(resources) = self.resources.as_ref() else {
            return;
        };
        let path = self
            .screenshot_dir()
            .join(ll_render::screenshot::screenshot_file_name(frame.0));
        // 存图失败只记日志：按一次截图键失败的代价应当是一条日志，
        // 不是一局游戏，与本模块其余降级路径同一条纪律。
        if let Err(error) =
            ll_render::screenshot::save_png(&resources.gpu, &resources.render_target, &path)
        {
            tracing::warn!(%error, path = %path.display(), "截图失败");
        }
    }

    /// 截图目录：与配置文件同一个数据目录下的 `screenshots/`。
    ///
    /// 从 `config_path` 的父目录推，而不是再传一份路径进来——两者本来
    /// 就同属一个数据目录（见 `crate::GamePaths`），多传一个参数只会
    /// 多一处可能对不上的地方。
    fn screenshot_dir(&self) -> PathBuf {
        self.config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("screenshots")
    }

    /// 按本帧激活的缩放动作调整 `self.zoom`。`was_activated` 而非
    /// `was_just_pressed`：缩放键参与自动重复（`GameKey::is_repeatable`
    /// 已把 `ZoomIn`/`ZoomOut` 收进去），长按应当连续变化；滚轮每次
    /// 滚动只调用一次 `InputState::pulse`，`was_activated` 对它同样
    /// 恰好触发一帧，两种输入源殊途同归，见 `ll-platform` 的
    /// `crate::keybind::WheelDirection` 模块文档。
    ///
    /// 钳制到 `[MIN_SAFE_ZOOM, MAX_SAFE_ZOOM]`，不是 `Zoom` 的通用
    /// 上下限——这是拉远不会让渲染剔除范围超出常驻区块集合覆盖范围的
    /// 唯一强制点，见 `crate::world::MIN_SAFE_ZOOM` 文档。
    fn update_zoom(&mut self, input: &InputState) {
        let mut value = self.zoom.get();
        if input.was_activated(GameKey::ZoomIn) {
            value += ZOOM_STEP;
        }
        if input.was_activated(GameKey::ZoomOut) {
            value -= ZOOM_STEP;
        }
        self.zoom = Zoom::new(value.clamp(MIN_SAFE_ZOOM, MAX_SAFE_ZOOM));
    }

    /// 地图打开时把方向键与缩放键接到世界地图视野上。
    ///
    /// # 为什么复用既有按键，不新增 `GameKey`
    ///
    /// `ZoomIn`/`ZoomOut` 与四个方向键**已经存在**，且已经接好了自动
    /// 重复与滚轮（见 `ll_platform::input::GameKey::ZoomIn` 文档）。
    /// 新增两个「地图专用缩放键」意味着玩家要记两套键、要在设置里绑两
    /// 次，而两套键在任何时刻都恰好只有一套可用——纯粹的重复。复用还有
    /// 一个直接好处：滚轮缩放在地图上白拿，不用再接一遍。
    ///
    /// 方向键走 `was_activated`（参与自动重复）而不是 `was_just_pressed`：
    /// 按住方向键连续平移是地图的通行手感，与它们在游戏内驱动连续移动
    /// 是同一条既有约定。
    ///
    /// 世界地图只可能在有世界的时候开着（开关那一步已经过了同一道闸门，
    /// 见 [`Demo::advance`]），因此这里的 `else` 分支在生产路径上不可
    /// 达。写成早退而不是 `expect`：与 `GpuResources::resolve_key`「取不到就
    /// 跳过本次」同一条表现层降级纪律——一次意外的落空不该让游戏崩溃。
    fn pan_and_zoom_world_map(&mut self, input: &InputState) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if input.was_activated(GameKey::ZoomIn) {
            session.world_map_view.zoom_in();
        }
        if input.was_activated(GameKey::ZoomOut) {
            session.world_map_view.zoom_out();
        }
        // 四个方向各算一次而不是 `else if` 串联：同时按下左和上应当斜着
        // 平移，与游戏内八向移动的既有预期一致。
        let dx = i32::from(input.was_activated(GameKey::Right))
            - i32::from(input.was_activated(GameKey::Left));
        let dy = i32::from(input.was_activated(GameKey::Down))
            - i32::from(input.was_activated(GameKey::Up));
        if dx != 0 || dy != 0 {
            session.world_map_view.pan(&session.continent_field, dx, dy);
        }
    }

    fn maintain_streaming(&mut self) {
        let Some(session) = self.session.as_mut() else {
            // 世界尚未存在（首页）——没有邻域可维护。
            return;
        };
        let player = session.game_world.player;
        let Some(agent) = session.game_world.world.actors.get(player) else {
            return;
        };
        if !matches!(agent.current_space, Space::Surface { .. }) {
            return;
        }
        let pos = agent.pos;
        let clock = session.game_world.world.clock;
        session.game_world.world.terrain.stream_neighborhood(
            &session.game_world.noise,
            &session.game_world.params,
            &self.content.terrain_ids,
            pos,
            STREAM_RADIUS_ZONES,
            clock,
        );
        // NPC 物化——**必须排在流式加载之后**：物化要读地形判断「这一格
        // 能不能站人」，读的正是上一行刚刚装进来的那些区块（见
        // `crate::world::materialize_nearby_settlements` 文档「时机」一节）。
        let spawned = crate::world::materialize_nearby_settlements(
            &mut session.game_world.world,
            &self.content,
            &self.settlement_roles,
        );
        // 新出现的实体要自己排进时间轴——`rebuild_timeline` 那条整条重建
        // 的路径在这里用不了（会丢掉 `TurnEngine::pending`），见
        // `ll_sim::turn::TurnEngine::schedule` 文档。
        for actor in spawned {
            session.engine.schedule(actor, clock);
        }
    }
}

impl AppHandler for Demo {
    fn on_resume(&mut self, window: Arc<Window>, size: PhysicalSize<u32>) {
        tracing::info!(width = size.width, height = size.height, "window resumed");
        // **先记窗口尺寸，再建 GPU 资源**：尺寸是窗口的事实，不是 GPU
        // 的，见 [`Demo::viewport`] 字段文档。
        self.viewport = Some((size.width as f32, size.height as f32));
        self.resources = Some(GpuResources::new(
            window,
            size,
            self.config.display,
            &self.content.asset_vfs,
        ));
    }

    fn on_resize(&mut self, size: PhysicalSize<u32>) {
        // 理由同 [`AppHandler::on_resume`]：窗口尺寸先记下，GPU 资源还
        // 没建出来也不影响它。
        self.viewport = Some((size.width as f32, size.height as f32));
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.resize(size);
    }

    fn on_frame(&mut self, frame: FrameId, input: &mut InputState) -> FrameOutcome {
        // 墙钟采样,见 `ll_platform::fps` 模块文档「为什么用墙钟,不用
        // 帧计数」一节——`Instant::now()` 只在这一处调用,产出的浮点数
        // 只流向状态栏文本,不进 `self.game_world`/`WorldState`。
        let fps = self.fps_counter.record_frame(std::time::Instant::now());

        // 截图键（默认 F2）——一次性动作，与地图键同一类。排在最前面：
        // 存的是**上一帧已经画好**的离屏目标，与本帧要不要推进世界无关。
        if input.was_just_pressed(GameKey::Screenshot) {
            self.take_screenshot(frame);
        }

        // 模态屏开着时，这一帧的输入全部归它——**必须排在下面那条
        // 「取消键退出游戏」之前**：否则玩家想关个菜单会直接退出整局
        // （与 `crate::player_action` 里 `player_command` 第 ② 步防的
        // 是同一个陷阱）。
        if self.modal.screen().is_some() {
            if self.update_screen(input) {
                return FrameOutcome::Exit;
            }
        } else if input.was_just_pressed(GameKey::Menu) && self.modal.is_empty() {
            // 菜单键（默认 Tab）——交接文档第四节第 17 条那条死路径的
            // 消费点。`was_just_pressed` 而非 `was_activated`：一次性
            // 动作键，长按不该反复开关。
            //
            // 背包/制作/交互列表开着时不叠第二块模态 UI：两块屏叠在
            // 一起会立刻引出「Esc 关哪一层」的新裁定，而没有任何人
            // 要求过这件事。
            self.open_menu(input);
        }

        // 菜单开着时取消键归菜单用（关掉它），见 `crate::player_action`
        // 里 `player_command` 第 ② 步的同一段说明。
        //
        // # 什么都没开时按取消键：**开主菜单，不退出游戏**
        //
        // 这一段推翻了本函数此前的行为。游戏内菜单落地之前，顶层按取消
        // 键是唯一的退出通道，于是它直接返回 [`FrameOutcome::Exit`]
        // ——按一下 Esc 整局就没了，没有任何确认。项目所有者实机撞到这
        // 件事并要求改掉。
        //
        // 现在退出有了正经去处：菜单里那一项，经 `update_screen` 的
        // `ScreenOutcome::Quit` 走同一个 `FrameOutcome::Exit`。因此 Esc
        // 回归它在绝大多数游戏里的含义——**逐层往回退**：开着子菜单就
        // 关子菜单，什么都没开就开主菜单，再从菜单里选退出。
        //
        // **刻意不改键位表**：Esc 绑给 `GameKey::Cancel`（`ll_platform`
        // 的默认表，玩家磁盘上那份也是这么写的）本来就是对的。错的是
        // 「顶层 Cancel 等于退出」这条行为，不是那条绑定——改键位只会
        // 把同一个问题挪到另一个键上。
        //
        // # 这条判据此前是一串手工合取（规格 N2/N8）
        //
        // 原文是 `screen.is_none() && !menu.is_open() && Cancel`——
        // **每加一套模态 UI 就要多一项，且漏了不报错**，而世界地图那
        // 一项从来没被加进去：开着地图按 Esc 不关地图，反而在地图上面
        // 盖一层暂停菜单（规格 D3）。现在只剩一条
        // [`crate::modal::Modal::is_empty`]：**一层都没盖着才开菜单，
        // 否则这一帧的取消键归栈顶那一层自己处理**（模态屏在
        // `update_screen` 里，地图在 `Demo::handle_world_map` 里，玩家
        // 菜单在 `crate::player_action::player_command` 里）。
        if self.modal.is_empty() && input.was_just_pressed(ll_platform::input::GameKey::Cancel) {
            self.open_menu(input);
        }

        self.advance(input, frame);

        // 必须在借出 `resources`（可变借 `self` 的一个字段，但方法调用
        // 会借走整个 `self`）之前算好——它读的是 `session`，与渲染无关。
        let can_save_manually = self.can_save_manually();
        let has_save = !self.save_slots.is_empty();
        // 行文字与光标位置必须在借出 `resources`（可变借 `self` 的一个
        // 字段，而方法调用会借走整个 `self`）之前算好——**而且这正是
        // 输入侧刚刚用过的同一个函数**，见 `screen_row_texts` 的文档。
        let screen = self.modal.screen();
        let screen_rows = screen.and_then(|state| {
            screen_row_texts(
                state,
                &self.config,
                &self.catalog,
                &self.screen_focus,
                has_save,
                can_save_manually,
                &self.save_slots,
                &self.content,
                self.new_game_draft.as_ref(),
                self.session
                    .as_ref()
                    .map(|session| (&session.game_world.world, session.game_world.player)),
            )
        });
        let hovered_row = self.pointer.hovered_row();

        let Some(resources) = self.resources.as_mut() else {
            return FrameOutcome::Continue;
        };

        // 世界尚未存在（首页）时**跳过世界层**，但下面那次
        // `batch.flush` 照跑：空批次会走 `ll_render::batch` 的
        // `wgpu::LoadOp::Clear(BLACK)`，首页背后因此是干净的黑，而不是
        // 上一帧的残影或未初始化内存。
        if let Some(session) = self.session.as_ref() {
            // 当前动画帧应显示的图集条目名，两层兜底见
            // `current_sprite_name` 文档；两层都失败时（连
            // `FALLBACK_SPRITE` 本身都缺失）才会在 `GpuResources::resolve_key`
            // 里记一条错误日志，那已经是资产整体损坏，不再是「可选帧
            // 缺失」。
            let sprite_name = current_sprite_name(
                self.anim.playback(),
                self.content.clip_table.as_clips(),
                frame,
                resources.atlas.metadata(),
                FALLBACK_SPRITE,
            );

            render_surface(
                &session.game_world,
                &self.content,
                &session.camera,
                self.zoom,
                sprite_name,
                resources,
            );
        }

        resources
            .batch
            .flush(&resources.gpu, resources.render_target.view());

        // 世界层已经 blit 到窗口 surface——HUD（状态栏/角色面板/背包/
        // 装备栏，P7 第一批）是紧接着追加的第二/三条渲染通道，画在
        // 同一张 surface 视图上，见 `GpuResources::acquire_and_blit`
        // 文档。取不到可用帧时（`acquire_and_blit` 返回 `None`）本帧
        // 直接跳过，与既有降级行为一致。
        if let Some((surface_frame, view)) = resources.acquire_and_blit() {
            // HUD 是画在世界之上的观测层——没有世界就没有 HUD 可画
            // （首页那一刻血条、时钟、背包全都无从谈起）。屏
            // （`draw_screen`）不受影响：它本来就是盖住世界的模态层，
            // 首页正是它唯一一种「底下没有世界」的用法。
            //
            // 世界地图那两个参数（`continent_field`/`world_map_view`）
            // 同样从 `session` 上取：它们是世界的派生物，与世界同生同死，
            // 见 `crate::session::Session` 模块文档那张表。
            if let Some(session) = self.session.as_ref() {
                draw_hud(
                    &session.game_world,
                    &self.content,
                    &self.catalog,
                    &self.config.language,
                    resources,
                    &mut self.measurer,
                    &view,
                    &mut self.hud_anim,
                    frame,
                    fps,
                    self.modal.world_map_open(),
                    &session.continent_field,
                    &session.world_map_view,
                    self.modal.player_menu(),
                    self.feedback,
                    // 正常游玩：不改写任何东西。
                    None,
                );
            } else if matches!(self.modal.screen(), Some(ScreenState::SpawnPick { .. }))
                && let Some(draft) = self.new_game_draft.as_ref()
                && let (Some(world), Some(field), Some(view_of_map), Some(exploration)) = (
                    draft.world.world(),
                    draft.continent_field.as_ref(),
                    draft.map_view.as_ref(),
                    draft.exploration.as_ref(),
                )
            {
                // 选出生地屏：世界已经建好但玩家还没入世（`session` 仍是
                // `None`），HUD 因此从**草稿**里取世界、地形场与视野。
                // 世界地图强制打开——这块屏的全部画面就是那张地图。
                //
                // 四个 `as_ref` 写在一个 `let` 里而不是各自 `expect`：
                // 它们四个同生同死（`generate_draft_world` 一次性全部
                // 赋值），一条模式匹配比四条各自会 panic 的断言诚实。
                draw_hud(
                    world,
                    &self.content,
                    &self.catalog,
                    &self.config.language,
                    resources,
                    &mut self.measurer,
                    &view,
                    &mut self.hud_anim,
                    frame,
                    fps,
                    true,
                    field,
                    view_of_map,
                    PlayerMenu::default(),
                    None,
                    Some(SpawnPickHud {
                        exploration,
                        cursor_cell: draft.cursor_cell,
                    }),
                );
            }
            draw_screen(
                screen,
                screen_rows,
                &self.catalog,
                &self.config.language,
                self.screen_notice,
                hovered_row,
                resources,
                &mut self.measurer,
                &view,
            );
            resources.present_frame(surface_frame);
        }

        FrameOutcome::Continue
    }

    /// 平台层每次按键/滚轮事件都问一句：这一帧该按哪张表解析物理键。
    ///
    /// 答案完全由 [`Demo::modal`] 决定——它是本项目里「现在盖着屏幕的
    /// 是哪一层」的唯一真相源，见 [`crate::modal::Modal`] 模块文档与
    /// [`AppHandler::input_context`] 的完整论证。
    fn input_context(&self) -> InputContext {
        self.modal.input_context()
    }

    /// 把设置界面这一帧改好的键位表交给平台层，见
    /// [`AppHandler::take_rebound_keys`]。
    fn take_rebound_keys(&mut self) -> Option<KeyBindings> {
        self.pending_bindings.take()
    }

    /// 退出前存档——**只在真的有一局世界的时候**，判断在
    /// [`Demo::save_on_exit`] 里，见那里的说明（从首页直接离开时存档
    /// 会覆盖玩家真正那一份存档）。
    fn on_exit(&mut self) {
        tracing::info!("demo exiting");
        self.save_on_exit();
    }
}

/// 测试专用的世界取用入口——本模块的断言全部跑在 [] 建出的
/// `Demo` 上，那条路走的是 [`Demo::new`]（世界已经建好），因此这里
/// 解包是安全的。
///
/// 做成两个方法而不是让测试各写一遍 `session.as_ref().unwrap()`：那样
/// 每条断言里都会多出一行与断言主题无关的解包噪音。
#[cfg(test)]
impl Demo {
    fn test_world(&self) -> &GameWorld {
        &self
            .session
            .as_ref()
            .expect("测试里世界必然已经建好")
            .game_world
    }

    fn test_world_mut(&mut self) -> &mut GameWorld {
        &mut self
            .session
            .as_mut()
            .expect("测试里世界必然已经建好")
            .game_world
    }
}

/// 存档相关的断言（自动存档节拍、死亡接线、手动存档、回主菜单）住在
/// 一个**独立文件**里。
///
/// `app.rs` 已经 4000 行开外（既有违规，交接文档第四节第 8 条记着这笔
/// 账），本批次给它加的产品代码只有几十行，但断言有四百多行。用
/// `#[path]` 把它们挪进 `app_save_tests.rs`：子模块看得见父模块的私有
/// 项（`Demo` 的字段、`handle_player_death` 这些私有方法），断言因此
/// 仍然走真实的私有路径，而 `app.rs` 本身没有因为本批次多出四百行。
#[cfg(test)]
#[path = "app_save_tests.rs"]
mod save_tests;

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
