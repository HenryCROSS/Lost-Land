//! 暗视接线：把 `RaceDef::darkvision_cells` 真正喂进视野半径计算。
//!
//! # 缺口是什么，以及它此前为什么「接上了也等于没接」
//!
//! `race-system.md`「五、暗视」一节最初给出的公式是
//! `effective_light = max(实际光照, darkvision_floor)`——本模块此前
//! 实现的正是那条 `max`，字段存取、依赖倒置、渲染侧接线一应俱全，
//! **但那个形态在本作的量纲下永远不可能生效**：本体矮人声明的
//! `darkvision_floor` 是 4，而午夜环境光是 100
//! （`ll_core::light::MIDNIGHT_LIGHT`），最暗的冬夜下雨也还有约 52，
//! `max(52, 4)` 恒等于 52。旁证是当时的测试夹具必须写成
//! `FixedDarkvision(600)`——把值放大 150 倍才能让功能表现出可观测
//! 差异，那本身就是「机制对、数值错」的自白。更深的一层是下游还有
//! 第二个下限（`ll_world::light` 的 4 格），两个下限串在一起时后面
//! 那个把前面那个整个吃掉：基准 12 格时光照 300 只算出 3 格，仍旧被
//! 4 格下限抬回 4，暗视值从 100 涨到 300 最终一格没变。
//!
//! 项目所有者据此把暗视从「光照千分比下限」改成「**夜间视野格数
//! 下限**」：种族直接声明「我夜里至少看得见几格」，中间不再隔着一层
//! 会把它吸收掉的换算。公式与「为什么不是 `max(默认值, 声明值)`」的
//! 论证都落在 [`ll_world::light::sight_radius_at`]，本模块只负责
//! 「从种族索引查到这个格数」这一步依赖倒置。
//!
//! # 暗视只买视野格数，不买画面亮度
//!
//! 项目所有者的另一条裁定：暗视**只**影响看多远，不影响看多清。
//! 因此本模块不再产出 `LightLevel`——`effective_light_for_race` 连同
//! 它返回的「有效光照」这个概念一并删除。画面亮度那一路
//! （`ll_game::layout::effective_tint`）读的是环境光本身，从来就没有
//! 经过本模块（核实过：`effective_light_for_race` 在删除前全仓库只有
//! `ll_game::layout::effective_sight_radius_for_race` 一个真实调用
//! 点），因此这条裁定在代码上不是「把色调那一路的暗视摘掉」，而是
//! 「确认它本来就没有，并让新形态无法再意外长出来」——新形态的返回
//! 值是格数，语法上就喂不进色调换算。
//!
//! # 为什么定义在 `ll-sim`，不是 `ll-world::light`
//!
//! `ll_world::light` 的 `ambient_light`/`sight_radius_at` 只认识
//! `Tick`/`Season`/`LightLevel` 这类纯世界层数据，不认识「种族」这个
//! 概念——`RaceDef`/`RaceTable` 定义在下游的 `ll_mod::race`，依赖方向
//! `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格 §5）不允许
//! `ll-world` 反过来依赖 `ll-mod`。与 [`crate::character::RaceStatModifierSource`]/
//! [`crate::traits::TraitGrantSource`] 同一套依赖倒置手法：本模块只
//! 声明「给我一个种族索引，还我它的暗视格数」这个最小接口
//! （[`RaceDarkvisionSource`]），真实实现（`ll_mod::race::RaceTable`）
//! 留给下游补齐；换算本身只操作整数，不需要认识 `RaceDef`。
//!
//! # 决策层落点：`check_field_consumers.py` 只认 `ll-sim/src/*.rs` 与
//! `ll-world/src/{fov,light}.rs`
//!
//! 该门禁按「决策层文件里有没有出现 `.darkvision_cells`」判定字段是否
//! 真正被消费——见其头注释「存储层 vs 决策层」一节。`darkvision_cells`
//! 这个字段名在 `TARGET_TYPES` 覆盖的其余结构体（`ItemDef`/`ClassDef`/
//! `SkillDef`/……/`Agent`）里都不存在，不存在 `RaceDef.stat_modifiers`
//! 撞上 `TraitDef.stat_modifiers` 那种同名字段污染判定的风险（见
//! `ll_mod::race` 模块 `RaceStatModifierSource` 文档「方法名故意不叫
//! `stat_modifiers`」一节），因此这里的 trait 方法直接叫
//! [`RaceDarkvisionSource::darkvision_cells`]，不需要像
//! `race_stat_modifiers` 那样刻意避让。[`sight_radius_for_race`] 是那处
//! `.darkvision_cells` 读取的所在，也是本模块唯一的对外函数。

use ll_core::ident::ContentIndex;
use ll_world::light::{LightLevel, sight_radius_under_weather};
use ll_world::weather::Weather;

/// `resolve`/渲染侧需要的种族暗视查询接口——真实实现是
/// `ll_mod::race::RaceTable`，见模块文档「为什么定义在 `ll-sim`」一节。
pub trait RaceDarkvisionSource {
    /// 给定种族索引，返回它声明的**夜间视野格数下限**；未注册的索引
    /// 返回 `0`——查不到就是查不到（ADR 0015），与
    /// [`crate::character::RaceStatModifierSource::race_stat_modifiers`]
    /// 「查不到就返回全零修正」同一条纪律，不是 panic，也不是伪造一份
    /// 看似合法的非零数据。
    ///
    /// `0` 在下游被解读成「**未声明**暗视」而不是「声明了 0 格」，落回
    /// [`ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS`]——因此本方法对
    /// 查不到的索引返回 `0`，恰好等价于「这个生物没有任何暗视天赋，
    /// 按常人处理」，不需要额外的哨兵值。想表达「夜里几乎全瞎」的种族
    /// 声明一个真实的小值（例如 `2`）即可，见
    /// [`ll_world::light::sight_radius_at`] 文档。
    fn darkvision_cells(&self, race: ContentIndex) -> u32;
}

/// 把某个种族的暗视格数接进天气版视野半径换算——暗视在生产路径上的
/// 唯一入口。
///
/// # 为什么直接走天气版，不给一个不带天气的版本
///
/// `ll_world::light::sight_radius_under_weather` 把夜间下限应用了
/// **两次**（一次在 `sight_radius_at` 内部，天气乘数之后再一次），
/// 两处都必须认识同一个 `darkvision_cells`，否则雾雪会把矮人从 7 格
/// 削回默认的 4 格。把「查到格数」与「两处都用上它」绑在同一个函数
/// 里，是让调用方不可能只接一半（ADR 0021：抽象的理由是有算法可
/// 共享，这里共享的正是「别漏掉第二处下限」这条易错性）。不需要天气
/// 的调用方直接用 `ll_world::light::sight_radius_under_weather` 传
/// `Weather::CLEAR`，或者用 `sight_radius_at`，本模块不为它们再包一层。
///
/// # 热路径纪律
///
/// 每帧每格都会算视野（ADR 0016/0017）：本函数只做一次查表加几次整数
/// 运算，不跨脚本边界（一次脚本调用 326ns，视野这条路径上不可接受）。
pub fn sight_radius_for_race(
    base_radius: u32,
    light: LightLevel,
    weather: Weather,
    race: ContentIndex,
    darkvision: &dyn RaceDarkvisionSource,
) -> u32 {
    let cells = darkvision.darkvision_cells(race);
    sight_radius_under_weather(base_radius, light, weather, cells)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    /// 与 `ll_game::layout::BASE_SIGHT_RADIUS` 同值——本 crate 不依赖
    /// `ll-game`（依赖方向不允许），这里复制那个取值并在此说明，理由
    /// 同 `ll_world::light` 测试模块的同名常量。
    const PLAYER_BASE_SIGHT_RADIUS: u32 = 12;

    /// 午夜环境光（`ll_core::light::MIDNIGHT_LIGHT`）——本模块不依赖
    /// 世界时钟，直接给光照值，避免为了构造一个 `Tick` 而把昼夜曲线
    /// 也拖进本模块的断言里。
    const MIDNIGHT_LIGHT: LightLevel = LightLevel(100);

    /// 测试用帮手：从一个全新的 `Interner` 里 intern 出一个索引——同
    /// `crate::character` 测试模块同名帮手，本模块只关心索引之间
    /// "相不相等"，不关心具体指向哪条命名空间标识符。
    fn index(interner: &mut Interner, raw: &str) -> ContentIndex {
        interner.intern(NamespacedId::parse(raw).expect("测试用标识符恒合法"))
    }

    /// 测试用的最小 `RaceDarkvisionSource` 实现——不依赖 `ll_mod`，
    /// 只声明"这个索引对应多少暗视格数"。
    ///
    /// 取值不再需要像旧形态那样刻意放大 150 倍才能观测到差异（旧夹具
    /// 写的是 `FixedDarkvision(600)`，而本体矮人声明的是 4）——新形态
    /// 里种族声明的就是格数本身，测试用的数字与内容里的数字同一个量纲。
    struct FixedDarkvision(u32);

    impl RaceDarkvisionSource for FixedDarkvision {
        fn darkvision_cells(&self, _race: ContentIndex) -> u32 {
            self.0
        }
    }

    #[test]
    fn 暗视种族夜间视野大于无暗视种族() {
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "lostland:dwarf");

        // Act
        let dwarf = sight_radius_for_race(
            PLAYER_BASE_SIGHT_RADIUS,
            MIDNIGHT_LIGHT,
            Weather::CLEAR,
            race,
            &FixedDarkvision(7),
        );
        let human = sight_radius_for_race(
            PLAYER_BASE_SIGHT_RADIUS,
            MIDNIGHT_LIGHT,
            Weather::CLEAR,
            race,
            &FixedDarkvision(0),
        );

        // Assert
        assert_eq!(dwarf, 7);
        assert_eq!(human, ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS);
    }

    #[test]
    fn 查不到的种族按未声明处理而不是恐慌() {
        // Arrange：一个从未注册过暗视的索引，源恒返回 0。
        let mut interner = Interner::new();
        let unknown = index(&mut interner, "lostland:unknown");

        // Act
        let radius = sight_radius_for_race(
            PLAYER_BASE_SIGHT_RADIUS,
            MIDNIGHT_LIGHT,
            Weather::CLEAR,
            unknown,
            &FixedDarkvision(0),
        );

        // Assert
        assert_eq!(radius, ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS);
    }

    #[test]
    fn 声明低于默认值的种族夜里真的更瞎() {
        // 「不能写成 max(默认值, 声明值)」这条语义在依赖倒置这一层的
        // 同一条断言——本函数不得在传递过程中偷偷抬高声明值。
        // Arrange
        let mut interner = Interner::new();
        let race = index(&mut interner, "testmod:cave_worm");

        // Act
        let radius = sight_radius_for_race(
            PLAYER_BASE_SIGHT_RADIUS,
            MIDNIGHT_LIGHT,
            Weather::CLEAR,
            race,
            &FixedDarkvision(2),
        );

        // Assert
        assert_eq!(radius, 2);
    }
}
