//! 本体天气注册——「本体即 Mod」在 `Weather` 上的落点。
//!
//! `ll_world::weather::materialize_base_weathers` 定义了本体全部六种
//! 天气的声明与固定注册顺序，但它本身刻意不知道「谁来分配
//! `ContentIndex`」——签名接受一个解析回调，而不是绑死某个具体类型
//! （见其模块文档「为什么天气表定义在 `ll-world`」一节）。本模块补上
//! 生产路径缺的那一半：把回调实参换成真正的 [`Registry::intern`]。
//!
//! # 为什么这一步值得单独成模块
//!
//! 与 [`crate::base_terrain`]/[`crate::base_space_profile`] 同一个理由：
//! 这是「本体即 Mod」的检验点——本体天气注册与 mod 天气注册要走**完全
//! 相同**的 [`Registry::intern`] 调用。单独成模块，让
//! [`register_base_weathers`] 的实现只有唯一一行真正有意义的代码，任何
//! 人一眼就能看出这里没有任何本体专属的特权通道。

use ll_core::ident::NamespacedId;
use ll_world::weather::{BaseWeatherIds, WeatherError, WeatherTable, materialize_base_weathers};

use crate::registry::Registry;

/// 把本体全部六种天气注册进 `registry`，返回可用的
/// `(BaseWeatherIds, WeatherTable)`。
///
/// **这是本体天气唯一的生产注册入口**：内部只是把 `registry.intern`
/// 包成回调传给 [`materialize_base_weathers`]——本体天气因此与 mod 注册
/// 的自定义天气走同一条 [`Registry::intern`] 调用路径，`Registry` 内部
/// 完全无法区分某次 `intern` 调用来自本体还是 mod。
///
/// 调用方应在启动时、且仅在此时调用一次；返回的 `BaseWeatherIds` 此后
/// 按字段访问，是常量级开销，不会把注册表查询带进任何热路径。
pub fn register_base_weathers(
    registry: &mut Registry,
) -> Result<(BaseWeatherIds, WeatherTable), WeatherError> {
    materialize_base_weathers(&mut |id: NamespacedId| registry.intern(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

    #[test]
    fn 本体天气与mod内容共用registry同一段连续递增的索引号段() {
        // 与 base_space_profile 同一条验收手法：本体注册完之后，再拿同
        // 一个 Registry 直接 intern 一个 mod 风格的 id，两者分配到的索引
        // 连续递增，说明它们走的是完全相同的通道，没有只对本体开放的
        // 旁路。
        //
        // 边界：本测试只证明本体与 mod 走同一条注册路径（结构等价），
        // 不能证明 mod 脚本调得到这套 API。真正的证据在 crate::pipeline
        // 的脚本装载测试与 mods/example_mod/weather.scm。
        // Arrange
        let mut registry = Registry::new();

        // Act
        let (ids, _table) = register_base_weathers(&mut registry).expect("本体天气声明表内部一致");
        let mod_index =
            registry.intern(NamespacedId::parse("yourmod:ashfall").expect("合法标识符"));

        // Assert：materialize_base_weathers 内部注册顺序的最后一个是 snow。
        assert_eq!(mod_index.get(), ids.snow.get() + 1);
    }

    #[test]
    fn 本体天气重复注册返回错误而非静默覆盖() {
        // register_base_weathers 本身只会调用一次，这里模拟「另一次注册
        // 尝试」——确认本模块的包装没有弱化 WeatherTable::define 的重复
        // 定义校验。
        // Arrange
        let mut registry = Registry::new();
        let (ids, mut table) =
            register_base_weathers(&mut registry).expect("本体天气声明表内部一致");

        // Act
        let result = table.define(
            ids.clear,
            ll_world::weather::WeatherAttrs {
                display_name_key: NamespacedId::parse("lostland:weather.clear.display_name")
                    .expect("合法标识符"),
                light_scale: 1000,
                sight_scale: 1000,
                temperature_offset: 0,
                season_weights: [1, 1, 1, 1],
            },
        );

        // Assert
        assert!(result.is_err());
    }
}
