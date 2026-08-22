//! 暗视接线：把 `RaceDef::darkvision_floor` 真正喂进有效光照计算。
//!
//! # 缺口是什么
//!
//! `race-system.md`「五、暗视」一节给出的公式是
//! `effective_light = max(实际光照, darkvision_floor)`——`ll_mod::race`
//! 里 `darkvision_floor` 字段存取完整（列式存储、`RaceView` 查询、两条
//! 测试断言往返），但没有任何函数真正实现这条 `max`。
//! `scripts/ci/check_field_consumers.py` 把这个缺口列进了豁免清单：
//! 「存取完整……但没有任何函数实现……effective_light = max(实际光照,
//! darkvision_floor)」。本模块补上这条缺失的函数。
//!
//! # 为什么定义在 `ll-sim`，不是 `ll-world::light`
//!
//! `ll_world::light` 的 `ambient_light`/`sight_radius_at` 只认识
//! `Tick`/`Season`/`LightLevel` 这类纯世界层数据，不认识「种族」这个
//! 概念——`RaceDef`/`RaceTable` 定义在下游的 `ll_mod::race`，依赖方向
//! `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格 §5）不允许
//! `ll-world` 反过来依赖 `ll-mod`。与 [`crate::character::RaceStatModifierSource`]/
//! [`crate::traits::TraitGrantSource`] 同一套依赖倒置手法：本模块只
//! 声明「给我一个种族索引，还我它的暗视下限」这个最小接口
//! （[`RaceDarkvisionSource`]），真实实现（`ll_mod::race::RaceTable`）
//! 留给下游补齐；`max` 本身只操作两个 `i32`，不需要认识 `RaceDef`。
//!
//! # 决策层落点：`check_field_consumers.py` 只认 `ll-sim/src/*.rs` 与
//! `ll-world/src/{fov,light}.rs`
//!
//! 该门禁按「决策层文件里有没有出现 `.darkvision_floor`」判定字段是否
//! 真正被消费——见其头注释「存储层 vs 决策层」一节。`darkvision_floor`
//! 这个字段名在 `TARGET_TYPES` 覆盖的其余结构体（`ItemDef`/`ClassDef`/
//! `SkillDef`/……/`Agent`）里都不存在，不存在 `RaceDef.stat_modifiers`
//! 撞上 `TraitDef.stat_modifiers` 那种同名字段污染判定的风险（见
//! `ll_mod::race` 模块 `RaceStatModifierSource` 文档「方法名故意不叫
//! `stat_modifiers`」一节），因此这里的 trait 方法直接叫
//! [`RaceDarkvisionSource::darkvision_floor`]，不需要像
//! `race_stat_modifiers` 那样刻意避让。

use ll_core::ident::ContentIndex;
use ll_world::light::LightLevel;

/// `resolve`/渲染侧需要的种族暗视下限查询接口——真实实现是
/// `ll_mod::race::RaceTable`，见模块文档「为什么定义在 `ll-sim`」一节。
pub trait RaceDarkvisionSource {
    /// 给定种族索引，返回它声明的暗视下限；未注册的索引返回 `0`
    /// （无暗视）——查不到就是查不到（ADR 0015），与
    /// [`crate::character::RaceStatModifierSource::race_stat_modifiers`]
    /// 「查不到就返回全零修正」同一条纪律，不是 panic，也不是伪造一份
    /// 看似合法的非零数据。
    fn darkvision_floor(&self, race: ContentIndex) -> i32;
}

/// 按 `race-system.md`「五、暗视」一节的公式，把种族暗视下限叠加进
/// 某一时刻的实际环境光照：`effective_light = max(实际光照,
/// darkvision_floor)`。
///
/// 只改变喂给视野半径计算的输入，不碰 FOV 算法本身——与
/// `ll_mod::race::RaceDef::darkvision_floor` 字段文档同一句话（该类型
/// 定义在下游的 `ll-mod`，`ll-sim` 不能反过来依赖它，这里只能用反引号
/// 纯文本指向，不能用 intra-doc link，见本文件模块文档「决策层落点」
/// 一节同一条边界）（ADR 0018 归类判据第二步的又一个实例：自由度落在
/// 算法读的数据上，不在算法本身）。`darkvision_floor` 为零（本体人类
/// 的默认值）时本函数恒等于 `light` 本身，暗视为零的种族因此与「这个
/// 函数不存在」时行为完全一致——不会意外把无暗视种族的视野在白天也
/// 抬高。
pub fn effective_light_for_race(
    light: LightLevel,
    race: ContentIndex,
    darkvision: &dyn RaceDarkvisionSource,
) -> LightLevel {
    let floor = darkvision.darkvision_floor(race);
    LightLevel(light.0.max(floor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    /// 测试用帮手：从一个全新的 `Interner` 里 intern 出一个索引——同
    /// `crate::character` 测试模块同名帮手，本模块的叠加逻辑只关心
    /// 索引之间"相不相等"，不关心具体指向哪条命名空间标识符。
    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    /// 测试用的最小 `RaceDarkvisionSource` 实现——不依赖 `ll_mod`，
    /// 只声明"这个索引对应多少暗视下限"。
    struct FixedDarkvision(i32);

    impl RaceDarkvisionSource for FixedDarkvision {
        fn darkvision_floor(&self, _race: ContentIndex) -> i32 {
            self.0
        }
    }

    #[test]
    fn 黑暗中暗视下限抬高有效光照() {
        // Arrange：实际光照为零（漆黑），暗视下限为四十。
        let mut interner = Interner::new();
        let dwarf = index(&mut interner, "lostland:dwarf");
        let darkvision = FixedDarkvision(40);

        // Act
        let effective = effective_light_for_race(LightLevel(0), dwarf, &darkvision);

        // Assert
        assert_eq!(effective.0, 40);
    }

    #[test]
    fn 光照本就高于暗视下限时不受影响() {
        // Arrange：正午光照（千分之一千）远高于暗视下限。
        let mut interner = Interner::new();
        let dwarf = index(&mut interner, "lostland:dwarf");
        let darkvision = FixedDarkvision(40);

        // Act
        let effective = effective_light_for_race(LightLevel(1000), dwarf, &darkvision);

        // Assert
        assert_eq!(effective.0, 1000);
    }

    #[test]
    fn 暗视下限为零时对光照无影响() {
        // Arrange：人类（无暗视）在漆黑中。
        let mut interner = Interner::new();
        let human = index(&mut interner, "lostland:human");
        let no_darkvision = FixedDarkvision(0);

        // Act
        let effective = effective_light_for_race(LightLevel(0), human, &no_darkvision);

        // Assert
        assert_eq!(effective.0, 0);
    }
}
