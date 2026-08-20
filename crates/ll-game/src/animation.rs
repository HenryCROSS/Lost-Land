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
//! 本模块只负责本体
//! 二进制自己要决定的那部分：维护哪些剪辑（[`player_clips`]）、下标
//! 含义（[`WALK_CLIP`]/[`IDLE_CLIP`]）、播放节奏（见常量文档）——与
//! p5 demo 各自维护一份同构但物理独立的 `Clip` 表（`examples/` 不是
//! 可供下游 crate 依赖的库 API，见 `crate::layout` 模块文档同一取舍），
//! 但判定逻辑（判据、状态机、降级兜底）只有一份实现，不是又抄一遍
//! ——这正是本仓库反复点名过的「同一算术在多处手抄」教训
//! （`ll_render::sprite::sprite_draw_position` 模块文档）在这里的应用。

use ll_platform::input::InputState;
use ll_platform::window::FrameId;
use ll_render::anim::{AnimStateMachine, Clip, movement_key_held};

/// 行走动画剪辑在 [`player_clips`] 里的下标。
pub const WALK_CLIP: usize = 0;
/// 待机呼吸动画剪辑在 [`player_clips`] 里的下标。
pub const IDLE_CLIP: usize = 1;

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
/// 取舍。
pub const FALLBACK_SPRITE: &str = "hero_idle_0";

/// 行走动画每帧停留的游戏帧数，取值与
/// `p5_coordinate_acceptance::layout::WALK_FRAMES_PER_STEP` 一致——同一套
/// 图集帧（[`WALK_CYCLE`]），没有理由播放节奏不一样。
const WALK_FRAMES_PER_STEP: u32 = 8;

/// 六帧行走循环的播放顺序：接触 → 过渡 → 过腿 → 接触 → 过渡 → 过腿 →
/// 循环回接触。
///
/// 此前只有 `hero_walk_0`/`hero_walk_1` 两帧接触姿态直接互跳，两帧之间
/// 的像素差异（32/384）已经接近「行走对待机」的差异（48/384），观感
/// 生硬。`hero_walk_2`/`hero_walk_4` 是脚部朝中线过渡但仍贴地的姿态，
/// `hero_walk_3`/`hero_walk_5` 是脚部摆到中线附近、抬离地面 1 像素的
/// 「过腿」姿态（`ll-artgen` 的 `sprite::decorate_hero_walk` 用
/// `passing` 参数区分）——六帧沿这条顺序播放，相邻帧像素差异全部落在
/// 16~26 之间，见 `tools/ll-artgen/src/main.rs` 的
/// `六帧行走循环相邻帧像素差异全部小于两帧方案的直接互跳` 测试，比
/// 原先两帧互跳的 32 更小。`hero_walk_0`/`hero_walk_1` 的像素内容与
/// 扩帧前完全一致（未改动），只是现在有 4 张新的过渡帧穿插播放。
const WALK_CYCLE: [&str; 6] = [
    "hero_walk_0",
    "hero_walk_2",
    "hero_walk_3",
    "hero_walk_1",
    "hero_walk_4",
    "hero_walk_5",
];

/// 待机呼吸动画每帧停留的游戏帧数，取值与
/// `p5_coordinate_acceptance::layout::IDLE_BREATHE_FRAMES_PER_STEP` 一致
/// ——远大于行走的步长，呼吸本就该比迈步慢得多。
const IDLE_BREATHE_FRAMES_PER_STEP: u32 = 40;

/// 构造玩家精灵的行走/待机两段动画剪辑，下标含义见 [`WALK_CLIP`]/
/// [`IDLE_CLIP`]。
///
/// 行走剪辑播放 [`WALK_CYCLE`] 六帧，帧与帧之间是专门画的行走过渡姿态
/// （挪腿 + 抬脚），不再用立姿（[`FALLBACK_SPRITE`]）当过渡帧。
/// 两段剪辑的 `exit_grace_frames` 都填零：本体的行走/待机状态电平驱动
/// （[`update_player_animation`]），不经过 `AnimStateMachine::trigger`/
/// `update` 的「触发式状态+余韵」机制，这个字段从不被读取。
pub fn player_clips() -> Vec<Clip> {
    let walk = Clip {
        // 六帧全是行走过渡姿态，**不掺待机帧**。此前这里先后是「行走
        // 0 → 待机 → 行走 1 → 待机」的四帧循环、又改成只有两张行走图
        // 直接互跳——前者是用立姿当过渡帧，播出来的观感是「走两步停
        // 一下」，项目所有者两次实测都报告「按住 W 时除了 walk 贴图
        // 还会出现 idle 贴图」；后者不再掺待机帧，但两张接触姿态直接
        // 互跳仍然生硬（差异 32/384，接近行走对待机差异的 48/384）。
        // 这次补齐 4 张专门的过渡帧（见 [`WALK_CYCLE`] 文档），解决的
        // 还是同一条所有者要求：「原地站就是 idle 循环，移动就是 walk
        // 循环」，两个循环的帧不该重叠，且循环本身要看起来像在走路。
        frames: WALK_CYCLE.iter().map(|&name| name.to_string()).collect(),
        frames_per_step: WALK_FRAMES_PER_STEP,
        looping: true,
        exit_grace_frames: 0,
    };
    let idle = Clip {
        frames: vec![FALLBACK_SPRITE.to_string(), "hero_idle_1".to_string()],
        frames_per_step: IDLE_BREATHE_FRAMES_PER_STEP,
        looping: true,
        exit_grace_frames: 0,
    };
    vec![walk, idle]
}

/// 每帧无条件调用：按当前是否有任意移动键按住
/// （[`movement_key_held`]）电平驱动地设置玩家该播放的动画状态。
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
pub fn update_player_animation(anim: &mut AnimStateMachine, input: &InputState, frame: FrameId) {
    let target_clip = if movement_key_held(input) {
        WALK_CLIP
    } else {
        IDLE_CLIP
    };
    anim.set_level(target_clip, frame);
}

#[cfg(test)]
mod tests {
    /// 行走循环里混进待机帧会让「按住方向键」时出现站立贴图——这个
    /// 缺陷在 p1/p5/`ll-game` 三处被逐字抄了三遍，项目所有者两次实测
    /// 都报告了，而当时没有任何测试能发现它：既有测试只断言状态机停在
    /// 行走剪辑，从不检查那个剪辑**里装的是什么帧**。本测试补上这一层。
    #[test]
    fn 行走剪辑与待机剪辑的帧不重叠() {
        // Arrange
        let clips = super::player_clips();

        // Act
        let walk: std::collections::BTreeSet<&str> = clips[super::WALK_CLIP]
            .frames
            .iter()
            .map(String::as_str)
            .collect();
        let idle: std::collections::BTreeSet<&str> = clips[super::IDLE_CLIP]
            .frames
            .iter()
            .map(String::as_str)
            .collect();

        // Assert
        assert!(walk.intersection(&idle).next().is_none());
    }

    use ll_platform::input::GameKey;
    use ll_render::anim::{Playback, current_sprite_name};
    use ll_render::atlas::AtlasMetadata;

    use super::*;

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
            update_player_animation(&mut anim, &input, FrameId(frame));
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
        update_player_animation(&mut anim, &input, FrameId(0));
        assert_eq!(anim.active_clip(), WALK_CLIP, "前置条件：先进入行走");

        // Act：松开方向键。
        input.release(GameKey::Up);
        update_player_animation(&mut anim, &input, FrameId(1));

        // Assert
        assert_eq!(anim.active_clip(), IDLE_CLIP);
    }

    #[test]
    fn 未按任何移动键时保持待机() {
        // Arrange
        let mut anim = AnimStateMachine::new(IDLE_CLIP, FrameId(0));
        let input = InputState::new();

        // Act
        update_player_animation(&mut anim, &input, FrameId(0));

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
        update_player_animation(&mut anim, &input, FrameId(0));
        assert_eq!(anim.active_clip(), WALK_CLIP, "前置条件：先进入行走");

        // Act：模拟失焦清空。
        input.clear();
        update_player_animation(&mut anim, &input, FrameId(1));

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
            update_player_animation(&mut anim, &input, FrameId(0));

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
                    { "name": "hero_idle_0",
                      "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
                      "pivot": { "x": 8, "y": 24 },
                      "footprint": { "width": 1, "height": 1 } }
                ]
            }"#,
        )
        .expect("样例是合法 JSON");
        let clips = player_clips();
        let playback = Playback::new(WALK_CLIP, FrameId(0));

        // Act
        let resolved =
            current_sprite_name(&playback, &clips, FrameId(0), &metadata, FALLBACK_SPRITE);

        // Assert
        assert_eq!(resolved, FALLBACK_SPRITE);
    }
}
