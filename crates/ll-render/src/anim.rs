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

#[cfg(test)]
mod tests {
    use super::*;

    fn walk_clip() -> Clip {
        Clip {
            frames: vec!["w0".into(), "w1".into(), "w2".into()],
            frames_per_step: 5,
            looping: true,
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
