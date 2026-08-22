//! 角色创建期的种族属性修正：把 [`RaceStatModifierSource`] 查到的六项
//! 固定增减量一次性烘焙进 [`BaseStats`]。
//!
//! # 烘焙，不是每次派生（`knowledge/design/race-system.md` 二节）
//!
//! `race-system.md`「二、属性修正」一节的结论是：种族对六维主属性的
//! 修正在角色创建（或 NPC 生成）那一刻**一次性加进 `BaseStats`**，此后
//! 种族与属性彻底脱钩——不是每次派生时都重新叠加一遍。理由：`BaseStats`
//! 是存档字段，不是 [`crate::resolve`] 那类「衍生属性绝不进存档」纪律
//! 约束的对象（那条纪律约束的是 `DerivedStats`）。若做成每次派生时叠加，
//! 所有读 `stats.strength` 的地方都要多穿一层「先查种族再加」的查询，
//! 漏一处就是一个隐蔽的数值缺陷。种族在这个模型下是不可变的（没有
//! 「种族随时间变化」的玩法需求，变形/诅咒改种族这类需求走
//! `Effect::ChangeRace` 重新烘焙一次，不是让 `BaseStats` 变成持续依赖
//! 种族的派生量），因此烘焙没有失效问题。
//!
//! # 为什么放在 `ll-sim`，不是 `ll-game`
//!
//! 真正调用这份逻辑的生产入口（`ll_game::world::spawn_player`）确实在
//! `ll-game`，但完整的 `RaceDef`/`RaceTable` 定义在下游的 `ll_mod::race`
//! ——依赖方向 `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格
//! §5）不允许 `ll-sim` 反过来依赖 `ll-mod`。与 `traits::TraitGrantSource`/
//! `quest::QuestCatalog`/`xp_curve::XpCurveCatalog` 同一套依赖倒置手法：
//! 本模块只声明 [`RaceStatModifierSource`] 这个最小只读接口，真实实现
//! 由 `ll_mod::race::RaceTable` 补齐（`impl` 在该模块里）。烘焙运算本身
//! （加法这一步）不依赖任何游戏状态，属于「驱动模拟结算、影响玩法输出」
//! 的决策层逻辑——一个矮人角色确实因为这次调用而拥有和人类不同的
//! 攻防数值，不是纯粹的存储搬运。
//!
//! # 为什么不直接在 `ll-game` 里查表再相加
//!
//! 若把「查表 + 相加」两步都留在 `ll-game`，这份逻辑就只对 `spawn_player`
//! 这一个调用方可见，且完全不落在本仓库「决策层」的既定边界内
//! （`scripts/ci/check_field_consumers.py` 把 `ll-sim/src/*.rs` 与
//! `ll-world/src/{fov,light}.rs` 划为决策层，`ll-game` 不在其中）——这不
//! 是绕开门禁的取巧,是这份检查本身的判据在提醒我们:「种族修正真的参与
//! 游戏结算」这件事,应当由一段决策层代码来体现,而不是分散在应用层的
//! 世界搭建代码里。把查表这一步收进本模块,让 [`bake_race_stat_modifiers`]
//! 成为唯一的公开入口,未来 NPC 生成、`Effect::ChangeRace` 等任何需要
//! 「给定种族,烘焙一份属性」的调用方都复用同一份实现,不需要各自重新
//! 查表再相加一遍。

use ll_core::ident::ContentIndex;
use ll_world::entity::BaseStats;

/// `resolve`/角色创建侧需要的种族属性修正查询接口——真实实现是
/// `ll_mod::race::RaceTable`，见模块文档「为什么放在 `ll-sim`」一节。
///
/// # 方法名故意不叫 `stat_modifiers`
///
/// 语义上这个方法就是「查 `RaceDef::stat_modifiers`」，直接同名本该是
/// 更自然的选择，但 `ll_mod::trait_def::TraitDef` 恰好也有一个同名字段
/// `stat_modifiers`（天赋授予的属性修正，当前仍是死字段——
/// `scripts/ci/check_field_consumers.py` 的 `EXEMPTIONS` 里
/// `TraitDef.stat_modifiers` 一条原样保留）。该门禁按"字段名全文正则"
/// 判定"决策层是否读取"（不做类型感知，见其头注释「已知局限」第 2
/// 条），若这里也写 `.stat_modifiers`，`RaceDef.stat_modifiers` 会被
/// 正确识别为已接线，但同一个正则同时会把毫不相关的
/// `TraitDef.stat_modifiers` 一并误判成"已接线"——两个不同结构体的
/// 同名字段在这份门禁眼里是同一个字符串。`race_stat_modifiers` 这个
/// 名字就是刻意避开这次撞车：宁可换一个不那么对称的方法名，也不要让
/// 一个字段的真实接线连带污染另一个字段的状态判定。
pub trait RaceStatModifierSource {
    /// 给定种族索引，返回它声明的六项固定增减量；未注册的索引返回全零
    /// 修正——查不到就是查不到（ADR 0015），不是 panic，也不是伪造一份
    /// 看似合法的非零数据。
    fn race_stat_modifiers(&self, race: ContentIndex) -> BaseStats;
}

/// 唯一的公开入口：把 `race` 声明的六项固定增减量一次性烘焙进 `base`，
/// 返回烘焙后的新值——调用方（`ll_game::world::spawn_player` 等角色/NPC
/// 生成流程）在创建那一刻调用一次，产出的值直接写进 `Agent.stats`，此后
/// 不再持有对 `race_stats`/`race` 的引用，见模块文档「烘焙，不是每次
/// 派生」一节。
///
/// 未注册的种族索引（正常运行不该发生——生产调用方的种族索引恒来自
/// 注册期缓存）经 [`RaceStatModifierSource::race_stat_modifiers`] 的既有
/// 纪律退化成全零修正，本函数因此对任何 `race` 输入都返回一个确定的
/// 结果，不需要返回 `Option`/`Result`。
pub fn bake_race_stat_modifiers(
    base: BaseStats,
    race: ContentIndex,
    race_stats: &dyn RaceStatModifierSource,
) -> BaseStats {
    base.add_modifiers(race_stats.race_stat_modifiers(race))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    /// 测试用帮手：从一个全新的 `Interner` 里 intern 出一个索引——同
    /// `crate::traits` 测试模块同名帮手,本模块的烘焙逻辑只关心索引
    /// 之间"相不相等",不关心具体指向哪条命名空间标识符。
    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    /// 测试用的最小 `RaceStatModifierSource` 实现——不依赖 `ll_mod`
    /// （依赖方向不允许本 crate 反过来依赖它），只按索引查一张手搭的
    /// 小表,查不到就退化成全零修正,与文档纪律一致。
    struct FakeRaceStats {
        entries: Vec<(ContentIndex, BaseStats)>,
    }

    impl RaceStatModifierSource for FakeRaceStats {
        fn race_stat_modifiers(&self, race: ContentIndex) -> BaseStats {
            self.entries
                .iter()
                .find(|(id, _)| *id == race)
                .map(|(_, modifiers)| *modifiers)
                .unwrap_or(BaseStats {
                    strength: 0,
                    dexterity: 0,
                    constitution: 0,
                    intelligence: 0,
                    willpower: 0,
                    charisma: 0,
                    luck: 0,
                })
        }
    }

    fn zero_modifiers() -> BaseStats {
        BaseStats {
            strength: 0,
            dexterity: 0,
            constitution: 0,
            intelligence: 0,
            willpower: 0,
            charisma: 0,
            luck: 0,
        }
    }

    #[test]
    fn 带非零修正的种族烘焙后属性真的包含了修正() {
        // Arrange：注册一个「+2 体质 +1 力量」的种族，模拟矮人。
        let mut interner = Interner::new();
        let dwarf = index(&mut interner, "lostland:dwarf");
        let source = FakeRaceStats {
            entries: vec![(
                dwarf,
                BaseStats {
                    strength: 1,
                    dexterity: 0,
                    constitution: 2,
                    intelligence: 0,
                    willpower: 0,
                    charisma: 0,
                    luck: 0,
                },
            )],
        };

        // Act
        let baked = bake_race_stat_modifiers(BaseStats::BASELINE, dwarf, &source);

        // Assert
        assert_eq!(baked.constitution, BaseStats::BASELINE.constitution + 2);
        assert_eq!(baked.strength, BaseStats::BASELINE.strength + 1);
    }

    #[test]
    fn 带非零幸运修正的种族烘焙后幸运真的包含了修正() {
        // Arrange：注册一个「+4 幸运」的种族，模拟半身人——核实
        // RaceDef.stat_modifiers（BaseStats 类型）并入幸运后不需要为
        // 种族幸运加成单开一条分支，bake_race_stat_modifiers 复用的
        // BaseStats::add_modifiers 对幸运与其余六项走同一条加法路径
        // （见 BaseStats::add_modifiers 文档）。
        let mut interner = Interner::new();
        let halfling = index(&mut interner, "lostland:halfling");
        let source = FakeRaceStats {
            entries: vec![(
                halfling,
                BaseStats {
                    strength: 0,
                    dexterity: 0,
                    constitution: 0,
                    intelligence: 0,
                    willpower: 0,
                    charisma: 0,
                    luck: 4,
                },
            )],
        };

        // Act
        let baked = bake_race_stat_modifiers(BaseStats::BASELINE, halfling, &source);

        // Assert
        assert_eq!(baked.luck, BaseStats::BASELINE.luck + 4);
    }

    #[test]
    fn 修正为零的种族烘焙后属性等于基线() {
        // 反例：证明本函数不是「无论如何都加点什么」——零修正的种族,
        // 烘焙结果必须原样等于基线。
        // Arrange
        let mut interner = Interner::new();
        let human = index(&mut interner, "lostland:human");
        let source = FakeRaceStats {
            entries: vec![(human, zero_modifiers())],
        };

        // Act
        let baked = bake_race_stat_modifiers(BaseStats::BASELINE, human, &source);

        // Assert
        assert_eq!(baked, BaseStats::BASELINE);
    }

    #[test]
    fn 未注册的种族索引烘焙后退化成基线而非panic() {
        // Arrange：source 里完全没有登记任何条目。
        let mut interner = Interner::new();
        let never_defined = index(&mut interner, "yourmod:never_defined");
        let source = FakeRaceStats {
            entries: Vec::new(),
        };

        // Act
        let baked = bake_race_stat_modifiers(BaseStats::BASELINE, never_defined, &source);

        // Assert
        assert_eq!(baked, BaseStats::BASELINE);
    }
}
