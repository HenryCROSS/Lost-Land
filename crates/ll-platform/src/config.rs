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
//! [`GameConfig`] 只装用户偏好，绝不能进
//! `ll_world::state::WorldState`、不参与 `WorldState::hash()`、不影响
//! 确定性重放——这与 [`crate::keybind`] 模块文档「持久化」一节的约束
//! 完全一致（本模块正是那条约束描述的「未来的配置系统」）。`ll-platform`
//! 从未、也不应该反向依赖 `ll-world`/`ll-sim`，这条依赖方向本身就是
//! 「配置不可能不小心变成世界状态」的结构性保证：`ll-world` 里的任何
//! 类型物理上进不了这个 crate。
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
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            bindings: KeyBindings::default_bindings(),
            display: DisplayConfig::default(),
            language: default_language(),
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

    match json5::from_str(&text) {
        Ok(config) => config,
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
    use crate::keybind::{InputContext, Modifiers};
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
}
