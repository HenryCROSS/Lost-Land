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

use ll_platform::input::{GameKey, InputState};
use ll_platform::window::FrameId;

use crate::atlas::AtlasMetadata;

/// 一段动画剪辑：帧序列、播放速度、是否循环。
///
/// 帧用条目名（[`String`]）而非贴图句柄或索引：动画剪辑与图集条目分属
/// 两套独立数据（剪辑描述「按什么顺序播放」，图集描述「这一帧长什么
/// 样」），用名字间接引用让两者可以独立由不同的 mod 资产提供，互不
/// 耦合。
///
/// # 派生 `Clone`/`PartialEq`/`Eq`
///
/// 这是 ADR 0016/0017 第一档「声明静态值」的数据形状本身——`ll-mod`
/// 的 `ClipTable`（把 `register-animation-clip`/本体注册的剪辑声明
/// 物化成 [`AnimStateMachine`]/[`Playback`] 直接消费的平铺表，见其
/// 模块文档）需要按 [`ll_core::ident::ContentIndex`] 下标存一份、按
/// 下标取一份，取值时要么克隆出所有权、要么原样搬进返回的 `Vec`，两条
/// 路都需要 `Clone`；测试断言「这段剪辑长这样」需要 `PartialEq`。三个
/// 字段（`Vec<String>`/`u32`/`bool`）本身都已经是这两个 trait 的
/// 自然持有者，派生不引入任何新的运行期开销。
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// 本体英雄角色行走动画的权威帧序列——见 [`base_hero_clips`] 文档
/// 「为什么这份内容放在机制 crate 里」一节。
pub const HERO_WALK_FRAMES: [&str; 2] = ["hero_walk_0", "hero_walk_1"];
/// 本体英雄角色行走动画每帧停留的游戏帧数。
pub const HERO_WALK_FRAMES_PER_STEP: u32 = 8;
/// 本体英雄角色待机呼吸动画的权威帧序列，见 [`base_hero_clips`]。
pub const HERO_IDLE_FRAMES: [&str; 2] = ["hero_idle_0", "hero_idle_1"];
/// 本体英雄角色待机呼吸动画每帧停留的游戏帧数——远大于行走的步长，
/// 呼吸本就该比迈步慢得多。
pub const HERO_IDLE_FRAMES_PER_STEP: u32 = 40;

/// 构造本体英雄角色的行走/待机两段剪辑：`(行走, 待机)`。
///
/// # 为什么这份内容放在机制 crate 里，而不是内容注册表（`ll-mod`）
///
/// 这份具体的帧名/节奏本质上是**内容**，不是机制——本函数存在的
/// 唯一理由是消掉一处历史 bug：同一份「行走剪辑不该掺待机帧」的
/// 数据此前在 `ll-render` 的 `p1_acceptance`、`ll-sim` 的
/// `p5_coordinate_acceptance`、`ll-game` 三处被逐字抄了三遍，抄三遍
/// 就错三遍（项目所有者两次实测报告过同一个缺陷）。真正面向 mod 开放、
/// 玩家实际读到的权威定义是 `ll_mod::base_clip::register_base_clips`
/// ——它才是「本体即 Mod」这条注册路径上的入口，向脚本层暴露、参与
/// 装载报告、与 mod 自定义剪辑共用同一个 [`ll_core::ident::ContentIndex`]
/// 号段。
///
/// 但 `p1_acceptance`（本 crate 自己的验收 demo）与
/// `p5_coordinate_acceptance`（`ll-sim` 的验收 demo，经既有
/// `dev-dependency` 引用本 crate）两处历史遗留调用点**架构上无法依赖
/// `ll-mod`**——依赖顺序是 `ll-render`/`ll-world` ← `ll-sim` ←
/// `ll-script` ← `ll-mod`（规格 §5），`ll-mod` 反过来已经是 `ll-sim`
/// 的生产依赖方（`crates/ll-mod/Cargo.toml`：P5-B 批次为
/// `SkillCatalog`/`QuestCatalog` trait 实现新增），`ll-sim` 再依赖
/// `ll-mod` 会直接成环。三个消费方里有两个够不着 `ll-mod`，若把这份
/// 数据的唯一定义放在 `ll-mod`，就注定至少要在 `ll-render`/`ll-sim`
/// 里再抄一份——等于什么也没解决。`ll-render` 是三者共同能触达的
/// 最底层 crate（`p1` 是本 crate 自己的例子；`p5` 已经把本 crate列为
/// `dev-dependency`；`ll_mod::base_clip::register_base_clips` 允许
/// 依赖 `ll-render`，因为依赖方向上 `ll-mod` 本就排在 `ll-render` 的
/// 下游），因此本函数是能让三处调用点、外加 `ll-mod` 的本体注册路径
/// 共用同一份 Rust 字面量的唯一落点——不是说机制/内容不该分离，是在
/// 「三个消费方里有两个够不着内容注册表」这条真实的 Cargo 依赖约束下，
/// 唯一能把重复次数从三次收敛到一次的选择。
pub fn base_hero_clips() -> (Clip, Clip) {
    let walk = Clip {
        frames: HERO_WALK_FRAMES.iter().map(|s| s.to_string()).collect(),
        frames_per_step: HERO_WALK_FRAMES_PER_STEP,
        looping: true,
        // 本体的行走/待机状态电平驱动（`AnimStateMachine::set_level`），
        // 不经过 `trigger`/`update` 的「触发+余韵」机制，这个字段在
        // 这两段剪辑上从不被读取。
        exit_grace_frames: 0,
    };
    let idle = Clip {
        frames: HERO_IDLE_FRAMES.iter().map(|s| s.to_string()).collect(),
        frames_per_step: HERO_IDLE_FRAMES_PER_STEP,
        looping: true,
        exit_grace_frames: 0,
    };
    (walk, idle)
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

/// 表现层动画状态机，同时支持两种互不相同的驱动方式：**离散事件+余韵**
/// （[`AnimStateMachine::trigger`] + [`AnimStateMachine::update`]）与
/// **电平驱动**（[`AnimStateMachine::set_level`]）。管理的是状态自己的
/// 生命周期,与哪种驱动方式无关。
///
/// # 两种驱动方式分别解决什么问题
///
/// **离散事件+余韵**：回合制的攻击、施法、受击、死亡这类事件只在结算
/// 的那一帧存在于 `Intent`/`Effect` 流里——下一帧它就不存在了（见
/// `knowledge/design/animation-and-vfx-boundary.md`「结算是瞬时的，
/// 动画只是回放」）。若动画状态直接绑定「本帧有没有这个事件」，状态会
/// 在事件消失的下一帧立刻回落到默认状态；这类事件本身没有「按住」这个
/// 概念（不像方向键，攻击键通常也是点按），[`AnimStateMachine::trigger`]
/// 让每个状态维持 [`Clip::exit_grace_frames`] 声明的余韵帧数，期间收到
/// 同状态新事件只续期、不重建播放。
///
/// **电平驱动**：移动这类由物理按键「按住/松开」两个边缘事件界定的
/// 连续状态，更直接的判据是**按键当前是否仍按住**（见
/// `ll_platform::input::InputState::is_held`），而不是「本帧有没有产生
/// 一次移动意图」——后者只在按下与自动重复脉冲的帧为真，脉冲之间的
/// 空档若拿「事件+余韵」硬凑，余韵长度需要盖过自动重复的初始延迟
/// （`RepeatConfig::default` 的 `initial_delay`，350ms，约 21 帧于
/// 60fps）与后续间隔（90ms，约 5.4 帧）两者之中较长的一个，否则连续
/// 按住时仍会在初次触发后的第一段空档里露出一帧默认状态；调到能盖住
/// 又会让松开后的停止拖上小半秒。按住/松开是边缘事件，两者
/// 之间的「按住」本身连续，没有脉冲间隙需要靠超时容忍——
/// [`AnimStateMachine::set_level`] 把每帧「现在该处于哪个状态」的判断
/// 完全交给调用方，本类型只负责「状态没变就不重建播放」这一件事。
///
/// # 不假设只有两态，也不假设只有一种驱动方式
///
/// 状态本身就是调用方 `Clip` 表里的下标，不是写死的「行走/待机」二
/// 态枚举；调用方也不需要为每个状态统一选择同一种驱动方式——移动这类
/// 有「按住」语义的状态用 `set_level`，攻击/施法/受击/死亡这类纯粹由
/// 结算事件界定的一次性状态继续用 `trigger`+`update`，两条路径可以在
/// 同一个 `AnimStateMachine` 实例上交替使用（`set_level` 不读也不写
/// `expires_at` 以外的字段，见其文档）。这也是同一套机制能同时服务
/// 玩家（有键盘可以「按住」）与 NPC（没有键盘，只有 AI 决策出的「持续
/// 移动意图」标志，或攻击这类离散动作）的原因：两者都只是「谁在调用
/// `set_level`/`trigger`」的区别，本类型内部不区分调用者是谁。
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

    /// 电平驱动地设置当前活跃状态：`clip` 就是这一帧该处于的状态，不
    /// 存在「过期」这个概念——调用方（例如「移动方向键是否按住」）应当
    /// **每帧无条件调用**，而不是像 [`AnimStateMachine::trigger`] 那样
    /// 只在事件到达的帧调用、再配合 [`AnimStateMachine::update`] 老化。
    ///
    /// 与 `trigger` 的关键区别：`trigger` 只在离散事件到达的那一帧被
    /// 调用，状态在两次事件之间靠 [`Clip::exit_grace_frames`] 声明的
    /// 余韵维持；`set_level` 每帧都被调用，「现在该处于哪个状态」这件
    /// 事本身已经由调用方每帧算好，不需要余韵去猜下一次调用何时到达，
    /// 也就不读 `clips`（不需要查 `exit_grace_frames`）。
    ///
    /// - `clip` 与当前活跃剪辑相同：不重建 `playback`，继续播放——与
    ///   `trigger` 同样的理由，剪辑仍从原来的起始帧连续播放，不会跳回
    ///   第一帧。
    /// - `clip` 与当前不同：从 `now` 重新起播新剪辑。
    ///
    /// 调用后 `expires_at` 恒为 [`None`]：电平驱动的状态没有「超时回落」
    /// 这件事，`update` 对它是无操作（`update` 只在 `expires_at` 为
    /// `Some` 时才可能回落，见其文档）。若同一个实例此前用 `trigger`
    /// 进入过某个状态，改调 `set_level` 会立刻清掉遗留的 `expires_at`
    /// ——这是刻意的：电平驱动接管之后，调用方自己的判断就是唯一权威,
    /// 不应该再被上一次 `trigger` 留下的余韵计时悄悄打断。
    pub fn set_level(&mut self, clip: usize, now: FrameId) {
        if clip != self.current_clip {
            self.current_clip = clip;
            self.playback = Playback::new(clip, now);
        }
        self.expires_at = None;
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

/// 是否有任意一个移动方向键当前处于按住状态。
///
/// 供 [`AnimStateMachine::set_level`] 的电平驱动调用方判断「这一帧该不
/// 该播行走动画」——按下/松开是边缘事件，两者之间「按住」本身连续，
/// 不存在脉冲间隙需要余韵去容忍，直接查 [`InputState::is_held`] 即可，
/// 详细论证见 [`AnimStateMachine`] 模块文档「电平驱动」一节。
///
/// 本函数原是 `ll-sim` 的 `p5_coordinate_acceptance` demo 里的
/// `player_is_moving`，随「游戏本体二进制也要接线同一套移动驱动动画」
/// 一起提到这里——两处调用方（P5 demo 与本体二进制 `ll-game`）需要
/// 完全相同的判据，重复实现一份没有理由，见本仓库「同一算术在多处
/// 手抄」教训（`ll_render::sprite::sprite_draw_position` 模块文档）。
///
/// # 留给未来「行动能力」过滤的接入点
///
/// 目前的判定就是「是否有任意一个移动方向键处于按住状态」——本仓库此
/// 刻没有任何会阻断移动的机制（眩晕/定身零实现，物品/背包系统排在
/// 未来阶段），因此「按键是否按住」与「是否正在执行移动动作」在结果
/// 上完全等价。这不是巧合：本函数刻意按后者的语义写文档——将来输入
/// 上下文/行动能力/动画状态三层抽象落地后，应当只需要在本函数最前面
/// 插入一次「当前行动能力是否允许移动」查询、查询为否时提前返回
/// `false`，不需要改动任何调用方一行代码。
pub fn movement_key_held(input: &InputState) -> bool {
    input.is_held(GameKey::Up)
        || input.is_held(GameKey::Down)
        || input.is_held(GameKey::Left)
        || input.is_held(GameKey::Right)
}

/// 在图集元数据里核实 `frame_name` 是否存在，不存在则退回 `fallback`。
///
/// 动画剪辑引用的帧名最终来自可被 mod 覆盖的资产，属于外部不可信输入
/// （见模块文档「降级而非崩溃」）：mod 只提供部分帧是完全正常的情况，
/// 不应该因此报错或让精灵消失，这里只做最朴素的「存在性探测 + 兜底
/// 换名」，不引入除字符串查找之外的机制。
pub fn resolve_sprite_name<'a>(
    metadata: &AtlasMetadata,
    frame_name: &'a str,
    fallback: &'a str,
) -> &'a str {
    if metadata.lookup(frame_name).is_some() {
        frame_name
    } else {
        fallback
    }
}

/// 算出 `frame` 这一帧应显示的图集条目名，两层兜底叠加：
///
/// 1. [`Playback::current_frame`] 对损坏的剪辑数据（空剪辑、剪辑下标
///    越界，见模块文档「降级而非崩溃」）返回 [`None`]，这里退回
///    `fallback`。
/// 2. 就算剪辑给出了一个帧名，那一帧也可能不在图集里（mod 只提供部分
///    帧是正常情况），再用 [`resolve_sprite_name`] 确认。
///
/// 调用方应当传入自己那唯一「必须存在」的一帧作为 `fallback`（例如
/// `hero_idle_0`）——两层兜底都退回同一帧，这也是「行走/待机动画都是
/// 可选的，都缺失时退回单张静态图」这条产品要求在代码里唯一的落点：
/// 调用方完全不需要为「两段动画都被 mod 移除」这种情况另写特殊分支，
/// 缺帧时 `current_frame` 与 `resolve_sprite_name` 会一路降级到同一个
/// `fallback`。
pub fn current_sprite_name<'a>(
    playback: &Playback,
    clips: &'a [Clip],
    frame: FrameId,
    metadata: &AtlasMetadata,
    fallback: &'a str,
) -> &'a str {
    let raw = playback.current_frame(clips, frame).unwrap_or(fallback);
    resolve_sprite_name(metadata, raw, fallback)
}

#[cfg(test)]
mod driving_and_fallback_tests {
    use super::*;

    fn sample_metadata() -> AtlasMetadata {
        AtlasMetadata::parse(
            r#"{
                "image": "placeholder.png",
                "entries": [
                    { "name": "known_frame",
                      "rect": { "x": 0, "y": 0, "width": 16, "height": 24 },
                      "pivot": { "x": 8, "y": 24 },
                      "footprint": { "width": 1, "height": 1 } }
                ]
            }"#,
        )
        .expect("样例是合法 JSON")
    }

    #[test]
    fn 四个方向键任意按住都判定为正在移动() {
        for key in [GameKey::Up, GameKey::Down, GameKey::Left, GameKey::Right] {
            // Arrange
            let mut input = InputState::new();
            input.press(key);

            // Act & Assert
            assert!(movement_key_held(&input), "方向键 {key:?} 应判定为正在移动");
        }
    }

    #[test]
    fn 没有任何方向键按住时判定为未在移动() {
        // Arrange
        let input = InputState::new();

        // Act & Assert
        assert!(!movement_key_held(&input));
    }

    #[test]
    fn 帧名在图集里存在时按原样使用() {
        // Arrange
        let metadata = sample_metadata();

        // Act
        let resolved = resolve_sprite_name(&metadata, "known_frame", "fallback_frame");

        // Assert
        assert_eq!(resolved, "known_frame");
    }

    #[test]
    fn 帧名在图集里缺失时退回兜底帧() {
        // 模拟 mod 覆盖图集后没有提供某一帧——完全正常的情况，必须退回
        // 兜底帧，而不是画出空白或让调用方 panic。
        // Arrange
        let metadata = sample_metadata();

        // Act
        let resolved = resolve_sprite_name(&metadata, "missing_from_mod", "fallback_frame");

        // Assert
        assert_eq!(resolved, "fallback_frame");
    }

    #[test]
    fn 剪辑数据损坏时退回兜底帧而不是崩溃() {
        // 模拟一段损坏的动画数据：Playback 引用的剪辑下标越界（例如 mod
        // 打包时漏掉了某段剪辑定义）——current_frame 对此返回 None，这里
        // 锁住「调用方在此基础上还能优雅退回静态图」这一步。
        // Arrange
        let metadata = sample_metadata();
        let clips = vec![Clip {
            frames: vec!["known_frame".to_string()],
            frames_per_step: 5,
            looping: true,
            exit_grace_frames: 0,
        }];
        let corrupted_playback = Playback::new(99, FrameId(0));

        // Act
        let resolved = current_sprite_name(
            &corrupted_playback,
            &clips,
            FrameId(0),
            &metadata,
            "fallback_frame",
        );

        // Assert
        assert_eq!(resolved, "fallback_frame");
    }

    #[test]
    fn 剪辑数据完好时按剪辑当前帧显示() {
        // Arrange
        let metadata = sample_metadata();
        let clips = vec![Clip {
            frames: vec!["known_frame".to_string()],
            frames_per_step: 5,
            looping: true,
            exit_grace_frames: 0,
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act
        let resolved =
            current_sprite_name(&playback, &clips, FrameId(0), &metadata, "fallback_frame");

        // Assert
        assert_eq!(resolved, "known_frame");
    }

    #[test]
    fn 两段动画都被剪辑数据排除时退回同一兜底帧() {
        // 项目所有者要求：走路和待机动画都是可选的，都没有就退回单张
        // 静态图——用一段只引用图集里不存在的帧的剪辑模拟"mod 移除了
        // 全部可选动画帧"，两层兜底应当一路降级到同一个 fallback。
        // Arrange
        let metadata = sample_metadata();
        let clips = vec![Clip {
            frames: vec!["hero_walk_removed_by_mod".to_string()],
            frames_per_step: 5,
            looping: true,
            exit_grace_frames: 0,
        }];
        let playback = Playback::new(0, FrameId(0));

        // Act
        let resolved =
            current_sprite_name(&playback, &clips, FrameId(0), &metadata, "fallback_frame");

        // Assert
        assert_eq!(resolved, "fallback_frame");
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
    fn 本体行走剪辑与待机剪辑的帧不重叠() {
        // 行走循环里混进待机帧会让「按住方向键」时出现站立贴图——这个
        // 缺陷曾在 p1_acceptance/p5_coordinate_acceptance/ll-game 三处
        // 被逐字抄了三遍（项目所有者两次实测报告），三处现在都改成调用
        // `base_hero_clips`，本测试锁住这唯一一份数据不再犯同一个错。
        //
        // 这条约束**只对这两段本体剪辑成立**，不是 `ClipTable::define`/
        // `register-animation-clip` 的通用校验规则——mod 可以自由定义
        // 帧有重叠的剪辑（例如刻意复用某几帧表达"半个循环"），见
        // `ll-mod::clip` 模块文档「行走/待机不重叠是断言，不是校验」
        // 一节。
        // Arrange
        let (walk, idle) = super::base_hero_clips();

        // Act
        let walk_frames: std::collections::BTreeSet<&str> =
            walk.frames.iter().map(String::as_str).collect();
        let idle_frames: std::collections::BTreeSet<&str> =
            idle.frames.iter().map(String::as_str).collect();

        // Assert
        assert!(walk_frames.intersection(&idle_frames).next().is_none());
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

#[cfg(test)]
mod set_level_tests {
    use super::*;

    /// 与 `anim_state_machine_tests::clips` 同一套形状（行走 3 帧循环、
    /// 每步 5 帧；待机单帧），独立定义一份而不是跨 `mod` 复用私有测试
    /// 辅助函数——理由同 `anim_state_machine_tests::clips` 文档。
    /// `exit_grace_frames` 全填零：`set_level` 从不读取这个字段（见其
    /// 文档），非零值只会误导读者以为它参与了判断。
    fn clips() -> Vec<Clip> {
        vec![
            Clip {
                frames: vec!["w0".into(), "w1".into(), "w2".into()],
                frames_per_step: 5,
                looping: true,
                exit_grace_frames: 0,
            },
            Clip {
                frames: vec!["idle".into()],
                frames_per_step: 1,
                looping: true,
                exit_grace_frames: 0,
            },
        ]
    }

    const 待机: usize = 1;
    const 行走: usize = 0;

    #[test]
    fn 电平驱动连续多帧调用同一状态全程不回弹到默认状态() {
        // 这正是项目所有者报告的缺陷的回归测试：移动状态里不应该掺入
        // idle 贴图。旧的「触发+余韵」方案靠余韵去覆盖自动重复脉冲之间
        // 的空档，余韵长度总有算不准的时候；`set_level` 从根上不存在
        // 这类空档——调用方（按住状态）每帧都调用，这里逐帧模拟「每帧
        // 都在按住方向键」，覆盖的帧跨度（0..=40）明显超过旧方案 12 帧
        // 的余韵、也超过按键自动重复的初始延迟（约 21 帧于 60fps），
        // 全程状态必须停在「行走」，一帧都不例外。
        // Arrange
        let mut machine = AnimStateMachine::new(待机, FrameId(0));

        // Act & Assert
        for frame in 0..=40u64 {
            machine.set_level(行走, FrameId(frame));
            assert_eq!(machine.active_clip(), 行走, "第 {frame} 帧不应回弹到待机");
        }
    }

    #[test]
    fn 电平驱动连续调用同一状态时不重建播放进度() {
        // 若每帧调用 `set_level` 都错误地重建 `Playback`，t=6 时的帧
        // 计算会把本帧自己当成起播点，算出第 0 步（w0）而不是延续自
        // t=0 起播、已经过去一步的 w1——与
        // `anim_state_machine_tests::续期时不重建播放进度` 同一个底层
        // 关切，这里验证 `set_level` 路径同样成立。
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));

        // Act：逐帧调用，模拟连续按住。
        for frame in 0..=6u64 {
            machine.set_level(行走, FrameId(frame));
        }
        let frame = machine.playback().current_frame(&clips, FrameId(6));

        // Assert：`frames_per_step` 是 5，从 t=0 算到 t=6 应该是第 1
        // 步（w1）。
        assert_eq!(frame, Some("w1"));
    }

    #[test]
    fn 电平驱动切换状态时从新剪辑起始帧重新起播() {
        // Arrange
        let clips = clips();
        let mut machine = AnimStateMachine::new(待机, FrameId(0));
        for frame in 0..=4u64 {
            machine.set_level(行走, FrameId(frame)); // 行走播到中途
        }

        // Act：松开方向键，切回待机。
        machine.set_level(待机, FrameId(5));

        // Assert
        assert_eq!(machine.active_clip(), 待机);
        assert_eq!(
            machine.playback().current_frame(&clips, FrameId(5)),
            Some("idle")
        );
    }

    #[test]
    fn 松开后立即切回默认状态不拖延() {
        // 项目所有者明确要求「停下时要及时切回 idle，不能拖沓」——
        // `set_level` 没有余韵机制，切换态即时生效，这里验证切换当帧
        // 就能观察到默认状态，不需要再等任何帧数。
        // Arrange
        let mut machine = AnimStateMachine::new(待机, FrameId(0));
        machine.set_level(行走, FrameId(0));
        assert_eq!(machine.active_clip(), 行走, "前置条件：先进入行走状态");

        // Act：同一帧内松开方向键。
        machine.set_level(待机, FrameId(1));

        // Assert
        assert_eq!(machine.active_clip(), 待机);
    }

    #[test]
    fn 电平驱动后expires_at恒为空闲置字段() {
        // `set_level` 文档承诺调用后 `expires_at` 恒为 `None`——这里
        // 通过行为间接验证：即使此前用 `trigger` 进入过一个还没过期、
        // 带正数余韵的触发式状态，改调 `set_level` 之后再调用 `update`
        // （用远超那次遗留余韵的帧号）也不应该有任何可观察的状态变化，
        // 因为 `set_level` 已经清空了遗留的过期计时。
        // Arrange：本测试专用一份带非零余韵的剪辑表，与本模块其余测试
        // 共用的 `clips()`（余韵全零，见其文档）区分开——余韵为零会让
        // `trigger` 遗留的 `expires_at` 从一开始就已经过期，测不出
        // 「set_level 主动清空了它」与「它本来就已经过期」的差别。
        let grace_clips = vec![
            Clip {
                frames: vec!["w0".into()],
                frames_per_step: 5,
                looping: true,
                exit_grace_frames: 5,
            },
            Clip {
                frames: vec!["idle".into()],
                frames_per_step: 1,
                looping: true,
                exit_grace_frames: 0,
            },
        ];
        let mut machine = AnimStateMachine::new(待机, FrameId(0));
        machine.trigger(&grace_clips, 行走, FrameId(0)); // 到期于第 5 帧，此刻还没过期
        machine.set_level(行走, FrameId(1)); // 电平驱动接管

        // Act：`update` 用远超那次遗留余韵到期帧（5）的帧号。
        machine.update(FrameId(1000));

        // Assert：`update` 对电平驱动的状态是无操作，仍停在行走——若
        // `set_level` 没有清空遗留的 `expires_at`，这里会错误地回落
        // 到待机。
        assert_eq!(machine.active_clip(), 行走);
    }
}
