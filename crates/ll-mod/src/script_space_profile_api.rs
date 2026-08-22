//! 把 `register-space-profile` 注册进脚本引擎：mod 脚本借此定义自定义
//! 空间层属性（地表/洞窟/地下城/建筑内部之外的第五、第六种空间）。
//!
//! # 为什么这条通道此前是缺的（ADR 0018 的一处真实缺口）
//!
//! 坐标系统重写批次（任务 3）建了 `ll_world::space_profile::SpaceProfileTable`
//! 与 `materialize_base_space_profiles`，随后的补漏批次又补上了
//! [`crate::base_space_profile`] 这条「本体走 `Registry::intern`」的
//! 生产注册路径——但**始终没有脚本侧的注册函数**。结果是：
//! [`crate::pipeline::GameplayTables`] 里其余十五张表都能被 mod 脚本
//! 写入，唯独空间层属性只能由 Rust 写死。
//!
//! 按 ADR 0018「归类判据」三步法，空间层属性明确落在**玩法层**：
//!
//! 1. 第一步（有没有设计自由度）——有。「洞窟是不是伸手不见五指」
//!    「地下城能不能挖」「建筑内部漏进来多少光」全都是设计选择，不是
//!    工程正确性问题：一个走「地表也很暗、全靠火把」路线的 mod 与一个
//!    走「地下城自带幽光」路线的 mod 都同样「工程正确」，差别纯粹是
//!    设计意图。
//! 2. 第二步（自由度落在算法还是数据）——落在**数据**。
//!    [`ll_world::space_profile::effective_ambient_light`] 这条组合
//!    规则本身（露天转发给昼夜曲线、非露天取地板值）仍然是引擎层的
//!    原生 Rust，本模块不把它开放给脚本；开放的只是它读的那张表。
//!    这与 ADR 0018 表格里「寻路算法是引擎层，它读的地形代价表是玩法
//!    层」逐字同构，也与该表格「环境光照计算……曲线参数化留作未来
//!    玩法层扩展点」那一行的方向一致。
//! 3. 第三步（高频调用）——不适用：本表按 ADR 0016/0017 第一档物化成
//!    扁平列，查询是常量级下标访问，与地形表同一套代价。
//!
//! # 为什么放在 `ll-mod` 而不是 `ll-script`
//!
//! 理由与 [`crate::script_terrain_api`] 逐字相同（本模块照抄它已经
//! 验证过的那一套）：注册函数需要同时持有 [`crate::registry::Registry`]
//! （`ll-mod`）与 `ll_world::space_profile::SpaceProfileTable`
//! （`ll-world`）的可变引用，而依赖方向是 `ll-script` ← `ll-mod`，
//! `ll-script` 不认识、也不该认识 `ll-mod` 的类型。
//!
//! # `thread_local!` 与 `Registry` 的分工
//!
//! 同样照抄 [`crate::script_terrain_api`]：`Registry` 走
//! [`crate::active_registry`] 的**共享**目标（同一个脚本文件里
//! `register-terrain` 与 `register-space-profile` 必须共用同一个
//! `Registry` 实例，否则 `ContentIndex` 会在两类内容之间撞车），本
//! 模块只持有 `SpaceProfileTable` 自己那一份。
//!
//! # 内容值哈希不需要为本模块新增任何东西
//!
//! [`crate::content_hash`] 早在「内容值哈希覆盖面扩展批次」就已经把
//! 空间层属性收进覆盖面：`ContentTableKind::SpaceProfile = 7` 判别值、
//! `ContentValueTables::space_profile` 字段、`write_space_profile_fields`
//! 六个字段全在。本模块只是给同一张表**多开一条写入来源**，哈希算法
//! 逐字节不变——因此 `CONTENT_HASH_ALGORITHM_VERSION` 不随本模块递增，
//! 这不是「论证了这次无害所以免于升版号」（那条论证本身会出错，见该
//! 常量文档），而是**根本没有新增任何被哈希的表或字段**。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;
use ll_world::space_profile::{SpaceProfileAttrs, SpaceProfileError, SpaceProfileTable};

use crate::active_registry::with_active_registry;
use crate::registry::Registry;

thread_local! {
    /// 当前调用窗口内，`register-space-profile` 应该写入的空间层属性
    /// 表。`Registry` 走 [`crate::active_registry`] 的共享目标，理由见
    /// 模块文档。
    static ACTIVE_TABLE: RefCell<Option<SpaceProfileTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-space-profile` 可写入的
/// 目标，取走其所有权。`Registry` 由调用方另行调用
/// [`crate::active_registry::set_active_registry`] 设置。
pub fn set_active_target(table: SpaceProfileTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `SpaceProfileTable`。
///
/// 调用约定与 [`crate::script_terrain_api::take_active_target`] 完全
/// 一致：**必须**与 [`set_active_target`] 成对出现。没有先
/// `set_active_target` 就调用会 panic——这不是脚本触发得到的路径
/// （脚本只能调用 `register-space-profile`，够不到这两个函数），而是
/// 装载管线自身的接线契约。
pub fn take_active_target() -> SpaceProfileTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-space-profile` 注册进 `engine`。
///
/// **必须**在调用 [`set_active_target`] 之后、[`ScriptEngine::load_source`]
/// 求值脚本之前完成注册，理由见 [`crate::script_terrain_api::register_terrain_api`]。
pub fn register_space_profile_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-space-profile", register_space_profile);
}

/// `(register-space-profile id ambient-light-floor exposed-to-sky
///                          base-temperature diggable buildable reverb-tag)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"yourmod:abyss"`。
/// - `ambient-light-floor`：环境光地板值，千分比整数，必须落在
///   `0..=1000`（与 `ll_world::light::LightLevel` 同一量纲）。**仅在
///   `exposed-to-sky` 为假时生效**——为真时这一格空间的环境光完全
///   跟随世界时钟，地板值不参与运算，见
///   [`ll_world::space_profile::effective_ambient_light`]。
/// - `exposed-to-sky`：布尔。为真表示露天，环境光受昼夜/四季（未来还有
///   天气）影响；为假表示封闭空间，环境光恒等于 `ambient-light-floor`。
/// - `base-temperature`：温度基准，整数。当前**没有消费方**——温度/
///   保暖系统尚不存在，这个字段能被内容设定但暂时不影响任何结算，见
///   `ll_world::space_profile::SpaceProfile::base_temperature`。
/// - `diggable`/`buildable`：布尔。同样**暂无消费方**——采矿/营造动作
///   尚不存在。之所以现在就开放给脚本，是因为这两个字段早已是
///   `SpaceProfileAttrs` 的一部分、也早已进内容值哈希，若只开放一部分
///   字段，脚本注册出来的层属性就会与 Rust 注册出来的**不同构**，本
///   模块要补的恰恰是「同一条通道」这件事。
/// - `reverb-tag`：音效环境标签的完整标识符字符串，空串 `""` 表示
///   「没有」——与 `register-terrain` 的 `opens-into` 同一套哨兵约定
///   （Steel 的 FFI 转换层没有现成的 `Option<String>`，而合法的命名
///   空间字符串恒非空，不会与真实标识符混淆）。**注意与 `opens-into`
///   的一处实质差异**：`reverb_tag` 存的是字面 `NamespacedId`，不是
///   `ContentIndex`，因此这里**不**对它调用 `registry.intern`——它不
///   指向任何一张内容表里的条目，见 [`crate::content_hash`] 模块文档
///   「`ContentIndex` 字段」一节倒数第二段。
///
/// # 全部参数都是整数/布尔/字符串（ADR 0020）
///
/// 没有任何浮点参数：`ambient_light_floor` 是千分比整数，
/// `base_temperature` 是整数。空间层属性会经
/// `effective_ambient_light` → `effective_sight_radius` 影响视野半径、
/// 进而影响 `ExplorationMemory`（世界状态），属于 ADR 0020 的乙区，
/// 必须量化——脚本侧连表达一个浮点的机会都不给。
///
/// 返回 `Result<bool, String>`，错误处理约定见
/// [`crate::script_terrain_api`] 同名一段。
fn register_space_profile(
    id: String,
    ambient_light_floor: i64,
    exposed_to_sky: bool,
    base_temperature: i64,
    diggable: bool,
    buildable: bool,
    reverb_tag: String,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                // 装载管线接线错误（忘了先 set_active_target）——不是 mod
                // 作者能触发的情形，但脚本调用不能 panic（四道防线①②），
                // 只能降级成一条错误消息。
                return Err(
                    "register-space-profile 在没有活跃空间层属性表的窗口内被调用".to_string(),
                );
            };
            do_register_space_profile(
                registry,
                table,
                &id,
                ambient_light_floor,
                exposed_to_sky,
                base_temperature,
                diggable,
                buildable,
                &reverb_tag,
            )
        })
    })
}

/// [`register_space_profile`] 的纯函数核心：不依赖线程局部状态，方便
/// 单元测试不必绕过 `thread_local!`。
#[allow(clippy::too_many_arguments)]
fn do_register_space_profile(
    registry: &mut Registry,
    table: &mut SpaceProfileTable,
    id: &str,
    ambient_light_floor: i64,
    exposed_to_sky: bool,
    base_temperature: i64,
    diggable: bool,
    buildable: bool,
    reverb_tag: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;

    let reverb_tag = if reverb_tag.is_empty() {
        None
    } else {
        Some(
            NamespacedId::parse(reverb_tag)
                .map_err(|err| format!("非法 reverb-tag 标识符 {reverb_tag:?}：{err}"))?,
        )
    };

    // i64 → i32 的窄化：Steel 整数的宿主表示是 i64，而两个数值字段在
    // 表里都是 i32。**不钳位、直接拒绝**——与 `register-terrain` 把
    // 负的 `move_cost` 钳成 0 是刻意的不同处理：那里钳位是因为「负代价」
    // 在数据层面只是取舍（0 是一个有意义的答案）；这里两个字段都没有
    // 「超出 i32 时最接近的合理值是多少」这种答案（越界的
    // ambient_light_floor 本来就会被 SpaceProfileTable::define 的
    // 0..=1000 校验拒绝，先钳成 i32::MAX 只会把错误消息变得更难懂），
    // 按 ADR 0017「注册期完整校验」在这里就报错更诚实。
    let ambient_light_floor = i32::try_from(ambient_light_floor).map_err(|_| {
        format!("ambient-light-floor {ambient_light_floor} 超出 32 位整数范围，合法区间是 0..=1000")
    })?;
    let base_temperature = i32::try_from(base_temperature)
        .map_err(|_| format!("base-temperature {base_temperature} 超出 32 位整数范围"))?;

    let index = registry.intern(parsed_id);

    table
        .define(
            index,
            SpaceProfileAttrs {
                ambient_light_floor,
                exposed_to_sky,
                base_temperature,
                diggable,
                buildable,
                reverb_tag,
            },
        )
        .map(|()| true)
        .map_err(|err: SpaceProfileError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::Tick;
    use ll_world::space_profile::{SpaceProfile, effective_ambient_light};

    #[test]
    fn 合法空间层属性声明注册成功并写入表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();

        // Act
        let result = do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:abyss",
            0,
            false,
            -40,
            true,
            false,
            "",
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:abyss").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.is_defined(index));
        assert!(!table.exposed_to_sky(index));
        assert_eq!(table.ambient_light_floor(index), 0);
        assert_eq!(table.base_temperature(index), -40);
        assert!(table.diggable(index));
        assert!(!table.buildable(index));
        assert_eq!(table.reverb_tag(index), None);
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();

        // Act
        let result = do_register_space_profile(
            &mut registry,
            &mut table,
            "Not Valid",
            0,
            false,
            0,
            false,
            false,
            "",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 环境光地板值越界时返回spaceprofiletable的校验错误() {
        // Arrange：1001 超出 0..=1000（ADR 0017「注册期完整校验」）。
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();

        // Act
        let result = do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:too_bright",
            1001,
            false,
            0,
            false,
            false,
            "",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 超出32位的数值返回错误而不是静默截断() {
        // Arrange：Steel 侧能表达 i64，表里存的是 i32——截断会让一个
        // 荒谬的输入变成一个看起来合理的值，必须拒绝。
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();

        // Act
        let result = do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:overflow",
            0,
            false,
            i64::from(i32::MAX) + 1,
            false,
            false,
            "",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 同一个id重复声明时返回重复定义错误() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();
        do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:abyss",
            0,
            false,
            0,
            false,
            false,
            "",
        )
        .expect("第一次注册应当成功");

        // Act
        let result = do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:abyss",
            500,
            true,
            0,
            false,
            false,
            "",
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn reverb_tag非空时按字面标识符存下且不进注册表() {
        // Arrange：reverb_tag 存的是字面 NamespacedId，不是 ContentIndex
        // ——不该被 intern 进 Registry（见本函数文档）。
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();

        // Act
        let result = do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:cathedral",
            120,
            false,
            180,
            false,
            true,
            "yourmod:long_reverb",
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:cathedral").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert_eq!(
            table.reverb_tag(index),
            Some(NamespacedId::parse("yourmod:long_reverb").unwrap())
        );
        assert!(
            registry
                .get(&NamespacedId::parse("yourmod:long_reverb").unwrap())
                .is_none(),
            "reverb_tag 是字面标识符，不指向任何内容表条目，不该被 intern"
        );
    }

    #[test]
    fn 脚本注册的非露天空间环境光与世界时钟无关() {
        // 本测试是本模块存在意义的落点：脚本注册出来的层属性，喂进
        // 既有的 effective_ambient_light 之后，语义与 Rust 注册出来的
        // 逐字相同——非露天空间正午与午夜一样暗。
        // Arrange
        let mut registry = Registry::new();
        let mut table = SpaceProfileTable::new();
        do_register_space_profile(
            &mut registry,
            &mut table,
            "yourmod:abyss",
            30,
            false,
            0,
            true,
            false,
            "",
        )
        .expect("合法声明应当注册成功");
        let index = registry
            .get(&NamespacedId::parse("yourmod:abyss").unwrap())
            .expect("刚注册的内容应能查到索引");
        let profile = SpaceProfile {
            id: NamespacedId::parse("yourmod:abyss").unwrap(),
            ambient_light_floor: table.ambient_light_floor(index),
            exposed_to_sky: table.exposed_to_sky(index),
            base_temperature: table.base_temperature(index),
            diggable: table.diggable(index),
            buildable: table.buildable(index),
            reverb_tag: table.reverb_tag(index),
        };

        // Act
        let midnight = effective_ambient_light(&profile, Tick(0));
        let noon = effective_ambient_light(&profile, Tick(ll_core::time::TICKS_PER_DAY / 2));

        // Assert
        assert_eq!(midnight, noon, "非露天空间的环境光不随世界时钟变化");
        assert_eq!(midnight.0, 30, "恒等于脚本声明的 ambient-light-floor");
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_space_profile() {
        // 端到端验证：脚本里写 (register-space-profile ...)，不需要脚本
        // 作者知道 Rust 侧的 Registry/SpaceProfileTable 是怎么接线的。
        // Arrange
        let mut engine = ScriptEngine::new();
        register_space_profile_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SpaceProfileTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-space-profile "yourmod:abyss" 0 #f -40 #t #f "")"#.to_string(),
        );

        // Assert
        assert!(result.is_ok(), "脚本求值应当成功：{result:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:abyss").unwrap())
            .expect("脚本注册的空间层属性应当进了注册表");
        assert!(table.is_defined(index));
        assert!(!table.exposed_to_sky(index));
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误，宿主必须优雅报错。
        let mut engine = ScriptEngine::new();
        register_space_profile_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SpaceProfileTable::new());

        // Act
        let result = engine
            .load_source(r#"(register-space-profile "Not Valid" 0 #f 0 #f #f "")"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：即便脚本出错，接线契约仍要求成对调用，否则下一个测试
        // 用例会因为 thread_local 里残留旧值而互相污染。
        take_active_target();
        crate::active_registry::take_active_registry();
    }
}
