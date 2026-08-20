//! 配置文件系统：把用户偏好（当前只有键位绑定）落盘成一个 JSON 文件。
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

/// 游戏配置：当前只承载键位绑定，未来的图形/音频选项按同样的模式
/// （新增字段 + `#[serde(default = ...)]` 兜底旧配置文件）追加，不需要
/// 改动本模块的读写逻辑。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    /// 物理按键 → 抽象动作的绑定表，见 [`crate::keybind`] 模块文档。
    #[serde(default = "KeyBindings::default_bindings")]
    pub bindings: KeyBindings,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            bindings: KeyBindings::default_bindings(),
        }
    }
}

/// 从 `path` 加载配置；文件不存在、无法读取或内容不是合法的
/// [`GameConfig`]（含 JSON 语法错误与 [`KeyBindings`] 自身的冲突校验
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

    match serde_json::from_str(&text) {
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
/// 会手改的东西，不值得为了省几百字节的空白符换成压缩格式。
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
