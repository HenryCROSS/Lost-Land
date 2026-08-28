//! 配置文件系统：把用户偏好（当前只有键位绑定）落盘成一个 JSON5 文件。
//!
//! # 格式：JSON5，读写不对称
//!
//! 项目所有者 2026-08-20 裁定「全用 json5 吧，还可以写注释方便日后
//! 维护」，本仓库全部手写配置格式统一成 JSON5——`config.json5` 是玩家
//! 可能会手改的文件（例如手动调整按键绑定），理应享受注释与尾逗号。
//! 但读写用的是两个不同的库，理由是 [`json5`] crate 只提供解析、不
//! 提供序列化：[`load_or_default`] 走 `json5::from_str`，能读手写文件
//! 里的注释；[`save`] 继续走 `serde_json::to_string_pretty`——JSON 是
//! JSON5 的严格子集，游戏自己写出的配置文件不需要注释（没有人手写它），
//! 用哪个库写出结果都一样能被 `json5::from_str` 读回，不需要为「写」
//! 这一侧另外拉一个能序列化的 JSON5 库。
//!
//! # 为什么现在补
//!
//! [`crate::keybind`] 模块文档早就写明「本项目目前没有配置文件系统」
//! 且预留了接缝——[`crate::keybind::KeyBindings`] 已经能从数据构造
//! （[`crate::keybind::KeyBindings::from_bindings`]）并完整
//! 序列化往返，唯独没有任何东西真正去加载它。游戏本体二进制第一次
//! 需要「重启后记得上次的按键绑定」这条能力，本模块补上那唯一缺失的
//! 一环：一个文件路径 + 读写两个函数，不重新发明 `KeyBindings` 自己
//! 已经做好的校验。
//!
//! # 硬约束：配置不是世界状态
//!
//! [`GameConfig`] 里的**运行期偏好**——[`GameConfig::bindings`]、
//! [`GameConfig::display`]、[`GameConfig::language`]——绝不能进
//! `ll_world::state::WorldState`、不参与 `WorldState::hash()`、不影响
//! 确定性重放，这与 [`crate::keybind`] 模块文档「持久化」一节的约束
//! 完全一致（本模块正是那条约束描述的「未来的配置系统」）。`ll-platform`
//! 从未、也不应该反向依赖 `ll-world`/`ll-sim`，这条依赖方向本身就是
//! 「配置不可能不小心变成世界状态」的结构性保证：`ll-world` 里的任何
//! 类型物理上进不了这个 crate。
//!
//! # 一个类别不同的字段：[`GameConfig::new_game`]（建档期初值）
//!
//! 世界生成参数落地批次新增的 [`NewGameConfig`] 是**另一类东西**，
//! 这里把区别写清楚，免得它被当成上一节那条约束的破例。
//!
//! - 运行期偏好回答「同一个世界，我想怎么玩它」：改按键、改语言、改
//!   滤波，随时可改，改完立即生效，对世界本身零影响。
//! - [`NewGameConfig`] 回答「我接下来要建的那个**新**世界长什么样」。
//!   它只在**没有存档、真的要新建世界的那一刻**被读一次；世界一旦建
//!   成，这组数值就被 `ll_world::state::WorldState::terrain_shape`
//!   接管、随存档一起持久化，此后读档路径**再也不会回头看这个文件**。
//!
//! 真正需要守住的那条不变式因此完好无损：**改配置文件不会改变任何
//! 一个已经存在的存档的重放结果**。`ll_game` 侧有一条测试
//! （`改动新游戏配置不影响已存在存档读回后的世界摘要`）直接钉死它。
//!
//! 依赖方向也没有松动：本类型只装 `String`/`Option<u64>`/`Option<i32>`
//! 这类原始值，不引用 `ll-world` 的任何类型；「这个字符串对应哪一组
//! 地形阈值」由 `ll_game` 在两个 crate 都能看见的地方解析。
//!
//! # 合并而不是整体替换（加载语义）
//!
//! [`load_or_default`] 读出磁盘上那份配置之后，会用
//! [`KeyBindings::fill_missing_defaults`] 把**文件里完全没有提到的动作**
//! 补上它们的内置默认绑定，而不是让磁盘那张表整体替换默认表。
//!
//! 这修的不是「某几个键忘了做」，而是一条会随每次新增按键复发的结构性
//! 缺陷：整体替换意味着一份**在某个新动作出现之前**写出的配置文件，
//! 之后无论本体加多少条默认绑定，那个玩家都永远收不到——而且完全静默，
//! 没有任何报错，玩家只看到「按了没反应」。实测：本仓库所有者机器上那份
//! `config.json5` 写于「输入接线」批次（提交 `ed1584f`）之前，只有 12 个
//! 动作，`Interact`/`Inventory`/`Craft`/`PickUp`/`Drop`/`Equip`/`Use`/
//! `Place` 八个新动作与整张 `InputContext::Menu` 默认表一条都没有。
//!
//! 逐条的合并规则、键位槽冲突怎么裁、以及「刻意解绑」与「文件写出时还
//! 没有这个动作」这对矛盾如何区分，见
//! [`KeyBindings::fill_missing_defaults`] 与
//! [`GameConfig::unbound_actions`] 的文档。
//!
//! 合并只发生在**加载**这一侧，不回写磁盘：回写会抹掉玩家手写的 JSON5
//! 注释（本模块选 JSON5 正是为了让玩家能写注释），代价大于收益。合并是
//! 幂等的纯函数，每次加载重算一遍即可。
//!
//! # 损坏时的退化策略
//!
//! 配置文件是用户可编辑的明文 JSON，随时可能被手改坏、被半写入的
//! 进程崩溃截断、或单纯不存在（首次启动）。[`load_or_default`] 对这
//! 三种情况一视同仁：记一条日志说明原因，退回
//! [`GameConfig::default`]，**绝不 panic**——一个游戏因为配置文件损坏
//! 就打不开，比忽略这个文件、退回默认键位更糟。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::input::GameKey;
use crate::keybind::KeyBindings;

/// 游戏配置：键位绑定 + 显示选项，未来的音频等选项按同样的模式
/// （新增字段 + `#[serde(default = ...)]` 兜底旧配置文件）追加，不需要
/// 改动本模块的读写逻辑。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// 物理按键 → 抽象动作的绑定表，见 [`crate::keybind`] 模块文档。
    #[serde(default = "KeyBindings::default_bindings")]
    pub bindings: KeyBindings,
    /// 垂直同步与画面缩放滤波选项，见 [`DisplayConfig`]。
    #[serde(default)]
    pub display: DisplayConfig,
    /// 显示语言，取值是 `.ftl` 文件名去掉扩展名的语言标签（如
    /// `"zh-CN"`/`"en"`，见 `ll_i18n::Catalog` 模块文档「语言标签」
    /// 一节的约定）。
    ///
    /// # 为什么是用户偏好，不是世界状态
    ///
    /// 语言选择只影响「同一份内容用哪种文字呈现」，不影响世界本身
    /// 是什么——与 [`bindings`](Self::bindings)/[`display`](Self::display)
    /// 同一条纪律：绝不能进 `ll_world::state::WorldState`、不参与
    /// `WorldState::hash()`、不影响确定性重放。种子分享给用不同语言
    /// 客户端的朋友，双方看到的世界必须逐位相同，只是文字长得不一样
    /// ——这也是
    /// `knowledge/design/naming-and-localization.md`「i18n 的坑与解法」
    /// 一节反复强调的边界：本地化只换皮肤，不换世界。
    ///
    /// # 为什么是 `String` 而不是一个语言枚举
    ///
    /// mod 可以带自己的 `locales/<语言标签>.ftl`（见
    /// `knowledge/design/mod-package-structure.md`「本地化文件」一节），
    /// 语言标签集合因此不是本体编译期就能穷举完的封闭集合——一个写死
    /// 的 Rust 枚举会在每次有人想加一种新语言翻译时逼着改这里的代码。
    /// `ll_i18n::Catalog` 本身也只按字符串标签索引已装载的 `FluentBundle`，
    /// 用 `String` 与它的实际形状一致，不需要在这两层之间来回转换。
    #[serde(default = "default_language")]
    pub language: String,
    /// 玩家**刻意**不给任何输入的动作——见模块文档「合并而不是整体
    /// 替换」一节。
    ///
    /// # 为什么需要这个字段
    ///
    /// [`bindings`](Self::bindings) 是一张 `(键, 修饰键, 上下文) → 动作`
    /// 的平表，「玩家刻意解绑了 `Screenshot`」在这张表里的表现是
    /// **表里没有 `Screenshot` 这个动作**；而「这份文件写出时本体还没有
    /// `Screenshot` 这个动作」在表里的表现**一模一样**。两者在原本的
    /// 数据形状下物理上不可区分，而 [`load_or_default`] 的合并必须对它们
    /// 做出相反的处理（前者要保持解绑，后者要补上默认键位）。本字段就是
    /// 把「刻意解绑」这个意图从「缺席」里显式拎出来的唯一办法。
    ///
    /// # 缺省即「没有刻意解绑任何动作」
    ///
    /// `#[serde(default)]` 让本字段引入之前写出的配置文件读回一个空列表
    /// ——那些文件里的每一处缺席都会被当成「文件写出时还没有这个动作」，
    /// 于是拿到默认键位。这是刻意选的一边：老文件里真正的「刻意解绑」
    /// 会被默认值绑回来一次。代价有限（多一个能用的键，玩家可以再解一次
    /// 并被本字段记住），而反过来选（缺席一律当解绑）会让本模块要修的
    /// 那条缺陷原封不动地留着。
    #[serde(default)]
    pub unbound_actions: Vec<GameKey>,
    /// 新建世界时使用的地形形态与种子，见 [`NewGameConfig`]。**只在
    /// 真的要新建世界的那一刻读一次**，与上面三个运行期偏好类别不同，
    /// 见模块文档「一个类别不同的字段」一节。
    #[serde(default)]
    pub new_game: NewGameConfig,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            bindings: KeyBindings::default_bindings(),
            display: DisplayConfig::default(),
            language: default_language(),
            unbound_actions: Vec::new(),
            new_game: NewGameConfig::default(),
        }
    }
}

/// 新建世界时的地形形态与种子选择。
///
/// # 为什么是「预设名 + 可选逐项覆盖」两层
///
/// 项目所有者的原话是「这些应该都作为可调节参数」，同时又要「先做一份
/// 预设，以后我再调」。两层正好各答一半：
///
/// - [`Self::terrain_preset`] 给绝大多数人一个能直接用的名字（大陆 /
///   群岛 / 山地 / 内陆），不需要知道任何一个阈值是什么意思。
/// - 四个 `Option` 覆盖字段给想自己调的人一条**逐项**的通路：只写想
///   改的那一项，其余仍取预设值。要的正是「都作为可调节参数」。
///
/// # 为什么覆盖字段是 `Option` 而不是直接给默认数值
///
/// 直接给数值就分不清「玩家真的想要海平面 400」与「玩家没写这一项」。
/// 分不清就没法做「预设打底、逐项覆盖」——只能整组一起给或整组一起
/// 不给。`Option` 让「没写」成为一个可判断的状态。
///
/// # 依赖方向
///
/// 本类型刻意只用原始类型，不引用 `ll_world::generate::TerrainShape`
/// ——`ll-platform` 不依赖 `ll-world`，见模块文档。把这几个值解析成
/// 真正的形态参数（含取值范围校验）是 `ll_game` 的事。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewGameConfig {
    /// 地形形态预设的稳定标识，取值见
    /// `ll_content::world_identity::TERRAIN_PRESETS` 的 `id` 字段
    /// （`continent` / `archipelago` / `highland` / `inland`）。
    ///
    /// 存标识而不是译名：译名会随语言与文案修订变化，标识不会——玩家
    /// 把中文界面切成英文之后，配置文件不该突然失效。
    #[serde(default = "default_terrain_preset")]
    pub terrain_preset: String,
    /// 世界种子。留空（或写 `null`）表示用本体的固定默认种子。
    ///
    /// 不做成「留空即随机」：本项目的确定性纪律要求「同一份构建反复
    /// 运行产出同一个世界」是默认行为，随机开局是一条独立的能力，应当
    /// 由未来真正的开局界面用一个显式选项提供，而不是靠一个空字段隐式
    /// 触发。
    #[serde(default)]
    pub seed: Option<u64>,
    /// 覆盖预设的海平面（千分比）。
    #[serde(default)]
    pub sea_level: Option<i32>,
    /// 覆盖预设的山地阈值（千分比）。
    #[serde(default)]
    pub mountain_level: Option<i32>,
    /// 覆盖预设的噪声倍频层数。
    #[serde(default)]
    pub octaves: Option<u32>,
    /// 覆盖预设的大陆尺度缩减档位。
    #[serde(default)]
    pub continent_shrink: Option<u32>,
    /// 覆盖预设的气候条带单侧带宽（千分比）。
    ///
    /// 干热带与极地带各占这么宽的一段纬度，其余是温带。写 `0` 即**关掉
    /// 气候条带**（整图温带，地形分带回到气候条带落地之前的样子）。
    #[serde(default)]
    pub climate_band_width: Option<i32>,
}

/// [`NewGameConfig::terrain_preset`] 的默认值——与
/// `ll_content::world_identity::DEFAULT_TERRAIN_PRESET_ID` 保持一致。
///
/// 这里写字面量而不是引用那个常量，理由与本类型「依赖方向」一节相同：
/// `ll-platform` 不依赖 `ll-content`。两处必须同步，`ll_game` 侧有一条
/// 测试（`配置默认预设标识在预设表里查得到`）钉死它们不会分叉。
fn default_terrain_preset() -> String {
    "continent".to_string()
}

impl Default for NewGameConfig {
    fn default() -> Self {
        NewGameConfig {
            terrain_preset: default_terrain_preset(),
            seed: None,
            sea_level: None,
            mountain_level: None,
            octaves: None,
            continent_shrink: None,
            climate_band_width: None,
        }
    }
}

/// `language` 字段的默认值——独立具名函数，理由与
/// [`default_vsync`] 同一个模式（见 [`DisplayConfig`] 文档）。选
/// `zh-CN` 是因为这是本项目的原始开发语言（代码注释、设计文档、测试
/// 名全部是中文），首次启动且没有配置文件时应当先说开发者自己的语言，
/// 而不是默认已经做了一次「选择英语更保险」的隐含假设。
fn default_language() -> String {
    "zh-CN".to_string()
}

/// 显示相关的图形选项：垂直同步开关、画面缩放滤波方式。
///
/// 与 `bindings` 同一条纪律——只是用户偏好，绝不能进
/// `ll_world::state::WorldState`、不参与 `hash()`、不影响确定性重放
/// （[ADR 0020](../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
/// 甲区：结果只变成像素/呈现时序，从不回流世界状态）。`ll-platform`
/// 从未、也不应该反向依赖 `ll-world`/`ll-sim`，理由与 [`crate::keybind`]
/// 模块文档「持久化」一节完全一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// 垂直同步开关。`true` 用 `Fifo` 呈现模式（画面撕裂绝不会发生，
    /// 帧率被锁定在显示器刷新率以内），`false` 用 `Immediate`（延迟
    /// 最低但可能画面撕裂）——具体如何落到 wgpu 的呈现模式、平台不
    /// 支持时如何回退，见 `ll_render::gpu::GpuContext::new` 文档。
    #[serde(default = "default_vsync")]
    pub vsync: bool,
    /// 离屏画面（固定 640×360）放大到窗口尺寸时的采样滤波方式，见
    /// [`ScaleFilter`]。
    #[serde(default)]
    pub scale_filter: ScaleFilter,
}

/// `vsync` 字段的默认值——独立的具名函数而非字面量，理由与
/// [`KeyBindings::default_bindings`] 作为 `bindings` 字段默认值同一个
/// 模式：字段级默认值统一走具名函数，`derive(Default)` 在结构体级别
/// 组合它们（见 [`DisplayConfig`] 的 `Default` 派生），不需要在
/// `impl Default for GameConfig` 里手写第二份初始化逻辑。
fn default_vsync() -> bool {
    true
}

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            vsync: default_vsync(),
            scale_filter: ScaleFilter::default(),
        }
    }
}

/// 离屏画面（固定 640×360）放大到窗口尺寸时的采样方式。
///
/// # 为什么没有 MSAA 选项
///
/// 传统多重采样抗锯齿（MSAA）平滑的是三角形几何边缘的锯齿；本项目的
/// 呈现管线画的是铺满视口的全屏三角形（`ll_render::target` 的 blit
/// 通道），边缘裁在视口之外，不存在需要抗锯齿的可见几何边缘——真正
/// 决定画面观感的是离屏画布的像素怎么被放大取样。像素游戏的硬边缘是
/// 刻意画出来的美术语言，MSAA 会把它们和传统抗锯齿一样糊掉，这不是
/// 这类游戏需要的「抗锯齿」；真正对应的旋钮就是这里的采样方式选择——
/// 详细论证见 `ll_render` crate 的 `ll_render::target::BlitFilter` 文档
/// （不做成文档内链：`ll-platform` 不依赖 `ll-render`，rustdoc 在这个
/// crate 的作用域里解析不到那个类型，链接会被判定为 broken_intra_doc_links。
/// 本类型与它逐个变体一一对应，二者分处 `ll-platform`/`ll-render` 两个
/// crate 正是因为这条依赖方向——`ll-game` 在构造 blit 参数时把
/// `ScaleFilter` 映射成 `BlitFilter`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ScaleFilter {
    /// 最近邻——像素边缘恒定锐利，但窗口尺寸不是逻辑分辨率整数倍时
    /// 会让相邻像素被放大成宽窄不一的方块、画面轻微抖动闪烁。
    #[default]
    Nearest,
    /// 锐利双线性（sharp bilinear）——只在纹素边界上做平滑过渡，纹素
    /// 内部保持平坦：任意窗口尺寸下像素边缘依然锐利，同时消除
    /// `Nearest` 在非整数倍下的不均匀瑕疵。
    SharpBilinear,
}

/// 从 `path` 加载配置；文件不存在、无法读取或内容不是合法的
/// [`GameConfig`]（含 JSON5 语法错误与 [`KeyBindings`] 自身的冲突校验
/// 失败，见 `crate::keybind` 模块文档 ADR 0011 一节）时，记一条日志并
/// 退回 [`GameConfig::default`]——**绝不 panic**，见模块文档「损坏时的
/// 退化策略」。
///
/// 解析成功时**不是**直接返回磁盘上那份表：绑定表要先经
/// [`KeyBindings::fill_missing_defaults`] 与内置默认表合并，否则新增的
/// 默认绑定对已有配置文件的玩家永久不可达，见模块文档「合并而不是整体
/// 替换」一节。
///
/// 配置文件里写着本版本已经不认识的动作名/键名（旧版本遗留）时，只有
/// **那一行**被丢弃并记一条 `warn`，整份配置不会因此解析失败退回全默认
/// ——那会把玩家的全部自定义一次性抹掉，见 `crate::keybind` 的
/// `KeyBindingsRepr` 文档「为什么两张表都改成宽容行」一节。
pub fn load_or_default(path: &Path) -> GameConfig {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            tracing::info!(
                path = %path.display(),
                %error,
                "配置文件不存在或无法读取，使用默认配置"
            );
            return GameConfig::default();
        }
    };

    match json5::from_str::<GameConfig>(&text) {
        Ok(config) => GameConfig {
            // 合并而不是整体替换，见模块文档同名一节。
            bindings: config
                .bindings
                .fill_missing_defaults(&config.unbound_actions),
            ..config
        },
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "配置文件解析失败（内容损坏或包含冲突的键位绑定），使用默认配置"
            );
            GameConfig::default()
        }
    }
}

/// 配置写出失败的原因——只用于诊断日志，调用方不应该因为写配置失败
/// 就让游戏本身崩溃（存盘失败不该阻塞游玩）。
#[derive(Debug)]
pub enum ConfigSaveError {
    /// 编码为 JSON 失败——`GameConfig` 全部字段都是 serde 标准可派生
    /// 类型，正常情况下不会发生。
    Encode(serde_json::Error),
    /// 文件系统 I/O 失败。
    Io(std::io::Error),
}

impl std::fmt::Display for ConfigSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSaveError::Encode(err) => write!(f, "配置编码失败：{err}"),
            ConfigSaveError::Io(err) => write!(f, "配置写入失败：{err}"),
        }
    }
}

impl std::error::Error for ConfigSaveError {}

/// 把 `config` 写出到 `path`，人类可读的缩进 JSON——配置文件是用户
/// 会手改的东西，不值得为了省几百字节的空白符换成压缩格式。写出的是
/// 普通 JSON 而非带注释的 JSON5：`serde_json` 不提供 JSON5 序列化,
/// 但 JSON 是 JSON5 的严格子集,程序自动写出的这份文件不携带任何
/// 说明性注释（没有人手写它，注释也无从谈起），用 [`load_or_default`]
/// 的 `json5::from_str` 读回去完全等价，见模块文档「格式：JSON5，
/// 读写不对称」一节。
pub fn save(path: &Path, config: &GameConfig) -> Result<(), ConfigSaveError> {
    let json = serde_json::to_string_pretty(config).map_err(ConfigSaveError::Encode)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(ConfigSaveError::Io)?;
    }
    fs::write(path, json).map_err(ConfigSaveError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybind::{InputContext, Modifiers, WheelDirection};
    use winit::keyboard::KeyCode;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "ll-platform-config-test-{name}-{}.json",
            std::process::id()
        ));
        path
    }

    #[test]
    fn 配置文件不存在时退回默认绑定表() {
        // Arrange
        let path = temp_path("missing");
        let _ = fs::remove_file(&path);

        // Act
        let config = load_or_default(&path);

        // Assert
        let action =
            config
                .bindings
                .resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Gameplay);
        assert_eq!(action, Some(crate::input::GameKey::Up));
    }

    #[test]
    fn 配置文件内容损坏时退回默认配置而不panic() {
        // Arrange：写入一段不是合法 JSON 的内容，模拟被手改坏或截断的
        // 配置文件。
        let path = temp_path("corrupted");
        fs::write(&path, b"{ this is not valid json").expect("测试用写入应当成功");

        // Act
        let config = load_or_default(&path);

        // Assert：没有 panic，且退回的是默认绑定表。
        assert_eq!(
            config.bindings.bindings().len(),
            KeyBindings::default_bindings().bindings().len()
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 配置文件内容为冲突绑定时退回默认配置而不panic() {
        // 与 crate::keybind 的 ADR 0011 测试同一类攻击面：合法 JSON，
        // 但 KeyBindings 自身的校验（同一个键绑给两个不同动作）会拒绝
        // 它——配置加载必须把这类拒绝也当成「损坏」处理，而不是让
        // Deserialize 的错误一路 panic 出去。
        // Arrange
        let path = temp_path("conflicting-bindings");
        let json = r#"{"bindings":{"bindings":[
            {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Menu"},
            {"key":"KeyQ","modifiers":{"shift":false,"ctrl":false,"alt":false},"context":"Gameplay","action":"Map"}
        ]}}"#;
        fs::write(&path, json).expect("测试用写入应当成功");

        // Act
        let config = load_or_default(&path);

        // Assert
        assert_eq!(
            config.bindings.bindings().len(),
            KeyBindings::default_bindings().bindings().len()
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 写出后读回的配置与原配置绑定数量一致() {
        // Arrange
        let path = temp_path("roundtrip");
        let config = GameConfig::default();

        // Act
        save(&path, &config).expect("写出应当成功");
        let loaded = load_or_default(&path);

        // Assert
        assert_eq!(
            loaded.bindings.bindings().len(),
            config.bindings.bindings().len()
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 默认配置的垂直同步默认开启() {
        // 与 `ll_render::gpu::GpuContext::new` 文档一致：找不到用户
        // 偏好时应当默认「不撕裂」而不是默认「最低延迟」。
        // Arrange & Act
        let config = GameConfig::default();

        // Assert
        assert!(config.display.vsync);
    }

    #[test]
    fn 默认配置的缩放滤波默认最近邻() {
        // 默认值必须与既有渲染行为逐位一致——最近邻是本项目此前唯一
        // 用过的滤波方式,切换默认值会让所有没碰过设置的玩家画面突然
        // 改变观感。
        // Arrange & Act
        let config = GameConfig::default();

        // Assert
        assert_eq!(config.display.scale_filter, ScaleFilter::Nearest);
    }

    #[test]
    fn 缺少display字段的旧配置文件仍能反序列化() {
        // 兜底旧配置文件——本字段引入之前写出的 JSON 不含 display 键,
        // 应当退回默认显示配置而不是解析失败。走 json5::from_str 而不是
        // serde_json::from_str——这才是 load_or_default 实际使用的解析器
        // （见模块文档「格式：JSON5，读写不对称」一节），测试应验证真实
        // 路径,不是一个凑巧行为相同的替代品。
        // Arrange
        let json = r#"{"bindings":{"bindings":[]}}"#;

        // Act
        let config: GameConfig = json5::from_str(json).expect("缺失 display 字段应当兜底");

        // Assert
        assert_eq!(config.display, DisplayConfig::default());
    }

    #[test]
    fn 缺少language字段的旧配置文件仍能反序列化() {
        // 与「缺少display字段」同一条纪律：本字段是后补的，早期写出的
        // 配置文件不含 language 键，不该因此解析失败。同样走
        // json5::from_str，理由见上一条测试。
        // Arrange
        let json = r#"{"bindings":{"bindings":[]}}"#;

        // Act
        let config: GameConfig = json5::from_str(json).expect("缺失 language 字段应当兜底");

        // Assert
        assert_eq!(config.language, "zh-CN");
    }

    #[test]
    fn 带注释与尾逗号的配置文件能通过load_or_default正常读出() {
        // JSON5 相对 JSON 的两项核心增益（项目所有者选它正是为了这两样，
        // 见模块文档「格式：JSON5，读写不对称」一节）：手改的配置文件能
        // 加解释性注释，结尾多余的逗号不会让解析报错。本测试直接验证
        // load_or_default 这条生产路径确实能处理两者，不只是文档空口
        // 宣称「格式是 JSON5」。
        // Arrange
        let path = temp_path("json5-comments-trailing-comma");
        let json5_text = r#"{
            // 玩家手改：喜欢锐利双线性滤波
            "display": {
                "vsync": false,
                "scale_filter": "SharpBilinear",
            },
        }"#;
        fs::write(&path, json5_text).expect("测试用写入应当成功");

        // Act
        let config = load_or_default(&path);

        // Assert
        assert_eq!(config.display.scale_filter, ScaleFilter::SharpBilinear);
        assert!(!config.display.vsync);

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 默认配置的显示语言是中文() {
        // Arrange & Act
        let config = GameConfig::default();

        // Assert
        assert_eq!(config.language, "zh-CN");
    }

    #[test]
    fn 写出后读回的显示语言与原配置一致() {
        // Arrange
        let path = temp_path("language-roundtrip");
        let config = GameConfig {
            language: "en".to_string(),
            ..GameConfig::default()
        };

        // Act
        save(&path, &config).expect("写出应当成功");
        let loaded = load_or_default(&path);

        // Assert
        assert_eq!(loaded.language, "en");

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 显示配置能序列化后再反序列化出等价内容() {
        // Arrange
        let display = DisplayConfig {
            vsync: false,
            scale_filter: ScaleFilter::SharpBilinear,
        };

        // Act
        let json = serde_json::to_string(&display).expect("应能序列化");
        let restored: DisplayConfig = serde_json::from_str(&json).expect("刚序列化的数据应能读回");

        // Assert
        assert_eq!(restored, display);
    }

    #[test]
    fn 写出后读回的绑定能解析出相同动作() {
        // 不只是数量一致——逐条绑定内容本身要能正确往返。
        // Arrange
        let path = temp_path("roundtrip-resolve");
        let config = GameConfig::default();
        save(&path, &config).expect("写出应当成功");

        // Act
        let loaded = load_or_default(&path);
        let action =
            loaded
                .bindings
                .resolve(KeyCode::KeyW, Modifiers::NONE, InputContext::Gameplay);

        // Assert
        assert_eq!(action, Some(crate::input::GameKey::Up));

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    /// 项目所有者机器上那份 `config.json5` 的绑定表逐条复刻——写于
    /// 「输入接线」批次（提交 `ed1584f`）之前，只有 12 个动作 17 条
    /// 绑定，八个新动作与整张 `InputContext::Menu` 默认表一条都没有。
    ///
    /// 这不是一个「构造得刚好能过」的夹具：所有者跑起游戏按空格、按 I、
    /// 按 C 全都没反应，根因就是这份表被整体替换进了绑定表。
    const 旧配置绑定: &[(&str, &str)] = &[
        ("ArrowUp", "Up"),
        ("KeyW", "Up"),
        ("ArrowDown", "Down"),
        ("KeyS", "Down"),
        ("ArrowLeft", "Left"),
        ("KeyA", "Left"),
        ("ArrowRight", "Right"),
        ("KeyD", "Right"),
        ("Enter", "Confirm"),
        // 旧默认值：空格是确认键的第二个绑定。现在空格的默认动作是
        // `Interact`——这正是键位槽冲突那条规则的真实来源。
        ("Space", "Confirm"),
        ("Escape", "Cancel"),
        ("Tab", "Menu"),
        ("KeyM", "Map"),
        ("Period", "Wait"),
        ("F2", "Screenshot"),
        ("Equal", "ZoomIn"),
        ("Minus", "ZoomOut"),
    ];

    /// 旧格式配置文本里那段滚轮绑定——单独抽出来，好让「完全没有
    /// wheel_bindings 键的更老配置」那条测试能把它整段删掉。
    const 旧配置滚轮片段: &str = r#","wheel_bindings":[{"direction":"Away","context":"Gameplay","action":"ZoomIn"},{"direction":"Toward","context":"Gameplay","action":"ZoomOut"}]"#;

    /// 按**旧版本的写出格式**拼一份配置文件文本：没有 `unbound_actions`
    /// 键，也没有任何本批新增的字段——测的是真正的老文件，不是一份用
    /// 今天的 `save` 写出来、只是内容少几条的文件。
    fn 旧格式配置文本(bindings: &[(&str, &str)]) -> String {
        let rows: Vec<String> = bindings
            .iter()
            .map(|(key, action)| {
                format!(
                    r#"{{"key":"{key}","modifiers":{{"shift":false,"ctrl":false,"alt":false}},"context":"Gameplay","action":"{action}"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"bindings":{{"bindings":[{}]{}}},"display":{{"vsync":true,"scale_filter":"Nearest"}},"language":"zh-CN"}}"#,
            rows.join(","),
            旧配置滚轮片段
        )
    }

    fn 写入并加载(name: &str, text: &str) -> (std::path::PathBuf, GameConfig) {
        let path = temp_path(name);
        fs::write(&path, text).expect("测试用写入应当成功");
        let config = load_or_default(&path);
        (path, config)
    }

    fn 游戏内解析(config: &GameConfig, key: KeyCode) -> Option<GameKey> {
        config
            .bindings
            .resolve(key, Modifiers::NONE, InputContext::Gameplay)
    }

    #[test]
    fn 缺少新键的旧配置加载后八个新动作全部可用() {
        // 本批的验收线本身。八个动作里七个的默认键位在旧配置里是空的
        // （I/C/G/X/E/U/P），第八个（Interact 的 Space）被旧默认值
        // `Confirm` 占着，走的是键位槽抢占那条规则。
        //
        // 反例（已实跑验证会红）：把 `load_or_default` 里那句
        // `bindings: config.bindings.fill_missing_defaults(...)` 换回
        // 原来的整体替换，这条测试八条断言全红。
        // Arrange & Act
        let (path, config) = 写入并加载("legacy-eight-keys", &旧格式配置文本(旧配置绑定));

        // Assert
        assert_eq!(游戏内解析(&config, KeyCode::KeyI), Some(GameKey::Inventory));
        assert_eq!(游戏内解析(&config, KeyCode::KeyC), Some(GameKey::Craft));
        assert_eq!(游戏内解析(&config, KeyCode::KeyG), Some(GameKey::PickUp));
        assert_eq!(游戏内解析(&config, KeyCode::KeyX), Some(GameKey::Drop));
        assert_eq!(游戏内解析(&config, KeyCode::KeyE), Some(GameKey::Equip));
        assert_eq!(游戏内解析(&config, KeyCode::KeyU), Some(GameKey::Use));
        assert_eq!(游戏内解析(&config, KeyCode::KeyP), Some(GameKey::Place));
        assert_eq!(游戏内解析(&config, KeyCode::Space), Some(GameKey::Interact));

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 旧配置里玩家原有的自定义绑定合并后不丢() {
        // 合并的另一半：补默认值不能顺手把玩家自己改过的东西冲掉。
        // 这份夹具把地图键从 `M` 改绑到 `J`——合并的粒度是**动作**，
        // `Map` 已经有绑定，所以默认的 `M` 不会被塞回来。
        // Arrange
        let mut bindings: Vec<(&str, &str)> = 旧配置绑定.to_vec();
        for row in &mut bindings {
            if row.1 == "Map" {
                row.0 = "KeyJ";
            }
        }

        // Act
        let (path, config) = 写入并加载("legacy-custom-kept", &旧格式配置文本(&bindings));

        // Assert
        assert_eq!(游戏内解析(&config, KeyCode::KeyJ), Some(GameKey::Map));
        assert_eq!(
            游戏内解析(&config, KeyCode::KeyM),
            None,
            "玩家把地图键从 M 挪到了 J，合并不该把 M 又绑回来——否则「改绑」会变成「多绑」"
        );
        // 顺带确认这份夹具确实还在测合并：新键照样补上了。
        assert_eq!(游戏内解析(&config, KeyCode::KeyI), Some(GameKey::Inventory));

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 刻意解绑的动作不会被默认值悄悄绑回来() {
        // 「刻意解绑」与「文件写出时还没有这个动作」在 bindings 那张
        // 平表里长得一模一样，`unbound_actions` 是把前者显式拎出来的
        // 唯一办法，见 `GameConfig::unbound_actions` 文档。
        //
        // 反例（已实跑验证会红）：把 `fill_missing_defaults` 里
        // `unbound_actions.contains(&candidate.action)` 那条 `continue`
        // 删掉，第一条断言当场红。
        // Arrange
        let text = 旧格式配置文本(旧配置绑定).replace(
            r#","language":"zh-CN""#,
            r#","language":"zh-CN","unbound_actions":["Inventory","Screenshot"]"#,
        );

        // Act
        let (path, config) = 写入并加载("deliberately-unbound", &text);

        // Assert
        assert_eq!(
            游戏内解析(&config, KeyCode::KeyI),
            None,
            "玩家刻意解绑了背包键，合并不该把默认的 I 绑回来"
        );
        assert_eq!(
            config.unbound_actions,
            vec![GameKey::Inventory, GameKey::Screenshot]
        );
        // 没被解绑的那些仍然补上——否则这条测试可能只是「合并整个没跑」。
        assert_eq!(游戏内解析(&config, KeyCode::KeyC), Some(GameKey::Craft));

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 抢占默认键位时被抢的动作在同一上下文下必须还剩别的键() {
        // 抢占那条规则的下限守卫：`Space` 从 `Confirm` 手里被
        // `Interact` 抢走，前提是 `Confirm` 还有 `Enter`。
        // Arrange & Act
        let (path, config) = 写入并加载("displacement-guard", &旧格式配置文本(旧配置绑定));

        // Assert
        assert_eq!(游戏内解析(&config, KeyCode::Space), Some(GameKey::Interact));
        assert_eq!(
            游戏内解析(&config, KeyCode::Enter),
            Some(GameKey::Confirm),
            "被抢了空格的确认键必须还留着回车，否则合并把一个动作变成了零键位"
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 占位动作只剩一个键时默认键位不抢占() {
        // 守卫的另一侧：把回车整条删掉，`Confirm` 在 Gameplay 下只剩
        // 空格，此时 `Interact` 不该抢——宁可让新动作暂时没有键位，
        // 也不把一个玩家还在用的动作变成零键位。
        //
        // 反例（已实跑验证会红）：把 `occupant_keys_left >= 2` 那条
        // 守卫改成无条件抢占，两条断言同时红。
        // Arrange
        let bindings: Vec<(&str, &str)> = 旧配置绑定
            .iter()
            .copied()
            .filter(|(key, _)| *key != "Enter")
            .collect();

        // Act
        let (path, config) = 写入并加载("no-displacement", &旧格式配置文本(&bindings));

        // Assert
        assert_eq!(游戏内解析(&config, KeyCode::Space), Some(GameKey::Confirm));
        assert_eq!(
            config
                .bindings
                .bindings_for(GameKey::Interact)
                .filter(|binding| binding.context == InputContext::Gameplay)
                .count(),
            0,
            "空格是 Interact 唯一的默认键位，抢不到就应当没有键位（并已记 warn 日志）"
        );
        // 不受这条规则牵连的新键仍然补上。
        assert_eq!(游戏内解析(&config, KeyCode::KeyI), Some(GameKey::Inventory));

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 配置里有本版本不认识的动作名时只丢那一行而不是整份退回默认() {
        // 旧版本遗留的动作名（或键名）不能让整份配置解析失败——那会把
        // 玩家的全部自定义一次性抹掉，见 `crate::keybind` 的
        // `KeyBindingsRepr` 文档「为什么两张表都改成宽容行」一节。
        //
        // 反例（已实跑验证会红）：把 `KeyBindingsRepr::bindings` 换回
        // `Vec<KeyBinding>`（直接反序列化成强类型），三条断言全红——
        // 整份配置退回全默认，`KeyJ` 那条自定义消失、`KeyM` 又冒出来。
        // Arrange：一条认不出来的动作 + 一条玩家的自定义绑定。
        let mut bindings: Vec<(&str, &str)> = 旧配置绑定.to_vec();
        for row in &mut bindings {
            if row.1 == "Map" {
                row.0 = "KeyJ";
            }
        }
        bindings.push(("KeyZ", "SummonDragon"));

        // Act
        let (path, config) = 写入并加载("unknown-action-name", &旧格式配置文本(&bindings));

        // Assert
        assert_eq!(
            游戏内解析(&config, KeyCode::KeyJ),
            Some(GameKey::Map),
            "认不出来的那一行不该连累玩家其余的自定义绑定"
        );
        assert_eq!(游戏内解析(&config, KeyCode::KeyZ), None);
        assert_eq!(游戏内解析(&config, KeyCode::KeyM), None);

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 同一份配置文件两次加载得到逐条相同的绑定表() {
        // C5：绑定表的遍历与冲突裁决不得依赖 `HashMap`/`HashSet` 的
        // 迭代顺序。合并全程只在固定顺序的常量表与 `Vec` 上线性扫描，
        // 这条测试钉住那个结论。
        // Arrange
        let (path, first) = 写入并加载("deterministic-merge", &旧格式配置文本(旧配置绑定));

        // Act
        let second = load_or_default(&path);

        // Assert
        assert_eq!(first.bindings.bindings(), second.bindings.bindings());
        assert_eq!(
            first.bindings.wheel_bindings(),
            second.bindings.wheel_bindings()
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 旧配置加载后菜单上下文的默认绑定也补上了() {
        // 被整体替换埋掉的不止那八个动作：整张 `InputContext::Menu`
        // 默认表在所有者那份文件里同样一条都没有。
        // Arrange & Act
        let (path, config) = 写入并加载("legacy-menu-context", &旧格式配置文本(旧配置绑定));

        // Assert
        assert_eq!(
            config
                .bindings
                .resolve(KeyCode::ArrowUp, Modifiers::NONE, InputContext::Menu),
            Some(GameKey::Up)
        );
        assert_eq!(
            config
                .bindings
                .resolve(KeyCode::Escape, Modifiers::NONE, InputContext::Menu),
            Some(GameKey::Cancel)
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 缺少滚轮绑定的旧配置加载后滚轮缩放仍然可用() {
        // 同一条缺陷的第二个实例：`wheel_bindings` 字段是后补的，在它
        // 之前写出的配置文件里没有这个键，读回是空列表。若合并只看
        // 按键表（`ZoomIn` 有 `Equal`，判定为「已有绑定」），滚轮缩放
        // 就会对这些玩家永久不可达。
        // Arrange：一份完全没有 wheel_bindings 键的配置。
        let text = 旧格式配置文本(旧配置绑定).replace(旧配置滚轮片段, "");
        assert!(
            !text.contains("wheel_bindings"),
            "夹具本身必须真的不含 wheel_bindings，否则这条测试什么也没验证"
        );

        // Act
        let (path, config) = 写入并加载("legacy-no-wheel", &text);

        // Assert
        assert_eq!(
            config
                .bindings
                .resolve_wheel(WheelDirection::Away, InputContext::Gameplay),
            Some(GameKey::ZoomIn)
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn 默认配置刻意解绑列表为空() {
        // 缺省即「没有刻意解绑任何动作」——见
        // `GameConfig::unbound_actions` 文档。
        // Arrange & Act
        let config = GameConfig::default();

        // Assert
        assert!(config.unbound_actions.is_empty());
    }

    #[test]
    fn 合并对已经完整的配置是恒等的() {
        // 幂等：写出一份完整默认配置再读回，合并不该多补/多删任何一条
        // ——否则「每次加载都跑一遍合并」会让绑定表随加载次数漂移。
        // Arrange
        let path = temp_path("merge-idempotent");
        let config = GameConfig::default();
        save(&path, &config).expect("写出应当成功");

        // Act
        let loaded = load_or_default(&path);

        // Assert
        assert_eq!(loaded.bindings.bindings(), config.bindings.bindings());
        assert_eq!(
            loaded.bindings.wheel_bindings(),
            config.bindings.wheel_bindings()
        );

        // Cleanup
        let _ = fs::remove_file(&path);
    }
}
