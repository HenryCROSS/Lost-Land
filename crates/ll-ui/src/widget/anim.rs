//! HUD 数值动画：血条/经验条这类「显示值应该平滑追上真实值，但数字
//! 必须瞬时」的场景。
//!
//! # 时钟源：帧计数，不是墙钟时间
//!
//! 项目里精灵动画已经在用帧计数（`ll_platform::input::FrameId`/
//! `Playback::current_frame`），本模块延续同一条纪律,不引入
//! `std::time::Instant`/系统时钟一类的墙钟时间源——帧计数天然属于
//! ADR 0020 的甲区（渲染/表现层），不会有「暂停/卡顿导致动画时长漂移」
//! 这类墙钟时间常见的坑，也不会有任何途径污染世界状态。本 crate 不
//! 依赖 `ll-platform`（见 crate 顶层文档「依赖方向」），因此这里的
//! 帧计数类型是一个裸 `u64`（[`FrameTick`]），调用方（`ll-game::app`）
//! 传入 `FrameId(frame).0`，不需要引入新的跨 crate 依赖。
//!
//! # 数字瞬时，条形动画——两条通道从不共用一个数
//!
//! 这是项目所有者反复强调的硬规则：roguelike 里玩家靠精确数值做决策，
//! 血量文本必须立刻显示真实值，绝不能让数字也跟着动画平滑过渡（那等于
//! 在关键时刻向玩家隐瞒真实状态）。本模块只字面地服务「条形宽度该画
//! 多长」这一个问题——[`crate::hud::character_panel`]/
//! [`crate::hud::status_bar`] 里生成文本标签的函数从未、也不会引用
//! [`AnimatedValue`]，两条通道在类型层面就没有交叉的机会，不是靠约定
//! 「记得不要把动画值传给文本」这种容易被忘记的纪律。
//!
//! # 收敛保证：达到时长后精确返回目标值，不是渐近逼近
//!
//! [`AnimatedValue::value_at`] 在 `elapsed >= duration` 时直接返回
//! `self.target`（原样的浮点值，不经过任何插值公式），不是让 `lerp`
//! 无限逼近但永远差一点点——这是本模块测试「给定足够多帧后显示值
//! 精确等于目标值」的字面依据。

/// 帧计数——与 `ll_platform::input::FrameId`/`Playback::current_frame`
/// 同一个时钟源，见模块文档「时钟源」一节。
pub type FrameTick = u64;

/// 一次数值动画从起点到终点默认花费的帧数——足够短以显得「跟手」，
/// 又足够长以能看出过渡（60 帧/秒下约 1/3 秒）。没有配置入口：这是
/// 表现层的手感取舍，本批次不需要按控件单独调节。
pub const DEFAULT_ANIM_DURATION_FRAMES: u32 = 20;

/// 一个会平滑追上目标值的标量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimatedValue {
    start_value: f32,
    target: f32,
    start_frame: FrameTick,
    duration_frames: u32,
}

impl AnimatedValue {
    /// 新建一个恒定于 `initial` 的动画值——构造时没有正在进行的过渡，
    /// `value_at` 在任何 `now` 下都返回 `initial`，直到第一次
    /// [`Self::retarget`] 到一个不同的值。
    pub fn new(initial: f32) -> AnimatedValue {
        Self::with_duration(initial, DEFAULT_ANIM_DURATION_FRAMES)
    }

    /// 同 [`Self::new`]，但用调用方指定的过渡时长而不是
    /// [`DEFAULT_ANIM_DURATION_FRAMES`]——双层血条的余晖层用更长的时长
    /// 制造「追赶」的滞后感，见
    /// `crate::widget::bar::FlatTwoLayerBarAppearance` 模块文档。
    pub fn with_duration(initial: f32, duration_frames: u32) -> AnimatedValue {
        AnimatedValue {
            start_value: initial,
            target: initial,
            start_frame: 0,
            duration_frames,
        }
    }

    /// 把目标改成 `target`——若与当前目标不同，从「现在这一帧实际显示
    /// 的值」重新起跑一段新的过渡（不是从上一段的起点，也不是从上一段
    /// 的目标）；若目标未变（调用方每帧都会传入「当前真实值」，多数
    /// 帧里目标不变),不做任何事,不重启动画。
    pub fn retarget(&mut self, target: f32, now: FrameTick) {
        if (target - self.target).abs() <= f32::EPSILON {
            return;
        }
        let current = self.value_at(now);
        self.start_value = current;
        self.target = target;
        self.start_frame = now;
    }

    /// 直接跳到 `value`，不经过任何过渡——升级时「先冲满、清零、重新
    /// 起跑」这类需要瞬间归位的场景用它，见
    /// `crate::hud::character_panel` 里经验条的处理。
    pub fn snap_to(&mut self, value: f32) {
        self.start_value = value;
        self.target = value;
    }

    /// 求 `now` 这一帧应该显示的值。
    pub fn value_at(&self, now: FrameTick) -> f32 {
        let elapsed = now.saturating_sub(self.start_frame);
        if elapsed >= self.duration_frames as u64 {
            // 收敛保证：直接返回目标本身，不再经过插值公式——见模块
            // 文档「收敛保证」一节。
            return self.target;
        }
        let t = elapsed as f32 / self.duration_frames as f32;
        self.start_value + (self.target - self.start_value) * t
    }

    /// 当前目标值——不经过任何插值,调用方需要「真实要到达的值」而非
    /// 「这一帧显示的值」时用它（例如判断某个条形是否已经完全收敛）。
    pub fn target(&self) -> f32 {
        self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 推进足够多帧后显示值精确等于目标值() {
        // 这是本模块存在的核心保证——见模块文档「收敛保证」一节：
        // 不是渐近逼近，是达到时长后精确返回目标值本身。
        // Arrange
        let mut value = AnimatedValue::new(0.0);
        value.retarget(100.0, 0);

        // Act
        let converged = value.value_at(DEFAULT_ANIM_DURATION_FRAMES as u64 + 1);

        // Assert
        assert_eq!(converged, 100.0);
    }

    #[test]
    fn 动画进行中显示值严格介于起点与终点之间() {
        // Arrange
        let mut value = AnimatedValue::new(0.0);
        value.retarget(100.0, 0);

        // Act：取动画时长一半的那一帧。
        let midpoint = value.value_at(DEFAULT_ANIM_DURATION_FRAMES as u64 / 2);

        // Assert
        assert!(midpoint > 0.0 && midpoint < 100.0);
    }

    #[test]
    fn 目标不变时重复retarget不会重启动画起点() {
        // Arrange
        let mut value = AnimatedValue::new(0.0);
        value.retarget(100.0, 0);
        let midpoint_before = value.value_at(10);

        // Act：同一个目标值再调用一次 retarget——不应该把 start_frame
        // 挪到新的 now,否则「每帧都传真实值」这个既有调用模式会让动画
        // 永远停在起点附近,永不收敛。
        value.retarget(100.0, 10);
        let midpoint_after = value.value_at(10);

        // Assert
        assert_eq!(midpoint_before, midpoint_after);
    }

    #[test]
    fn 动画进行中改变目标会从当前显示值重新起跑() {
        // Arrange：先朝 100 跑到一半,再突然改目标为 50。
        let mut value = AnimatedValue::new(0.0);
        value.retarget(100.0, 0);
        let midpoint = value.value_at(10);

        // Act
        value.retarget(50.0, 10);
        let just_after_retarget = value.value_at(10);

        // Assert：改目标的那一刻,显示值不应该发生跳变。
        assert_eq!(just_after_retarget, midpoint);
    }

    #[test]
    fn with_duration构造的动画按自定义时长收敛() {
        // Arrange：时长只有默认值的两倍,用来验证自定义时长真的生效
        // （而不是悄悄退回默认值)。
        let custom_duration = DEFAULT_ANIM_DURATION_FRAMES * 2;
        let mut value = AnimatedValue::with_duration(0.0, custom_duration);
        value.retarget(100.0, 0);

        // Act：推进到默认时长（还没到自定义时长）。
        let before_custom_duration = value.value_at(DEFAULT_ANIM_DURATION_FRAMES as u64);

        // Assert：默认时长这一刻还没收敛（因为真正的时长是它的两倍）。
        assert!(before_custom_duration < 100.0);
    }

    #[test]
    fn snap_to立即让当前值与目标值都等于给定值() {
        // Arrange
        let mut value = AnimatedValue::new(0.0);
        value.retarget(100.0, 0);

        // Act
        value.snap_to(0.0);

        // Assert
        assert_eq!(value.value_at(0), 0.0);
        assert_eq!(value.target(), 0.0);
    }
}
