//! 本体空间层属性注册——「本体即 Mod」在 `SpaceProfile` 上的落点
//! （批次 C 补齐批次 A 留下的缺口）。
//!
//! `ll_world::space_profile::materialize_base_space_profiles` 定义了
//! 本体全部四种基础空间类型的声明与固定注册顺序，但它本身刻意不知道
//! 「谁来分配 `ContentIndex`」——签名接受一个解析回调，而不是绑死某个
//! 具体类型（见其模块文档「与 Registry 的关系」）。本模块补上生产路径
//! 缺的那一半：把回调实参换成真正的 [`Registry::intern`]。
//!
//! # 为什么这一步值得单独成模块，而不是内联在别处
//!
//! 与 [`crate::base_terrain`] 同一个理由：这是「本体即 Mod」的检验点
//! ——本体空间层属性注册与未来 mod 空间类型注册要走**完全相同**的
//! [`Registry::intern`] 调用。单独成模块，让 [`register_base_space_profiles`]
//! 的实现只有唯一一行真正有意义的代码，任何人一眼就能看出这里没有
//! 任何本体专属的特权通道。
//!
//! # 为什么此前缺了这一半（如实记录）
//!
//! 批次 A（任务 3）只建了 `ll-world` 侧的 `SpaceProfileTable` 与
//! `materialize_base_space_profiles`，未建这条生产路径——当时
//! `SpaceProfile` 尚无消费方，任务清单也只列了一个文件。没有这条路径，
//! 本体的空间类型就没有真正走注册表，「本体即 Mod」在这一项上是半截
//! 的。本模块补齐，照 [`crate::base_terrain`] 的模式。

use ll_core::ident::NamespacedId;
use ll_world::space_profile::{
    BaseSpaceProfileIds, SpaceProfileError, SpaceProfileTable, materialize_base_space_profiles,
};

use crate::registry::Registry;

/// 把本体全部四种基础空间类型注册进 `registry`，返回可用的
/// `(BaseSpaceProfileIds, SpaceProfileTable)`。
///
/// **这是本体空间层属性唯一的生产注册入口**：内部只是把
/// `registry.intern` 包成回调传给 [`materialize_base_space_profiles`]
/// ——本体空间类型因此与未来 mod 注册的自定义空间类型走同一条
/// [`Registry::intern`] 调用路径，`Registry` 内部完全无法区分某次
/// `intern` 调用来自本体还是 mod（`crate::registry` 模块文档：注册表
/// 「只认命名空间字符串」）。
///
/// 调用方应在启动时、且仅在此时调用一次；返回的 `BaseSpaceProfileIds`
/// 此后按字段访问，是常量级开销，不会把注册表查询带进任何热路径。
pub fn register_base_space_profiles(
    registry: &mut Registry,
) -> Result<(BaseSpaceProfileIds, SpaceProfileTable), SpaceProfileError> {
    materialize_base_space_profiles(&mut |id: NamespacedId| registry.intern(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 本体空间层属性通过与mod内容完全相同的intern调用路径注册() {
        // 这是本任务最核心的一条断言：本体注册与 mod 注册除了命名空间
        // 字符串不同之外,没有任何结构性差异——都只是往同一个
        // Registry::intern 里塞一个 NamespacedId。用「本体注册完之后,
        // 再拿同一个 Registry 直接 intern 一个 mod 风格的 id,两者
        // 分配到的索引连续递增」证明它们走的是完全相同的通道,没有
        // 任何一条只对本体开放的旁路。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (ids, _table) =
            register_base_space_profiles(&mut registry).expect("本体空间层属性声明表内部一致");
        let mod_index = registry.intern(NamespacedId::parse("yourmod:abyss").expect("合法标识符"));

        // Assert：mod 内容紧接在本体四种基础空间类型之后分配到索引,
        // 说明两者共用同一个单调递增的号段,没有为本体预留任何特殊
        // 区间。materialize_base_space_profiles 内部注册顺序的最后一个
        // 是 building_interior。
        assert_eq!(mod_index.get(), ids.building_interior.get() + 1);
    }

    #[test]
    fn 本体空间层属性与mod注册的自定义内容在registry内部结构上不可区分() {
        // 直接的「本体即 Mod」验收断言：除了命名空间字符串本身,注册表
        // 内部（content_hash 的累积方式）看不出这条内容是谁注册的。
        // Arrange
        let mut registry = Registry::new();
        let (_ids, _table) =
            register_base_space_profiles(&mut registry).expect("本体空间层属性声明表内部一致");

        // Act：本体命名空间（lostland）与一个假想 mod 命名空间
        // （yourmod）各自都已经在注册表里贡献过内容哈希——两者都是
        // 通过 Registry::intern 产生的,查询接口完全相同。
        let lostland_hash = registry.content_hash_of("lostland");
        registry.intern(NamespacedId::parse("yourmod:abyss").expect("合法标识符"));
        let yourmod_hash = registry.content_hash_of("yourmod");

        // Assert：两个命名空间都成功贡献了内容摘要——Registry 没有为
        // "lostland" 这个命名空间字符串做任何特殊处理或旁路。
        assert!(lostland_hash.is_some());
        assert!(yourmod_hash.is_some());
    }

    #[test]
    fn 本体空间层属性重复注册返回错误而非静默覆盖() {
        // register_base_space_profiles 本身只会调用一次,这里模拟
        // 「另一次注册尝试」——直接复用 ll_world::space_profile 的
        // SpaceProfileTable::define 校验（见其单元测试）；这里只确认
        // register_base_space_profiles 产出的 SpaceProfileTable 确实是
        // 那同一个会拒绝重复定义的类型,没有被本模块的包装弱化掉这条
        // 校验。
        // Arrange
        let mut registry = Registry::new();
        let (ids, mut table) =
            register_base_space_profiles(&mut registry).expect("本体空间层属性声明表内部一致");

        // Act
        let result = table.define(
            ids.surface,
            ll_world::space_profile::SpaceProfileAttrs {
                ambient_light_floor: 0,
                exposed_to_sky: false,
                base_temperature: 0,
                diggable: false,
                buildable: false,
                reverb_tag: None,
            },
        );

        // Assert
        assert!(result.is_err());
    }
}
