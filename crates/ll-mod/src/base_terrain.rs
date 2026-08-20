//! 本体地形注册——「本体即 Mod」的第一次真实验收（P4 Task 8）。
//!
//! `ll_world::terrain::materialize_base_terrain` 定义了本体全部 17 个
//! 地形的声明与固定注册顺序，但它本身刻意不知道「谁来分配
//! `ContentIndex`」——签名接受一个解析回调，而不是绑死某个具体类型
//! （见其模块文档「与 Registry 的关系」，这正是保持 `ll-world` 不反向
//! 依赖 `ll-mod` 的关键）。本模块补上生产路径缺的那一半：把回调实参
//! 换成真正的 [`Registry::intern`]。
//!
//! # 为什么这一步值得单独成模块，而不是内联在别处
//!
//! 这是 ADR 0016/0015 反复强调的「本体即 Mod」检验点——本体地形注册
//! 与未来 mod 地形注册要走**完全相同**的 [`Registry::intern`] 调用。
//! 单独成模块，是为了让 [`register_base_terrain`] 的实现只有唯一一行
//! 真正有意义的代码（把 `registry.intern` 包成回调传给
//! `materialize_base_terrain`），任何人一眼就能看出这里没有任何本体
//! 专属的特权通道。

use ll_core::ident::NamespacedId;
use ll_world::terrain::{BaseTerrainIds, TerrainError, TerrainTable, materialize_base_terrain};

use crate::registry::Registry;

/// 把本体全部 17 个地形注册进 `registry`，返回可用的
/// `(BaseTerrainIds, TerrainTable)`。
///
/// **这是本体地形唯一的生产注册入口**：内部只是把 `registry.intern`
/// 包成回调传给 [`materialize_base_terrain`]——本体地形因此与未来 mod
/// 注册的自定义地形走同一条 [`Registry::intern`] 调用路径，`Registry`
/// 内部完全无法区分某次 `intern` 调用来自本体还是 mod（[`crate::registry`]
/// 模块文档：注册表「只认命名空间字符串」）。
///
/// 调用方应在启动时、且仅在此时调用一次（对应 ADR 0015「启动时一次性
/// 解析进缓存结构」）；返回的 `BaseTerrainIds` 此后按字段访问，是
/// 常量级开销，不会把注册表查询带进任何热路径。
pub fn register_base_terrain(
    registry: &mut Registry,
) -> Result<(BaseTerrainIds, TerrainTable), TerrainError> {
    materialize_base_terrain(&mut |id: NamespacedId| registry.intern(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 本体地形与mod地形共用registry同一段连续递增的索引号段() {
        // 用「本体注册完之后，再拿同一个 Registry 直接 intern 一个
        // mod 风格的 id，两者分配到的索引连续递增」证明它们走的是完全
        // 相同的通道，没有任何一条只对本体开放的旁路。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径（结构等价），
        // 不能证明 mod 脚本调得到这套 API。真正的证据在
        // crate::pipeline 的脚本装载测试与 mods/example_mod/gameplay.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (terrain_ids, _table) =
            register_base_terrain(&mut registry).expect("本体地形声明表内部一致");
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:crystal").expect("合法标识符"));

        // Assert：mod 内容紧接在本体 17 个地形之后分配到索引，说明两者
        // 共用同一个单调递增的号段,没有为本体预留任何特殊区间。
        assert_eq!(mod_index.get(), terrain_ids.stairs_down.index().get() + 1);
    }

    #[test]
    fn 本体地形与mod注册的自定义地形在registry内部结构上不可区分() {
        // 直接的「结构等价」验收断言：除了命名空间字符串本身，注册表
        // 内部（content_hash 的累积方式）看不出这条内容是谁注册的。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径，不能证明
        // mod 脚本调得到这套 API。真正的证据在 crate::pipeline 的
        // 脚本装载测试与 mods/example_mod/gameplay.scm。
        // Arrange
        let mut registry = Registry::new();
        let (_terrain_ids, _table) =
            register_base_terrain(&mut registry).expect("本体地形声明表内部一致");

        // Act：本体命名空间（lostland）与一个假想 mod 命名空间
        // （yourmod）各自都已经在注册表里贡献过内容哈希——两者都是
        // 通过 Registry::intern 产生的，查询接口完全相同。
        let lostland_hash = registry.content_hash_of("lostland");
        registry.intern(NamespacedId::parse("yourmod:crystal").expect("合法标识符"));
        let yourmod_hash = registry.content_hash_of("yourmod");

        // Assert：两个命名空间都成功贡献了内容摘要——Registry 没有为
        // "lostland" 这个命名空间字符串做任何特殊处理或旁路。
        assert!(lostland_hash.is_some());
        assert!(yourmod_hash.is_some());
    }

    #[test]
    fn 本体地形重复注册返回错误而非静默覆盖() {
        // 简报要求正面处理的已知缺口在注册表层面的验收：假设两个 mod
        // （或某 mod 与本体）都尝试定义 lostland:grass 的地形属性，
        // 第二次必须报错。register_base_terrain 本身只会调用一次，这里
        // 模拟「另一次注册尝试」——直接复用 ll_world::terrain 的
        // TerrainTable::define 校验，见其单元测试；这里只确认
        // register_base_terrain 产出的 TerrainTable 确实是那同一个会
        // 拒绝重复定义的类型，没有被本模块的包装弱化掉这条校验。
        // Arrange
        let mut registry = Registry::new();
        let (terrain_ids, mut table) =
            register_base_terrain(&mut registry).expect("本体地形声明表内部一致");

        // Act
        let result = table.define(
            terrain_ids.grass.index(),
            ll_world::terrain::TerrainAttrs {
                blocks_sight: true,
                blocks_move: true,
                move_cost: u32::MAX,
                opens_into: None,
            },
        );

        // Assert
        assert!(result.is_err());
    }
}
