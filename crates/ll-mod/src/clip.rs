//! 动画剪辑注册表——「本体即 Mod」在动画上的落点，把 `crates/ll-render`
//! `Clip`/`Playback`/`AnimStateMachine` 已经交付的**机制**之上，此前
//! 一直缺失的**内容**声明补成可注册的表。
//!
//! # 起因：一个刚修完的真实 bug 暴露的架构问题
//!
//! 行走剪辑的帧列表曾是「走姿0 → 立姿 → 走姿1 → 立姿」，按住方向键时
//! 会掺出待机贴图；项目所有者两次实测报告，修复本身很简单（拿掉立姿
//! 过渡帧），但这份数据同时在 `ll-render`（`p1_acceptance`）、`ll-sim`
//! （`p5_coordinate_acceptance`）、`ll-game` 三处被逐字抄了三遍——抄
//! 三遍就能错三遍，写死在 Rust 里没有单一真相来源。项目所有者的判断：
//! 「这为何是用 rust 写的，这应该是一个更灵活的可配置文件或者是 steel
//! 脚本才对」，这正是本模块存在的理由。
//!
//! # 照抄 `class.rs`/`race.rs` 已验证的模式
//!
//! `ClipDef` 与 `ClassDef`/`RaceDef` 同一个理由直接落在 `ll-mod`：
//! 剪辑声明本身不依赖任何「世界空间」概念（不像地形/层属性要跟
//! `ChunkGrid`/`Space` 打交道），没有必要为它单独在 `ll-world` 开一个
//! 定义、`ll-mod` 只做薄封装的两层结构。
//!
//! # 与另外六种内容类型的一处结构性差异：直接复用 `ll_render::anim::Clip`
//!
//! 另外六个 `*Table`（`TerrainTable`/`ClassTable`/……）都各自定义一个
//! 独立的 `*Attrs` 输入类型，与它们各自的运行期消费者（例如
//! `TerrainKind` 的查询方法）没有共享的 Rust 类型。动画剪辑不一样：
//! [`ll_render::anim::Playback`]/[`ll_render::anim::AnimStateMachine`]
//! 早已把「一段剪辑」的运行期形状定成 [`ll_render::anim::Clip`]（帧
//! 序列、步长、是否循环、退出余韵四个字段，见其文档），这个形状与
//! 本注册表想要表达的「一条剪辑声明」**完全同构**——没有理由再定义
//! 一个字段一模一样的 `ClipAttrs` 只是为了「看起来跟其他六个一样有
//! 一个独立的 Attrs 类型」。[`ClipTable`] 因此直接以
//! [`ll_render::anim::Clip`] 作为 [`ClipTable::define`] 的输入/
//! [`ClipTable::get`] 的输出类型，注册期物化出来的表本身就是
//! [`ClipTable::as_clips`] 直接可用的 `&[Clip]`——不需要额外的转换
//! 步骤，这正是 ADR 0016/0017 第一档「声明静态值 → 注册期物化 → 运行期
//! 只查表」在这类内容上最贴合的落法：`Clip` 既是声明的形状，也是运行期
//! 查表的形状，物化前后是同一个类型。
//!
//! # `exit_grace_frames` 暴露给脚本：结论与理由
//!
//! **暴露**。[`ll_render::anim::Clip::exit_grace_frames`] 是「状态退出
//! 前的余韵」——只有交给 [`ll_render::anim::AnimStateMachine::trigger`]/
//! [`ll_render::anim::AnimStateMachine::update`] 管理的触发式状态动画
//! 才会读它（攻击、施法、受击这类回合结算里的离散事件）。若不把这个
//! 字段暴露给 `register-animation-clip`，mod 作者能声明的触发式剪辑
//! 就只能被迫恒为零余韵，而零余韵正是本仓库刚修过的那个 bug 的另一种
//! 面孔——连续触发的间隔一旦跟不上零余韵的严格要求就会闪烁。这个字段
//! 本身就是「项目所有者要求的可自定义动画延迟」（见 `Clip` 文档），
//! 不开放给内容作者，等于自己造了一个「本体做得到、mod 做不到」的
//! 缺口，正是 ADR 0016 守门规则要拦住的那类情形。本体的行走/待机两段
//! 剪辑走电平驱动（`set_level`），从不读这个字段，填零即可——参见
//! [`ll_render::anim::base_hero_clips`]。
//!
//! # 行走/待机不重叠是断言，不是校验
//!
//! `ll-game` 曾经的 bug 是「行走剪辑里混进了待机帧」。修复落地后有人
//! 可能会问：`ClipTable::define`/`register-animation-clip` 要不要顺手
//! 加一条「同一次装载会话里任意两段剪辑不得共享帧名」的注册期校验？
//! **不要**——这条约束只对本体的行走/待机两段剪辑成立，是产品要求
//! （「原地站就是 idle 循环，移动就是 walk 循环，两个循环的帧不该
//! 重叠」），不是动画剪辑这类内容与生俱来的不变式。mod 作者完全可能有
//! 正当理由让两段剪辑共享几帧（例如「警戒」剪辑复用「待机」剪辑最后
//! 一帧当起手姿势），若把这条产品偏好做成引擎硬校验，会平白挡掉这些
//! 合法用法——这正是 ADR 0015「注册校验是解析，不是强加内容作者不需要
//! 的不变式」一贯的分寸。这条约束因此只体现为一条锁定
//! [`ll_render::anim::base_hero_clips`] 输出的回归测试（见其模块内
//! 测试），不体现为本模块任何一处 `Err` 分支。

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_render::anim::Clip;
use std::fmt;

/// 剪辑注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误
/// 在加载时就报出来，而不是等到某个实体真的播放到这段剪辑时才表现成
/// 「画面卡在首帧」这种令人费解的运行期行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipError {
    /// 同一个内容索引被定义了两次，理由同 [`crate::race::RaceError`]。
    DuplicateDefinition(ContentIndex),
    /// 剪辑没有任何帧——[`ll_render::anim::Playback::current_frame`]
    /// 对这种情形优雅降级返回 `None`（见其文档「降级而非崩溃」，
    /// 那条防线是为了兜底*运行期*才出现的损坏数据，例如引用了一个
    /// 越界的剪辑下标），但一条**从未想过要有任何帧**的声明本身就是
    /// 内容作者的笔误：零帧的剪辑播出来是「什么都不画」，没有任何
    /// 场景会主动希望注册出这样一条内容。在注册期就拒绝它，比让它
    /// 静默通过、直到有人真的触发这段剪辑才发现"这里什么都没画"要
    /// 诚实得多。
    EmptyFrames(ContentIndex),
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipError::DuplicateDefinition(index) => {
                write!(f, "剪辑索引 {} 被重复定义", index.get())
            }
            ClipError::EmptyFrames(index) => {
                write!(f, "剪辑索引 {} 声明了零帧，这是内容作者的笔误", index.get())
            }
        }
    }
}

impl std::error::Error for ClipError {}

/// 一段空白占位剪辑——[`ClipTable::define`] 在扩容未定义槽位时使用，
/// 理由同 `RaceTable::define` 扩容时填 `ZERO_STAT_MODIFIERS`：未定义
/// 的槽位永远被 `defined` 位图挡住，不会被外部查询实际读到；即便有
/// 调用方绕过 `defined` 直接读 [`ClipTable::as_clips`] 拿到这个占位值，
/// [`ll_render::anim::Playback::current_frame`] 对空帧列表本就有既定的
/// 「返回 `None`，不崩溃」降级路径（见其文档）。
fn empty_clip() -> Clip {
    Clip {
        frames: Vec::new(),
        frames_per_step: 0,
        looping: false,
        exit_grace_frames: 0,
    }
}

/// 动画剪辑的列式存储：按 [`ContentIndex`] 下标索引，与
/// [`crate::race::RaceTable`] 同一套「全局号段、defined 位图」道理
/// （见其文档）——地形、职业、种族、剪辑共享同一个 `Interner`/
/// `Registry`。
///
/// 与另外六张表的结构性差异（直接存 [`Clip`] 而非独立的 `*Attrs`
/// 类型）见模块文档。
#[derive(Debug, Default, Clone)]
pub struct ClipTable {
    clips: Vec<Clip>,
    defined: Vec<bool>,
}

impl ClipTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上一段剪辑。
    ///
    /// 校验：不得重复定义（同 [`crate::race::RaceTable::define`]）；
    /// 帧列表不得为空（见 [`ClipError::EmptyFrames`] 文档）。
    /// `frames_per_step`/`looping`/`exit_grace_frames` 不做额外校验
    /// ——`frames_per_step` 为零、剪辑单帧循环这些形状都有各自合法的
    /// 用途（见 `ll_render::anim::Playback::current_frame` 文档「步长
    /// 为零时……恒定停在首帧」，这是刻意支持的降级/静态图表达，不是
    /// 需要在注册期拒绝的错误组合）。
    pub fn define(&mut self, index: ContentIndex, clip: Clip) -> Result<(), ClipError> {
        if clip.frames.is_empty() {
            return Err(ClipError::EmptyFrames(index));
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.clips.resize_with(new_len, empty_clip);
        }

        if self.defined[idx] {
            return Err(ClipError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.clips[idx] = clip;
        Ok(())
    }

    /// 给定的剪辑索引当前是否已经登记过。
    pub fn is_defined(&self, clip: ContentIndex) -> bool {
        self.defined
            .get(clip.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 查询一段剪辑，未注册的索引返回 `None`（对齐 ADR 0015 的解析
    /// 纪律，同 [`crate::race::RaceTable::get`]）。
    pub fn get(&self, clip: ContentIndex) -> Option<&Clip> {
        if !self.is_defined(clip) {
            return None;
        }
        self.clips.get(clip.get() as usize)
    }

    /// 物化成 [`ll_render::anim::Playback::current_frame`]/
    /// [`ll_render::anim::AnimStateMachine`] 直接消费的平铺切片。
    ///
    /// 这一步是 ADR 0016/0017 第一档「注册期物化，运行期零脚本参与」
    /// 在动画剪辑上的落点，但因为 [`ClipTable`] 内部本就直接存
    /// [`Clip`]（见模块文档「与另外六种内容类型的一处结构性差异」），
    /// 这里不需要做任何转换——只是把内部表示原样借出成切片。未定义的
    /// 槽位是 [`empty_clip`]，`Playback` 对空帧剪辑的既有降级路径
    /// （见其文档）保证即使某个下标恰好落在未定义的槽位上也不会
    /// panic，只会表现成「这一帧无画面，退回调用方的兜底帧」。
    pub fn as_clips(&self) -> &[Clip] {
        &self.clips
    }
}

/// 本体英雄角色剪辑在当前注册表里的索引缓存。
#[derive(Debug, Clone, Copy)]
pub struct BaseClipIds {
    /// 行走剪辑：`lostland:hero_walk`。
    pub hero_walk: ContentIndex,
    /// 待机呼吸剪辑：`lostland:hero_idle`。
    pub hero_idle: ContentIndex,
}

/// 本体剪辑注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调，理由同
/// [`ll_world::terrain::materialize_base_terrain`] 文档；帧数据本身来自
/// [`ll_render::anim::base_hero_clips`]（唯一权威定义，见其文档「为
/// 什么这份内容放在机制 crate 里」）。
pub fn materialize_base_clips(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseClipIds, ClipTable), ClipError> {
    let mut table = ClipTable::new();
    let (walk, idle) = ll_render::anim::base_hero_clips();

    let hero_walk = define_base(&mut table, intern, "lostland:hero_walk", walk)?;
    let hero_idle = define_base(&mut table, intern, "lostland:hero_idle", idle)?;

    Ok((
        BaseClipIds {
            hero_walk,
            hero_idle,
        },
        table,
    ))
}

/// [`materialize_base_clips`] 的内部帮手：intern 一个字面量命名空间 ID
/// 并写入 `clip`，理由同 `race.rs`/`class.rs` 同名帮手（固定字面量
/// 恒合法，`expect` 不是防御性代码，是「这个 panic 只可能在本体自己的
/// 声明表写错时触发，理应在开发期就暴露」的既定纪律）。
fn define_base(
    table: &mut ClipTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    clip: Clip,
) -> Result<ContentIndex, ClipError> {
    let index = intern(NamespacedId::parse(id).expect("本体剪辑 id 字面量恒合法"));
    table.define(index, clip)?;
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::Interner;

    fn sample_clip() -> Clip {
        Clip {
            frames: vec!["a".to_string(), "b".to_string()],
            frames_per_step: 4,
            looping: true,
            exit_grace_frames: 0,
        }
    }

    /// 造一个可用的 `ContentIndex`——`ContentIndex` 只能经
    /// `Interner::intern` 分配，没有公开的裸下标构造函数（见
    /// `ll_core::ident` 模块文档「不变式」一节），测试用一个独立的
    /// `Interner` 实例造出可控数量的索引。
    fn fresh_index(interner: &mut Interner, namespace_suffix: &str) -> ContentIndex {
        interner
            .intern(NamespacedId::parse(&format!("test:{namespace_suffix}")).expect("合法标识符"))
    }

    #[test]
    fn 合法剪辑注册成功并可查回原值() {
        // Arrange
        let mut interner = Interner::new();
        let mut table = ClipTable::new();
        let index = fresh_index(&mut interner, "a");

        // Act
        let result = table.define(index, sample_clip());

        // Assert
        assert_eq!(result, Ok(()));
        assert_eq!(table.get(index), Some(&sample_clip()));
    }

    #[test]
    fn 重复定义同一索引返回错误() {
        // Arrange
        let mut interner = Interner::new();
        let mut table = ClipTable::new();
        let index = fresh_index(&mut interner, "a");
        table.define(index, sample_clip()).expect("首次定义应成功");

        // Act
        let result = table.define(index, sample_clip());

        // Assert
        assert_eq!(result, Err(ClipError::DuplicateDefinition(index)));
    }

    #[test]
    fn 空帧列表在注册期就被拒绝() {
        // Arrange
        let mut interner = Interner::new();
        let mut table = ClipTable::new();
        let index = fresh_index(&mut interner, "a");
        let empty = Clip {
            frames: Vec::new(),
            ..sample_clip()
        };

        // Act
        let result = table.define(index, empty);

        // Assert
        assert_eq!(result, Err(ClipError::EmptyFrames(index)));
    }

    #[test]
    fn 未注册索引查询返回空值() {
        // Arrange
        let mut interner = Interner::new();
        let table = ClipTable::new();
        let index = fresh_index(&mut interner, "a");

        // Act & Assert
        assert_eq!(table.get(index), None);
    }

    #[test]
    fn 物化后的切片下标与contentindex一一对应() {
        // Arrange：先 intern 两个不相干的占位标识符把号段推进到 2，
        // 再定义第三个——验证 as_clips 返回的切片长度覆盖到该下标，
        // 且该下标处确实是刚注册的剪辑。
        let mut interner = Interner::new();
        fresh_index(&mut interner, "gap0");
        fresh_index(&mut interner, "gap1");
        let index = fresh_index(&mut interner, "real");
        let mut table = ClipTable::new();
        table.define(index, sample_clip()).expect("注册应成功");

        // Act
        let clips = table.as_clips();

        // Assert
        assert_eq!(clips.len(), 3);
        assert_eq!(clips[index.get() as usize], sample_clip());
    }

    #[test]
    fn 未定义槽位物化后是空剪辑而不panic() {
        // 模拟其它内容类型（如地形）先把全局 ContentIndex 号段推进到
        // 较高的下标，本表在这些下标上从未 define 过——as_clips 仍要
        // 能安全地把这些槽位借出成切片，不能索引越界。
        // Arrange
        let mut interner = Interner::new();
        fresh_index(&mut interner, "gap0");
        fresh_index(&mut interner, "gap1");
        fresh_index(&mut interner, "gap2");
        let index = fresh_index(&mut interner, "real");
        let mut table = ClipTable::new();
        table.define(index, sample_clip()).expect("注册应成功");

        // Act
        let clips = table.as_clips();

        // Assert：下标 0..3 都是从未 define 过的占位空剪辑。
        for gap in &clips[..3] {
            assert!(gap.frames.is_empty());
        }
    }

    #[test]
    fn 本体两段剪辑注册后索引不同() {
        // Arrange & Act
        let mut interner = Interner::new();
        let (ids, table) =
            materialize_base_clips(&mut |id| interner.intern(id)).expect("本体剪辑声明表内部一致");

        // Assert：只验证两个索引不同——具体数值由 `intern` 回调决定，
        // 生产路径见 crate::base_clip 的「共用同一段号段」测试。
        assert_ne!(ids.hero_walk, ids.hero_idle);
        assert!(table.get(ids.hero_walk).is_some());
        assert!(table.get(ids.hero_idle).is_some());
    }
}
