//! 本体动画剪辑注册——「本体即 Mod」在动画上的落点，照
//! [`crate::base_terrain`]/[`crate::base_placeholder`] 的既有模式。
//!
//! `crate::clip::materialize_base_clips` 定义了本体两段剪辑（行走、
//! 待机）的声明与固定注册顺序，但它本身刻意不知道「谁来分配
//! `ContentIndex`」——签名接受一个解析回调，而不是绑死某个具体类型
//! （见其模块文档）。本模块补上生产路径缺的那一半：把回调实参换成
//! 真正的 [`Registry::intern`]。
//!
//! # 为什么这一步值得单独成模块，而不是内联在别处
//!
//! 与 [`crate::base_terrain`]/[`crate::base_placeholder`] 同一个理由：这是
//! 「本体即 Mod」的检验点——本体剪辑注册与未来 mod 剪辑注册（经
//! `register-animation-clip`，见 [`crate::script_clip_api`]）要走
//! **完全相同**的 [`Registry::intern`] 调用。单独成模块，让
//! [`register_base_clips`] 的实现只有唯一一行真正有意义的代码，任何人
//! 一眼就能看出这里没有任何本体专属的特权通道。

use ll_core::ident::NamespacedId;

use crate::clip::{BaseClipIds, ClipError, ClipTable, materialize_base_clips};
use crate::registry::Registry;

/// 把本体两段剪辑（行走、待机）注册进 `registry`，返回可用的
/// `(BaseClipIds, ClipTable)`。
///
/// **这是本体剪辑唯一的生产注册入口**：内部只是把 `registry.intern`
/// 包成回调传给 [`materialize_base_clips`]——本体剪辑因此与未来 mod
/// 注册的自定义剪辑走同一条 [`Registry::intern`] 调用路径。
///
/// 调用方应在启动时、且仅在此时调用一次；返回的 `BaseClipIds` 此后按
/// 字段访问，是常量级开销，不会把注册表查询带进任何热路径。
pub fn register_base_clips(registry: &mut Registry) -> Result<(BaseClipIds, ClipTable), ClipError> {
    materialize_base_clips(&mut |id: NamespacedId| registry.intern(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 本体剪辑与mod剪辑共用registry同一段连续递增的索引号段() {
        // 用「本体注册完之后，再拿同一个 Registry 直接 intern 一个
        // mod 风格的 id，两者分配到的索引连续递增」证明它们走的是完全
        // 相同的通道，没有任何一条只对本体开放的旁路。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径（结构等价），
        // 不能证明 mod 脚本调得到这套 API。真正的证据在
        // crate::pipeline 的脚本装载测试与
        // mods/example_mod/animation.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (clip_ids, _table) =
            register_base_clips(&mut registry).expect("本体剪辑声明表内部一致");
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:jump_squash").expect("合法标识符"));

        // Assert：mod 内容紧接在本体两段剪辑之后分配到索引，说明两者
        // 共用同一个单调递增的号段，没有为本体预留任何特殊区间。
        assert_eq!(mod_index.get(), clip_ids.hero_idle.get() + 1);
    }

    #[test]
    fn 本体剪辑重复注册返回错误而非静默覆盖() {
        // 简报要求正面处理的已知缺口在注册表层面的验收：模拟另一次
        // 注册尝试——register_base_clips 本身只会调用一次，这里直接
        // 复用 ClipTable::define 的重复定义校验（见其单元测试），确认
        // register_base_clips 产出的 ClipTable 确实是那同一个会拒绝
        // 重复定义的类型，没有被本模块的包装弱化掉这条校验。
        // Arrange
        let mut registry = Registry::new();
        let (clip_ids, mut table) =
            register_base_clips(&mut registry).expect("本体剪辑声明表内部一致");

        // Act
        let result = table.define(clip_ids.hero_walk, ll_render::anim::base_hero_clips().0);

        // Assert
        assert!(result.is_err());
    }
}
