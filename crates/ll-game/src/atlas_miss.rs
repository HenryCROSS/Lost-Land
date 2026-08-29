//! 图集查图未命中该不该留痕——**这一个决定的唯一落点**。
//!
//! # 这个模块修的是哪条缺陷
//!
//! 角色创建批次给精灵查找接上了回退链（[`crate::surface_draw::SurfaceDraw::preferred_keys`]，
//! 候选是 `<种族>_<职业>_<性别>` → `<种族>_<职业>` → `<种族>`）。今天
//! 一张合成图都没有，所以前两级**必然**未命中——那正是回退链的正常
//! 工作方式。而绘制那一侧对**每一次**查不到都打一条 `ERROR`，于是
//! 每帧、每个 NPC、每个候选键各刷一行，项目所有者实机撞到：
//!
//! ```text
//! ERROR ll_game::app: 图集条目缺失，跳过本次绘制 name="lostland_human_lostland_farmer"
//! ERROR ll_game::app: 图集条目缺失，跳过本次绘制 name="lostland_human_lostland_fisher_female"
//! ```
//!
//! 三重代价：① 日志被淹（所有者正要靠 `logs/lostland.<日期>.log` 报
//! 缺陷）；② `ERROR` 这个级别被用在正常路径上，真出事时没人看得见；
//! ③ 每帧格式化几十条字符串。
//!
//! # 判据：还有候选可试 ⇒ 一个字都不打；全部落空 ⇒ 一条 `WARN`，去重
//!
//! 所有者的裁定是「这一类改成 Warning」，但**级别降级不能替代去重**：
//! `WARN` 刷屏和 `ERROR` 刷屏一样会把日志文件淹掉。两件事都要做。
//!
//! - **中间未命中：完全不打。** 连 `trace!` 都不打——回退链的中间步骤
//!   未命中不是任何意义上的事件。这一条由**类型**保证：探测走
//!   `exists` 这个 `FnMut(&str) -> bool`，它拿不到任何日志通道；本模块
//!   里能产出「要打的话」的只有 [`ChainOutcome::MissedFirstTime`] 一个
//!   变体。
//! - **全部落空：一条 `WARN`，同一组候选在整个进程生命期内只打一次。**
//!
//! # 为什么是去重而不是限流
//!
//! 1. **成本上界是内容规模，不是时长。** 去重之后日志总行数 ≤ 不同候选
//!    链的条数（本体今天是 9 个种族 + 13 个职业这个量级），与帧率、NPC
//!    数量、游玩时长全部无关。限流的上界是「时长 ÷ 周期」，玩一小时
//!    仍然是几十上百行。
//! 2. **信息量在第一次就全部给出了。** 第 4000 次「同一个键还是查不到」
//!    不携带任何新信息。
//! 3. **它保住了这条诊断线索的真正价值。** 本仓库踩过「查裸名字、图集
//!    存带前缀，五张 UI 贴图全部查不到、每帧静默退回纯色、**不打任何
//!    日志**」那个缺陷（`knowledge/handoff/2026-08-27-session-handoff.md`
//!    二节）。在本模块的策略下，那个缺陷会在启动后立刻产出 5 行 `WARN`
//!    然后安静下去——**响亮、有限、看得见**，正是当时缺的东西。
//!    **没有矫枉过正改成完全静默。**
//!
//! # 为什么键是整条候选链，不是单个键
//!
//! 玩家（以及排查的人）关心的是「这条绘制指令一个候选都没命中，所以
//! 那个东西没画出来」，不是「第 2 个候选没命中」。以整条链为键，同一
//! 条指令无论试了几个候选都只占一行，而两条**不同**的指令（例如两个
//! 不同职业的挂件）仍然各报各的。

use std::collections::BTreeSet;

/// 候选链之间的分隔符——只出现在日志文本与去重键里，不参与任何查图。
const CANDIDATE_SEPARATOR: &str = " → ";

/// 一条绘制指令走完整条回退链之后的结局。
///
/// **能产出日志的只有 [`Self::MissedFirstTime`] 一个变体**，另外两个
/// 在类型上就没有可打的内容——「回退链中间未命中不打日志」因此是结构
/// 性的，不靠调用方自觉，也因此可以用普通的 `assert_eq!` 断言，不需要
/// 抓 `tracing` 的全局订阅者（那是进程级共享状态，在并行测试下本身就
/// 是一处不确定性）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainOutcome<'a> {
    /// 命中了候选链里第 `index` 个（`0` 最优先）。
    Hit {
        /// 命中的是第几个候选，`0` 表示最优先的那个。
        index: usize,
        /// 命中的那个图集键。
        key: &'a str,
    },
    /// 全部候选落空，且这组候选**第一次**落空——调用方应当打一条
    /// `WARN`，内容就是 `candidates`。
    MissedFirstTime {
        /// 整条候选链，已按优先级拼成一行可读文本。
        candidates: String,
    },
    /// 全部候选落空，但这组候选之前已经报过——**静默**。
    MissedAgain,
}

/// 已经报告过的候选链，按整条链去重。
///
/// 容器是 [`BTreeSet`] 而不是 `HashSet`：约束 C5 禁止逻辑依赖哈希容器
/// 的迭代顺序。本类型今天只做存在性判定、不迭代，但沿用仓库既有习惯
/// 能让「以后想按顺序把没画出来的东西列一遍」这件事不需要先换容器。
#[derive(Debug, Default)]
pub struct MissLedger {
    seen: BTreeSet<String>,
}

impl MissLedger {
    /// 建一个空账本。
    pub fn new() -> MissLedger {
        MissLedger::default()
    }

    /// 按优先级挨个**静默**探测 `keys`，返回结局；全部落空时顺带登记。
    ///
    /// `exists` 必须是静默的存在性判定——它的返回类型是 `bool`，拿不到
    /// 也不该拿到任何日志通道，见模块文档。
    ///
    /// 命中路径上**一个字符串都不构造**：只有真的全部落空时才把候选链
    /// 拼出来。这是「每帧格式化几十条字符串」那一重代价的落点。
    pub fn resolve<'a>(
        &mut self,
        keys: impl IntoIterator<Item = &'a str>,
        mut exists: impl FnMut(&str) -> bool,
    ) -> ChainOutcome<'a> {
        let mut tried: Vec<&'a str> = Vec::new();
        for (index, key) in keys.into_iter().enumerate() {
            if exists(key) {
                return ChainOutcome::Hit { index, key };
            }
            tried.push(key);
        }
        let candidates = tried.join(CANDIDATE_SEPARATOR);
        if self.seen.insert(candidates.clone()) {
            ChainOutcome::MissedFirstTime { candidates }
        } else {
            ChainOutcome::MissedAgain
        }
    }

    /// 至今登记过多少条**不同的**候选链，也就是至今为止本账本让调用方
    /// 打过多少行日志。测试与诊断用。
    pub fn reported(&self) -> usize {
        self.seen.len()
    }

    /// 一条 [`crate::surface_draw::SurfaceDraw`] 的完整查图结局：先看
    /// 压制，再走回退链。
    ///
    /// # 为什么压制判定也收在这里
    ///
    /// 它是本次缺陷的**主要**来源：压制判定问的是「这个键在不在」，
    /// 却调了会打日志的取用接口，而 `superseded_by` 装的正是今天一张
    /// 都不存在的合成图键——每个 NPC 每帧两行。把它和回退链收在同一
    /// 个函数里，「探测是静默的」这件事就只需要成立一次，而且可以被
    /// 一条不需要 GPU 的单元测试钉住（消费方
    /// `crate::app::push_surface_draw` 要一台真实设备，测不到）。
    pub fn resolve_draw<'a>(
        &mut self,
        superseded_by: impl IntoIterator<Item = &'a str>,
        keys: impl IntoIterator<Item = &'a str>,
        mut exists: impl FnMut(&str) -> bool,
    ) -> DrawResolution<'a> {
        // 压制：任何一个键在图集里查得到，这一层就整个不画。**静默**
        // ——「压制键不存在」是今天的常态（零张合成图），不是事件。
        if superseded_by.into_iter().any(&mut exists) {
            return DrawResolution::Superseded;
        }
        match self.resolve(keys, exists) {
            ChainOutcome::Hit { key, .. } => DrawResolution::Draw { key },
            ChainOutcome::MissedFirstTime { candidates } => DrawResolution::Missed {
                report: Some(candidates),
            },
            ChainOutcome::MissedAgain => DrawResolution::Missed { report: None },
        }
    }
}

/// 一条绘制指令查完图之后该怎么办。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawResolution<'a> {
    /// 被 [`crate::surface_draw::SurfaceDraw::superseded_by`] 压制，整条
    /// 指令不画。**不说话**。
    Superseded,
    /// 用这个键画。
    Draw {
        /// 命中的图集键。
        key: &'a str,
    },
    /// 整条候选链落空，这一层没画出来。
    Missed {
        /// `Some` 表示这组候选第一次落空、调用方应当打一条 `WARN`；
        /// `None` 表示之前已经报过，**静默**。
        report: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个只认识 `present` 里那些键的图集替身。
    fn atlas(present: &'static [&'static str]) -> impl FnMut(&str) -> bool {
        move |key: &str| present.contains(&key)
    }

    #[test]
    fn 回退链命中较后候选时一条记录都不留() {
        // 这正是所有者实机撞到的那个形状：前两级合成图不存在，退到第三
        // 级种族图命中。中间两次未命中是回退链的**正常工作方式**，不该
        // 产生任何日志。
        // Arrange
        let mut ledger = MissLedger::new();
        let keys = [
            "lostland_human_lostland_farmer_female",
            "lostland_human_lostland_farmer",
            "lostland:human",
        ];

        // Act
        let outcome = ledger.resolve(keys, atlas(&["lostland:human"]));

        // Assert
        assert_eq!(
            outcome,
            ChainOutcome::Hit {
                index: 2,
                key: "lostland:human",
            },
            "应当命中第三个候选"
        );
        assert_eq!(
            ledger.reported(),
            0,
            "回退链的中间步骤未命中不是事件，一条记录都不该留"
        );
    }

    #[test]
    fn 最优先的候选就命中时也不留记录() {
        // Arrange
        let mut ledger = MissLedger::new();

        // Act
        let outcome = ledger.resolve(["a", "b"], atlas(&["a"]));

        // Assert
        assert_eq!(outcome, ChainOutcome::Hit { index: 0, key: "a" });
        assert_eq!(ledger.reported(), 0);
    }

    #[test]
    fn 全部候选落空时恰好留下一条记录且不再重复() {
        // 「值得记一笔，但绝不能每帧每实体重复」——第一次要说话，之后
        // 无论问多少次都必须闭嘴，而账本恒为 1。
        // Arrange
        let mut ledger = MissLedger::new();
        let keys = ["lostland_human_lostland_farmer", "lostland:human"];

        // Act
        let first = ledger.resolve(keys, atlas(&[]));
        let second = ledger.resolve(keys, atlas(&[]));
        let third = ledger.resolve(keys, atlas(&[]));

        // Assert
        assert_eq!(
            first,
            ChainOutcome::MissedFirstTime {
                candidates: "lostland_human_lostland_farmer → lostland:human".to_string(),
            },
            "第一次落空必须留下一条可打的记录，不能静默"
        );
        assert_eq!(second, ChainOutcome::MissedAgain, "第二次必须静默");
        assert_eq!(third, ChainOutcome::MissedAgain, "第三次仍然静默");
        assert_eq!(
            ledger.reported(),
            1,
            "同一组候选无论落空多少次，都只占一行日志"
        );
    }

    #[test]
    fn 两组不同的候选各报各的() {
        // 去重的粒度是「整条候选链」，不是「反正落空过就再也不说话」
        // ——两个不同职业的挂件各自缺图是两件事。
        // Arrange
        let mut ledger = MissLedger::new();

        // Act
        let farmer = ledger.resolve(["lostland:farmer"], atlas(&[]));
        let fisher = ledger.resolve(["lostland:fisher"], atlas(&[]));

        // Assert
        assert!(matches!(farmer, ChainOutcome::MissedFirstTime { .. }));
        assert!(matches!(fisher, ChainOutcome::MissedFirstTime { .. }));
        assert_eq!(ledger.reported(), 2);
    }

    #[test]
    fn 空候选链落空一次并去重() {
        // 地面物品堆那类「恒定空 preferred_keys + 无兜底」的指令不该
        // 每帧刷屏，也不该 panic。
        // Arrange
        let mut ledger = MissLedger::new();
        let empty: [&str; 0] = [];

        // Act
        let first = ledger.resolve(empty, atlas(&[]));
        let second = ledger.resolve(empty, atlas(&[]));

        // Assert
        assert!(matches!(first, ChainOutcome::MissedFirstTime { .. }));
        assert_eq!(second, ChainOutcome::MissedAgain);
        assert_eq!(ledger.reported(), 1);
    }

    #[test]
    fn 压制判定不留任何记录() {
        // A3：`superseded_by` 装的是今天一张都不存在的合成图键。判定
        // 它们「在不在」是**探测**，不是取用——一条记录都不该留，否则
        // 就退回每个 NPC 每帧两行的那个缺陷。
        // Arrange
        let mut ledger = MissLedger::new();
        let superseded = [
            "lostland_human_lostland_farmer_female",
            "lostland_human_lostland_farmer",
        ];

        // Act：压制键全都不存在，身子键存在。
        let resolution =
            ledger.resolve_draw(superseded, ["lostland:human"], atlas(&["lostland:human"]));

        // Assert
        assert_eq!(
            resolution,
            DrawResolution::Draw {
                key: "lostland:human"
            }
        );
        assert_eq!(
            ledger.reported(),
            0,
            "压制判定是探测，两次未命中都不该留下任何可打的记录"
        );
    }

    #[test]
    fn 压制键真的存在时整条指令不画且不说话() {
        // Arrange
        let mut ledger = MissLedger::new();

        // Act
        let resolution = ledger.resolve_draw(
            ["lostland_human_lostland_farmer"],
            ["lostland:farmer"],
            atlas(&["lostland_human_lostland_farmer"]),
        );

        // Assert
        assert_eq!(resolution, DrawResolution::Superseded);
        assert_eq!(ledger.reported(), 0);
    }

    #[test]
    fn 可选层落空时画一百帧也只留一条记录() {
        // A4：职业挂件层——单个候选、没有兜底（`fallback_key: None`），
        // 「没有为某个职业准备挂件贴图」是 `SurfaceDraw::fallback_key`
        // 字段文档明写的**正常状态**。它此前每帧每 NPC 刷一行。
        // Arrange
        let mut ledger = MissLedger::new();
        let mut reports = 0usize;
        let empty: [&str; 0] = [];

        // Act：连画 100 帧。
        for _ in 0..100 {
            if let DrawResolution::Missed { report: Some(_) } =
                ledger.resolve_draw(empty, ["lostland:farmer"], atlas(&[]))
            {
                reports += 1;
            }
        }

        // Assert
        assert_eq!(reports, 1, "100 帧里只有第一帧该说话");
        assert_eq!(ledger.reported(), 1);
    }

    #[test]
    fn 命中之后不再探测后面的候选() {
        // 「按优先级取第一个查得到的」——命中即停，后面的候选一次都不
        // 该被问到（否则回退链的次序就没有意义了）。
        // Arrange
        let mut ledger = MissLedger::new();
        let mut asked: Vec<String> = Vec::new();

        // Act
        let outcome = ledger.resolve(["a", "b", "c"], |key| {
            asked.push(key.to_string());
            key == "b"
        });

        // Assert
        assert_eq!(outcome, ChainOutcome::Hit { index: 1, key: "b" });
        assert_eq!(asked, vec!["a".to_string(), "b".to_string()]);
    }
}
