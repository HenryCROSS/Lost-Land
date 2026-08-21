//! 击杀产出经验值：`resolve` 侧需要的「这个生物种类值多少经验」只读
//! 接口——`knowledge/design/level-and-experience-system.md` 五节核实出
//! 的真正缺口（「没有任何地方注册『某种生物值多少经验』」），本模块是
//! 消费端的落点。
//!
//! # 为什么是 trait，不是具体类型
//!
//! 与 [`crate::skill::SkillCatalog`]/[`crate::quest::QuestCatalog`] 同
//! 一套依赖倒置手法：真正持有「种族 → 经验值」映射的一方是 `ll-mod`
//! （见 `crates/ll-mod/src/race.rs` 的 `RaceDef::xp_reward` 字段），
//! `ll-sim` 不能反过来依赖它（依赖方向 `ll-world` ← `ll-sim` ← `ll-mod`）。
//! `resolve` 因此只依赖本模块声明的接口，不知道背后是 `RaceTable` 还
//! 是别的什么存储形状。

use ll_core::ident::ContentIndex;

/// 给定一个「生物种类」（[`ll_world::entity::Agent::creature_kind`]，
/// `None` 时回退到 [`ll_world::entity::Agent::race`]——与
/// `Effect::IncrementKillCount` 现有的归并键完全同一个键空间，见
/// `crate::resolve` 模块 `append_kill_history` 文档），返回杀死这个
/// 种类应该获得多少经验。
///
/// # 为什么恒返回 `i64` 而不是 `Option<i64>`
///
/// 未注册的种类没有理由阻断整条结算链——查不到就当作「这个东西不值
/// 经验」（0），与技能查询「查不到就是查不到，静默不产出效果」是同一
/// 条纪律的另一种体现：这里不是「静默跳过」，是「诚实地给出零」，两者
/// 效果相同（不产出 `Effect::GrantExperience`），但不需要调用方在
/// `Option` 上多包一层判断。
pub trait ExperienceCatalog {
    /// 查询给定种类的击杀经验值，未注册时约定返回 0。
    fn xp_reward_for(&self, kind: ContentIndex) -> i64;
}

/// 空经验目录：任何种类查询恒返回 0，不产出任何经验。
///
/// 与 [`crate::skill::NoSkills`]/[`crate::quest::NoQuests`] 同一个模式
/// ——调用方没有接好内容注册表时的保底实现，见两者文档。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoExperience;

impl ExperienceCatalog for NoExperience {
    fn xp_reward_for(&self, _kind: ContentIndex) -> i64 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空经验目录对任意种类查询恒返回零() {
        // Arrange
        let catalog = NoExperience;

        // Act
        let reward = catalog.xp_reward_for(ContentIndex::default());

        // Assert
        assert_eq!(reward, 0);
    }
}
