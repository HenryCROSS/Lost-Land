//! 本体种族注册——「本体即 Mod」在种族系统上的落点，照
//! [`crate::base_terrain`]/[`crate::base_placeholder`] 的既有模式。
//!
//! `crate::race::materialize_base_races` 定义了本体全部三个种族的声明
//! 与固定注册顺序，但它本身刻意不知道「谁来分配 `ContentIndex`」——
//! 签名接受一个解析回调，而不是绑死某个具体类型（见其模块文档）。本
//! 模块补上生产路径缺的那一半：把回调实参换成真正的
//! [`Registry::intern`]。
//!
//! # 为什么这一步值得单独成模块，而不是内联在别处
//!
//! 与 [`crate::base_terrain`] 同一个理由：这是「本体即 Mod」的检验点
//! ——本体种族注册与未来 mod 种族注册要走**完全相同**的
//! [`Registry::intern`] 调用。单独成模块，让 [`register_base_races`]
//! 的实现只有唯一一行真正有意义的代码，任何人一眼就能看出这里没有
//! 任何本体专属的特权通道。
//!
//! # 调用顺序与 `register_base_placeholder_content` 无关
//!
//! 两者共用同一个 `Registry`，但注册顺序不影响正确性——
//! `Registry::intern` 对不同的命名空间字符串各自独立分配索引，`crate::race`
//! 模块文档「与 `lostland:placeholder_race` 的协调」一节已经论证过两者
//! 不会冲突。调用方按任意顺序在启动时各调用一次即可。

use ll_core::ident::NamespacedId;

use crate::race::{BaseRaceIds, RaceError, RaceTable, materialize_base_races};
use crate::registry::Registry;

/// 把本体全部三个种族注册进 `registry`，返回可用的
/// `(BaseRaceIds, RaceTable)`。
///
/// **这是本体种族唯一的生产注册入口**：内部只是把 `registry.intern`
/// 包成回调传给 [`materialize_base_races`]——本体种族因此与未来 mod
/// 注册的自定义种族走同一条 [`Registry::intern`] 调用路径。
///
/// 调用方应在启动时、且仅在此时调用一次；返回的 `BaseRaceIds` 此后按
/// 字段访问，是常量级开销，不会把注册表查询带进任何热路径。
pub fn register_base_races(registry: &mut Registry) -> Result<(BaseRaceIds, RaceTable), RaceError> {
    materialize_base_races(&mut |id: NamespacedId| registry.intern(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 本体种族与mod种族共用registry同一段连续递增的索引号段() {
        // 本测试只证明本体种族与 mod 种族走同一条 Registry::intern
        // 调用路径、共用同一个单调递增的号段——不为本体预留任何特殊
        // 区间。这是「结构等价」，不是「mod 脚本调得到这套 API」的
        // 证据；后者的证据在 crate::pipeline 的脚本装载测试与
        // mods/example_mod/gameplay.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (race_ids, _table) =
            register_base_races(&mut registry).expect("本体种族声明表内部一致");
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:half_elf").expect("合法标识符"));

        // Assert：mod 内容紧接在本体三个种族之后分配到索引。
        assert_eq!(mod_index.get(), race_ids.elf.get() + 1);
    }

    #[test]
    fn 本体种族重复注册返回错误而非静默覆盖() {
        // Arrange
        let mut registry = Registry::new();
        let (race_ids, mut table) =
            register_base_races(&mut registry).expect("本体种族声明表内部一致");

        // Act
        let result = table.define(
            race_ids.human,
            crate::race::RaceAttrs {
                display_name_key: NamespacedId::parse("lostland:race.human.display_name")
                    .expect("合法"),
                stat_modifiers: ll_world::entity::BaseStats {
                    strength: 0,
                    dexterity: 0,
                    constitution: 0,
                    intelligence: 0,
                    willpower: 0,
                    charisma: 0,
                },
                darkvision_floor: 0,
                footprint: (1, 1),
                lifespan_years: 80,
                xp_reward: 0,
            },
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 本体种族与占位内容共用registry不产生索引冲突() {
        // Arrange
        let mut registry = Registry::new();
        let placeholder = crate::base_placeholder::register_base_placeholder_content(&mut registry);

        // Act
        let (race_ids, _table) =
            register_base_races(&mut registry).expect("本体种族声明表内部一致");

        // Assert
        assert_ne!(placeholder, race_ids.human);
        assert_ne!(placeholder, race_ids.dwarf);
        assert_ne!(placeholder, race_ids.elf);
    }
}
