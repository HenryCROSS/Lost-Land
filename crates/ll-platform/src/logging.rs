//! 日志初始化。
//!
//! 选用 `tracing` 而非 `log`，因为本项目是多线程的：`tracing` 的 span
//! 能标明一条日志属于哪个任务、在哪条线程，而 `log` 只能给出扁平的一行
//! 文本。等到需要排查「离屏世界推进为何偶发出错」时，这个差别决定了
//! 能不能查出来。

use crate::PlatformError;
use tracing_subscriber::EnvFilter;

/// 初始化全局日志。
///
/// `verbose` 为真时默认级别提升到 `debug`。无论何种情况，环境变量
/// `LOSTLAND_LOG` 都拥有更高优先级，便于临时排查而无需重新编译。
///
/// 重复调用返回 [`PlatformError::LoggingAlreadyInitialized`] 而非 panic：
/// 热重载与测试场景下重复调用是常态，日志初始化失败绝不该让游戏起不来。
pub fn init_logging(verbose: bool) -> Result<(), PlatformError> {
    let default_level = if verbose { "debug" } else { "info" };

    let filter = match EnvFilter::try_from_env("LOSTLAND_LOG") {
        Ok(filter) => filter,
        Err(error) => {
            // 变量未设置是常态，内容非法则是开发者敲错了字。两者都返回 Err，
            // 一视同仁地吞掉会让敲错的人完全得不到反馈，只能疑惑过滤器
            // 为何不生效。此处只在变量确实被设置过时才告警。
            if std::env::var_os("LOSTLAND_LOG").is_some() {
                eprintln!(
                    "LOSTLAND_LOG is set but could not be parsed ({error}); falling back to {default_level}"
                );
            }
            EnvFilter::new(default_level)
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        // 显示线程名，因为在固定职责线程模型下，「这条日志来自哪条线程」
        // 往往是定位问题的第一线索。
        .with_thread_names(true)
        .with_target(true)
        .try_init()
        .map_err(|_| PlatformError::LoggingAlreadyInitialized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 重复初始化返回错误而非崩溃() {
        // 日志初始化失败绝不能让游戏起不来。热重载与测试场景下重复调用
        // 是常态，必须优雅拒绝。
        // Arrange：首次调用可能成功也可能已被同进程内其他测试占用，
        // 两种情况都不影响本测试要验证的行为。
        let _ = init_logging(false);

        // Act
        let second = init_logging(false);

        // Assert
        assert!(matches!(
            second,
            Err(crate::PlatformError::LoggingAlreadyInitialized)
        ));
    }
}
