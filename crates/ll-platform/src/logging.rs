//! 日志初始化：同时写 stdout 与 `logs/` 下的滚动文件。
//!
//! 选用 `tracing` 而非 `log`，因为本项目是多线程的：`tracing` 的 span
//! 能标明一条日志属于哪个任务、在哪条线程，而 `log` 只能给出扁平的一行
//! 文本。等到需要排查「离屏世界推进为何偶发出错」时，这个差别决定了
//! 能不能查出来。
//!
//! # 为什么要有文件日志
//!
//! 项目所有者实机遇到的崩溃（例如 `SurfaceWindow` 的常驻前置被违反）
//! 只在游戏窗口里一闪而过，stdout 随进程一起消失。落盘之后，「崩溃前
//! 已物化几座据点、几个 NPC」这类线索才有可能被事后读到——这是本模块
//! 存在文件那一路的唯一理由。
//!
//! # 刻意不用 `tracing_appender::non_blocking`
//!
//! `non_blocking` 把写盘挪到后台线程，代价是它返回一个 `WorkerGuard`：
//! **守卫被 drop 之后日志会静默停止写入**。写下本段时 [`init_logging`]
//! 有 7 个调用点（生产路径 `ll_game` 的 `run`，外加六个各阶段验收
//! example——那六个已于 2026-08-29 随所有者裁定删除，见 ADR 0030），
//! 要接住守卫就得改 7 处签名，而其中任何一处忘了把守卫存活到进程结束，
//! 症状都是「日志文件是空的，没有任何报错」——「声明了但没接线」这类
//! 静默失效正是本仓库反复吃过亏的失败模式。
//!
//! 因此改用阻塞式的 `tracing_appender::rolling` appender 直接当
//! writer：**无守卫、无签名变化**。取舍是写日志变成同步阻塞的 I/O，
//! 理由是 info 级日志量极小（一局游戏几十行），这点开销换来的是
//! 「物理上不可能忘记持有守卫」。真到了要打每 tick 级别日志的那天，
//! 再引入 `non_blocking` 并一次性改掉全部调用点也不迟——那时它是一个
//! 有明确收益的决定，而不是现在这样白付一份静默失效的风险。
//!
//! # 为什么是 `registry` + 两个 layer，不是 `fmt()`
//!
//! `tracing_subscriber::fmt()` 构建器只接受**一个** writer，
//! `.with_writer(文件)` 会把 stdout 那一路顶掉。文件日志是**追加**一
//! 路，不是替换：终端里仍要能实时看到日志。所以改用
//! `tracing_subscriber::registry()` 叠两个 `fmt::layer()`，两路共用
//! 同一个 [`EnvFilter`]。文件那一路 `.with_ansi(false)`，否则落盘的是
//! 满屏 ANSI 转义序列，而不是能直接读的文本。

use std::io;
use std::path::Path;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

use crate::PlatformError;

/// 日志目录，相对进程当前工作目录。已列入仓库根 `.gitignore`。
pub const LOG_DIR: &str = "logs";

/// 日志文件名前缀，配合 [`LOG_FILE_SUFFIX`] 与按日轮转的日期段，落盘
/// 形如 `logs/lostland.2026-08-27.log`。
pub const LOG_FILE_PREFIX: &str = "lostland";

/// 日志文件名后缀（扩展名）。
pub const LOG_FILE_SUFFIX: &str = "log";

/// 保留的历史日志文件数上限——超出后最旧的会被删掉。
///
/// 取 7：按日轮转下正好是一周。玩家不会主动清理这个目录，无上限的
/// 日志目录迟早会变成「游戏自己占满磁盘」的缺陷；一周足够覆盖「昨天
/// 晚上崩了一次，今天才想起来找日志」这个真实的排查窗口。
pub const MAX_LOG_FILES: usize = 7;

/// 初始化全局日志：stdout 与 [`LOG_DIR`] 下的滚动文件各一路。
///
/// `verbose` 为真时默认级别提升到 `debug`。无论何种情况，环境变量
/// `LOSTLAND_LOG` 都拥有更高优先级，便于临时排查而无需重新编译。
/// 两路日志共用同一个过滤器，不存在「终端看得见、文件里没有」这类
/// 需要分别记住两套规则的错位。
///
/// 重复调用返回 [`PlatformError::LoggingAlreadyInitialized`] 而非 panic：
/// 热重载与测试场景下重复调用是常态，日志初始化失败绝不该让游戏起不来。
///
/// # 文件那一路失败时降级，不返回 `Err`
///
/// 建目录或开文件失败（只读介质、权限不足、路径被同名文件占住）时，
/// 本函数**只告警并退回单 stdout**，仍然返回 `Ok`。理由与上一段同源：
/// 日志是排查手段，不是游戏能否运行的前提。把「日志文件写不出来」升级
/// 成「游戏起不来」，是拿一个次要设施的故障去毁掉主要功能。
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

    // 显示线程名，因为在固定职责线程模型下，「这条日志来自哪条线程」
    // 往往是定位问题的第一线索。两路取同一套格式选项，只有 ANSI 一项
    // 不同——见模块文档。
    let stdout_layer = fmt::layer().with_thread_names(true).with_target(true);

    let init_result = match build_rolling_appender(Path::new(LOG_DIR)) {
        Ok(appender) => {
            let file_layer = fmt::layer()
                .with_thread_names(true)
                .with_target(true)
                // 落盘的必须是纯文本：带 ANSI 的日志文件用任何编辑器
                // 打开都是满屏转义序列，等于白写。
                .with_ansi(false)
                .with_writer(appender);
            tracing_subscriber::registry()
                .with(filter)
                .with(stdout_layer)
                .with(file_layer)
                .try_init()
        }
        Err(error) => {
            // 降级路径必须自己喊出来：静默退回单 stdout，就成了又一个
            // 「以为在写文件，其实一个字节都没落盘」的静默失效。
            eprintln!("无法初始化文件日志（{LOG_DIR}：{error}），本次只写 stdout；游戏照常运行");
            tracing_subscriber::registry()
                .with(filter)
                .with(stdout_layer)
                .try_init()
        }
    };

    init_result.map_err(|_| PlatformError::LoggingAlreadyInitialized)
}

/// 构造按日轮转的文件 appender：先确保目录存在，再建 appender。
///
/// 单独抽出来是为了**可测**：[`init_logging`] 装的是进程级全局订阅
/// 者，一个进程只能成功初始化一次，无法用它反复验证「目录建出来了
/// 没有、文件里有没有内容」。本函数不碰任何全局状态，可以对着临时
/// 目录随便跑，见本模块测试。
///
/// # 为什么按日轮转
///
/// 单机游戏的日志按大小轮转没有意义——玩家描述问题时说的是「昨天
/// 晚上」，不是「第 3 个 5MB 分片」。按日轮转让文件名里的日期直接
/// 就是检索键。配合 [`MAX_LOG_FILES`] 上限，磁盘占用有界。
///
/// # 返回 `Err` 而不是 panic
///
/// `tracing_appender::rolling` 的便捷构造函数（`rolling::daily` 等）
/// 在建不出文件时会 panic。这里走 `builder().build()` 这条返回
/// `Result` 的路，把失败原样交给调用方去决定怎么降级——见
/// [`init_logging`] 文档「文件那一路失败时降级」一节。
pub fn build_rolling_appender(dir: &Path) -> io::Result<RollingFileAppender> {
    std::fs::create_dir_all(dir)?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(dir)
        // `InitError` 不是 `io::Error`，但调用方唯一关心的是「失败了、
        // 原因是什么」。统一成 `io::Error` 免得为一个只有降级一种处理
        // 方式的错误再引入一个错误类型。
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// 在系统临时目录下开一个本次测试专属的目录路径（**不预先创建**
    /// ——被测函数的职责之一就是把它建出来）。带进程 id 与用例标签，
    /// 避免同一进程内多条测试互相踩。
    fn scratch_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("lostland-log-test-{}-{tag}", std::process::id()))
    }

    #[test]
    fn 目录不存在时会被建出来且日志真的落盘() {
        // Arrange：一个确定不存在的目录。
        let dir = scratch_dir("basic");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!dir.exists(), "前置：测试目录此刻不应存在");

        // Act：构造 appender 并写一行。
        let mut appender = build_rolling_appender(&dir).expect("临时目录下必然能建出日志文件");
        writeln!(appender, "一行足够独特的日志内容").expect("写入 appender 不应失败");
        appender.flush().expect("flush 不应失败");

        // Assert：目录被建出来了，且目录里存在一个非空、含刚写内容的文件。
        assert!(dir.is_dir(), "build_rolling_appender 应当自己把目录建出来");
        let contents: Vec<String> = std::fs::read_dir(&dir)
            .expect("目录已存在")
            .map(|entry| entry.expect("读取目录项"))
            .map(|entry| std::fs::read_to_string(entry.path()).expect("日志文件应可读"))
            .collect();
        assert!(!contents.is_empty(), "目录里应当至少有一个日志文件");
        assert!(
            contents
                .iter()
                .any(|text| text.contains("一行足够独特的日志内容")),
            "日志文件里应当真的有写进去的内容，实际读到：{contents:?}"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 文件名带约定的前缀与后缀() {
        // 文件名格式是排查时的检索键（见 build_rolling_appender 文档
        // 「为什么按日轮转」），值得被钉住。
        // Arrange
        let dir = scratch_dir("naming");
        let _ = std::fs::remove_dir_all(&dir);

        // Act
        let mut appender = build_rolling_appender(&dir).expect("临时目录下必然能建出日志文件");
        writeln!(appender, "x").expect("写入 appender 不应失败");
        appender.flush().expect("flush 不应失败");

        // Assert
        let names: Vec<String> = std::fs::read_dir(&dir)
            .expect("目录已存在")
            .map(|entry| {
                entry
                    .expect("读取目录项")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            names
                .iter()
                .any(|name| name.starts_with(LOG_FILE_PREFIX) && name.ends_with(LOG_FILE_SUFFIX)),
            "文件名应形如 `lostland.<日期>.log`，实际：{names:?}"
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 目录建不出来时返回错误而不是恐慌() {
        // 这是 init_logging 降级路径的触发条件（见其文档「文件那一路
        // 失败时降级」一节）：必须是一个可以被 match 的 Err，不是
        // panic——panic 会让游戏起不来，正是要避免的后果。
        // Arrange：先造一个**文件**，再拿它当目录用。
        let blocker = scratch_dir("blocked");
        let _ = std::fs::remove_dir_all(&blocker);
        let _ = std::fs::remove_file(&blocker);
        std::fs::write(&blocker, b"not a directory").expect("临时文件应可写");

        // Act
        let result = build_rolling_appender(&blocker);

        // Assert
        assert!(
            result.is_err(),
            "路径被同名文件占住时应返回 Err，而不是成功或 panic"
        );

        // Cleanup
        let _ = std::fs::remove_file(&blocker);
    }

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
