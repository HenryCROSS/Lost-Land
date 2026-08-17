//! 并行任务池与线程间通道。
//!
//! # 线程模型
//!
//! 项目采用**固定职责线程 + 任务池**，不引入异步运行时：
//!
//! - 主线程：窗口事件循环、输入采集、世界写入、渲染提交
//! - 任务池：只读的重计算（视野、寻路、地图生成、离屏世界推进）
//! - IO 线程：资产加载、存档写入、脚本热重载监听
//!
//! 不用异步运行时的原因是这里没有海量并发连接需要等待，只有 CPU 密集的
//! 批量计算；异步只会让函数签名被传染，却换不来任何好处。
//!
//! # 顺序保持是硬要求
//!
//! [`JobPool::map_collect`] 保证输出顺序与输入一致。这不是便利特性而是
//! 确定性的前提：若结果顺序随线程调度变化，后续对结果做的任何折叠运算
//! 都会失去确定性，跨平台世界摘要就对不上了。

use crate::PlatformError;
use rayon::prelude::*;

pub use crossbeam_channel::{Receiver, Sender, unbounded as channel};

/// 承担只读重计算的并行任务池。
#[derive(Debug)]
pub struct JobPool {
    inner: rayon::ThreadPool,
}

impl JobPool {
    /// 建立任务池。
    ///
    /// `threads` 为零时退化为单线程——配置文件可能写出 0，与其崩溃不如
    /// 退化运行。
    ///
    /// 返回 [`Err`] 的真实原因是**操作系统拒绝创建工作线程**（句柄或内存
    /// 耗尽、容器的线程数上限），而非参数非法——参数已在上方钳制。
    pub fn new(threads: usize) -> Result<Self, PlatformError> {
        let threads = threads.max(1);
        let inner = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            // 线程命名后，日志与性能分析器里就能一眼看出是任务池线程。
            .thread_name(|index| format!("ll-job-{index}"))
            .build()
            .map_err(|error| PlatformError::ThreadPool(error.to_string()))?;
        Ok(JobPool { inner })
    }

    /// 池中的线程数。
    pub fn thread_count(&self) -> usize {
        self.inner.current_num_threads()
    }

    /// 并行映射，**输出顺序与输入一致**。
    ///
    /// 闭包必须是纯函数：它会在多个线程上同时执行，任何共享可变状态都会
    /// 破坏确定性。这正是「意图—结算—效果」架构中结算阶段只读世界的
    /// 原因。
    pub fn map_collect<T, R, F>(&self, items: &[T], f: F) -> Vec<R>
    where
        T: Sync,
        R: Send,
        F: Fn(&T) -> R + Sync + Send,
    {
        self.inner.install(|| items.par_iter().map(f).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 并行映射保持输入顺序() {
        // 顺序保持是确定性的前提。若结果顺序随线程调度变化，后续对结果
        // 做的任何折叠运算都会失去确定性。
        // Arrange
        let pool = JobPool::new(4).expect("测试环境应能创建线程");
        let input: Vec<u64> = (0..1_000).collect();

        // Act
        let output = pool.map_collect(&input, |n| n * 2);

        // Assert
        let expected: Vec<u64> = (0..1_000).map(|n| n * 2).collect();
        assert_eq!(output, expected);
    }

    #[test]
    fn 空输入返回空结果() {
        // Arrange
        let pool = JobPool::new(2).expect("测试环境应能创建线程");
        let input: Vec<u64> = Vec::new();

        // Act
        let output = pool.map_collect(&input, |n| n * 2);

        // Assert
        assert!(output.is_empty());
    }

    #[test]
    fn 线程数为零时退化为单线程而非崩溃() {
        // 配置文件可能写出 0，与其崩溃不如退化。
        // Arrange
        let pool = JobPool::new(0).expect("测试环境应能创建线程");
        let input = vec![1_u64, 2, 3];

        // Act
        let output = pool.map_collect(&input, |n| n + 1);

        // Assert
        assert_eq!(output, vec![2, 3, 4]);
    }

    #[test]
    fn 线程数为零时池内至少有一条线程() {
        // Arrange
        let pool = JobPool::new(0).expect("测试环境应能创建线程");

        // Act
        let count = pool.thread_count();

        // Assert
        assert_eq!(count, 1);
    }

    #[test]
    fn 通道先发后收能取回消息() {
        // 只验证同线程的收发契约。真正的跨线程行为由 crossbeam 自身的
        // 测试套件保证，此处重复搭一条线程只会让测试变慢且更易不稳定。
        // Arrange
        let (sender, receiver) = channel::<u32>();

        // Act
        sender.send(7).expect("接收端仍存活");

        // Assert
        assert_eq!(receiver.recv().expect("已有消息在途"), 7);
    }
}
