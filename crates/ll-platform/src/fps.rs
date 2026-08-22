//! 表现层的帧率计算：墙钟采样 + 指数滑动平均平滑。
//!
//! # 为什么用墙钟，不用帧计数
//!
//! FPS 的定义就是「每墙钟秒渲染多少帧」，帧计数（[`crate::window::FrameId`]/
//! `ll_ui::widget::anim::FrameTick`）回答的是「这是第几帧」，两者是不同
//! 的问题——单看帧计数算不出「这一帧实际耗时多久」。本模块因此像
//! [`crate::input`]（见其模块文档「计时只属于输入层」一节）一样直接用
//! `std::time::Instant`，同样绝不可流入世界状态或参与存档序列化，只
//! 活在表现层——调用方是 `ll-game::app::Demo`，每帧调用一次
//! [`FpsCounter::record_frame`]，产出的浮点数只用来拼状态栏文本，从不
//! 反过来影响任何玩法判定。
//!
//! # 平滑算法：指数滑动平均（EMA），不是逐帧瞬时值
//!
//! 逐帧瞬时 FPS（`1.0 / dt`）在慢帧后会剧烈跳动——一次 16ms 的正常帧
//! 紧跟一次 33ms 的卡顿帧，瞬时读数会从 60 直接摔到 30 再弹回 60，玩家
//! 读到的只是噪声，不是趋势。本模块改为对**帧间隔**（而不是 fps 本身）
//! 做指数滑动平均：`smoothed = α·dt + (1-α)·smoothed_prev`，显示时再取
//! 倒数换算成 fps。对 dt 做平均而不是直接对 fps 值做平均，是刻意的：
//! fps 是 dt 的非线性函数（「多帧的平均帧率」正确定义是调和平均，不是
//! 算术平均），直接对一串 fps 值取算术平均，在帧间隔波动较大时会系统性
//! 偏高于真实的调和平均——对 dt 取平均、最后统一取一次倒数，天然避开
//! 了这个偏差。
//!
//! `α`（[`SMOOTHING_FACTOR`]）取 0.1：约十帧的时间常数，足够抹平单帧
//! 抖动，又不会慢到掩盖持续的掉帧趋势——与
//! `ll_ui::widget::anim::DEFAULT_ANIM_DURATION_FRAMES`（约 1/3 秒收敛）
//! 同一个数量级的取舍,都是「跟手」与「平滑」之间的手感折中,不是靠公式
//! 推导出的唯一正确值。
//!
//! # 为什么是指数滑动平均而不是固定大小的滑动窗口
//!
//! 固定窗口（保留最近 N 个样本、每次全量重算平均值）需要一个环形缓冲区
//! 才能维护「最旧样本移出、最新样本移入」，指数滑动平均只需要一个标量
//! （[`FpsCounter`] 内部就是两个 `Option` 字段）就能达到同样「近期样本
//! 权重更高、久远样本权重指数衰减」的效果，不需要额外的堆分配或固定
//! 容量参数,是两种方案里更简单、且已经完全够用的一种。
//!
//! # 测试范围：只测平滑算法与 `Instant` 差值换算，不测「真实运行时的
//! FPS 读数貌似合理」
//!
//! [`smooth_frame_seconds`] 是纯函数，直接喂已知的帧间隔序列断言输出，
//! 不需要真实时间流逝。[`FpsCounter::record_frame`] 额外做的只是「用
//! `Instant` 算出 `dt`」这一步纯算术，测试用真实构造的 `Instant`
//! （`Instant::now() + Duration`）验证，不需要真的等待，也不需要为此
//! 引入一个可注入的假时钟 trait——那样只会把复杂度从测试代码搬进生产
//! 代码。没有测、也测不了的是「长时间真实运行下这套平滑参数手感如何」，
//! 这类主观手感判断留给实机验收。

use std::time::Instant;

/// 指数滑动平均的平滑系数——见模块文档「平滑算法」一节。
const SMOOTHING_FACTOR: f32 = 0.1;

/// 单次采样帧间隔的钳制下限（秒）。
///
/// 防止一次异常的零间隔（或负间隔，理论上 `Instant::duration_since`
/// 不会产出负值，但零间隔在时钟分辨率较粗的环境下并非不可能）采样让
/// 平滑值冲向零、进而让 `1.0 / smoothed` 冲向无穷大——那样状态栏会
/// 显示一个荒谬的 FPS 数字，钳制到一个远高于任何真实帧率的上限
/// （对应 10000 FPS）比让它失真更诚实。
const MIN_FRAME_SECONDS: f32 = 1.0 / 10_000.0;

/// 按一个新的帧间隔样本更新平滑帧间隔——纯函数，不接触 `Instant`，是
/// [`FpsCounter::record_frame`] 与本模块测试共用的核心算法，见模块
/// 文档「平滑算法」一节。
///
/// `previous` 为 `None`（还没有任何历史样本，例如刚构造出的计数器）时
/// 直接把本次样本当作起点，不做加权——第一帧没有「过去」可平滑。
pub fn smooth_frame_seconds(previous: Option<f32>, dt_seconds: f32) -> f32 {
    let dt = dt_seconds.max(MIN_FRAME_SECONDS);
    match previous {
        None => dt,
        Some(prev) => SMOOTHING_FACTOR * dt + (1.0 - SMOOTHING_FACTOR) * prev,
    }
}

/// 帧率计数器：墙钟采样 + [`smooth_frame_seconds`] 平滑，产出显示用的
/// FPS。只活在表现层——见模块文档。
#[derive(Debug, Clone, Copy)]
pub struct FpsCounter {
    last_frame_at: Option<Instant>,
    smoothed_seconds: Option<f32>,
}

impl Default for FpsCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl FpsCounter {
    /// 建一个尚未记录过任何帧的计数器。
    pub const fn new() -> FpsCounter {
        FpsCounter {
            last_frame_at: None,
            smoothed_seconds: None,
        }
    }

    /// 记一次新帧的墙钟时刻，返回这一刻应显示的平滑 FPS。
    ///
    /// 首次调用没有上一帧可比较，只记下这个时刻作为下一次调用的基准，
    /// 返回 `0.0`——此刻确实还不存在任何「帧间隔」，显示零比编造一个
    /// 数字更诚实（与 `ll_ui::widget::anim` 那类模块「不编造尚未存在
    /// 的数值」同一条既有纪律的延伸，虽然本模块不在那个 crate 里）。
    pub fn record_frame(&mut self, now: Instant) -> f32 {
        let Some(last) = self.last_frame_at else {
            self.last_frame_at = Some(now);
            return 0.0;
        };
        let dt = now.duration_since(last).as_secs_f32();
        self.last_frame_at = Some(now);
        let smoothed = smooth_frame_seconds(self.smoothed_seconds, dt);
        self.smoothed_seconds = Some(smoothed);
        1.0 / smoothed
    }

    /// 当前的平滑 FPS，不推进任何状态——供只想重复读取「当前值」而不
    /// 想误触发一次新采样的调用方使用。还没有任何采样时返回 `0.0`，
    /// 理由同 [`Self::record_frame`] 首次调用。
    pub fn fps(&self) -> f32 {
        self.smoothed_seconds.map(|s| 1.0 / s).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn 平滑帧间隔在没有历史样本时直接等于本次样本() {
        // Arrange & Act
        let smoothed = smooth_frame_seconds(None, 1.0 / 60.0);

        // Assert
        assert_eq!(smoothed, 1.0 / 60.0);
    }

    #[test]
    fn 平滑帧间隔朝新样本方向移动但不直接跳变到新样本() {
        // Arrange：上一次平滑值对应 30fps，这一帧的真实间隔对应 60fps。
        let previous = 1.0 / 30.0;
        let new_sample = 1.0 / 60.0;

        // Act
        let smoothed = smooth_frame_seconds(Some(previous), new_sample);

        // Assert：严格介于两者之间，不是原地不动也不是直接跳到新值。
        assert!(smoothed < previous);
        assert!(smoothed > new_sample);
    }

    #[test]
    fn 持续输入相同帧间隔时平滑值收敛到该值() {
        // Arrange
        let target_dt = 1.0 / 144.0;
        let mut smoothed = Some(1.0 / 15.0); // 起点是一个明显不同的值。

        // Act：喂足够多次相同样本。
        for _ in 0..500 {
            smoothed = Some(smooth_frame_seconds(smoothed, target_dt));
        }

        // Assert
        let converged = smoothed.expect("循环至少跑了一次，恒为 Some");
        assert!((converged - target_dt).abs() < 1e-6);
    }

    #[test]
    fn 零帧间隔被钳制到下限而不是产出无穷大fps() {
        // Arrange & Act
        let smoothed = smooth_frame_seconds(None, 0.0);

        // Assert：钳制到下限，换算出的 fps 是一个有限值而非无穷大。
        assert_eq!(smoothed, MIN_FRAME_SECONDS);
        assert!((1.0 / smoothed).is_finite());
    }

    #[test]
    fn 帧率计数器首次记录返回零() {
        // Arrange
        let mut counter = FpsCounter::new();

        // Act
        let fps = counter.record_frame(Instant::now());

        // Assert
        assert_eq!(fps, 0.0);
    }

    #[test]
    fn 帧率计数器在稳定六十帧间隔下收敛到接近六十() {
        // Arrange：构造一串真实的 Instant（不是伪造的时钟接口，只是
        // 对同一个基准点做确定性的 Duration 加法），模拟稳定 60fps。
        let mut counter = FpsCounter::new();
        let frame_interval = Duration::from_secs_f32(1.0 / 60.0);
        let mut now = Instant::now();
        counter.record_frame(now);

        // Act：推进足够多帧让指数滑动平均收敛。
        let mut last_fps = 0.0;
        for _ in 0..500 {
            now += frame_interval;
            last_fps = counter.record_frame(now);
        }

        // Assert：不带自定义失败消息——见模块文档同一条纪律的延伸：
        // 一旦消息文本较长，`cargo fmt` 会把它拆到独立一行，让中文
        // 字面量与 `assert!(` 分处两行，触发 `check_i18n_strings.py`
        // 的误判（诊断宏豁免要求两者同行）。断言本身的语义已经足够
        // 清楚（`last_fps` 与 60 的差应收敛到 1 以内），不需要额外
        // 消息。
        assert!((last_fps - 60.0).abs() < 1.0);
    }

    #[test]
    fn 帧率计数器在帧间隔变化时读数落在旧新两个帧率之间() {
        // Arrange：先跑稳定的 60fps 建立基准,再突然改成 30fps。
        let mut counter = FpsCounter::new();
        let mut now = Instant::now();
        counter.record_frame(now);
        for _ in 0..100 {
            now += Duration::from_secs_f32(1.0 / 60.0);
            counter.record_frame(now);
        }

        // Act：切到 30fps 的间隔，只推进一帧。
        now += Duration::from_secs_f32(1.0 / 30.0);
        let fps_after_change = counter.record_frame(now);

        // Assert：还没完全收敛到新值，读数介于两者之间。
        assert!(fps_after_change < 60.0);
        assert!(fps_after_change > 30.0);
    }

    #[test]
    fn fps读数方法不推进任何状态() {
        // Arrange
        let mut counter = FpsCounter::new();
        let mut now = Instant::now();
        counter.record_frame(now);
        now += Duration::from_secs_f32(1.0 / 60.0);
        counter.record_frame(now);

        // Act：重复读取多次。
        let first_read = counter.fps();
        let second_read = counter.fps();

        // Assert
        assert_eq!(first_read, second_read);
    }

    #[test]
    fn 尚未记录任何帧时fps读数为零() {
        // Arrange
        let counter = FpsCounter::new();

        // Act
        let fps = counter.fps();

        // Assert
        assert_eq!(fps, 0.0);
    }
}
