//! 玩家精灵行走/待机动画：把每帧输入换算成该播放的动画状态。
//!
//! # 为什么拆成独立文件
//!
//! 与 [`crate::layout`] 同一个拆分理由（见其模块文档）：脱离 GPU/世界
//! 状态也能被 `cargo test -p ll-game` 直接覆盖。`crate::app` 只持有真正
//! 的 GPU 资源、驱动事件循环，按 crate 顶层文档「为什么拆成库 + 薄
//! 二进制」一节的既定取舍不做单元测试——`按住移动键连续多帧动画状态
//! 不回弹到待机` 这条项目所有者报告过的缺陷，必须在这类薄胶水层之外
//! 才能被程序化验证到。
//!
//! # 这是「声明了但从没接线」的第十处修复
//!
//! 此前 `crate::app` 只有一张写死的静态图（`hero_idle_0`），图集里的
//! `hero_walk_0`/`hero_walk_1`/`hero_idle_1` 从未被播放过——
//! [`ll_render::anim::AnimStateMachine`] 早已交付，却完全没有被本体
//! 二进制引用。本模块是接线本身：状态机、驱动判据（判断这一帧该不该
//! 播行走）、降级兜底三件事全部复用 `ll-render` 已经交付、`ll-sim` 的
//! `p5_coordinate_acceptance` demo 验证过的实现（[`ll_render::anim`] 的
//! [`movement_key_held`]/[`ll_render::anim::current_sprite_name`]），
//! 本模块只负责本体二进制自己要决定的那部分：判据本身
//! （[`update_player_animation`]）——判定逻辑（判据、状态机、降级兜底）
//! 只有一份实现，不是又抄一遍——这正是本仓库反复点名过的「同一算术在
//! 多处手抄」教训（`ll_render::sprite::sprite_draw_position` 模块
//! 文档）在这里的应用。
//!
//! # 剪辑数据不再由本模块构造
//!
//! 此前本模块自己维护一份 `player_clips()`/`WALK_CLIP`/`IDLE_CLIP`——
//! 与 `p1_acceptance`/`p5_coordinate_acceptance` 各自维护的同构拷贝
//! 一起，是「行走剪辑不该掺待机帧」这个 bug 被逐字抄三遍的三处之一
//! （见 `ll_render::anim::base_hero_clips` 模块文档「起因」）。剪辑
//! 数据现在由 [`ll_mod::base_clip::register_base_clips`] 经完整的
//! 内容注册管线装载（`crate::content::LoadedContent::clip_ids`/
//! `clip_table`），本模块与 `crate::app::Demo` 只是这条链路的消费方：
//! `update_player_animation` 接收装载期已经确定的剪辑下标
//! （`walk_clip`/`idle_clip`），不再自己决定「行走剪辑长什么样」。

use ll_platform::input::InputState;
use ll_platform::window::FrameId;
use ll_render::anim::{AnimStateMachine, movement_key_held};

/// 玩家精灵唯一必须存在的一帧。
///
/// 行走（[`WALK_CYCLE`] 六帧）与待机呼吸（`hero_idle_1`）都是
/// 「锦上添花」的可选资产——本体内嵌的图集恒定包含它们，但动画帧名
/// 最终来自可被 mod 覆盖的剪辑数据，属于外部不可信输入（见
/// `ll_render::anim` 模块文档「降级而非崩溃」一节）。`hero_idle_0` 是
/// 唯一「必须存在」的一帧，缺了它玩家标记本就画不出来，因此拿它当
/// 两段可选动画共同的兜底：mod 只提供这一帧、甚至完全不提供动画剪辑
/// 数据，都是完全正常的情况，本体应当退回这一张静态图，而不是报错或
/// 让玩家标记消失——与 `p5_coordinate_acceptance::FALLBACK_SPRITE` 同一
/// 取舍。带 `lostland:` 前缀——图集条目名统一用完整命名空间字符串，
/// 见 `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 文档。
pub const FALLBACK_SPRITE: &str = "lostland:hero_idle_0";

/// 每帧无条件调用：按当前是否有任意移动键按住
/// （[`movement_key_held`]）电平驱动地设置玩家该播放的动画状态。
///
/// `walk_clip`/`idle_clip` 是装载期由内容注册表分配的剪辑下标（见
/// `crate::content::LoadedContent::clip_ids`），不再是写死的模块常量
/// ——mod 覆盖或新增剪辑内容不会改变这两个参数的传入方式，只会改变
/// `Registry` 实际分配出来的具体数值。
///
/// 为什么是电平驱动（[`AnimStateMachine::set_level`]）而非意图脉冲
/// 驱动（`trigger`+`update`）：与
/// `p5_coordinate_acceptance::Demo::update_player_animation` 同一个决策，
/// 详细论证（按键自动重复的初始延迟盖不住旧余韵方案、按住/松开之间
/// 没有脉冲间隙需要余韵去容忍）见该函数文档，不在这里重复。
///
/// 不读取移动是否真的成功（撞墙也播行走动画）：`ll_sim::resolve` 对
/// 「目的地不可通行」同样产出会推进时钟的效果，这一步在模拟里就是
/// 一次真实的移动尝试，调用方不需要读 `resolve`/`apply` 的结果来判断
/// 「这一步是否真的挪动了位置」——只问按键状态已经足够。
pub fn update_player_animation(
    anim: &mut AnimStateMachine,
    input: &InputState,
    frame: FrameId,
    walk_clip: usize,
    idle_clip: usize,
) {
    let target_clip = if movement_key_held(input) {
        walk_clip
    } else {
        idle_clip
    };
    anim.set_level(target_clip, frame);
}

#[cfg(test)]
mod tests {
    use ll_platform::input::GameKey;
    use ll_render::anim::{Playback, current_sprite_name};
    use ll_render::atlas::AtlasMetadata;

    use super::*;

    /// 测试专用的下标约定——生产路径的实际数值来自
    /// `ll_mod::base_clip::register_base_clips` 分配的
    /// `ContentIndex`，测试不依赖具体数值，只依赖两者不同。
    const WALK_CLIP: usize = 0;
    const IDLE_CLIP: usize = 1;

    /// 测试专用剪辑表：直接复用唯一权威定义
    /// [`ll_render::anim::base_hero_clips`]，不再自己抄一份帧数据。
    fn test_clips() -> Vec<ll_render::anim::Clip> {
        let (walk, idle) = ll_render::anim::base_hero_clips();
        vec![walk, idle]
    }

    #[test]
    fn 按住移动键期间连续多帧动画状态全程停在行走不掉回待机() {
        // 项目所有者报告过的缺陷的回归测试：给定「按下后持续按住」这一串
        // 输入状态，断言状态机每一帧选出的剪辑都是行走。覆盖的帧跨度
        // （0..=40）明显超过按键自动重复的初始延迟（约 21 帧于
        // 60fps）——若动画仍然按脉冲驱动，这条测试会在某一帧观察到待机
        // 贴图掺入。与
        // `p5_coordinate_acceptance::player_animation_tests` 的同名测试
        // 是同一断言的独立实现（本体二进制自己的动画状态是独立的
        // `AnimStateMachine` 实例）。
        // Arrange
        let mut anim = AnimStateMachine::new(IDLE_CLIP, FrameId(0));
        let mut input = InputState::new();
        input.press(GameKey::Up);

        // Act & Assert
        for frame in 0..=40u64 {
            update_player_animation(&mut anim, &input, FrameId(frame), WALK_CLIP, IDLE_CLIP);
            assert_eq!(anim.active_clip(), WALK_CLIP, "第 {frame} 帧不应回弹到待机");
        }
    }

    #[test]
    fn 松开移动键后下一帧立即切回待机不拖延() {
        // 项目所有者明确要求「停下时要及时切回 idle，不能拖沓」。
        // Arrange
        let mut anim = AnimStateMachine::new(IDLE_CLIP, FrameId(0));
        let mut input = InputState::new();
        input.press(GameKey::Up);
        update_player_animation(&mut anim, &input, FrameId(0), WALK_CLIP, IDLE_CLIP);
        assert_eq!(anim.active_clip(), WALK_CLIP, "前置条件：先进入行走");

        // Act：松开方向键。
        input.release(GameKey::Up);
        update_player_animation(&mut anim, &input, FrameId(1), WALK_CLIP, IDLE_CLIP);

        // Assert
        assert_eq!(anim.active_clip(), IDLE_CLIP);
    }

    #[test]
    fn 未按任何移动键时保持待机() {
        // Arrange
        let mut anim = AnimStateMachine::new(IDLE_CLIP, FrameId(0));
        let input = InputState::new();

        // Act
        update_player_animation(&mut anim, &input, FrameId(0), WALK_CLIP, IDLE_CLIP);

        // Assert
        assert_eq!(anim.active_clip(), IDLE_CLIP);
    }

    #[test]
    fn 清空按键状态后动画立即回到待机() {
        // 模拟窗口失焦：`ll_platform::window` 的 `WindowEvent::Focused(false)`
        // 处理器已经接了 `InputState::clear`，本测试验证动画层看到清空
        // 后的 `InputState` 会正确回落到待机。
        // Arrange
        let mut anim = AnimStateMachine::new(IDLE_CLIP, FrameId(0));
        let mut input = InputState::new();
        input.press(GameKey::Right);
        update_player_animation(&mut anim, &input, FrameId(0), WALK_CLIP, IDLE_CLIP);
        assert_eq!(anim.active_clip(), WALK_CLIP, "前置条件：先进入行走");

        // Act：模拟失焦清空。
        input.clear();
        update_player_animation(&mut anim, &input, FrameId(1), WALK_CLIP, IDLE_CLIP);

        // Assert
        assert_eq!(anim.active_clip(), IDLE_CLIP);
    }

    #[test]
    fn 四个方向键分别按住都会进入行走状态() {
        for key in [GameKey::Up, GameKey::Down, GameKey::Left, GameKey::Right] {
            // Arrange
            let mut anim = AnimStateMachine::new(IDLE_CLIP, FrameId(0));
            let mut input = InputState::new();
            input.press(key);

            // Act
            update_player_animation(&mut anim, &input, FrameId(0), WALK_CLIP, IDLE_CLIP);

            // Assert
            assert_eq!(anim.active_clip(), WALK_CLIP, "方向键 {key:?} 应触发行走");
        }
    }

    #[test]
    fn 行走与待机可选帧都缺失于图集时退回唯一必须存在的静态图() {
        // 项目所有者要求：走路和待机动画都是可选的，都没有就退回单张
        // 静态图。用一份只含 FALLBACK_SPRITE 的图集元数据模拟 mod 移除
        // 了全部可选动画帧,断言两层兜底（`current_sprite_name`）一路
        // 降级到同一张图,而不是画出空白或 panic。
        // Arrange
        let metadata = AtlasMetadata::parse(
            r#"{
                "image": "placeholder.png",
                "entries": [
                    { "name": "lostland:hero_idle_0",
                      "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
                      "pivot": { "x": 8, "y": 24 },
                      "footprint": { "width": 1, "height": 1 } }
                ]
            }"#,
        )
        .expect("样例是合法 JSON");
        let clips = test_clips();
        let playback = Playback::new(WALK_CLIP, FrameId(0));

        // Act
        let resolved =
            current_sprite_name(&playback, &clips, FrameId(0), &metadata, FALLBACK_SPRITE);

        // Assert
        assert_eq!(resolved, FALLBACK_SPRITE);
    }
}
