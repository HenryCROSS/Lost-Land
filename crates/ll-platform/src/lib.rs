//! 平台层：窗口、输入、日志与并行任务池。
//!
//! 本层封装一切与操作系统打交道的部分，使上层完全不感知平台差异。
//! 它**不含任何游戏逻辑**——判断某个类型该放这里还是放上层，标准是
//! 「换一个操作系统时它是否需要改」。

pub mod input;
pub mod jobs;
pub mod keybind;
pub mod logging;
pub mod window;

use core::fmt;

/// 平台层的错误。
#[derive(Debug)]
pub enum PlatformError {
    /// 日志系统重复初始化。
    LoggingAlreadyInitialized,
    /// 无法创建工作线程池，附带底层原因。
    ThreadPool(String),
    /// 事件循环创建或运行失败，附带底层原因。
    EventLoop(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlatformError::LoggingAlreadyInitialized => {
                write!(f, "logging subsystem was already initialized")
            }
            PlatformError::ThreadPool(reason) => {
                write!(f, "failed to build worker thread pool: {reason}")
            }
            PlatformError::EventLoop(reason) => {
                write!(f, "event loop failed: {reason}")
            }
        }
    }
}

impl core::error::Error for PlatformError {}
