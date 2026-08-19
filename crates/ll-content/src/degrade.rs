//! 缺失 mod 降级策略：按内容类型分级，外加只读模式。
//!
//! 规格 §10.4：缺失 mod 不得崩溃。但「不崩溃」是底线，不是处理策略——
//! `knowledge/design/identity-and-ids.md` 六、②给出的分级表才是策略：
//!
//! | 缺失的东西 | 合理处理 |
//! |---|---|
//! | 某个物品类型 | 丢弃该物品并提示 |
//! | 某个 NPC 的种族/职业 | 降级为占位 |
//! | **玩家角色的种族/职业** | **不能降级——玩家会失去自己的角色** |
//! | 世界生成用的地形生成器 | 世界已生成完毕，影响有限 |
//!
//! 本模块把这张表落到 [`decide_degrade_action`]，并交付「只读模式」
//! 这一档：撞上「不可降级」时不直接拒绝打开存档，而是允许查看/导出、
//! 不能继续游玩（[`ReadOnlySave`]）——好过「要么丢数据要么打不开」的
//! 二选一。
//!
//! # 「谁是玩家」如何界定
//!
//! 本模块需要区分「这条记录属于玩家角色」还是「属于某个 NPC」
//! （[`OwnerContext`]）。核实过 `WorldState` 此前没有官方记录点——三个
//! 既有验收 demo 都是各自在应用层用局部变量记住玩家的 `EntityId`，这
//! 等于把「谁是玩家」排除在存档之外。裁定 P5-3 已经定案：`WorldState`
//! 补一个显式的 `player_entity: Option<EntityId>` 字段（见
//! `ll_world::state::WorldState::player_entity` 文档），本模块的调用方
//! （未来任务 9 的读档管线）应该拿这个字段与当前记录的 `EntityId` 比较
//! 来产出 [`OwnerContext`]，本模块自身不做这个比较——它是纯决策函数，
//! 不持有也不查询 `WorldState`。
//!
//! # `ContentIndex` 缺占位值的既知债务
//!
//! 坐标系重写批次报告记录过一条待办：本项目目前没有为任何内容类型
//! （地形、空间层属性、种族、职业……）注册过一个「占位/未知」条目——
//! `materialize_base_terrain`/`register_base_space_profiles` 一类注册
//! 函数只声明真实存在的内容，不预留「找不到就退到这里」的兜底索引。
//! 本任务确实撞上了这条债务：[`decide_degrade_action`] 需要一个真实
//! 已注册的 `ContentIndex` 才能返回 [`DegradeAction::FallbackToPlaceholder`]，
//! 而调用方（在占位内容真正被本体注册之前）可能根本拿不出这样一个
//! 索引。本模块用 `placeholder: Option<ContentIndex>` 参数诚实表达这
//! 件事——拿不到占位索引时不会伪造一个（`ContentIndex::default()` 同
//! 样有「索引 0 可能是合法内容」的歧义，见
//! `ll_world::state::WorldState::surface_profile` 文档的同类核实），而
//! 是退化为 [`DegradeAction::Reject`]，交给只读模式兜底——「降级失败」
//! 本身也是一种需要诚实面对的失败，不能用一个可能指向错误内容的假
//! 占位索引掩盖过去。

use ll_core::ident::ContentIndex;
use ll_world::state::WorldState;

/// 缺失内容所属的类型——决定套用哪一档降级策略。
///
/// 「这条记录是不是玩家自己的」不属于这个枚举——那是 [`OwnerContext`]
/// 的职责，两个维度正交：同一个 [`ContentKind::CharacterAttribute`]
/// 缺失，落在 NPC 身上和落在玩家身上是两种不同的处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// 物品类型——背包/地面堆叠里引用的 `ItemDef`。
    Item,
    /// 角色的种族或职业——具体是谁的角色由 [`OwnerContext`] 决定。
    CharacterAttribute,
    /// 世界生成用的地形生成器——世界已生成完毕，缺失只影响未来可能的
    /// 重新生成，不影响已存在的地形。
    WorldGenerator,
}

/// 一条缺失内容记录归属于谁。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerContext {
    /// 属于玩家角色本人——决定 [`ContentKind::CharacterAttribute`] 必须
    /// 走 [`DegradeAction::Reject`]，不允许降级。
    Player,
    /// 属于某个 NPC（厚层或薄层均可）。
    Npc,
    /// 不属于任何角色——物品、地形生成器这类内容没有「归属」概念。
    None,
}

/// 缺失内容的处理动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeAction {
    /// 丢弃该内容的引用并记录警告——物品类、世界生成器类的归宿。
    DropWithWarning,
    /// 降级为一个已注册的占位内容——NPC 的种族/职业类的归宿。
    FallbackToPlaceholder(ContentIndex),
    /// 拒绝降级——玩家角色的种族/职业类的归宿,以及「本该降级为占位但
    /// 拿不出占位索引」这一已知债务命中时的诚实兜底（见模块文档）。
    Reject,
}

/// 按内容类型与归属，决定缺失内容应该走哪种降级策略。
///
/// `placeholder` 是调用方（当前会话的读档管线）为
/// [`ContentKind::CharacterAttribute`] 准备好的占位 `ContentIndex`——
/// 若当前会话确实注册过这样一条占位内容就传 `Some`，若还没有（模块
/// 文档「`ContentIndex` 缺占位值的既知债务」）就传 `None`，本函数会
/// 诚实地退化为 [`DegradeAction::Reject`] 而不是伪造一个索引。
/// [`ContentKind::Item`]/[`ContentKind::WorldGenerator`] 不消费这个
/// 参数——它们恒定丢弃，不需要占位。
pub fn decide_degrade_action(
    content_kind: ContentKind,
    owner: OwnerContext,
    placeholder: Option<ContentIndex>,
) -> DegradeAction {
    match content_kind {
        ContentKind::Item | ContentKind::WorldGenerator => DegradeAction::DropWithWarning,
        ContentKind::CharacterAttribute => match owner {
            OwnerContext::Player => DegradeAction::Reject,
            OwnerContext::Npc | OwnerContext::None => match placeholder {
                Some(index) => DegradeAction::FallbackToPlaceholder(index),
                None => DegradeAction::Reject,
            },
        },
    }
}

/// 一次读档过程中遇到的全部降级决策，综合成的整体结果。
///
/// 撞上至少一次 [`DegradeAction::Reject`] 时不直接拒绝打开——存档本身
/// 没有损坏，只是有些内容缺失且不可降级，[`ReadOnlySave`] 允许查看/
/// 导出，好过「要么丢数据要么打不开」的二选一。
///
/// `Rejected` 变体（存档本身损坏/schema 或 mod 版本不兼容）由任务 7
/// （[`crate::load_error::LoadError`]）落地时补上——本任务只交付
/// 「降级决策如何影响读档结果」这一半，另一半是完全不同的失败轴
/// （见 `knowledge/design/identity-and-ids.md` 六、④），不在这里混
/// 一起判定。
#[derive(Debug)]
pub enum LoadOutcome {
    /// 完全正常，可以继续游玩。
    Playable(WorldState),
    /// 撞上 `Reject` 类降级，但存档本身没有损坏——只读模式。
    ReadOnly(ReadOnlySave),
}

/// 只读存档：持有完整的 [`WorldState`]，但不暴露任何会推进世界的方法。
///
/// # 只读边界如何保证（编译期）
///
/// 本类型只暴露 [`Self::world`]（返回 `&WorldState`）与 [`Self::export`]
/// （消费 `self`，返回拥有所有权的 `WorldState`，供「导出」用途）——
/// 没有任何方法返回 `&mut WorldState`。`WorldState::advance`/
/// `ll_sim::apply::apply` 一类会推进 tick 或写入 `Effect` 的方法全部
/// 要求 `&mut WorldState`，只要 `ReadOnlySave` 不提供这个借用，调用方
/// 拿着一个 `&ReadOnlySave`（或 `ReadOnlySave` 本身，只要没调用
/// `export`）就没有任何路径能触碰到这些方法——这是编译期保证，不是
/// 运行期检查，见下方的 `compile_fail` 示例。
///
/// `export` 是刻意保留的逃生舱：规格要求只读模式仍然「允许查看、导出
/// 角色/物品」，导出之后调用方拿到的是一个普通 `WorldState`，那之后
/// 如何使用是调用方的选择，不是本类型能够（也不需要）继续约束的范围
/// ——本类型的保证止于「作为 `ReadOnlySave` 存在期间不能被推进」。
///
/// ```compile_fail
/// # use ll_content::degrade::ReadOnlySave;
/// # fn need_world_state() -> ll_world::state::WorldState { unimplemented!() }
/// let read_only = ReadOnlySave::new(need_world_state());
/// // world() 只返回共享引用,advance 需要 &mut self——编译失败。
/// read_only.world().advance(1);
/// ```
#[derive(Debug)]
pub struct ReadOnlySave {
    world: WorldState,
}

impl ReadOnlySave {
    /// 用一个已经构造好的 [`WorldState`] 建立只读视图。
    pub fn new(world: WorldState) -> Self {
        ReadOnlySave { world }
    }

    /// 只读查看——供查看/导出角色物品这类只读消费方使用。
    pub fn world(&self) -> &WorldState {
        &self.world
    }

    /// 导出：拿回完整所有权。见类型文档「只读边界如何保证」一节：这是
    /// 刻意保留的逃生舱，不是漏洞。
    pub fn export(self) -> WorldState {
        self.world
    }
}

/// 综合一次读档过程中遇到的全部降级决策，得到整体读档结果。
///
/// 只要 `decisions` 里出现至少一条 [`DegradeAction::Reject`]，结果就是
/// [`LoadOutcome::ReadOnly`]；否则是 [`LoadOutcome::Playable`]。
pub fn summarize_load_outcome(world: WorldState, decisions: &[DegradeAction]) -> LoadOutcome {
    let has_rejection = decisions
        .iter()
        .any(|decision| matches!(decision, DegradeAction::Reject));
    if has_rejection {
        LoadOutcome::ReadOnly(ReadOnlySave::new(world))
    } else {
        LoadOutcome::Playable(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};
    use ll_core::torus::TorusSize;
    use ll_world::generate::GenParams;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    fn placeholder_index() -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:placeholder").expect("合法标识符"))
    }

    fn test_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    #[test]
    fn 物品类内容缺失时决策为丢弃并警告() {
        // Arrange & Act
        let action = decide_degrade_action(ContentKind::Item, OwnerContext::None, None);

        // Assert
        assert_eq!(action, DegradeAction::DropWithWarning);
    }

    #[test]
    fn 世界生成器缺失时决策为丢弃并警告() {
        // Arrange & Act
        let action = decide_degrade_action(ContentKind::WorldGenerator, OwnerContext::None, None);

        // Assert
        assert_eq!(action, DegradeAction::DropWithWarning);
    }

    #[test]
    fn npc种族缺失时决策为降级占位() {
        // Arrange
        let placeholder = placeholder_index();

        // Act
        let action = decide_degrade_action(
            ContentKind::CharacterAttribute,
            OwnerContext::Npc,
            Some(placeholder),
        );

        // Assert
        assert_eq!(action, DegradeAction::FallbackToPlaceholder(placeholder));
    }

    #[test]
    fn 玩家角色种族缺失时决策为拒绝降级() {
        // 即便调用方确实准备好了占位索引,玩家角色也不能走占位这条路
        // ——这是本模块存在的核心理由。
        // Arrange
        let placeholder = placeholder_index();

        // Act
        let action = decide_degrade_action(
            ContentKind::CharacterAttribute,
            OwnerContext::Player,
            Some(placeholder),
        );

        // Assert
        assert_eq!(action, DegradeAction::Reject);
    }

    #[test]
    fn npc种族缺失且占位索引缺失时诚实退化为拒绝而非伪造索引() {
        // 撞上模块文档「ContentIndex 缺占位值的既知债务」——没有真实
        // 占位索引可用时,不能伪造一个,只能诚实地拒绝降级。
        // Arrange & Act
        let action =
            decide_degrade_action(ContentKind::CharacterAttribute, OwnerContext::Npc, None);

        // Assert
        assert_eq!(action, DegradeAction::Reject);
    }

    #[test]
    fn 拒绝降级触发只读模式而非直接报错拒绝打开() {
        // Arrange
        let decisions = vec![DegradeAction::DropWithWarning, DegradeAction::Reject];

        // Act
        let outcome = summarize_load_outcome(test_world(), &decisions);

        // Assert
        assert!(matches!(outcome, LoadOutcome::ReadOnly(_)));
    }

    #[test]
    fn 没有任何拒绝降级时读档结果为可游玩() {
        // Arrange
        let decisions = vec![DegradeAction::DropWithWarning];

        // Act
        let outcome = summarize_load_outcome(test_world(), &decisions);

        // Assert
        assert!(matches!(outcome, LoadOutcome::Playable(_)));
    }

    #[test]
    fn 只读存档可以导出为普通worldstate() {
        // Arrange
        let read_only = ReadOnlySave::new(test_world());

        // Act
        let exported = read_only.export();

        // Assert：导出后是一个普通值,拥有完整所有权(编译期已经验证——
        // 这里只额外确认导出保留了原世界的数据,不是造出一个空壳)。
        assert_eq!(exported.seed, 0);
    }
}
