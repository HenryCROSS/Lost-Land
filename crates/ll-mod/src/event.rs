//! 运行期事件订阅表：mod 声明「我关心哪几种运行期事件、由哪个函数
//! 处理」，装载期登记，结算期按登记顺序回调。
//!
//! # 补的是哪个缺口
//!
//! 在本模块之前，mod 脚本只能在**装载期**注册内容（`register-*` 那
//! 一整套），对**运行期发生了什么一无所知**：一场战斗打死了谁、谁拿
//! 到了多少经验，脚本全都看不见。唯一一条运行期脚本通道是行为树
//! （`crate::script_behavior_source`），而它是「被问：这个实体这一
//! 回合做什么」，不是「被告知：刚刚发生了什么」——两者方向相反，用途
//! 也不重叠（行为树只在轮到某个非受控实体行动时被调用一次，看不到
//! 别人身上发生的事）。
//!
//! # 为什么必须先声明才回调
//!
//! 跨脚本边界一次调用约 326ns（ADR 0016/0017 的实测口径）。结算是
//! 热路径：`ll_sim::turn::TurnEngine::perform` 每结算一次行动就会走
//! 一遍效果列表。若每条效果都无条件回调一次脚本，**没有装任何 mod
//! 的玩家也要为这套机制付钱**，而他一条订阅都没有。
//!
//! 因此本模块的形状是「订阅表」而不是「事件总线」：
//!
//! - 没有任何 mod 订阅某种事件时，
//!   [`EventSubscriptionTable::has_subscriber`] 为假，宿主连 payload
//!   都不构造，开销是一次对小 `Vec` 的线性扫描。
//! - 订阅了才回调，且只回调订阅了**这一种**事件的那几个处理函数。
//!
//! 这条判据也决定了[`GameEventKind`]为什么只有两种：一个没有消费者
//! 的事件种类是纯负担（本项目已经发现三十处「声明了但从没接线」）。
//! 两种的选取理由、以及被刻意排除的那几种，见 [`GameEventKind`] 文档。
//!
//! # 确定性（约束 C5）
//!
//! 订阅存在一个 [`Vec`] 里，**按登记顺序**——登记顺序来自装载管线的
//! 拓扑排序（`crate::pipeline::load_all` 按 `crate::topo::topo_sort`
//! 的结果逐个 mod 装载），同一份 mod 集合两次装载得到逐条相同的顺序。
//! 多个 mod 订阅同一种事件时，回调顺序因此是确定的：**先装载的 mod
//! 先回调**，而先装载意味着「它是别人的依赖」。这不是随手选的顺序，
//! 它与依赖方向一致：被依赖者先看到事件，依赖者后看到。
//!
//! 绝不用 `HashMap<GameEventKind, Vec<...>>` 之类按事件种类分桶的
//! 结构再遍历桶——那会让「同一事件的多个订阅之间谁先谁后」取决于
//! 哈希迭代顺序，正是 C5 点名禁止的。
//!
//! # 这不是一张「内容表」
//!
//! [`EventSubscriptionTable`] 里没有任何 [`ll_core::ident::ContentIndex`]
//! ——它存的是 `(mod 命名空间, 事件种类, 处理函数名)` 三元组，不是
//! 「一条内容的字段值」。因此它**不进** `crate::content_hash` 的
//! [`ContentValueTables`](crate::content_hash::ContentValueTables)、
//! 不需要 `classify_index` 认领、不进存档 remap，
//! `CONTENT_HASH_ALGORITHM_VERSION` 也不因它递增。
//!
//! 这与 `crate::xp_curve::XpCurveBindings` 是同一类东西（一张只做映射、
//! 自己不持有内容条目的表），那条先例已经在
//! `ll_game::content::load_content` 的注释里显式记录过。**订阅表甚至
//! 比它更彻底**：曲线绑定至少两端都是 `ContentIndex`，订阅表连一个
//! 都没有。
//!
//! 订阅表也**不进存档**：它是「本次装载装了哪些 mod、它们订阅了什么」
//! 的派生物，读档时会随内容重新装载一次重建，与
//! `ll_mod::clip::ClipTable` 不进 `WorldState` 同一条理由（ADR 0020）。

use std::fmt;

/// 一种可以被 mod 订阅的运行期事件。
///
/// # 只有两种，判据是什么
///
/// `knowledge/design/mod-lifecycle-and-event-api.md` 二、2 节已经按
/// 频率量级把候选事件逐条评过一遍，本枚举**照它的结论取**，不另立
/// 一套判据：该表判为「逐条投递给全局监听器给得起」的是击杀/死亡、
/// 任务推进、历史事件、季节切换四类（量级几十到几百条/局）。
///
/// 本批次落地其中**在生产结算路径上真的会产出**的那一支——击杀，
/// 外加它的下游 `GrantExperience`（每次击杀至多一条，与击杀同量级）。
/// 任务推进/历史事件/季节切换三类的 `Effect` 侧接线各自还缺东西
/// （分别是任务完成判定、世界史生成、季节派生），等它们真的在生产
/// 路径上产出时再开，那时代价只是「本枚举 + `parse` + 宿主的 payload
/// 构造」三处。
///
/// 被**刻意排除**的候选：
///
/// - **`Effect::Damage`（命中/伤害）**：设计文档那张表把它判为
///   **逐条给不起**——`damage-formula-mod-api.md` 与
///   `buffs-and-triggers.md` 已经各自独立论证过一次（三轴战斗与背景
///   模拟落地后累计几十万次/局量级）。本批次的「先声明才回调」只让
///   **没订阅的玩家**不花钱，订阅了的 mod 仍然要为每一次命中付一次
///   跨界；那正是那两份文档判为付不起的东西。设计文档给出的正确形状
///   是**批量投递**（该文档二、3 节的 `EventBatchHandle` + 算子代数），
///   那是一次独立的、与「批量查询」共享形状的设计，不夹带在本批次。
/// - `Effect::MoveTo`：频率比命中更高，而"谁走到哪儿了"这件事行为树
///   自己就能查（`nearby-enemy` 等）。
/// - `Effect::SetScriptState`：脚本自己写的东西再回调给脚本，是一条
///   现成的无限递归入口，且没有任何用例。
/// - `Effect::ScheduleNext`/`Effect::MarkExplored` 之类的簿记效果：
///   它们是引擎内部推进的一部分，对 mod 没有语义。
///
/// **加变体之前必须先有一个真实的 mod 用例**，这条纪律比机制本身重要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GameEventKind {
    /// 有实体被杀死（`ll_sim::effect::Effect::Kill`）。
    ///
    /// 用例：赏金/声望系统、「杀满 N 个某类怪物解锁某物」这类计数。
    /// 这是三种里最有价值的一种：击杀在整个 `ll-sim` 里只有一个产出
    /// 路径，而围绕它的玩法钩子几乎是所有 roguelike mod 的第一需求。
    Killed,
    /// 有实体获得经验（`ll_sim::effect::Effect::GrantExperience`）。
    ///
    /// 用例：经验加成类 mod 的观察点；「本局共获得多少经验」这类统计。
    ExperienceGained,
}

impl GameEventKind {
    /// 脚本里写的那个字符串。
    ///
    /// 与 [`Self::parse`] 是同一份映射的两个方向，两者都不带通配分支，
    /// 新增变体时会双向编译失败。
    pub fn as_str(self) -> &'static str {
        match self {
            GameEventKind::Killed => "killed",
            GameEventKind::ExperienceGained => "experience-gained",
        }
    }

    /// 解析脚本传进来的事件种类字符串。
    ///
    /// 无法识别时返回 `None`——调用方（`crate::script_event_api`）负责
    /// 把它变成一条**点名了全部合法取值**的装载期错误，而不是静默
    /// 注册出一条永远不会被触发的订阅（ADR 0017「注册期完整校验」）。
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "killed" => GameEventKind::Killed,
            "experience-gained" => GameEventKind::ExperienceGained,
            _ => return None,
        })
    }

    /// 全部合法取值，供错误文案列举。
    pub const ALL: [GameEventKind; 2] = [GameEventKind::Killed, GameEventKind::ExperienceGained];
}

impl fmt::Display for GameEventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一条订阅：哪个 mod、关心哪种事件、由哪个函数处理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSubscription {
    /// 订阅方 mod 的命名空间。
    ///
    /// 由宿主在装载窗口里固化（脚本没有参数能覆盖它），与
    /// `ll_world::script_state::ScriptStateWrite::mod_namespace` 同一条
    /// 纪律：这里存的命名空间恒等于发起订阅的那个 mod 自己，处理函数
    /// 产出的状态写入因此也只能落在它自己的命名空间下。
    pub mod_namespace: String,
    /// 关心哪种事件。
    pub kind: GameEventKind,
    /// 处理函数名——必须是同一个 mod 的脚本在顶层 `define` 出来的
    /// 零参函数。
    ///
    /// 存字符串而不是任何 Steel 侧的函数值：装载期的引擎与结算期的
    /// 引擎不是同一个（两者的白名单能力表刻意不兼容，见
    /// `mods/example_mod/mod.json5` 里 `entry_points` 上方的注释），
    /// 一个装载期引擎里的闭包在结算期引擎上毫无意义。按名字调用是
    /// 唯一跨得过这道隔离墙的形式，与
    /// `crate::script_behavior_source::ScriptBehaviorSource::tree_entry_fn`
    /// 同一个手法。
    pub handler: String,
}

/// 装载期收集起来的全部事件订阅，按登记顺序。
///
/// 顺序即回调顺序，见模块文档「确定性」一节。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventSubscriptionTable {
    subscriptions: Vec<EventSubscription>,
}

/// 事件订阅注册期可能出现的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventSubscriptionError {
    /// 事件种类字符串无法识别。
    UnknownKind {
        /// 脚本传进来的那个字符串。
        raw: String,
    },
    /// 处理函数名是空串——没有任何函数叫这个名字，注册它只会在结算期
    /// 变成一条静默失败的回调。
    EmptyHandler,
    /// 同一个 mod 对同一种事件登记了同一个处理函数两次。
    ///
    /// 不静默去重：重复登记多半意味着脚本作者复制粘贴时漏改了一处，
    /// 静默吞掉会让「为什么我的处理函数被调了两次」变成一个查不出来
    /// 的问题——与 `crate::class::ClassError::DuplicateDefinition`
    /// 同一条 ADR 0017 纪律。
    Duplicate {
        /// 重复的那条订阅。
        subscription: EventSubscription,
    },
}

impl fmt::Display for EventSubscriptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventSubscriptionError::UnknownKind { raw } => {
                let all: Vec<&str> = GameEventKind::ALL.iter().map(|k| k.as_str()).collect();
                write!(
                    f,
                    "未知的事件种类 {raw:?}——合法取值只有：{}",
                    all.join("、")
                )
            }
            EventSubscriptionError::EmptyHandler => {
                write!(f, "事件处理函数名不能为空串")
            }
            EventSubscriptionError::Duplicate { subscription } => write!(
                f,
                "mod {:?} 已经用同一个处理函数 {:?} 订阅过事件 {} 了",
                subscription.mod_namespace, subscription.handler, subscription.kind
            ),
        }
    }
}

impl std::error::Error for EventSubscriptionError {}

impl EventSubscriptionTable {
    /// 一张空订阅表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一条订阅。重复登记同一条返回错误，见
    /// [`EventSubscriptionError::Duplicate`]。
    pub fn subscribe(
        &mut self,
        subscription: EventSubscription,
    ) -> Result<(), EventSubscriptionError> {
        if subscription.handler.is_empty() {
            return Err(EventSubscriptionError::EmptyHandler);
        }
        if self.subscriptions.contains(&subscription) {
            return Err(EventSubscriptionError::Duplicate { subscription });
        }
        self.subscriptions.push(subscription);
        Ok(())
    }

    /// 全部订阅，按登记顺序。
    pub fn all(&self) -> &[EventSubscription] {
        &self.subscriptions
    }

    /// 有没有任何 mod 订阅了 `kind`——宿主每次结算都会问一次，这是
    /// 「没人订阅就一分钱都不花」那条承诺的落点，见模块文档。
    ///
    /// 线性扫描而不是预先建一张按种类分桶的表：订阅总数以「装了几个
    /// mod」为量级（个位数到几十条），线性扫过去比维护第二份索引更
    /// 简单，也不引入任何哈希容器（C5）。
    pub fn has_subscriber(&self, kind: GameEventKind) -> bool {
        self.subscriptions.iter().any(|s| s.kind == kind)
    }

    /// 一条订阅都没有——宿主据此可以整个跳过事件分发的搭建。
    pub fn is_empty(&self) -> bool {
        self.subscriptions.is_empty()
    }

    /// 订阅了 `kind` 的全部订阅，按登记顺序。
    pub fn subscribers_of(
        &self,
        kind: GameEventKind,
    ) -> impl Iterator<Item = &EventSubscription> + '_ {
        self.subscriptions.iter().filter(move |s| s.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subscription(namespace: &str, kind: GameEventKind, handler: &str) -> EventSubscription {
        EventSubscription {
            mod_namespace: namespace.to_string(),
            kind,
            handler: handler.to_string(),
        }
    }

    #[test]
    fn 事件种类字符串双向映射一致() {
        // 守卫 `as_str`/`parse` 两份映射不分叉——新增一个变体只改一边，
        // 本条立刻变红。
        // Arrange & Act & Assert
        for kind in GameEventKind::ALL {
            assert_eq!(GameEventKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn 未知事件种类解析失败而不是退化成某个默认值() {
        // 用 "damaged" 当反例不是随手挑的：它是被**刻意排除**的那一条
        // （见 GameEventKind 文档），而排除的东西必须真的解析不出来，
        // 不能靠"没人写"来保证。
        // Arrange & Act
        let parsed = GameEventKind::parse("damaged");

        // Assert
        assert_eq!(parsed, None);
    }

    #[test]
    fn 空订阅表对任何事件种类都没有订阅者() {
        // 这是「没人订阅就一分钱都不花」那条承诺的最小守卫。
        // Arrange
        let table = EventSubscriptionTable::new();

        // Act & Assert
        assert!(table.is_empty());
        for kind in GameEventKind::ALL {
            assert!(!table.has_subscriber(kind));
        }
    }

    #[test]
    fn 订阅顺序就是登记顺序不依赖任何哈希容器() {
        // 约束 C5：多个 mod 订阅同一种事件时回调顺序必须确定。
        // Arrange
        let mut table = EventSubscriptionTable::new();
        table
            .subscribe(subscription("moda", GameEventKind::Killed, "a-on-kill"))
            .expect("首次登记应当成功");
        table
            .subscribe(subscription("modb", GameEventKind::Killed, "b-on-kill"))
            .expect("首次登记应当成功");
        table
            .subscribe(subscription("modc", GameEventKind::Killed, "c-on-kill"))
            .expect("首次登记应当成功");

        // Act
        let order: Vec<&str> = table
            .subscribers_of(GameEventKind::Killed)
            .map(|s| s.mod_namespace.as_str())
            .collect();

        // Assert
        assert_eq!(order, vec!["moda", "modb", "modc"]);
    }

    #[test]
    fn 只回调订阅了这一种事件的处理函数() {
        // Arrange
        let mut table = EventSubscriptionTable::new();
        table
            .subscribe(subscription("moda", GameEventKind::Killed, "on-kill"))
            .expect("登记应当成功");
        table
            .subscribe(subscription(
                "moda",
                GameEventKind::ExperienceGained,
                "on-xp",
            ))
            .expect("登记应当成功");

        // Act
        let killed: Vec<&str> = table
            .subscribers_of(GameEventKind::Killed)
            .map(|s| s.handler.as_str())
            .collect();

        // Assert
        assert_eq!(killed, vec!["on-kill"]);
        let xp: Vec<&str> = table
            .subscribers_of(GameEventKind::ExperienceGained)
            .map(|s| s.handler.as_str())
            .collect();
        assert_eq!(xp, vec!["on-xp"]);
    }

    #[test]
    fn 重复登记同一条订阅返回错误而不是静默去重() {
        // Arrange
        let mut table = EventSubscriptionTable::new();
        let entry = subscription("moda", GameEventKind::Killed, "on-kill");
        table.subscribe(entry.clone()).expect("首次登记应当成功");

        // Act
        let result = table.subscribe(entry.clone());

        // Assert
        assert_eq!(
            result,
            Err(EventSubscriptionError::Duplicate {
                subscription: entry
            })
        );
    }

    #[test]
    fn 同一个mod用两个不同函数订阅同一种事件是合法的() {
        // 「重复」的判据是三元组整体相同，不是「同一个 mod + 同一种
        // 事件」——一个 mod 完全可以为同一种事件挂两个互不相干的处理
        // 函数。
        // Arrange
        let mut table = EventSubscriptionTable::new();
        table
            .subscribe(subscription("moda", GameEventKind::Killed, "count-kills"))
            .expect("登记应当成功");

        // Act
        let result = table.subscribe(subscription("moda", GameEventKind::Killed, "log-kills"));

        // Assert
        assert!(result.is_ok());
        assert_eq!(table.all().len(), 2);
    }

    #[test]
    fn 空处理函数名被拒绝() {
        // 注册一个叫空串的处理函数只会在结算期变成一条静默失败的回调。
        // Arrange
        let mut table = EventSubscriptionTable::new();

        // Act
        let result = table.subscribe(subscription("moda", GameEventKind::Killed, ""));

        // Assert
        assert_eq!(result, Err(EventSubscriptionError::EmptyHandler));
    }

    #[test]
    fn 未知事件种类的错误文案列出全部合法取值() {
        // 读到这条错误的是 mod 作者，文案必须直接给出下一步动作。
        // Arrange
        let error = EventSubscriptionError::UnknownKind {
            raw: "moved".to_string(),
        };

        // Act
        let text = error.to_string();

        // Assert
        assert!(text.contains("moved"));
        for kind in GameEventKind::ALL {
            assert!(text.contains(kind.as_str()), "错误文案漏了 {kind}");
        }
    }
}
