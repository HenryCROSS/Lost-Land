//! 动画：按帧号驱动的精灵序列播放，帧号取自 [`ll_platform::window::FrameId`]。
//!
//! # 整数帧号而非墙钟秒数
//!
//! 动画播放以整数帧号为时间基准，而不是墙钟浮点秒数。这样动画状态
//! （[`Playback`]）可以安全地进入世界状态并被存档序列化——存档读回后
//! 动画会从对应帧号精确接续，而不是跳到随机一帧；用浮点秒数做不到这
//! 一点，还会因为不同平台的浮点运算细节差异而破坏跨平台确定性。
//!
//! # 降级而非崩溃
//!
//! 动画剪辑数据最终来自可被 mod 覆盖的资产，属于外部不可信输入。空
//! 剪辑、步长为零、剪辑索引越界都可能来自损坏的数据，[`Playback::current_frame`]
//! 对这三种情形一律返回 [`None`] 或停在首帧，绝不 panic（除零、越界
//! 索引）。

use ll_platform::window::FrameId;

/// 一段动画剪辑：帧序列、播放速度、是否循环。
///
/// 帧用条目名（[`String`]）而非贴图句柄或索引：动画剪辑与图集条目分属
/// 两套独立数据（剪辑描述「按什么顺序播放」，图集描述「这一帧长什么
/// 样」），用名字间接引用让两者可以独立由不同的 mod 资产提供，互不
/// 耦合。
pub struct Clip {
    /// 依序播放的图集条目名。
    pub frames: Vec<String>,
    /// 每一帧停留的游戏帧数。为零时视为「不推进」，恒定停在首帧
    /// （见模块文档「降级而非崩溃」）。
    pub frames_per_step: u32,
    /// 播完最后一帧后是否从头循环。为假时停在末帧——施法、受击这类
    /// 一次性动画播完应停住，跳回起手姿势会看起来像抽搐。
    pub looping: bool,
    /// 由 [`AnimStateMachine`] 管理的触发式状态在没有新触发时，离开
    /// 这段剪辑前继续维持播放的帧数——即「状态退出前的余韵」。
    ///
    /// 这是项目所有者要求的可自定义动画延迟：回合制的离散事件（移动、
    /// 攻击……）只在结算的那一帧存在，若状态直接绑定「本帧有没有这个
    /// 事件」，事件消失的下一帧就会立刻回落到默认状态，连续触发时表
    /// 现为一闪一闪（见 `knowledge/design/animation-and-vfx-boundary.md`
    /// 「结算是瞬时的，动画只是回放」）。这个字段让每段剪辑自带一段
    /// 「即使暂时没有新事件也先别回落」的余量，具体数值由剪辑数据决
    /// 定而非写死的常量——按 ADR 0016/0017 的分级设计，这属于第一档
    /// 静态声明，应当物化成数据。
    ///
    /// 只有交给 [`AnimStateMachine`] 管理的触发式状态动画会读这个字
    /// 段；原地循环动画（模块文档 2.1）与直接单独使用 [`Playback`]
    /// 的场景都不读它，取值随意（通常填零）。
    pub exit_grace_frames: u32,
}

/// 一次具体的动画播放：播放哪段剪辑、从哪一帧开始。
///
/// 只存「剪辑索引 + 起始帧号」这两个整数，不存任何浮点或墙钟时间——
/// 这正是它能被存进世界状态、参与存档序列化的原因（见模块文档）。
/// 未在此处派生 `Serialize`/`Deserialize`：[`FrameId`] 本身来自
/// `ll-platform`、未派生这两个 trait，真到接线存档时应在那一层决定
/// 是否派生，不属于本任务范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Playback {
    clip: usize,
    started_at: FrameId,
}

impl Playback {
    /// 从 `started_at` 这一帧开始播放 `clip` 号剪辑。
    pub fn new(clip: usize, started_at: FrameId) -> Playback {
        Playback { clip, started_at }
    }

    /// 算出 `now` 这一帧应显示的图集条目名。
    ///
    /// 三种损坏数据情形一律降级，不 panic：
    /// - 剪辑索引越界（`clips.get(self.clip)` 落空）：返回 [`None`]。
    /// - 剪辑没有任何帧：返回 [`None`]，避免除以零。
    /// - `frames_per_step` 为零：不推进，恒定停在首帧。
    pub fn current_frame<'a>(&self, clips: &'a [Clip], now: FrameId) -> Option<&'a str> {
        let clip = clips.get(self.clip)?;
        if clip.frames.is_empty() {
            return None;
        }
        if clip.frames_per_step == 0 {
            return clip.frames.first().map(String::as_str);
        }

        // saturating_sub：`now` 按设计恒不早于 `started_at`，但对外部
        // 数据的防御性写法不假设调用方永远守规矩。
        let elapsed = now.0.saturating_sub(self.started_at.0);
        let step_index = elapsed / clip.frames_per_step as u64;
        let frame_count = clip.frames.len() as u64;

        let frame_index = if clip.looping {
            step_index % frame_count
        } else {
            step_index.min(frame_count - 1)
        };

        clip.frames.get(frame_index as usize).map(String::as_str)
    }
}

/// 由离散事件驱动的表现层触发式动画状态机，管理这类状态自己的生命
/// 周期。
///
/// # 要解决的问题：瞬时事件不能直接驱动动画状态
///
/// 回合制的移动、攻击、施法这类事件只在结算的那一帧存在于
/// `Intent`/`Effect` 流里——下一帧它就不存在了（见
/// `knowledge/design/animation-and-vfx-boundary.md`「结算是瞬时的，
/// 动画只是回放」）。若动画状态直接绑定「本帧有没有这个事件」，状态
/// 会在事件消失的下一帧立刻回落到默认状态；连续触发同一个状态（例如
/// 按住方向键连续移动）时，两次事件之间只要有一帧空档，画面就会回弹
/// 到默认状态再切回来，表现为一闪一闪。
///
/// # 解法：状态自带余韵，收到同状态事件续期而非重播
///
/// 本类型让每个触发式状态维持 [`Clip::exit_grace_frames`] 声明的帧数
/// ——这段时间内即使没有新事件也不回落；期间若收到同一个状态的新事
/// 件，只续期（刷新还能维持多久）而不重建播放，`Playback` 保持原样
/// 从原来的起始帧连续播放,不会跳回第一帧——这正是连续触发时不闪烁
/// 的关键。
///
/// # 不假设只有两态
///
/// 状态本身就是调用方 `Clip` 表里的下标，不是写死的「行走/待机」二
/// 态枚举——未来新增攻击、施法、受击、死亡等触发式状态，调用方只需
/// 要为它们各自定义一个 `Clip`（各自带上自己的 `exit_grace_frames`），
/// 在对应事件到达时调用 [`AnimStateMachine::trigger`]，不需要改动本
/// 类型一行代码，也不需要修改已有的行走/待机两态调用点。
pub struct AnimStateMachine {
    /// 不处于任何触发式状态时回落到的默认剪辑下标（通常是待机）。
    default_clip: usize,
    /// 当前活跃的剪辑下标。
    current_clip: usize,
    /// 当前状态的播放进度。
    playback: Playback,
    /// 当前触发式状态最迟维持到哪一帧（不含）——超过这一帧仍未收到
    /// 同状态新事件，下一次 [`AnimStateMachine::update`] 回落到默认
    /// 状态。处于默认状态时恒为 [`None`]（默认状态不会「过期」）。
    expires_at: Option<FrameId>,
}

impl AnimStateMachine {
    /// 以 `default_clip`（通常是待机）为初始状态，从 `now` 这一帧开始
    /// 播放。
    pub fn new(default_clip: usize, now: FrameId) -> AnimStateMachine {
        AnimStateMachine {
            default_clip,
            current_clip: default_clip,
            playback: Playback::new(default_clip, now),
            expires_at: None,
        }
    }

    /// 触发进入（或续期）`clip` 对应的状态。
    ///
    /// 调用方每收到一次应当驱动动画状态的离散事件（移动、攻击……）就
    /// 调用一次：`clip` 是这次事件对应的剪辑下标，`clips` 用于读取该
    /// 剪辑自己声明的 [`Clip::exit_grace_frames`]——`clip` 越界时降级
    /// 为零余韵而非 panic，呼应模块文档「降级而非崩溃」。
    ///
    /// - `clip` 与当前活跃剪辑相同：视为续期，只刷新 `expires_at`，不
    ///   重建 `playback`——连续触发同一状态时不闪烁的关键就在这里：
    ///   剪辑仍在从原来的起始帧连续播放。
    /// - `clip` 与当前不同：视为切换，从 `now` 重新起播新剪辑。
    pub fn trigger(&mut self, clips: &[Clip], clip: usize, now: FrameId) {
        let grace = clips.get(clip).map_or(0, |c| c.exit_grace_frames);
        if clip != self.current_clip {
            self.current_clip = clip;
            self.playback = Playback::new(clip, now);
        }
        self.expires_at = Some(FrameId(now.0.saturating_add(u64::from(grace))));
    }

    /// 每帧无条件调用一次：若当前处于某个触发式状态且已超过余韵仍未
    /// 收到新触发，回落到默认状态。
    ///
    /// 与 [`AnimStateMachine::trigger`] 是两个独立调用点——`trigger`
    /// 只在事件真正到达的那一帧调用，`update` 每帧都要调用，这正是
    /// 「没有新事件」这件事本身也能被观察到并驱动回落的机制。本方法
    /// 只推进表现层自己的状态，不读也不写 `WorldState`，与「回合绝不
    /// 等动画播完」这条铁律无冲突（见模块所在 crate 的设计文档）。
    pub fn update(&mut self, now: FrameId) {
        if self.current_clip == self.default_clip {
            return;
        }
        let Some(expires_at) = self.expires_at else {
            return;
        };
        if now >= expires_at {
            self.current_clip = self.default_clip;
            self.playback = Playback::new(self.default_clip, now);
            self.expires_at = None;
        }
    }

    /// 当前活跃的剪辑下标——调用方用它判断「现在是不是在播某个具体
    /// 状态」，测试也用它断言状态没有回弹到默认状态。
    pub fn active_clip(&self) -> usize {
        self.current_clip
    }

    /// 内部 `Playback` 的只读引用，供调用方喂给
    /// [`Playback::current_frame`] 或其它需要直接持有 `Playback` 的
    /// 既有代码路径——本类型只负责状态的生命周期，帧号到图集条目名
    /// 的换算继续复用现成的 `Playback`，不重复实现。
    pub fn playback(&self) -> &Playback {
        &self.playback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk_clip() -> Clip {
        Clip {
            frames: vec!["w0".into(), "w1".into(), "w2".into()],
            frames_per_step: 5,
            looping: true,
            // 与 `Playback` 本身无关（该字段只被 `AnimStateMachine`
            // 读取），这里给个非零值方便下面复用本函数的状态机测试。
            exit_grace_frames: 10,
        }
    }

    #[test]
    fn 起始时刻停在第一帧() {
        // Arrange
        let clips = vec![walk_clip()];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(100));

        // Assert
        assert_eq!(frame, Some("w0"));
    }

    #[test]
    fn 经过一个步长后推进到第二帧() {
        // Arrange
        let clips = vec![walk_clip()];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(105));

        // Assert
        assert_eq!(frame, Some("w1"));
    }

    #[test]
    fn 循环剪辑播完后回到首帧() {
        // Arrange：3 帧 × 5 步长 = 15，故第 115 帧回到起点。
        let clips = vec![walk_clip()];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(115));

        // Assert
        assert_eq!(frame, Some("w0"));
    }

    #[test]
    fn 非循环剪辑播完后停在末帧() {
        // 施法、受击这类一次性动画播完应停住，跳回起手姿势会像抽搐。
        // Arrange
        let clips = vec![Clip {
            looping: false,
            ..walk_clip()
        }];
        let playback = Playback::new(0, FrameId(100));

        // Act
        let frame = playback.current_frame(&clips, FrameId(999));

        // Assert
        assert_eq!(frame, Some("w2"));
    }

    #[test]
    fn 单帧剪辑恒返回该帧() {
        // 规格要求像素小人可以是静止的，也可以循环播放动画。
        // Arrange
        let clips = vec![Clip {
            frames: vec!["idle".into()],
            frames_per_step: 1,
            looping: true,
            exit_grace_frames: 0,
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(12345)), Some("idle"));
    }

    #[test]
    fn 空剪辑返回空值而非崩溃() {
        // 损坏的 mod 数据可能定义出没有任何帧的剪辑。
        // Arrange
        let clips = vec![Clip {
            frames: Vec::new(),
            frames_per_step: 5,
            looping: true,
            exit_grace_frames: 0,
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(10)), None);
    }

    #[test]
    fn 步长为零时停在首帧而非除零崩溃() {
        // 配置或 mod 可能写出 0。
        // Arrange
        let clips = vec![Clip {
            frames_per_step: 0,
            ..walk_clip()
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(999)), Some("w0"));
    }

    #[test]
    fn 剪辑索引越界返回空值() {
        // Arrange
        let clips = vec![walk_clip()];
        let playback = Playback::new(99, FrameId(0));

        // Act & Assert
        assert_eq!(playback.current_frame(&clips, FrameId(0)), None);
    }
}

#[cfg(test)]
mod anim_state_machine_tests {
    use super::*;

    /// 状态机测试专用的两段剪辑：下标 0 是触发式的「行走」，余韵 10
    /// 帧、每步 5 帧、3 帧循环（与 `tests::walk_clip` 形状一致，独立
    /// 定义一份而不是跨 `mod` 复用私有测试辅助函数）；下标 1 是默认
    /// 状态的「待机」——默认状态从不读 `exit_grace_frames`，填零。
    fn clips() -> Vec<Clip> {
        vec![
            Clip {
                frames: vec!["w0".into(), "w1".into(), "w2".into()],
                frames_per_step: 5,
                looping: true,
                exit_grace_frames: 10,
            },
            Clip {
                frames: vec!["idle".into()],
                frames_per_step: 1,
                looping: true,
                exit_grace_frames: 0,
            },
        ]
    }

    /// 默认（待机）剪辑在 [`clips`] 里的下标。
    const 待机: usize = 1;
    /// 触发式（行走）剪辑在 [`clips`] 里的下标。
    const 行走: usize = 0;

    #[test]
    fn 初始状态是默认剪辑() {
        // Arrange & Act
        let machine = AnimStateMachine::new(待机, FrameId(0));

        // Assert
        assert_eq!(machine.active_clip(), 待机);
    }

    #[test]
    fn 触发后立即切换到目标状态() {
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));

        // Act
        machine.trigger(&clips, 行走, FrameId(0));

        // Assert
        assert_eq!(machine.active_clip(), 行走);
    }

    #[test]
    fn 连续触发间隔小于余韵时状态不回弹到默认状态() {
        // 模拟按住方向键连续移动：每次移动事件的间隔（3 帧）小于行走
        // 剪辑声明的余韵（10 帧），状态应当全程停在「行走」，不出现
        // 回弹到「待机」再切回的闪烁——这正是项目所有者报告的缺陷。
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));

        // Act & Assert：五次触发，每次间隔 3 帧；每次触发前先跑一次
        // `update`（模拟每帧都会调用它），全程状态必须停在「行走」。
        machine.trigger(&clips, 行走, FrameId(0));
        for frame in [3u64, 6, 9, 12, 15] {
            machine.update(FrameId(frame));
            assert_eq!(machine.active_clip(), 行走, "第 {frame} 帧不应回弹到待机");
            machine.trigger(&clips, 行走, FrameId(frame));
        }
    }

    #[test]
    fn 续期时不重建播放进度() {
        // 若续期错误地重建了 `Playback`，t=6 时的帧计算会把 t=3 那次
        // 续期当成新的起播点，算出第 0 步（w0）而不是延续自 t=0 起播、
        // 已经过去一步的 w1——这正是「连续移动不能闪」在底层播放进度
        // 上的体现,不只是状态标签没变。
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));
        machine.trigger(&clips, 行走, FrameId(0));
        machine.update(FrameId(3));
        machine.trigger(&clips, 行走, FrameId(3)); // 续期，剪辑不变

        // Act：`walk_clip` 的 `frames_per_step` 是 5，从 t=0 算到 t=6
        // 应该是第 1 步（w1）。
        let frame = machine.playback().current_frame(&clips, FrameId(6));

        // Assert
        assert_eq!(frame, Some("w1"));
    }

    #[test]
    fn 状态切换时从新剪辑的起始帧重新起播() {
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));
        machine.trigger(&clips, 行走, FrameId(0));
        machine.update(FrameId(4)); // 行走播到中途

        // Act：从行走切回待机（模拟例如受击打断行走的场景）。
        machine.trigger(&clips, 待机, FrameId(4));

        // Assert
        assert_eq!(machine.active_clip(), 待机);
        assert_eq!(
            machine.playback().current_frame(&clips, FrameId(4)),
            Some("idle")
        );
    }

    #[test]
    fn 超过余韵仍无新触发时回落到默认状态() {
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));
        machine.trigger(&clips, 行走, FrameId(0)); // 余韵 10 帧，到期于第 10 帧

        // Act
        machine.update(FrameId(10));

        // Assert
        assert_eq!(machine.active_clip(), 待机);
    }

    #[test]
    fn 越界剪辑下标触发时降级为零余韵而不崩溃() {
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));

        // Act：触发一个不存在的剪辑下标。
        machine.trigger(&clips, 99, FrameId(0));
        machine.update(FrameId(1));

        // Assert：零余韵意味着下一帧 `update` 就已经过期，回落默认
        // 状态；关键是整个过程没有 panic。
        assert_eq!(machine.active_clip(), 待机);
    }
}
