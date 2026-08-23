//! 运行期事件分发：把 `ll_sim::effect::Effect` 翻译成事件负载、按
//! 订阅表回调 mod 的处理函数、再把处理函数的返回值翻译回 `Effect`。
//!
//! # 接在哪一条缝上
//!
//! `ll_sim::turn::TurnEngine::perform` 早就有一条
//! `on_effect(&WorldState, &Effect)` 回调，每条效果落地**之前**调用
//! 一次——它此前的唯一用途是给呈现层（伤害飘字）一个观察点。那正是
//! 「运行期发生了什么」在 `ll-sim` 与外界之间**唯一**已经存在的接缝，
//! 本模块因此接在它上面，而不是另开一条平行的事件总线：
//!
//! ```text
//! resolve → [Effect] ──┬─→ on_effect（本模块：翻译 + 回调 + 收反应）
//!                      └─→ apply（唯一写入口，C1）
//! ```
//!
//! 为了让监听器**能改世界**，那条回调的签名从 `-> ()` 放宽成
//! `-> Vec<Effect>`：处理函数产出的东西不是「写好的世界状态」，是
//! **一批还没落地的 `Effect`**，由 `perform` 在这一轮效果全部落地
//! 之后交给同一个 `apply` 执行。ADR 0023「脚本状态写入必须经 apply」
//! 因此逐字成立——本模块一行 `WorldState` 都不写，它连
//! `&mut WorldState` 都拿不到。
//!
//! # 性能形状：没人订阅就一分钱都不花
//!
//! 跨脚本边界一次调用约 326ns（ADR 0016/0017），而结算是热路径。
//! [`ScriptEventSource::dispatch`] 的第一件事是问订阅表
//! [`EventSubscriptionTable::has_subscriber`]——那是一次对小 `Vec` 的
//! 线性扫描，没有订阅者时**连事件负载都不构造**，直接返回空
//! `Vec`（不分配）。宿主还可以更早一步整个跳过：订阅表为空时根本
//! 不必建 [`ScriptEventSource`]（`ll_game::content` 就是这么做的）。
//!
//! 三种事件的频率都是「每回合若干次」而不是「每帧」，选取判据见
//! [`crate::event::GameEventKind`] 文档。
//!
//! # 确定性
//!
//! - **回调顺序**：订阅表按登记顺序（= mod 拓扑序 + 文件内顺序）线性
//!   遍历，见 [`crate::event`] 模块文档「确定性」一节。
//! - **C3（随机性）**：本模块**不注册** `ll_script::api::rng`。事件
//!   处理函数因此拿不到任何随机源——不是疏漏：`DetRng::for_entity`
//!   要求一个「事件计数器」把同一实体的不同事件分流，而一次结算里
//!   同一个实体完全可能收到多条同种事件（连续两次伤害），没有一个
//!   现成的、确定性的计数器可用。与其给一个可能重复的流，不如这一批
//!   先不给随机性——需要随机的 mod 用例出现时，再连同计数器一起设计。
//! - **C1（不写世界）**：见上文，处理函数只能返回数据。
//!
//! # 为什么不注册 `ll_script::api::state`
//!
//! 那套 API 的写入路径靠一个跨帧累积的 `thread_local`
//! （`ll_script::api::state::take_pending_writes`），而它**当前零生产
//! 调用方**、`ScriptBehaviorSource::decide` 从不清空它——那是一处
//! 已知缺陷，正在被单独修。本模块刻意不建立在它上面：处理函数产出
//! 状态写入的通道是**返回值**（见 [`parse_writes`]），与
//! `ll_script::api::intent::parse_intent`「脚本返回数据、宿主翻译」
//! 同一个模式，不依赖任何跨调用累积的缓冲区。
//!
//! 代价是处理函数**读不到**自己写过的脚本状态（`state-get!` 也没注册）
//! ——这一批因此只支持「无条件写入」类的用例（打标记、记最后一次
//! 事件的数值）。这是一条写下来的已知边界，不是遗漏；补它的正确形状
//! 是在那处 `thread_local` 缺陷修好之后，把读侧原语单独注册进来。

use ll_script::api::event::{EventPayload, with_active_event_for};
use ll_script::api::handle::ScriptEntityHandle;
use ll_script::host::{ScriptEngine, ScriptError};
use ll_sim::effect::Effect;
use ll_world::script_state::{
    PER_MOD_ENTITY_QUOTA_BYTES, PER_MOD_QUOTA_BYTES, ScriptStateTarget, ScriptStateWrite,
    ScriptValue, entity_mod_bytes, entry_size, mod_total_bytes,
};
use ll_world::state::WorldState;
use steel::rvals::{FromSteelVal, SteelVal};

use crate::event::{EventSubscriptionTable, GameEventKind};

/// 一个**只构造、尚未注册任何 API、尚未编译任何脚本**的事件引擎。
///
/// 与 [`crate::script_behavior_source::PreparedBehaviorEngine`] 逐字
/// 同一个理由：约束 C6 / ADR 0028 要求同一根线程上全部引擎构造先于
/// 全部脚本编译，而事件引擎需要装载完毕才有的数据（订阅表、脚本源码），
/// 所以「注册 API + 编译」天然排在 mod 装载之后。把「造引擎」提前到
/// 构造阶段是唯一能同时满足两者的做法。
pub struct PreparedEventEngine(ScriptEngine);

impl PreparedEventEngine {
    /// 构造一个空引擎。**必须在本线程编译任何脚本之前调用**。
    pub fn new() -> Self {
        Self(ScriptEngine::new())
    }

    fn into_engine(self) -> ScriptEngine {
        self.0
    }
}

impl Default for PreparedEventEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 建 [`ScriptEventSource`] 时可能出现的错误。
#[derive(Debug)]
pub enum EventSourceError {
    /// 某个订阅方 mod 的脚本编译失败。
    Script {
        /// 订阅方命名空间。
        mod_namespace: String,
        /// 原始编译错误。
        error: ScriptError,
    },
    /// 订阅里点名的处理函数在该 mod 的结算期引擎上查不到。
    ///
    /// 这是「注册期完整校验」（ADR 0017）在事件订阅上的落点：一条
    /// 指向不存在函数的订阅，若不在这里报出来，就会在结算期变成一条
    /// **永远静默失败**的回调——mod 作者只会看到"我的处理函数没被
    /// 调用"，而没有任何线索。
    UnknownHandler {
        /// 订阅方命名空间。
        mod_namespace: String,
        /// 查不到的那个函数名。
        handler: String,
    },
    /// 订阅方 mod 的脚本源码不在宿主给的源码清单里——多半是宿主接线
    /// 漏了一个 mod。
    MissingSource {
        /// 订阅方命名空间。
        mod_namespace: String,
    },
    /// 宿主给的预构造引擎数量不够。
    NotEnoughEngines {
        /// 需要几个（= 有订阅的 mod 个数）。
        required: usize,
        /// 实际给了几个。
        provided: usize,
    },
}

impl std::fmt::Display for EventSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSourceError::Script {
                mod_namespace,
                error,
            } => write!(f, "mod {mod_namespace:?} 的事件脚本编译失败：{error}"),
            EventSourceError::UnknownHandler {
                mod_namespace,
                handler,
            } => write!(
                f,
                "mod {mod_namespace:?} 用 on-event 订阅了处理函数 {handler:?}，\
                 但该 mod 的脚本里没有定义这个名字——请确认函数名拼写，\
                 以及它确实是一个顶层 define 出来的零参函数。"
            ),
            EventSourceError::MissingSource { mod_namespace } => write!(
                f,
                "mod {mod_namespace:?} 有事件订阅，但宿主没有提供它的脚本源码"
            ),
            EventSourceError::NotEnoughEngines { required, provided } => write!(
                f,
                "事件分发需要 {required} 个预构造引擎，宿主只给了 {provided} 个"
            ),
        }
    }
}

impl std::error::Error for EventSourceError {}

/// 一个订阅方 mod 的结算期引擎。
struct ModEngine {
    namespace: String,
    engine: ScriptEngine,
}

/// 按订阅表把运行期事件分发给各 mod 处理函数的分发器。
pub struct ScriptEventSource {
    engines: Vec<ModEngine>,
    subscriptions: EventSubscriptionTable,
}

impl ScriptEventSource {
    /// 建一个分发器。
    ///
    /// - `prepared`：预构造好的引擎，至少要有「有订阅的 mod」那么多个
    ///   （见类型 [`PreparedEventEngine`] 文档）。多余的会被就地丢弃。
    /// - `sources`：`(mod 命名空间, 脚本源码)`，同一个 mod 的多份脚本
    ///   给多条，全部装进该 mod 的同一个引擎——与装载期「作用域单位是
    ///   mod 而不是脚本文件」（提交 `3bd5e98`）保持一致。
    /// - `subscriptions`：装载期收集到的订阅表。
    ///
    /// 只为**真的有订阅**的 mod 建引擎：没订阅的 mod 一个字节的结算期
    /// 脚本环境都不占。
    pub fn new(
        prepared: Vec<PreparedEventEngine>,
        sources: &[(String, String)],
        subscriptions: EventSubscriptionTable,
    ) -> Result<Self, EventSourceError> {
        // 订阅方命名空间，按订阅登记顺序去重——顺序仍然确定（C5），
        // 且不引入任何哈希容器。
        let mut namespaces: Vec<&str> = Vec::new();
        for subscription in subscriptions.all() {
            if !namespaces.contains(&subscription.mod_namespace.as_str()) {
                namespaces.push(&subscription.mod_namespace);
            }
        }

        if prepared.len() < namespaces.len() {
            return Err(EventSourceError::NotEnoughEngines {
                required: namespaces.len(),
                provided: prepared.len(),
            });
        }

        let mut prepared = prepared.into_iter();
        let mut engines = Vec::with_capacity(namespaces.len());
        for namespace in namespaces {
            let mut engine = prepared.next().expect("上面已经核对过数量").into_engine();
            ll_script::api::query::register(&mut engine);
            ll_script::api::event::register(&mut engine);

            let mut compiled_any = false;
            for (source_namespace, source) in sources {
                if source_namespace != namespace {
                    continue;
                }
                compiled_any = true;
                engine
                    .load_source(source.clone())
                    .map_err(|error| EventSourceError::Script {
                        mod_namespace: namespace.to_string(),
                        error,
                    })?;
            }
            if !compiled_any {
                return Err(EventSourceError::MissingSource {
                    mod_namespace: namespace.to_string(),
                });
            }

            // ADR 0017：订阅点名的处理函数此刻就得存在，见
            // `EventSourceError::UnknownHandler` 文档。
            for subscription in subscriptions.all() {
                if subscription.mod_namespace != namespace {
                    continue;
                }
                if !engine.has_definition(&subscription.handler) {
                    return Err(EventSourceError::UnknownHandler {
                        mod_namespace: namespace.to_string(),
                        handler: subscription.handler.clone(),
                    });
                }
            }

            engines.push(ModEngine {
                namespace: namespace.to_string(),
                engine,
            });
        }

        Ok(Self {
            engines,
            subscriptions,
        })
    }

    /// 一条效果要落地了：翻译成事件、回调订阅者、收集它们产出的反应
    /// 效果。
    ///
    /// 返回的 `Effect` 由调用方（`ll_sim::turn::TurnEngine::perform`）
    /// 交给 `apply`——本函数自己一行世界都不写。
    ///
    /// 没有对应事件种类、或没有任何订阅者时返回空 `Vec`，不构造负载，
    /// 见模块文档「性能形状」一节。
    pub fn dispatch(&mut self, world: &WorldState, effect: &Effect) -> Vec<Effect> {
        let Some((kind, payload)) = payload_for(effect) else {
            return Vec::new();
        };
        if !self.subscriptions.has_subscriber(kind) {
            return Vec::new();
        }

        let mut writes: Vec<ScriptStateWrite> = Vec::new();
        for subscription in self.subscriptions.subscribers_of(kind) {
            let Some(mod_engine) = self
                .engines
                .iter_mut()
                .find(|e| e.namespace == subscription.mod_namespace)
            else {
                // 建 `ScriptEventSource` 时已经为每个订阅方 mod 建过
                // 引擎，理论上不可达；仍然选择跳过而不是 panic，与本
                // 仓库全部脚本边界的降级纪律一致。
                continue;
            };
            let returned = with_active_event_for(payload, || {
                mod_engine
                    .engine
                    .call_raw(&subscription.handler, Vec::new())
                    .ok()
            });
            let Some(value) = returned else {
                continue;
            };
            parse_writes(world, &subscription.mod_namespace, &value, &mut writes);
        }

        if writes.is_empty() {
            Vec::new()
        } else {
            vec![Effect::SetScriptState { writes }]
        }
    }
}

/// 把一条效果翻译成 `(事件种类, 负载)`；不是可订阅事件时返回 `None`。
///
/// `match` 里那条 `_ => None` 是**刻意**的通配分支——与本仓库其余
/// 「新增变体必须编译失败」的穷尽 `match` 不同：`Effect` 有三十多个
/// 变体，绝大多数是引擎内部的簿记（`ScheduleNext`/`MarkExplored`……），
/// 强迫每个新增变体在这里做一次「要不要开成事件」的决定，只会得到
/// 一长串机械的 `_ => None`。真正守住「事件种类不能悄悄膨胀」的是
/// [`GameEventKind`] 那个枚举本身与它的文档纪律（加变体之前必须先有
/// 真实 mod 用例）。
fn payload_for(effect: &Effect) -> Option<(GameEventKind, EventPayload)> {
    match effect {
        Effect::Damage { target, amount } => Some((
            GameEventKind::Damaged,
            EventPayload {
                kind: GameEventKind::Damaged.as_str(),
                actor: None,
                target: Some(*target),
                amount: i64::from(*amount),
            },
        )),
        Effect::Kill { target, killer, .. } => Some((
            GameEventKind::Killed,
            EventPayload {
                kind: GameEventKind::Killed.as_str(),
                actor: *killer,
                target: Some(*target),
                amount: 0,
            },
        )),
        Effect::GrantExperience { target, amount } => Some((
            GameEventKind::ExperienceGained,
            EventPayload {
                kind: GameEventKind::ExperienceGained.as_str(),
                actor: None,
                target: Some(*target),
                amount: *amount,
            },
        )),
        _ => None,
    }
}

/// 把处理函数的返回值翻译成一批脚本状态写入，追加进 `out`。
///
/// # 返回值的形状
///
/// 处理函数返回一个**写入列表**，每条写入是一个列表：
///
/// ```text
/// (list 'global "key" value)          ; 写这个 mod 的全局状态
/// (list 'entity <handle> "key" value) ; 写某个实体上这个 mod 的状态
/// ```
///
/// `value` 只支持整数/布尔/字符串三种——覆盖「计数器」「标记」两类
/// 用例，不支持列表/表：那两种会让配额判定与错误呈现都复杂一大截，
/// 而这一批没有任何用例需要它们（YAGNI）。
///
/// 返回 `#f`、空表、或任何无法解析的东西都当作「没有写入」，静默跳过
/// ——脚本边界的一贯降级纪律。不解析的**不是**静默的正确性损失：
/// 处理函数写错形状的表现是「我的写入没生效」，而它本来就没有别的
/// 生效路径。
///
/// # 命名空间由宿主固化
///
/// `mod_namespace` 是订阅表里记下的那个（宿主在装载窗口里固化，脚本
/// 参数里没有它，见 `crate::event::EventSubscription::mod_namespace`
/// 文档）——一个 mod 的处理函数**写不进**别的 mod 的命名空间。
///
/// # 配额
///
/// 逐条用 `ll_world::script_state` 的既有口径判定（已提交 + 本批已攒
/// 下的待写记录），超限就丢弃这一条并继续。与 `state-set!` 走的是
/// 同一组函数、同一对常量，不另立第二套口径。
fn parse_writes(
    world: &WorldState,
    mod_namespace: &str,
    value: &SteelVal,
    out: &mut Vec<ScriptStateWrite>,
) {
    let SteelVal::ListV(items) = value else {
        return;
    };
    for item in items {
        let Some(write) = parse_one_write(mod_namespace, item) else {
            continue;
        };
        if fits_quota(world, &write, out) {
            out.push(write);
        }
    }
}

/// 解析单条写入表达式。
fn parse_one_write(mod_namespace: &str, item: &SteelVal) -> Option<ScriptStateWrite> {
    let SteelVal::ListV(fields) = item else {
        return None;
    };
    let fields: Vec<SteelVal> = fields.iter().cloned().collect();
    let tag = match fields.first()? {
        SteelVal::SymbolV(s) => s.to_string(),
        SteelVal::StringV(s) => s.to_string(),
        _ => return None,
    };

    match tag.as_str() {
        "global" => {
            let key = as_string(fields.get(1)?)?;
            let value = as_script_value(fields.get(2)?)?;
            Some(ScriptStateWrite {
                target: ScriptStateTarget::Global,
                mod_namespace: mod_namespace.to_string(),
                key,
                value,
            })
        }
        "entity" => {
            let handle = ScriptEntityHandle::from_steelval(fields.get(1)?).ok()?;
            let key = as_string(fields.get(2)?)?;
            let value = as_script_value(fields.get(3)?)?;
            Some(ScriptStateWrite {
                target: ScriptStateTarget::Entity(handle.entity_id()),
                mod_namespace: mod_namespace.to_string(),
                key,
                value,
            })
        }
        _ => None,
    }
}

fn as_string(value: &SteelVal) -> Option<String> {
    match value {
        SteelVal::StringV(s) => Some(s.to_string()),
        SteelVal::SymbolV(s) => Some(s.to_string()),
        _ => None,
    }
}

fn as_script_value(value: &SteelVal) -> Option<ScriptValue> {
    match value {
        SteelVal::IntV(v) => Some(ScriptValue::Int(*v as i64)),
        SteelVal::BoolV(v) => Some(ScriptValue::Bool(*v)),
        SteelVal::StringV(s) => Some(ScriptValue::Str(s.to_string().into_boxed_str())),
        _ => None,
    }
}

/// 这条写入落地之后会不会超配额——口径与 `state-set!` 完全一致。
fn fits_quota(world: &WorldState, write: &ScriptStateWrite, pending: &[ScriptStateWrite]) -> bool {
    let added = entry_size(&write.key, &write.value);
    if added == usize::MAX {
        return false;
    }
    if mod_total_bytes(world, &write.mod_namespace, pending) + added > PER_MOD_QUOTA_BYTES {
        return false;
    }
    if let ScriptStateTarget::Entity(entity) = write.target
        && entity_mod_bytes(world, entity, &write.mod_namespace, pending) + added
            > PER_MOD_ENTITY_QUOTA_BYTES
    {
        return false;
    }
    true
}

/// 让 [`payload_for`] 可以被本 crate 的测试直接调用——不对外公开，
/// 它是一条实现细节而不是 API 表面。
#[cfg(test)]
pub(crate) fn payload_for_testing(effect: &Effect) -> Option<(GameEventKind, EventPayload)> {
    payload_for(effect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_world::entity::{Arena, EntityId};
    use ll_world::history::KillCause;

    fn some_entity() -> EntityId {
        let mut arena: Arena<()> = Arena::new();
        arena.spawn(())
    }

    #[test]
    fn 三种可订阅效果各自翻译出正确的事件种类与负载() {
        // Arrange
        let target = some_entity();
        let killer = some_entity();

        // Act & Assert
        let (kind, payload) = payload_for_testing(&Effect::Damage { target, amount: 12 })
            .expect("Damage 必须是可订阅事件");
        assert_eq!(kind, GameEventKind::Damaged);
        assert_eq!(payload.target, Some(target));
        assert_eq!(payload.amount, 12);

        let (kind, payload) = payload_for_testing(&Effect::Kill {
            target,
            killer: Some(killer),
            cause: KillCause::Fall,
        })
        .expect("Kill 必须是可订阅事件");
        assert_eq!(kind, GameEventKind::Killed);
        assert_eq!(payload.actor, Some(killer));
        assert_eq!(payload.target, Some(target));

        let (kind, payload) = payload_for_testing(&Effect::GrantExperience { target, amount: 40 })
            .expect("GrantExperience 必须是可订阅事件");
        assert_eq!(kind, GameEventKind::ExperienceGained);
        assert_eq!(payload.amount, 40);
    }

    #[test]
    fn 未开成事件的效果翻译出none() {
        // 「宁可少开几种」——MoveTo 是频率最高的一条效果，刻意不开，
        // 见 `crate::event::GameEventKind` 文档。
        // Arrange
        let actor = some_entity();
        let effect = Effect::MoveTo {
            actor,
            pos: ll_core::torus::TorusSize::new(8, 8)
                .expect("8x8 是合法尺寸")
                .wrap(0, 0),
        };

        // Act & Assert
        assert!(payload_for_testing(&effect).is_none());
    }
}
