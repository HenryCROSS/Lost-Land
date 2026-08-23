//! 脚本状态存储：`state-set!`/`state-get!`/`entity-state-set!`/
//! `entity-state-get!`/`state-get-foreign`。
//!
//! 落地 `knowledge/design/script-state-storage.md` 二、三、四节，配额
//! 见六节（[`ll_world::script_state::PER_MOD_QUOTA_BYTES`]/
//! [`ll_world::script_state::PER_MOD_ENTITY_QUOTA_BYTES`]）。
//!
//! # 写入路径必须经 `apply`（裁定 P5-1）——本模块如何做到
//!
//! 设计文档 8.2 节原写「直接写穿，没有中间层」，与规格 §4 约束 C1
//! 「`apply` 是全局唯一能改世界的地方」字面冲突——脚本状态就是
//! `WorldState` 的一部分，写它就是改世界。本模块因此**不直接写
//! `WorldState`**：`state-set!`/`entity-state-set!` 只把写入攒进一个
//! 线程局部缓冲（[`PENDING_WRITES`]），真正落盘要等宿主在脚本调用
//! 结束后调用 [`take_pending_writes`] 取走整批、包成一条
//! `ll_sim::effect::Effect::SetScriptState`，交给既有的
//! `resolve → apply` 管线——这正是裁定 P5-1「一次决策期间的多次写入
//! 收集成一条 Effect」的性能解法：`Effect` 流保持诚实（每一次状态
//! 变化都经过它），又不必为每次写入单独付一条 `Effect` 的开销。
//!
//! 读（`state-get!`/`entity-state-get!`/`state-get-foreign`）不修改
//! 世界，直接查已提交的 `WorldState`（经 [`crate::api::query`] 的活跃
//! 世界指针），但会先看一眼待写缓冲——同一次决策里"先写后读同一个
//! 键"应该读到刚写的值，不是要等到下一次 `apply` 之后才可见，这是对
//! 设计文档 8.2 节「脚本每次读到的都是当前 WorldState 里的最新值」这
//! 条承诺的忠实延续（原文假设直接写穿，本模块用「缓冲区优先查找」
//! 达到同样的可观察效果，同时仍然满足 C1）。
//!
//! # 两个注册入口：写能力不是默认发的
//!
//! 上面那条链路只在宿主**真的排空缓冲区**时才成立。[`register`]
//! （读 + 写六个函数）因此是一条**带承诺的**入口：调用它就是承诺每次
//! 脚本调用窗口结束后调用 [`take_pending_writes`]。承诺不成立的宿主
//! 必须改调 [`register_read_only`]（只有读的四个函数）——那条路径物理
//! 上产生不了待写记录。
//!
//! 现役唯一的行为树宿主
//! （`ll_mod::script_behavior_source::ScriptBehaviorSource`）走的正是
//! 只读那条：它的 `decide` 返回 `Option<Intent>`，没有任何位置能放下
//! 一批待写记录，接上写能力只会让写入永远烂在缓冲区里——**那本身就是
//! 一次 C1 违反**（写入跨帧累积、永远到不了 `apply`，而 `state-get!`
//! 又优先读缓冲区，于是第 1 帧的写入在第 100 帧仍然可见，存档里却
//! 什么都没有）。两个入口的分工就是为了让这种接法在类型层面写不出来。
//!
//! # 配额：为什么在这里判定，不是在 `apply` 里
//!
//! 配额超限必须**立即**告诉脚本「这次写入没有生效」（返回失败哨兵值），
//! 而 `apply` 是延后执行的——若把配额判定挪到 `apply`，`state-set!`
//! 就没有办法在返回值里诚实反映"到底成不成功"。因此配额判定发生在
//! 缓冲区写入这一刻：读已提交的 `WorldState`（经活跃世界指针）加上
//! 当前缓冲区里同一个 mod 已经攒下的待写记录，一起算出「这次写入之后
//! 会占多少字节」，见 [`ll_world::script_state::mod_total_bytes`]/
//! [`ll_world::script_state::entity_mod_bytes`] 文档。

use std::cell::RefCell;
use std::collections::BTreeMap;

use steel::rvals::{Custom, FromSteelVal, IntoSteelVal, SteelVal};

use ll_world::entity::EntityId;
use ll_world::script_state::{
    PER_MOD_ENTITY_QUOTA_BYTES, PER_MOD_QUOTA_BYTES, ScriptStateTarget, ScriptStateWrite,
    ScriptValue, entity_mod_bytes, entry_size, mod_total_bytes,
};

use crate::api::handle::ScriptEntityHandle;
use crate::api::log::ScriptDiagnostic;
use crate::api::query::with_active_world;
use crate::host::ScriptEngine;

thread_local! {
    /// 当前调用窗口内脚本已经写入、尚未经 `apply` 落盘的记录——见模块
    /// 文档「写入路径必须经 apply」一节。宿主必须在脚本调用结束后用
    /// [`take_pending_writes`] 取走整批，否则下一次调用会把这批记录
    /// 误当成"这次决策也写了这些"（与 `query`/`rng` 模块「不清空会
    /// 张冠李戴」同一条既有纪律）。
    static PENDING_WRITES: RefCell<Vec<ScriptStateWrite>> = const { RefCell::new(Vec::new()) };

    /// 当前调用窗口内产生的诊断（目前只有配额超限一种来源）——宿主同样
    /// 需要在调用结束后用 [`take_pending_diagnostics`] 取走。
    static PENDING_DIAGNOSTICS: RefCell<Vec<ScriptDiagnostic>> = const { RefCell::new(Vec::new()) };
}

/// 取走并清空本次调用窗口积累的全部待写记录。
///
/// 调用方是**注册了写能力的那类宿主**（即调用过 [`register`] 而不是
/// [`register_read_only`] 的宿主，见模块文档「两个注册入口」一节），
/// 必须在每次脚本调用结束后调用本函数，把结果
/// 包成一条 `ll_sim::effect::Effect::SetScriptState`（若非空）交给
/// `apply`——本函数本身不知道、也不需要知道 `Effect`（`ll-script` 依赖
/// `ll-sim`，但本模块刻意不 `use` `ll_sim::effect`，保持「脚本层只产出
/// 数据，包装成 `Effect` 是调用方的事」这条既有分工，与
/// `api::intent::parse_intent` 同一个模式）。
pub fn take_pending_writes() -> Vec<ScriptStateWrite> {
    PENDING_WRITES.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// 取走并清空本次调用窗口积累的全部诊断。
pub fn take_pending_diagnostics() -> Vec<ScriptDiagnostic> {
    PENDING_DIAGNOSTICS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// 内容引用的 Steel 侧标签——`ScriptValue::Ref` 在 Steel FFI 边界上的
/// 表示（设计文档三、2 节）。与普通字符串（`ScriptValue::Str`）分开
/// 一个 `Custom` 类型，而不是让 `state-set!` 靠猜测区分"这个字符串是
/// 不是内容引用"：脚本必须显式调用 `(content-ref "yourmod:foo")` 才能
/// 产出这个类型，写意图因此在脚本源码里是显式的，不是宿主的启发式
/// 判断。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ContentRefTag(Box<str>);

impl Custom for ContentRefTag {}

/// 注册**完整**的六个函数进 `engine`：读的四个（[`register_read_only`]）
/// 加上写的两个（`state-set!`/`entity-state-set!`）。
///
/// # 调用本函数的宿主必须排空缓冲区，否则违反约束 C1
///
/// 写入只是攒进 [`PENDING_WRITES`]（见模块文档「写入路径必须经
/// `apply`」一节）——**调用本函数就是承诺：每一次脚本调用窗口结束后
/// 都会调用 [`take_pending_writes`]，把整批包成一条
/// `ll_sim::effect::Effect::SetScriptState` 送进 `resolve → apply`。**
///
/// 不履行这条承诺的后果不是"写入丢失"这么温和：`PENDING_WRITES` 是
/// 线程局部的，不排空就**跨调用窗口累积**，而 `state-get!` 又优先查
/// 缓冲区——第 1 帧写下的值在第 100 帧仍然读得到，存档里却什么都没有。
/// 那正是约束 C1 点名要禁的「脚本持有跨帧的隐式状态」，只不过隐式状态
/// 这次藏在宿主侧的缓冲区里而不是 VM 内存里。
///
/// **没有履行这条承诺的宿主应当改调 [`register_read_only`]**——那条
/// 路径物理上产生不了待写记录，因此不需要任何排空纪律。
///
/// `mod_namespace` 由调用方（`ll-mod` 装载管线，正在为哪个 mod 构造这个
/// `ScriptEngine` 就传哪个命名空间）固化——**不是脚本参数**，脚本没有
/// 任何语法能覆盖它，这是命名空间隔离的类型层面保证（设计文档四、1
/// 节）。与 `script_terrain_api.rs` 的 `RefCell<Option<T>>` 模式不同：
/// 这里不需要线程局部存储持有命名空间本身——一个普通 `String` 满足
/// `Send + Sync + 'static`（`RegisterFn` 的约束），直接被闭包捕获即可，
/// 不需要绕道内部可变性。
pub fn register(engine: &mut ScriptEngine, mod_namespace: impl Into<String>) {
    let namespace = mod_namespace.into();
    register_read_only(engine, namespace.clone());
    register_writes(engine, namespace);
}

/// 只注册**读**的四个函数：`state-get!`/`entity-state-get!`/
/// `state-get-foreign`/`content-ref`。
///
/// # 谁该用这条路径：决策期脚本
///
/// 行为树是**决策**——按约束 C1，`decide` 那一层不写世界（
/// `ll_mod::script_behavior_source::ScriptBehaviorSource::decide` 的
/// 文档「C1：这里不写世界」一节已经把这条纪律写死：它只拿得到
/// `&WorldState`）。给它注册写入函数并不能让它真的写成：`decide`
/// 返回的是 `Option<Intent>`，没有任何位置能放下一批待写记录，于是
/// 写入只会永远烂在 [`PENDING_WRITES`] 里——见 [`register`] 文档
/// 「必须排空缓冲区」一节描述的那个后果。
///
/// **能力不存在，好过能力坏着。** 走本函数之后，脚本引用
/// `state-set!` 会在 `load_source` 的白名单校验那一刻被点名拒绝
/// （`register_fn` 是唯一把名字放进白名单的通道，见
/// [`crate::host::ScriptEngine::register_fn`]），mod 作者当场看到
/// 一条指名道姓的装载错误，而不是运行几百帧之后才发现存档里什么都
/// 没有。
///
/// # 读为什么可以留下
///
/// 读不改世界，是决策期完全正当的能力，而且**读得到真东西**：脚本
/// 状态的生产写入路径今天就存在于 Rust 侧（`ll_sim::quest` 的任务
/// 进度、`ll_sim::subclass` 的制作计数都产出
/// `Effect::SetScriptState` 经 `apply` 落盘），行为树用
/// `state-get!`/`entity-state-get!` 读它们是这些数据的正当消费方式。
///
/// `content-ref` 归在读这一半：`state-get!` 读出一条
/// `ScriptValue::Ref` 时，脚本需要它才能构造一个同类值来比对。
pub fn register_read_only(engine: &mut ScriptEngine, mod_namespace: impl Into<String>) {
    let namespace = mod_namespace.into();

    let ns = namespace.clone();
    engine.register_fn("state-get!", move |key: String| -> SteelVal {
        read_value(ScriptStateTarget::Global, &ns, &key)
    });

    let ns = namespace.clone();
    engine.register_fn(
        "entity-state-get!",
        move |handle: ScriptEntityHandle, key: String| -> SteelVal {
            read_entity(handle.entity_id(), &ns, &key)
        },
    );

    // state-get-foreign 不需要捕获自己的命名空间——目标命名空间由脚本
    // 显式传入，这正是「只读、需要显式声明要读谁」的跨 mod 查询本身
    // （设计文档四、2 节），命名空间隔离在这里不适用：本来就是允许
    // 读别人的通道。
    engine.register_fn(
        "state-get-foreign",
        move |foreign_namespace: String, key: String| -> SteelVal {
            read_value(ScriptStateTarget::Global, &foreign_namespace, &key)
        },
    );

    engine.register_fn("content-ref", |value: String| -> SteelVal {
        ContentRefTag(value.into_boxed_str())
            .into_steelval()
            .unwrap_or(SteelVal::Void)
    });
}

/// 注册写的两个函数。私有：外部只有 [`register`]（带排空承诺）与
/// [`register_read_only`]（不带写能力）两个入口，不给第三种「只注册写
/// 不注册读」的组合——那个组合没有任何用例，暴露它只会多一种可以接错
/// 的接法。
fn register_writes(engine: &mut ScriptEngine, namespace: String) {
    let ns = namespace.clone();
    engine.register_fn("state-set!", move |key: String, value: SteelVal| -> bool {
        try_write(ScriptStateTarget::Global, &ns, key, &value)
    });

    engine.register_fn(
        "entity-state-set!",
        move |handle: ScriptEntityHandle, key: String, value: SteelVal| -> bool {
            try_write(
                ScriptStateTarget::Entity(handle.entity_id()),
                &namespace,
                key,
                &value,
            )
        },
    );
}

/// 尝试把 `value` 写入 `target` 下 `mod_namespace` 的 `key`。
///
/// 返回是否成功——失败的两种原因分别处理：
/// 1. **目标实体已死亡**（只有 `Entity` 目标才有这个问题）：静默作废，
///    不产生诊断——这是「行为型操作在句柄失效时视为这一步无意义」的
///    既有降级哲学（`script-entity-handles-and-batch-queries.md` 3.4
///    节），不是配额问题，不该被误报成配额超限。
/// 2. **超出配额**：产生一条 [`ScriptDiagnostic::quota_exceeded`]——
///    这是设计文档六、3 节要求的"必须留痕"，不能只让脚本看到一个
///    孤零零的 `#f` 却毫无线索。
fn try_write(
    target: ScriptStateTarget,
    mod_namespace: &str,
    key: String,
    value: &SteelVal,
) -> bool {
    let Some(script_value) = steelval_to_scriptvalue(value) else {
        // 不认识的值形状（例如脚本传了一个函数值）——拒绝写入而不是
        // 猜测，与 api::intent::parse_intent 同一条既有降级哲学。
        return false;
    };

    if let ScriptStateTarget::Entity(id) = target {
        let alive = with_active_world(false, |world| world.actors.get(id).is_some());
        if !alive {
            return false;
        }
    }

    PENDING_WRITES.with(|cell| {
        let mut buffer = cell.borrow_mut();
        let removed = remove_pending(&mut buffer, target, mod_namespace, &key);
        let entry_bytes = entry_size(&key, &script_value);

        // 无活跃世界时按「已满」处理（usize::MAX），配额判定必然失败
        // ——这是「宿主接线可能有 bug」场景下的保守默认，宁可拒绝写入
        // 也不要在无法确定用量时放行，与 `entity_alive` 检查同一种
        // 「默认值选取偏向安全」的取舍，见 query.rs 模块文档「宿主接线
        // 可能有 bug」一节同一条既有精神。
        let mod_bytes = with_active_world(usize::MAX, |world| {
            mod_total_bytes(world, mod_namespace, &buffer)
        });
        let mod_ok =
            mod_bytes != usize::MAX && mod_bytes.saturating_add(entry_bytes) <= PER_MOD_QUOTA_BYTES;

        let entity_ok = match target {
            ScriptStateTarget::Entity(id) => {
                let entity_bytes = with_active_world(usize::MAX, |world| {
                    entity_mod_bytes(world, id, mod_namespace, &buffer)
                });
                entity_bytes != usize::MAX
                    && entity_bytes.saturating_add(entry_bytes) <= PER_MOD_ENTITY_QUOTA_BYTES
            }
            ScriptStateTarget::Global => true,
        };

        if mod_ok && entity_ok {
            buffer.push(ScriptStateWrite {
                target,
                mod_namespace: mod_namespace.to_string(),
                key,
                value: script_value,
            });
            true
        } else {
            if let Some(old) = removed {
                buffer.push(old);
            }
            PENDING_DIAGNOSTICS.with(|diagnostics| {
                diagnostics
                    .borrow_mut()
                    .push(ScriptDiagnostic::quota_exceeded(mod_namespace, &key));
            });
            false
        }
    })
}

/// 从缓冲区里移除与 `target`/`mod_namespace`/`key` 同名的待写记录（若
/// 存在），返回被移除的那一条——同一决策内重复写同一个键只保留最后
/// 一次的值，避免配额判定把中间过程的每一次覆写都重复计入（见
/// [`ScriptStateWrite::matches`] 文档）。
fn remove_pending(
    buffer: &mut Vec<ScriptStateWrite>,
    target: ScriptStateTarget,
    mod_namespace: &str,
    key: &str,
) -> Option<ScriptStateWrite> {
    let index = buffer
        .iter()
        .position(|write| write.matches(target, mod_namespace, key))?;
    Some(buffer.remove(index))
}

/// 读取 `target` 下 `mod_namespace` 的 `key`；查不到时返回
/// `SteelVal::Void`——不用 `#f`：`ScriptValue::Bool(false)` 是合法的
/// 存储值，若哨兵也用 `#f`，脚本无法区分"存的就是假"与"根本没存过"。
/// `Void` 不是任何 `ScriptValue` 变体的合法转换结果，因此可以无歧义
/// 地充当"未找到"的哨兵。
fn read_value(target: ScriptStateTarget, mod_namespace: &str, key: &str) -> SteelVal {
    if let Some(pending) = pending_lookup(target, mod_namespace, key) {
        return scriptvalue_to_steelval(&pending);
    }
    with_active_world(SteelVal::Void, |world| {
        let stored = match target {
            ScriptStateTarget::Global => world
                .global_script_state
                .get(&(mod_namespace.to_string(), key.to_string())),
            ScriptStateTarget::Entity(id) => world.actors.get(id).and_then(|agent| {
                agent
                    .script_state
                    .get(&(mod_namespace.to_string(), key.to_string()))
            }),
        };
        stored
            .map(scriptvalue_to_steelval)
            .unwrap_or(SteelVal::Void)
    })
}

/// [`read_value`] 的实体特化版本：目标实体已死亡时直接返回哨兵值，不
/// 尝试查询——与 [`try_write`] 对死亡实体的处理同一套降级哲学。
fn read_entity(entity: EntityId, mod_namespace: &str, key: &str) -> SteelVal {
    let alive = with_active_world(false, |world| world.actors.get(entity).is_some());
    if !alive {
        return SteelVal::Void;
    }
    read_value(ScriptStateTarget::Entity(entity), mod_namespace, key)
}

/// 在待写缓冲区里查找与 `target`/`mod_namespace`/`key` 同名的记录——
/// 支持"先写后读同一个键"在同一次决策内立即可见，见模块文档。
fn pending_lookup(
    target: ScriptStateTarget,
    mod_namespace: &str,
    key: &str,
) -> Option<ScriptValue> {
    PENDING_WRITES.with(|cell| {
        cell.borrow()
            .iter()
            .find(|write| write.matches(target, mod_namespace, key))
            .map(|write| write.value.clone())
    })
}

/// 把脚本传入的 `SteelVal` 转换成 [`ScriptValue`]；无法识别的形状返回
/// `None`（调用方按"这次写入没有意义"处理，不 panic，与
/// `api::intent::parse_intent` 同一条既有降级哲学）。
///
/// 判定顺序：先试 `ScriptEntityHandle`/[`ContentRefTag`]（两者都是
/// `SteelVal::Custom`，`downcast_ref` 按 `TypeId` 精确匹配，不会与
/// 别的 `Custom` 类型混淆），再落到基础类型/复合类型的字面匹配。
fn steelval_to_scriptvalue(value: &SteelVal) -> Option<ScriptValue> {
    if let Ok(handle) = ScriptEntityHandle::from_steelval(value) {
        return Some(ScriptValue::Entity(handle.entity_id()));
    }
    if let Ok(tag) = ContentRefTag::from_steelval(value) {
        return Some(ScriptValue::Ref(tag.0));
    }
    match value {
        SteelVal::IntV(n) => Some(ScriptValue::Int(*n as i64)),
        SteelVal::BoolV(b) => Some(ScriptValue::Bool(*b)),
        SteelVal::StringV(s) => Some(ScriptValue::Str(s.as_str().into())),
        SteelVal::ListV(list) => {
            let mut items = Vec::new();
            for item in list.iter() {
                items.push(steelval_to_scriptvalue(item)?);
            }
            Some(ScriptValue::List(items))
        }
        SteelVal::HashMapV(_) => {
            // 复用 steel-core 已有的 HashMap<K, V> 转换（见
            // conversions.rs），不手写 SteelHashMap 内部结构的遍历。
            let raw: std::collections::HashMap<String, SteelVal> =
                FromSteelVal::from_steelval(value).ok()?;
            let mut result = BTreeMap::new();
            for (key, item) in raw {
                result.insert(key.into_boxed_str(), steelval_to_scriptvalue(&item)?);
            }
            Some(ScriptValue::Map(result))
        }
        _ => None,
    }
}

/// 把 [`ScriptValue`] 转换成可以跨界返回给脚本的 `SteelVal`。
///
/// 与 `List`/`Map` 内部转换失败的处理：`Custom` 类型的 `into_steelval`
/// 在本设计里结构上恒成功（[`ContentRefTag`]/[`ScriptEntityHandle`]
/// 都没有校验逻辑），`Vec<SteelVal>`/`HashMap<String, SteelVal>` 的
/// `into_steelval` 同样只做纯数据搬运——理论上不会失败，但仍不 panic：
/// 失败时降级成 `SteelVal::Void`（与"查不到"用同一个哨兵，脚本侧看到
/// 的效果是"这次读取拿不到值"，不会崩溃）。
fn scriptvalue_to_steelval(value: &ScriptValue) -> SteelVal {
    match value {
        ScriptValue::Int(n) => SteelVal::IntV(*n as isize),
        ScriptValue::Bool(b) => SteelVal::BoolV(*b),
        ScriptValue::Str(s) => SteelVal::StringV(s.as_ref().into()),
        ScriptValue::Ref(s) => ContentRefTag(s.clone())
            .into_steelval()
            .unwrap_or(SteelVal::Void),
        ScriptValue::Entity(id) => ScriptEntityHandle::new(*id)
            .into_steelval()
            .unwrap_or(SteelVal::Void),
        ScriptValue::List(items) => {
            let values: Vec<SteelVal> = items.iter().map(scriptvalue_to_steelval).collect();
            values.into_steelval().unwrap_or(SteelVal::Void)
        }
        ScriptValue::Map(map) => {
            let raw: std::collections::HashMap<String, SteelVal> = map
                .iter()
                .map(|(key, item)| (key.to_string(), scriptvalue_to_steelval(item)))
                .collect();
            raw.into_steelval().unwrap_or(SteelVal::Void)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::query::{clear_active_world, set_active_world};
    use ll_core::ident::{ContentIndex, Interner, NamespacedId};
    use ll_core::time::Tick;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::space::Space;
    use ll_world::state::WorldState;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    fn test_world() -> WorldState {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn blank_agent(world: &WorldState) -> Agent {
        let mut interner = Interner::new();
        let profession =
            interner.intern(NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(0, 0);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, ContentIndex::default()),
            script_state: BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        }
    }

    /// 把 [`take_pending_writes`] 取到的一批写入直接应用到 `world`——
    /// 测试专用的迷你「apply」，模拟真实管线里 `Effect::SetScriptState`
    /// 经 `ll_sim::apply::apply` 落盘的效果，不依赖 `ll-sim`（本 crate
    /// 的测试不需要为此新增对 `ll_sim::effect`/`apply` 的直接依赖，
    /// 保持测试聚焦在本模块自身的行为）。
    fn commit_pending(world: &mut WorldState) {
        for write in take_pending_writes() {
            match write.target {
                ScriptStateTarget::Global => {
                    world
                        .global_script_state
                        .insert((write.mod_namespace, write.key), write.value);
                }
                ScriptStateTarget::Entity(id) => {
                    if let Some(agent) = world.actors.get_mut(id) {
                        agent
                            .script_state
                            .insert((write.mod_namespace, write.key), write.value);
                    }
                }
            }
        }
    }

    #[test]
    fn 全局存储写入后同一mod可以读回() {
        // Arrange
        let world = test_world();
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(
                r#"(define (probe) (state-set! "reputation" 42) (state-get! "reputation"))"#
                    .to_string(),
            )
            .unwrap();

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_world();
            result
        };

        // Assert：写入还没经过 apply（测试没有调用 commit_pending），
        // 但待写缓冲区让"先写后读同一个键"在同一次决策内立即可见。
        assert_eq!(result, Ok(SteelVal::IntV(42)));
    }

    #[test]
    fn 跨mod默认读取失败state_get_foreign可以显式跨读() {
        // Arrange：mod "lostland" 写一条记录并提交进世界。
        let mut world = test_world();
        // 两个引擎都在编译之前造齐——见 `ll_script::host` 里
        // `COMPILED_ON_THIS_THREAD` 上方注释：同一根线程上全部构造必须
        // 先于全部编译。
        let mut writer = ScriptEngine::new();
        let mut reader = ScriptEngine::new();
        register(&mut writer, "lostland");
        writer
            .load_source(r#"(define (probe) (state-set! "reputation" 42))"#.to_string())
            .unwrap();
        unsafe {
            set_active_world(&world);
            writer.call_raw("probe", Vec::new()).unwrap();
            clear_active_world();
        }
        commit_pending(&mut world);

        // Act：mod "yourmod" 默认读不到（不同命名空间），但用
        // state-get-foreign 显式跨读能拿到。
        register(&mut reader, "yourmod");
        reader
            .load_source(
                r#"(define (default-read) (state-get! "reputation"))
                   (define (foreign-read) (state-get-foreign "lostland" "reputation"))"#
                    .to_string(),
            )
            .unwrap();
        let (default_result, foreign_result) = unsafe {
            set_active_world(&world);
            let default_result = reader.call_raw("default-read", Vec::new());
            let foreign_result = reader.call_raw("foreign-read", Vec::new());
            clear_active_world();
            (default_result, foreign_result)
        };

        // Assert
        assert_eq!(default_result, Ok(SteelVal::Void));
        assert_eq!(foreign_result, Ok(SteelVal::IntV(42)));
    }

    #[test]
    fn 每实体存储随实体销毁而消失不产生孤儿() {
        // Arrange
        let mut world = test_world();
        let actor = world.actors.spawn(blank_agent(&world));
        let handle = ScriptEntityHandle::new(actor);
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(
                r#"(define (probe target) (entity-state-set! target "cooldown" 5))"#.to_string(),
            )
            .unwrap();
        unsafe {
            set_active_world(&world);
            engine
                .call_raw("probe", vec![handle.into_steelval().unwrap()])
                .unwrap();
            clear_active_world();
        }
        commit_pending(&mut world);
        assert!(
            world
                .actors
                .get(actor)
                .unwrap()
                .script_state
                .contains_key(&("lostland".to_string(), "cooldown".to_string()))
        );

        // Act：销毁实体——整个槽位（含 script_state）被 Arena 收走，不
        // 存在任何旁挂表需要额外清理。
        world.actors.despawn(actor);

        // Assert：查不到这个实体了，自然也查不到它的脚本状态；这不是
        // "碰巧查不到"，是因为携带这份数据的槽位已经不是 Occupied。
        assert_eq!(world.actors.get(actor), None);
    }

    #[test]
    fn 实体已死亡时entity_state_get返回哨兵值而非panic() {
        // Arrange
        let mut world = test_world();
        let actor = world.actors.spawn(blank_agent(&world));
        world.actors.despawn(actor);
        let handle = ScriptEntityHandle::new(actor);
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(
                r#"(define (probe target) (entity-state-get! target "cooldown"))"#.to_string(),
            )
            .unwrap();

        // Act & Assert：不应崩溃，返回哨兵值。
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", vec![handle.into_steelval().unwrap()]);
            clear_active_world();
            result
        };
        assert_eq!(result, Ok(SteelVal::Void));
    }

    #[test]
    fn state_set不能写入调用者命名空间之外的键() {
        // 类型层面已经保证（state-set! 没有命名空间参数）——这里补运行
        // 时断言确认没有逃逸路径：两个不同命名空间各自写入后，各自的
        // 记录只出现在自己的命名空间下。
        // Arrange
        let mut world = test_world();
        // 两个引擎都在编译之前造齐——见 `ll_script::host` 里
        // `COMPILED_ON_THIS_THREAD` 上方注释：同一根线程上全部构造必须
        // 先于全部编译。
        let mut engine_a = ScriptEngine::new();
        let mut engine_b = ScriptEngine::new();
        register(&mut engine_a, "moda");
        engine_a
            .load_source(r#"(define (probe) (state-set! "shared-key" 1))"#.to_string())
            .unwrap();
        register(&mut engine_b, "modb");
        engine_b
            .load_source(r#"(define (probe) (state-set! "shared-key" 2))"#.to_string())
            .unwrap();

        // Act
        unsafe {
            set_active_world(&world);
            engine_a.call_raw("probe", Vec::new()).unwrap();
            engine_b.call_raw("probe", Vec::new()).unwrap();
            clear_active_world();
        }
        commit_pending(&mut world);

        // Assert：同名键各自落在自己的命名空间下，互不覆盖、互不串扰。
        assert_eq!(
            world
                .global_script_state
                .get(&("moda".to_string(), "shared-key".to_string())),
            Some(&ScriptValue::Int(1))
        );
        assert_eq!(
            world
                .global_script_state
                .get(&("modb".to_string(), "shared-key".to_string())),
            Some(&ScriptValue::Int(2))
        );
    }

    #[test]
    fn 写入超过配额时返回失败哨兵值并产生诊断() {
        // Arrange：一个单独就超过 256KB 配额的字符串值。
        let world = test_world();
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(r#"(define (probe big) (state-set! "huge" big))"#.to_string())
            .unwrap();
        let huge = "x".repeat(PER_MOD_QUOTA_BYTES + 1);

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", vec![huge.into_steelval().unwrap()]);
            clear_active_world();
            result
        };
        let diagnostics = take_pending_diagnostics();

        // Assert
        assert_eq!(result, Ok(SteelVal::BoolV(false)));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, crate::api::log::Severity::Warning);
    }

    #[test]
    fn 单mod写入超过累计配额时后续写入被拒绝() {
        // Arrange：先写入一条占约四分之三配额的记录，留出约四分之一
        // 配额的余量；再尝试写入一条约二分之一配额的记录——余量不够
        // 装下它，应当被拒绝。两条记录的大小都留了远超 postcard 编码
        // 开销（长度前缀、变体判别字节等，至多几十字节）的余量，不依赖
        // 精确命中字节边界。
        let mut world = test_world();
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(
                r#"(define (fill big) (state-set! "big" big))
                   (define (probe small) (state-set! "small" small))"#
                    .to_string(),
            )
            .unwrap();
        let three_quarters = "x".repeat(PER_MOD_QUOTA_BYTES * 3 / 4);
        let one_half = "y".repeat(PER_MOD_QUOTA_BYTES / 2);

        // Act
        let (fill_ok, probe_ok) = unsafe {
            set_active_world(&world);
            let fill_ok = engine
                .call_raw("fill", vec![three_quarters.into_steelval().unwrap()])
                .unwrap();
            commit_pending(&mut world);
            let probe_ok = engine
                .call_raw("probe", vec![one_half.into_steelval().unwrap()])
                .unwrap();
            clear_active_world();
            (fill_ok, probe_ok)
        };

        // Assert
        assert_eq!(fill_ok, SteelVal::BoolV(true));
        assert_eq!(probe_ok, SteelVal::BoolV(false));
    }

    #[test]
    fn 单mod乘实体写入超过四千字节时被拒绝不影响该mod对其他实体的配额() {
        // Arrange
        let mut world = test_world();
        let actor_a = world.actors.spawn(blank_agent(&world));
        let actor_b = world.actors.spawn(blank_agent(&world));
        let handle_a = ScriptEntityHandle::new(actor_a);
        let handle_b = ScriptEntityHandle::new(actor_b);
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(
                r#"(define (probe target big) (entity-state-set! target "note" big))"#.to_string(),
            )
            .unwrap();
        let too_big = "x".repeat(PER_MOD_ENTITY_QUOTA_BYTES + 1);

        // Act
        let (result_a, result_b) = unsafe {
            set_active_world(&world);
            let result_a = engine
                .call_raw(
                    "probe",
                    vec![
                        handle_a.into_steelval().unwrap(),
                        too_big.into_steelval().unwrap(),
                    ],
                )
                .unwrap();
            let result_b = engine
                .call_raw(
                    "probe",
                    vec![
                        handle_b.into_steelval().unwrap(),
                        1i64.into_steelval().unwrap(),
                    ],
                )
                .unwrap();
            clear_active_world();
            (result_a, result_b)
        };

        // Assert：实体 A 因超过单实体配额被拒绝，不影响实体 B（同一个
        // mod）正常写入。
        assert_eq!(result_a, SteelVal::BoolV(false));
        assert_eq!(result_b, SteelVal::BoolV(true));
    }

    #[test]
    fn 配额判定是加载期静态划分不受其他mod实际用量影响() {
        // 直接对应设计文档六、1 节的确定性论证：构造两个 mod，一个写满
        // 自己的配额，断言另一个不受影响——配额是按 mod 静态划分，不是
        // 共享浮动总量。
        // Arrange
        let mut world = test_world();
        // 两个引擎都在编译之前造齐——见 `ll_script::host` 里
        // `COMPILED_ON_THIS_THREAD` 上方注释：同一根线程上全部构造必须
        // 先于全部编译。
        let mut engine_a = ScriptEngine::new();
        let mut engine_b = ScriptEngine::new();
        register(&mut engine_a, "moda");
        engine_a
            .load_source(r#"(define (fill big) (state-set! "big" big))"#.to_string())
            .unwrap();
        let almost_full = "x".repeat(PER_MOD_QUOTA_BYTES - 32);

        register(&mut engine_b, "modb");
        engine_b
            .load_source(r#"(define (probe) (state-set! "small" 1))"#.to_string())
            .unwrap();

        // Act：先把 moda 写到接近满额。
        unsafe {
            set_active_world(&world);
            engine_a
                .call_raw("fill", vec![almost_full.into_steelval().unwrap()])
                .unwrap();
            clear_active_world();
        }
        commit_pending(&mut world);

        let modb_result = unsafe {
            set_active_world(&world);
            let result = engine_b.call_raw("probe", Vec::new()).unwrap();
            clear_active_world();
            result
        };

        // Assert：modb 的写入完全不受 moda 用量影响。
        assert_eq!(modb_result, SteelVal::BoolV(true));
    }

    #[test]
    fn mod被移除后其脚本状态在读档时原样保留() {
        // 模拟「读档时某个 mod 缺失，但存档里有它的命名空间残留」：
        // 写一条属于 ghostmod 的记录、把世界序列化再反序列化（模拟一次
        // 读档往返），确认数据原样还在，可以被 state-get-foreign 读到
        // ——序列化/反序列化本身只做数据搬运，不做「这个 mod 还在不在」
        // 这类业务判断（设计文档七、1 节）。
        // Arrange
        let mut world = test_world();
        // 两个引擎都在编译之前造齐——见 `ll_script::host` 里
        // `COMPILED_ON_THIS_THREAD` 上方注释：同一根线程上全部构造必须
        // 先于全部编译。
        let mut engine = ScriptEngine::new();
        let mut reader = ScriptEngine::new();
        register(&mut engine, "ghostmod");
        engine
            .load_source(r#"(define (probe) (state-set! "memory" 999))"#.to_string())
            .unwrap();
        unsafe {
            set_active_world(&world);
            engine.call_raw("probe", Vec::new()).unwrap();
            clear_active_world();
        }
        commit_pending(&mut world);

        // Act：序列化往返，模拟"ghostmod 这次没有被加载"的读档场景——
        // 本层（ll-world/ll-script）不知道、也不需要知道当前 mod 集合
        // 是什么，这正是"不主动清除"的字面含义。
        let encoded = serde_json::to_vec(&world).expect("WorldState 全部字段可序列化");
        let mut reloaded: WorldState = serde_json::from_slice(&encoded).expect("往返不应失败");

        register(&mut reader, "someothermod");
        reader
            .load_source(r#"(define (probe) (state-get-foreign "ghostmod" "memory"))"#.to_string())
            .unwrap();
        let result = unsafe {
            set_active_world(&reloaded);
            let result = reader.call_raw("probe", Vec::new());
            clear_active_world();
            result
        };

        // Assert
        assert_eq!(result, Ok(SteelVal::IntV(999)));
        let _ = &mut reloaded; // 仅为压制未使用可变性的告警（下方无进一步可变操作）。
    }

    #[test]
    fn content_ref写入后读回仍是内容引用而非普通字符串() {
        // Arrange
        let world = test_world();
        let mut engine = ScriptEngine::new();
        register(&mut engine, "lostland");
        engine
            .load_source(
                r#"(define (probe)
                     (state-set! "last-item" (content-ref "yourmod:healing_potion"))
                     (state-get! "last-item"))"#
                    .to_string(),
            )
            .unwrap();

        // Act
        let result = unsafe {
            set_active_world(&world);
            let result = engine.call_raw("probe", Vec::new());
            clear_active_world();
            result
        };

        // Assert：往返之后仍是同一个 Custom 类型，不是被悄悄折叠成
        // 普通字符串。
        let value = result.expect("调用不应失败");
        assert!(ContentRefTag::from_steelval(&value).is_ok());
    }
}
