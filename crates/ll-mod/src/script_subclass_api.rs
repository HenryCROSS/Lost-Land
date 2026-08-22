//! 把 `register-subclass`/`register-subclass-unlock` 注册进脚本引擎：
//! mod 脚本借此定义自定义副职，以及「做满多少次什么事就能拿到它」。
//!
//! `register-subclass` 的模式与 [`crate::script_class_api`] 完全相同
//! （`SubclassDef` 本身比 `ClassDef` 更简单，只有 `id`/`display_name_key`
//! 两个字段，见 `crate::subclass` 模块文档「裁定 P5-4」一节）。
//!
//! `register-subclass-unlock`（副职获得机制批次新增）是注册后追加的第二
//! 个函数，模式照抄同样是「注册后追加」的
//! [`crate::script_recipe_category_api`]——命名上的取舍见该函数自己的
//! 文档「命名为什么是 `register-`」一节。

use std::cell::RefCell;

use ll_core::ident::NamespacedId;
use ll_script::host::ScriptEngine;

use crate::active_registry::with_active_registry;
use crate::registry::Registry;
use crate::subclass::{SubclassAttrs, SubclassError, SubclassTable};

thread_local! {
    /// 当前调用窗口内，`register-subclass` 应该写入的副职表。
    static ACTIVE_TABLE: RefCell<Option<SubclassTable>> = const { RefCell::new(None) };
}

/// 把 `table` 设为当前调用窗口内 `register-subclass` 可写入的目标。
pub fn set_active_target(table: SubclassTable) {
    ACTIVE_TABLE.with(|cell| *cell.borrow_mut() = Some(table));
}

/// 取回 [`set_active_target`] 放进去的 `SubclassTable`。
pub fn take_active_target() -> SubclassTable {
    ACTIVE_TABLE.with(|cell| {
        cell.borrow_mut()
            .take()
            .expect("take_active_target 必须与 set_active_target 成对调用")
    })
}

/// 把 `register-subclass`/`register-subclass-unlock` 注册进 `engine`。
pub fn register_subclass_api(engine: &mut ScriptEngine) {
    engine.register_fn("register-subclass", register_subclass);
    engine.register_fn("register-subclass-unlock", register_subclass_unlock);
}

/// `register-subclass-unlock` 目前唯一接受的触发器种类。
///
/// 设计文档四节还列了 `"items-gathered"`/`"rests-taken"` 两种，本批次
/// **刻意不实现**，理由见 `ll_sim::subclass::SubclassUnlockCatalog` 文档
/// 「为什么只有制作这一种触发器」一节（前者要指向的「物品类别」这个
/// 内容表根本不存在；后者今天没有任何消费者）。参数保留在签名里而不是
/// 砍掉，是为了第二种触发器落地时不需要做一次破坏性的参数位置变更——
/// 传了别的值会**当场报错并列出支持的取值**，不会静默被当成制作。
const TRIGGER_ITEMS_CRAFTED: &str = "items-crafted";

/// `(register-subclass id display-name-key)`。
///
/// - `id`：完整命名空间标识符字符串，如 `"yourmod:shadowdancer"`。
/// - `display-name-key`：指向 Fluent 本地化键的完整标识符字符串。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_subclass(id: String, display_name_key: String) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-subclass 在没有活跃副职表的窗口内被调用".to_string());
            };
            do_register_subclass(registry, table, &id, &display_name_key)
        })
    })
}

/// [`register_subclass`] 的纯函数核心，方便单元测试不必绕过
/// `thread_local!`。
fn do_register_subclass(
    registry: &mut Registry,
    table: &mut SubclassTable,
    id: &str,
    display_name_key: &str,
) -> Result<bool, String> {
    let parsed_id =
        NamespacedId::parse(id).map_err(|err| format!("非法内容标识符 {id:?}：{err}"))?;
    let index = registry.intern(parsed_id);

    let display_name_key = NamespacedId::parse(display_name_key)
        .map_err(|err| format!("非法本地化键标识符 {display_name_key:?}：{err}"))?;

    table
        .define(index, SubclassAttrs { display_name_key })
        .map(|()| true)
        .map_err(|err: SubclassError| err.to_string())
}

/// `(register-subclass-unlock subclass-id trigger-kind trigger-target
///   threshold)`——给一个已注册的副职追加它的获得条件。
///
/// - `subclass-id`：已经通过 `register-subclass` 注册过的完整标识符
///   （**要求已存在**，本函数不 `intern`，只 `get`）。
/// - `trigger-kind`：目前只接受 `"items-crafted"`，见
///   [`TRIGGER_ITEMS_CRAFTED`]。
/// - `trigger-target`：`"items-crafted"` 时是**已注册的配方类别** id
///   （同样只 `get` 不 `intern`——指向一个不存在的类别的获得条件谁都
///   触发不了，且完全不会报错，是最难查的一类内容 bug；这条与
///   `recipe-category-requires-subclass!` 对副职的处理逐字同源）。
/// - `threshold`：达标次数，≥ 1。
///
/// # 命名为什么是 `register-` 而不是设计文档建议的 `subclass-unlocks-via!`
///
/// 本批次逐条核实了 `crates/ll-mod/src/**` 里经 `register_fn` 注册的全部
/// 脚本函数名（含本函数共 41 个）：**36 个是 `register-<主体>[-<附加物>]`**，
/// 只有 3 个带 `!` 后缀，另外 2 个是 `?` 后缀的运行期查询
/// （`self-has-profession?`/`skill-ready?`，不是装载期注册函数）。而那 3 个
/// 带 `!` 的（`recipe-requires-station!`/`recipe-requires-tool!`/
/// `recipe-category-requires-subclass!`）**共享的不是「追加」这个语义，
/// 是字面上的 `-requires-` 这个谓词**——`!` 是那句英文短语的一部分，
/// 不是一个通用的「就地修改」标记（`register-class-trait`/
/// `register-race-starting-item`/`register-item-stat-bonus` 同样是往一份
/// 已注册的定义上追加，一个 `!` 都没有）。
///
/// 「副职的获得条件」不是一道 `requires` 闸门（它不拦住任何动作，只声明
/// 一个达标即触发的条件），因此叫 `subclass-unlocks-via!` 会凭空造出
/// **第三条**命名惯例。它落在多数派那一条里：与 `register-race-xp-reward`
/// （给已注册的种族追加一个数值声明）结构完全同构。
///
/// 返回 `Result<bool, String>`，理由同 `register_terrain` 文档。
fn register_subclass_unlock(
    subclass_id: String,
    trigger_kind: String,
    trigger_target: String,
    threshold: i64,
) -> Result<bool, String> {
    with_active_registry(|registry| {
        ACTIVE_TABLE.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(table) = slot.as_mut() else {
                return Err("register-subclass-unlock 在没有活跃副职表的窗口内被调用".to_string());
            };
            do_register_subclass_unlock(
                registry,
                table,
                &subclass_id,
                &trigger_kind,
                &trigger_target,
                threshold,
            )
        })
    })
}

/// [`register_subclass_unlock`] 的纯函数核心。
fn do_register_subclass_unlock(
    registry: &Registry,
    table: &mut SubclassTable,
    subclass_id: &str,
    trigger_kind: &str,
    trigger_target: &str,
    threshold: i64,
) -> Result<bool, String> {
    if trigger_kind != TRIGGER_ITEMS_CRAFTED {
        return Err(format!(
            "未知的副职获得条件触发器 {trigger_kind:?}，当前支持的取值只有 {TRIGGER_ITEMS_CRAFTED:?}"
        ));
    }
    let parsed_subclass = NamespacedId::parse(subclass_id)
        .map_err(|err| format!("非法副职标识符 {subclass_id:?}：{err}"))?;
    let Some(subclass_index) = registry.get(&parsed_subclass) else {
        return Err(format!(
            "副职 {subclass_id:?} 尚未通过 register-subclass 注册"
        ));
    };
    let parsed_category = NamespacedId::parse(trigger_target)
        .map_err(|err| format!("非法配方类别标识符 {trigger_target:?}：{err}"))?;
    let Some(category_index) = registry.get(&parsed_category) else {
        return Err(format!(
            "配方类别 {trigger_target:?} 尚未通过 register-recipe-category 注册"
        ));
    };
    // 阈值必须是正整数且装得进 `u32`——脚本侧传进来的是 `i64`，负数
    // 与超范围都是内容作者写错了数，当场报错而不是钳位（钳位会让
    // `-1` 悄悄变成某个能跑的数）。零由 `set_craft_unlock` 拒绝。
    let threshold = u32::try_from(threshold)
        .map_err(|_| format!("副职获得条件阈值 {threshold} 超出合法范围（需为 1..=u32::MAX）"))?;

    table
        .set_craft_unlock(subclass_index, category_index, parsed_category, threshold)
        .map(|()| true)
        .map_err(|err: SubclassError| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 合法副职声明注册成功并写入副职表() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();

        // Act
        let result = do_register_subclass(
            &mut registry,
            &mut table,
            "yourmod:shadowdancer",
            "yourmod:shadowdancer_display_name",
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.get(index).is_some());
    }

    #[test]
    fn 非法命名空间字符串返回错误而不panic() {
        // Arrange
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();

        // Act
        let result = do_register_subclass(&mut registry, &mut table, "Not Valid", "yourmod:x");

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 通过线程局部注册目标脚本能真正调用register_subclass() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_subclass_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SubclassTable::new());

        // Act
        let result = engine.load_source(
            r#"(register-subclass "yourmod:shadowdancer" "yourmod:shadowdancer_display_name")"#
                .to_string(),
        );

        // Assert
        assert!(result.is_ok());
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册的内容应能查到索引");
        assert!(table.get(index).is_some());
    }

    #[test]
    fn 脚本内注册失败时load_source返回err而不panic() {
        // Arrange：非法命名空间——脚本作者笔误。
        let mut engine = ScriptEngine::new();
        register_subclass_api(&mut engine);
        crate::active_registry::set_active_registry(Registry::new());
        set_active_target(SubclassTable::new());

        // Act
        let result =
            engine.load_source(r#"(register-subclass "Not Valid" "yourmod:x")"#.to_string());

        // Assert
        assert!(result.is_err());

        // Cleanup：同 script_terrain_api 的既有纪律。
        take_active_target();
        crate::active_registry::take_active_registry();
    }

    /// 造一份「副职与配方类别都已注册」的注册表 + 副职表，供下面几条
    /// 获得条件测试共用。
    fn registered_pair() -> (Registry, SubclassTable) {
        let mut registry = Registry::new();
        let mut table = SubclassTable::new();
        do_register_subclass(
            &mut registry,
            &mut table,
            "yourmod:shadowdancer",
            "yourmod:shadowdancer_display_name",
        )
        .expect("合法声明");
        registry.intern(NamespacedId::parse("yourmod:cooking").expect("合法"));
        (registry, table)
    }

    #[test]
    fn 合法的获得条件写进副职表() {
        // Arrange
        let (registry, mut table) = registered_pair();

        // Act
        let result = do_register_subclass_unlock(
            &registry,
            &mut table,
            "yourmod:shadowdancer",
            "items-crafted",
            "yourmod:cooking",
            5,
        );

        // Assert
        assert_eq!(result, Ok(true));
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册");
        let unlock = table.craft_unlock(index).expect("刚写入");
        assert_eq!(unlock.threshold, 5);
        assert_eq!(unlock.category_id.to_string(), "yourmod:cooking");
    }

    #[test]
    fn 未知的触发器种类当场报错并列出支持的取值() {
        // 「传了别的 kind 会当场报错，不会静默被当成制作」这句话的守卫。
        // Arrange
        let (registry, mut table) = registered_pair();

        // Act
        let result = do_register_subclass_unlock(
            &registry,
            &mut table,
            "yourmod:shadowdancer",
            "rests-taken",
            "yourmod:cooking",
            5,
        );

        // Assert
        let message = result.expect_err("未实现的触发器必须报错");
        assert!(message.contains("items-crafted"), "错误信息要指出支持什么");
    }

    #[test]
    fn 指向未注册配方类别的获得条件被拒绝() {
        // 悬空的获得条件谁都触发不了且完全不报错，是最难查的一类内容
        // bug——注册期直接拦住。
        // Arrange
        let (registry, mut table) = registered_pair();

        // Act
        let result = do_register_subclass_unlock(
            &registry,
            &mut table,
            "yourmod:shadowdancer",
            "items-crafted",
            "yourmod:never_registered",
            5,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 给未注册的副职追加获得条件被拒绝() {
        // Arrange
        let (registry, mut table) = registered_pair();

        // Act
        let result = do_register_subclass_unlock(
            &registry,
            &mut table,
            "yourmod:never_registered",
            "items-crafted",
            "yourmod:cooking",
            5,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 同一个副职声明两条获得条件被拒绝() {
        // 「一个副职只能有一条」——多条互相竞争的解锁路径会让「我还差
        // 多少」这句 UI 文案没法唯一地展示。
        // Arrange
        let (registry, mut table) = registered_pair();
        do_register_subclass_unlock(
            &registry,
            &mut table,
            "yourmod:shadowdancer",
            "items-crafted",
            "yourmod:cooking",
            5,
        )
        .expect("第一条合法");

        // Act
        let result = do_register_subclass_unlock(
            &registry,
            &mut table,
            "yourmod:shadowdancer",
            "items-crafted",
            "yourmod:cooking",
            9,
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 阈值为零或负数被拒绝而不是钳位() {
        // 钳位会让 `-1` 悄悄变成某个能跑的数。
        // Arrange
        let (registry, table) = registered_pair();

        // Act & Assert
        for bad in [0_i64, -1] {
            let mut fresh = table.clone();
            assert!(
                do_register_subclass_unlock(
                    &registry,
                    &mut fresh,
                    "yourmod:shadowdancer",
                    "items-crafted",
                    "yourmod:cooking",
                    bad,
                )
                .is_err(),
                "阈值 {bad} 必须被拒绝"
            );
        }
        // 上面每轮都用了一份克隆，原表必须一条获得条件都没有。
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册");
        assert!(table.craft_unlock(index).is_none());
    }

    #[test]
    fn 脚本能真的调用register_subclass_unlock() {
        // Arrange
        let mut engine = ScriptEngine::new();
        register_subclass_api(&mut engine);
        let mut registry = Registry::new();
        registry.intern(NamespacedId::parse("yourmod:cooking").expect("合法"));
        crate::active_registry::set_active_registry(registry);
        set_active_target(SubclassTable::new());

        // Act
        let result = engine.load_source(
            concat!(
                r#"(register-subclass "yourmod:shadowdancer" "yourmod:shadowdancer_display_name")"#,
                "
",
                r#"(register-subclass-unlock "yourmod:shadowdancer" "items-crafted" "yourmod:cooking" 4)"#
            )
            .to_string(),
        );

        // Assert
        assert!(result.is_ok(), "脚本调用应当成功：{result:?}");
        let registry = crate::active_registry::take_active_registry();
        let table = take_active_target();
        let index = registry
            .get(&NamespacedId::parse("yourmod:shadowdancer").unwrap())
            .expect("刚注册");
        assert_eq!(
            table
                .craft_unlock(index)
                .expect("脚本写入了获得条件")
                .threshold,
            4
        );
    }
}
