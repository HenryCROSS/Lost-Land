//! 把 `register-weather` 注册进脚本引擎：mod 脚本借此定义自定义天气
//! （晴/阴/雨/大风/雾/雪之外的第七、第八种天气）。
//!
//! # ADR 0018 归类：天气是玩法层内容
//!
//! 按 ADR 0018「归类判据」三步法：
//!
//! 1. **有没有设计自由度**——有。「雾把视野压到多短」「冬天有多大概率
//!    下雪」「阴天暗到什么程度」全是设计选择，不是工程正确性问题：一个
//!    走「天气温和、只做氛围」路线的 mod 与一个走「暴风雪能让人寸步难
//!    行」路线的 mod 都同样「工程正确」。
//! 2. **自由度落在算法还是数据**——落在**数据**。
//!    [`ll_world::weather::weather_kind_at`] 的加权选取算法本身、
//!    [`ll_world::light::ambient_light_under`] 的三因子相乘规则本身，
//!    都仍然是引擎层的原生 Rust，本模块不把它们开放给脚本；开放的只是
//!    它们读的那张表。这与 ADR 0018 表格里「寻路算法是引擎层，它读的
//!    地形代价表是玩法层」逐字同构。
//! 3. **高频调用**——不适用：本表按 ADR 0016/0017 第一档物化成扁平列，
//!    查询是常量级下标访问。这一条对天气尤其要紧，环境光是每帧每格都
//!    要算的热路径，绝不能在里面引入跨脚本边界调用（每次 326ns）——
//!    脚本只在**装载期**跑一次把数据写进表，运行期一次都不再进脚本。
//!
//! # 为什么放在 `ll-mod` 而不是 `ll-script`
//!
//! 理由与 [`crate::script_space_profile_api`] 逐字相同：注册函数需要
//! 同时持有 [`crate::registry::Registry`]（`ll-mod`）与
//! `ll_world::weather::WeatherTable`（`ll-world`）的可变引用，而依赖
//! 方向是 `ll-script` ← `ll-mod`，`ll-script` 不认识、也不该认识
//! `ll-mod` 的类型。
//!
//! # `thread_local!` 与 `Registry` 的分工
//!
//! 同样照抄 [`crate::script_space_profile_api`]：`Registry` 走
//! [`crate::active_registry`] 的**共享**目标（同一个脚本文件里
//! `register-terrain` 与 `register-weather` 必须共用同一个 `Registry`
//! 实例，否则 `ContentIndex` 会在两类内容之间撞车），本模块只持有
//! `WeatherTable` 自己那一份。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_world::weather::{WeatherAttrs, WeatherError, WeatherTable};

use crate::active_registry::with_active_registry;
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-weather` 应该写入的天气表。`Registry`
    /// 走 [`crate::active_registry`] 的共享目标，理由见模块文档。
    static ACTIVE_TABLE: RefCell<Option<WeatherTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-weather` 可写入的目标，取走
/// 其所有权。`Registry` 由调用方另行调用
/// [`crate::active_registry::set_active_registry`] 设置。
pub fn set_active_target(table: WeatherTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `WeatherTable`。
///
/// 调用约定与 [`crate::script_space_profile_api::take_active_target`]
/// 完全一致：**必须**与 [`set_active_target`] 成对出现。没有先
/// `set_active_target` 就调用会 panic——这不是脚本触发得到的路径（脚本
/// 只能调用 `register-weather`，够不到这两个函数），而是装载管线自身的
/// 接线契约。
pub fn take_active_target() -> WeatherTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-weather` 注册进 `engine`。
///
/// **必须**在调用 [`set_active_target`] 之后、[`ScriptEngine::load_source`]
/// 求值脚本之前完成注册，理由见
/// [`crate::script_terrain_api::register_terrain_api`]。
pub fn register_weather_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-weather", register_weather);
}

/// `(register-weather id display-name-key light-scale sight-scale
///                    temperature-offset
///                    spring-weight summer-weight autumn-weight winter-weight)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"yourmod:ashfall"`。
/// - `display-name-key`：展示名的 Fluent 本地化键，完整命名空间标识符
///   字符串，如 `"yourmod:weather.ashfall.display_name"`。状态栏
///   （`ll_ui::hud::status_bar`）拿它查出天气名给玩家看，因此**每种
///   天气都必须有名字**，不像 `register-space-profile` 的 `reverb-tag`
///   那样接受空串哨兵——一种玩家看得见却没有名字的天气只会在状态栏里
///   显示成键名本身。
/// - `light-scale`：环境光乘数，千分比整数，必须落在 `0..=1000`。晴天
///   是 1000（不缩放）。
/// - `sight-scale`：视野半径乘数，千分比整数，必须落在 `0..=1000`。
///   **不允许超过 1000**——视野「放大」是暗视这类观察者属性该做的事，
///   不是天气，见 `ll_world::weather::WeatherError::SightScaleOutOfRange`。
/// - `temperature-offset`：温度增量偏移，**十分之一摄氏度**的整数
///   （`-50` 即比不受天气影响时冷 5℃），必须落在
///   `ll_world::weather::WEATHER_TEMPERATURE_OFFSET_LIMIT` 的正负范围内
///   （±500，即 ±50℃）。取 0 表示这种天气不影响温度（本体的「晴」就是
///   0）。**是增量不是乘数**——温度没有零点可言，乘法会得出「-20℃ 打
///   八折等于变暖」这种荒谬结论，见
///   `ll_world::weather::WeatherDef::temperature_offset` 文档。
/// - `spring-weight`/`summer-weight`/`autumn-weight`/`winter-weight`：
///   四季各自的出现权重，非负整数。相对值，不必加起来等于任何特定的
///   数；取 0 表示这种天气在这一季**绝不出现**。四季拆成四个参数而不是
///   一个列表，理由是 ADR 0020 的注册函数签名纪律：只接受整数/`Milli`/
///   布尔/字符串这几种标量，不接受聚合类型。
///
/// # 全部参数都是整数/布尔/字符串（ADR 0020）
///
/// 没有任何浮点参数：两个乘数是千分比整数，温度偏移是十分之一摄氏度
/// 整数，四个权重是整数。天气经 `ambient_light_under` →
/// `effective_ambient_light` → `effective_sight_radius` 影响视野半径，
/// 又经 `ll_world::space_profile::effective_temperature` →
/// `ll_sim::exposure` 影响结算属性，两条都属于 ADR 0020 的乙区，必须
/// 量化——脚本侧连表达一个浮点的机会都不给。
///
/// # 参数增加是一次破坏性签名变更，且是刻意的
///
/// 温度系统批次在第四个参数之后插入了 `temperature-offset`，既有的
/// `(register-weather ...)` 调用会因为实参数量不符而在**装载期**报错，
/// 不会静默把权重错位读成温度。这正是想要的行为：把一个新的必填内容
/// 维度做成可选参数（或追加到末尾并给默认值），只会让 mod 作者在不知
/// 情的情况下发布一批「温度偏移全是 0」的天气，而那与「这种天气不影响
/// 温度」在数据上不可区分——`content_audit` 的字段覆盖检查要问的正是
/// 「有没有内容真的用上了这个旋钮」。仓库内唯一的既有调用点
/// （`mods/example_mod/weather.json5`）已随本批次更新。
///
/// 返回 `Result<bool, String>`，错误处理约定见
/// [`crate::script_terrain_api`] 同名一段。
#[allow(clippy::too_many_arguments)]
fn register_weather(
    id: String,
    display_name_key: String,
    light_scale: i64,
    sight_scale: i64,
    temperature_offset: i64,
    spring_weight: i64,
    summer_weight: i64,
    autumn_weight: i64,
    winter_weight: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                // 装载管线接线错误（忘了先 set_active_target）——不是 mod
                // 作者能触发的情形，但脚本调用不能 panic（四道防线①②），
                // 只能降级成一条错误消息。
                return Err("register-weather 在没有活跃天气表的窗口内被调用".to_string());
            };
            do_register_weather(
                registry,
                table,
                &id,
                &display_name_key,
                light_scale,
                sight_scale,
                temperature_offset,
                [spring_weight, summer_weight, autumn_weight, winter_weight],
            )
        })
    })
}

/// [`register_weather`] 的纯函数核心：不依赖线程局部状态，方便单元测试
/// 不必绕过 `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_weather(
    registry: &mut Registry,
    table: &mut WeatherTable,
    id: &str,
    display_name_key: &str,
    light_scale: i64,
    sight_scale: i64,
    temperature_offset: i64,
    season_weights: [i64; 4],
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let parsed_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法 display-name-key 标识符 {display_name_key:?}：{err}"))?;

    // i64 → i32/u32 的窄化：Steel 整数的宿主表示是 i64，而表里两个乘数
    // 是 i32、四个权重是 u32。**不钳位、直接拒绝**——与
    // `register-space-profile` 处理 `ambient-light-floor` 同一条纪律：
    // 越界的乘数本来就会被 WeatherTable::define 的 0..=1000 校验拒绝，
    // 先钳成 i32::MAX 只会把错误消息变得更难懂；负权重更没有「最接近的
    // 合理值」可言（钳成 0 会把「这里写错了」悄悄变成「这一季不出现」，
    // 是最糟的一种静默）。
    let light_scale = i32::try_from(light_scale).map_err(|_| {
        format!("light-scale {light_scale} 超出 32 位整数范围，合法区间是 0..=1000")
    })?;
    let sight_scale = i32::try_from(sight_scale).map_err(|_| {
        format!("sight-scale {sight_scale} 超出 32 位整数范围，合法区间是 0..=1000")
    })?;
    // 同一条「不钳位、直接拒绝」的纪律：越界的偏移本来就会被
    // WeatherTable::define 的 ±500 校验拒绝，先钳成 i32::MAX 只会把错误
    // 消息变得更难懂。
    let temperature_offset = i32::try_from(temperature_offset).map_err(|_| {
        format!("temperature-offset {temperature_offset} 超出 32 位整数范围，合法区间是 ±500")
    })?;

    const SEASON_NAMES: [&str; 4] = ["spring", "summer", "autumn", "winter"];
    let mut weights = [0u32; 4];
    for (slot, raw) in season_weights.iter().enumerate() {
        weights[slot] = u32::try_from(*raw).map_err(|_| {
            format!(
                "{}-weight {raw} 不是合法的非负 32 位权重",
                SEASON_NAMES[slot]
            )
        })?;
    }

    let index = registry.intern(parsed_id);

    table
        .define(
            index,
            WeatherAttrs {
                display_name_key: parsed_key,
                light_scale,
                sight_scale,
                temperature_offset,
                season_weights: weights,
            },
        )
        .map(|()| true)
        .map_err(|err: WeatherError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::{TICKS_PER_DAY, Tick};
    use ll_world::weather::Weather;

    #[test]
    fn 合法天气声明注册成功并写入表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:ashfall",
            "yourmod:weather.ashfall.display_name",
            420,
            360,
            0,
            [1, 2, 3, 4],
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:ashfall").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.is_defined(index));
        assert_eq!(table.light_scale(index), 420);
        assert_eq!(table.sight_scale(index), 360);
        assert_eq!(table.season_weights(index), [1, 2, 3, 4]);
        assert_eq!(
            table.display_name_key(index),
            Some(NamespacedId::parse("yourmod:weather.ashfall.display_name").unwrap())
        );
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "Not Valid",
            "yourmod:weather.x.display_name",
            1000,
            1000,
            0,
            [1, 1, 1, 1],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 非法展示名键返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:ashfall",
            "",
            1000,
            1000,
            0,
            [1, 1, 1, 1],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 光照乘数越界时返回weathertable的校验错误() {
        // Arrange：1001 超出 0..=1000（ADR 0017「注册期完整校验」）。
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:toobright",
            "yourmod:weather.toobright.display_name",
            1001,
            1000,
            0,
            [1, 1, 1, 1],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 负权重返回错误而不是被静默钳成零() {
        // 钳成 0 会把「这里写错了」悄悄变成「这一季不出现」，是最糟的
        // 一种静默——mod 作者永远不会发现自己的天气从不出现。
        // Arrange
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:ashfall",
            "yourmod:weather.ashfall.display_name",
            500,
            500,
            0,
            [-1, 1, 1, 1],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 超出32位的数值返回错误而不是静默截断() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:overflow",
            "yourmod:weather.overflow.display_name",
            i64::from(i32::MAX) + 1,
            1000,
            0,
            [1, 1, 1, 1],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 同一个id重复声明时返回重复定义错误() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();
        do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:ashfall",
            "yourmod:weather.ashfall.display_name",
            500,
            500,
            0,
            [1, 1, 1, 1],
        )
        .expect("第一次注册应当成功");

        // Act
        let result = do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:ashfall",
            "yourmod:weather.ashfall.display_name",
            700,
            700,
            0,
            [2, 2, 2, 2],
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 脚本注册的天气能被真正派生出来并压暗环境光() {
        // 本模块存在意义的落点：脚本注册出来的天气，喂进既有的
        // Weather::derive 与 ambient_light_under 之后，语义与 Rust 注册
        // 出来的逐字相同。
        // Arrange：只注册一种天气，权重非零，因此它必然被抽中。
        let mut registry = Registry::new();
        let mut table = WeatherTable::new();
        do_register_weather(
            &mut registry,
            &mut table,
            "yourmod:ashfall",
            "yourmod:weather.ashfall.display_name",
            300,
            400,
            0,
            [5, 5, 5, 5],
        )
        .expect("合法声明应当注册成功");
        let index = registry
            .get(&NamespacedId::parse("yourmod:ashfall").unwrap())
            .expect("刚注册的内容应能查到索引");
        let noon = Tick(30 * TICKS_PER_DAY + TICKS_PER_DAY / 2);

        // Act
        let weather = Weather::derive(1234, noon, &table);
        let dimmed = ll_world::light::ambient_light_under(noon, weather);
        let clear = ll_world::light::ambient_light(noon);

        // Assert
        assert_eq!(weather.kind, Some(index));
        assert_eq!((weather.light_scale, weather.sight_scale), (300, 400));
        assert!(dimmed < clear, "脚本注册的天气必须真的压暗环境光");
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_weather() {
        // 端到端验证：脚本里写 (register-weather ...)，不需要脚本作者
        // 知道 Rust 侧的 Registry/WeatherTable 是怎么接线的。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_weather_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(WeatherTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-weather "yourmod:ashfall" "yourmod:weather.ashfall.display_name" 420 360 -35 1 2 3 4)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok(), "脚本求值应当成功：{result:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:ashfall").unwrap())
            .expect("脚本注册的天气应当进了注册表");
        assert!(table.is_defined(index));
        assert_eq!(table.season_weights(index), [1, 2, 3, 4]);
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误，宿主必须优雅报错。
        let mut engine = ScriptEngine::new();
        register_weather_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(WeatherTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-weather "Not Valid" "yourmod:weather.x.display_name" 1 1 0 1 1 1 1)"#
                .to_string(),
        );

        // Assert
        assert!(result.is_err());

        // Cleanup：即便脚本出错，接线契约仍要求成对调用，否则下一个测试
        // 用例会因为 thread_local 里残留旧值而互相污染。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
