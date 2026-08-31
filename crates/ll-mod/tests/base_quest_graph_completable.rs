//! 端到端：**本体任务图从起点到终点全程可达**——本体注册的每一条任务
//! 都必须能被**今天真实在跑的那个判定器**完成。
//!
//! # 这份测试为什么必须存在
//!
//! 2026-08-29 的文档—代码一致性审计（`knowledge/audit/2026-08-29-doc-code-audit.md`
//! 二节缺陷第 1 项）抓到一处任何单元测试都咬不住的缺陷：
//! `mods/lostland/quests.json5` 里 `lostland:branch_b` 用的是三档
//! `QuestCondition::Script` 条件，而**求值器不存在**——
//! `ll_sim::quest::QuestCatalog` 只交付 `KillCount`，
//! `ll_mod::quest::RegisteredQuests::kill_count_quests` 对任何非
//! `KillCount` 条件直接 `continue`。`lostland:finale` 以 `branch_b` 为
//! 前置，于是**本体任务链的终点永远解不开**。
//!
//! 当时仓库里有的测试是这几类，没有一类会红：
//!
//! - 注册/查询/无环校验的单元测试——它们验的是「这条数据能不能被存进
//!   表、能不能查出来」，`Script` 条件在这些维度上完全正常；
//! - `base_mod_class_skill_quest.rs` 逐字段钉住四条任务的内容——它把
//!   `branch_b` 是 `Script` 这件事**当成期望值钉住了**，缺陷因此被写进
//!   了基准；
//! - `ll-content` 的验收 demo 只走到 `main_quest_1` 完成为止，没有往下
//!   走到 `finale`。
//!
//! **缺的是「声明的条件今天求不求得出来」这一条**。本文件补的就是它。
//!
//! # 清单从注册表现查，不手抄任务 id
//!
//! 本文件里没有任何一处写着 `lostland:finale` 这类字面量。「本体一共有
//! 哪些任务」从 `Registry::snapshot()` 按命名空间过滤现查——这样以后往
//! `quests.json5` 里加任务，它自动进入这道门，不需要有人记得回来改测试。
//! 这是仓库反复付过代价的那条纪律（`atlas_coverage.rs` 的手写地形清单、
//! 交接文档的常量表：凡是把真相源之外的副本当判据，迟早分叉）。
//!
//! # 判据走真实判定器，不走「条件枚举长什么样」
//!
//! 本文件**不**写 `matches!(condition, QuestCondition::KillCount { .. })`
//! ——那是在复述一遍判定器的内部规则，判定器改了它不会跟着改。判据是
//! 直接调用 [`ll_sim::quest::QuestCatalog::kill_count_quests`]：**它返回
//! 得出的任务，就是运行期真能标记完成的任务**，一条不多一条不少。将来
//! 三档接上求值器、`QuestCatalog` 多出一个查询，本文件把那个查询的结果
//! 并进来即可，判据本身不变。
//!
//! # mod 不受这道门约束
//!
//! 只查 `lostland` 命名空间。mod 作者有权声明一条今天求值不出来的条件
//! （那是他自己的账，而且 `mods/example_mod/` 恰好也全是一档）；本体
//! 不行——本体是「装上就能从头玩到尾」的那一份。这条口径同样写在
//! `ll_mod::quest::QuestCondition::Script` 的变体文档里。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ll_core::ident::NamespacedId;
use ll_mod::load_report::LoadStatus;
use ll_mod::load_session::LoadSession;
use ll_mod::quest::{QuestTable, RegisteredQuests};
use ll_mod::registry::Registry;
use ll_sim::quest::QuestCatalog;

/// 仓库根目录下的真实 `mods/` 路径，理由同 `base_mod_races.rs`。
const REAL_MODS_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mods");

/// 本体内容 mod 的命名空间。
const BASE_NAMESPACE: &str = "lostland";

struct Loaded {
    registry: Registry,
    quest: QuestTable,
}

/// 装载**整个** `mods/` 目录（不是只挑 `mods/lostland/`），理由同
/// `base_mod_races.rs` 模块文档：生产装载路径装的就是整个目录，只装一半
/// 会让「本体任务的前置指向了另一个 mod 的任务」这类跨 mod 情形在测试里
/// 根本不出现。
fn load_real_mods() -> Loaded {
    let mut session = LoadSession::with_engine_registrations();
    let report = session.load_all(Path::new(REAL_MODS_ROOT));
    let LoadSession {
        registry, quest, ..
    } = session;

    let base_id = NamespacedId::parse("lostland:self").expect("合法标识符");
    let status = report
        .entries
        .iter()
        .find(|(id, _)| *id == base_id)
        .map(|(_, status)| status);
    assert_eq!(
        status,
        Some(&LoadStatus::Loaded),
        "本体内容 mod（mods/lostland/）必须成功加载，否则下面的断言毫无意义"
    );

    Loaded { registry, quest }
}

/// 本体命名空间下已注册的全部任务节点标识符——**现查，不手抄**。
fn base_quest_ids(loaded: &Loaded) -> BTreeSet<NamespacedId> {
    loaded
        .registry
        .snapshot()
        .into_iter()
        .filter(|id| id.namespace() == BASE_NAMESPACE)
        .filter(|id| {
            loaded
                .registry
                .get(id)
                .is_some_and(|index| loaded.quest.is_defined(index))
        })
        .collect()
}

/// 今天的判定器**真能标记完成**的任务，连同它们各自的前置——直接取自
/// [`QuestCatalog`]，不复述它的内部规则，见模块文档「判据走真实判定器」。
fn completable_with_prerequisites(loaded: &Loaded) -> BTreeMap<NamespacedId, Vec<NamespacedId>> {
    let catalog = RegisteredQuests {
        table: &loaded.quest,
        registry: &loaded.registry,
    };
    catalog
        .kill_count_quests()
        .into_iter()
        .map(|rule| (rule.quest, rule.prerequisites))
        .collect()
}

/// 从「无前置的起点」出发，反复把「自己可完成、且全部前置都已可达」的
/// 节点收进来，直到不再增长——返回真正能被走到的那些任务。
///
/// 不在这里查环：无环由 `ll_mod::quest::validate_no_cycles` 在装载期
/// 保证（`quests.json5` 文件头「环检查」一节）。本函数即使遇上环也只会
/// 停下来不收它，不会死循环——环上的节点永远等不到自己的前置。
fn reachable(completable: &BTreeMap<NamespacedId, Vec<NamespacedId>>) -> BTreeSet<NamespacedId> {
    let mut reached: BTreeSet<NamespacedId> = BTreeSet::new();
    loop {
        let mut grew = false;
        for (quest, prerequisites) in completable {
            if reached.contains(quest) {
                continue;
            }
            if prerequisites.iter().all(|prior| reached.contains(prior)) {
                reached.insert(quest.clone());
                grew = true;
            }
        }
        if !grew {
            return reached;
        }
    }
}

#[test]
fn 本体任务图从起点到终点全程可达() {
    // Arrange
    let loaded = load_real_mods();
    let base = base_quest_ids(&loaded);
    // 先钉住「清单真的查到了东西」：现查清单若因为过滤条件写错而变成
    // 空集，下面那条全称断言会**空真**地通过，这道门就静默失效了。
    assert!(
        !base.is_empty(),
        "本体命名空间下一条任务都没查到——现查清单坏了，下面的断言会空真通过"
    );

    // Act
    let completable = completable_with_prerequisites(&loaded);
    let reached = reachable(&completable);

    // Assert
    let unreachable: Vec<String> = base
        .iter()
        .filter(|id| !reached.contains(*id))
        .map(|id| {
            let cause = match completable.get(id) {
                None => "它自己的完成条件今天没有求值器".to_string(),
                Some(prerequisites) => {
                    let blocked: Vec<String> = prerequisites
                        .iter()
                        .filter(|prior| !reached.contains(*prior))
                        .map(NamespacedId::to_string)
                        .collect();
                    format!("它的前置走不到：{}", blocked.join("、"))
                }
            };
            format!("{id}（{cause}）")
        })
        .collect();
    assert!(
        unreachable.is_empty(),
        "本体任务图有走不到的节点，本体任务链因此是断的：{}\n\
         —— 任何一条本体任务的完成条件都必须是今天真的求值得出来的那一档\
         （见 ll_mod::quest::QuestCondition::Script 变体文档「落地条件」\
         一节：三档接上求值器之前，本体内容不得使用它）。",
        unreachable.join("；")
    );
}

#[test]
fn 本体任务图确实是网状的_存在一个有多个前置的汇聚节点() {
    // 上一条断言「全程可达」。可达性单靠一条线性任务链也能满足——这条
    // 补另一头：图的形状仍然是 quests.json5 文件头画的那张网。两条一起
    // 才咬得住「把 finale 的前置删掉一个」这种「修好了可达性但毁掉了
    // 验收标的」的改法。
    // Arrange
    let loaded = load_real_mods();
    let base = base_quest_ids(&loaded);
    let completable = completable_with_prerequisites(&loaded);

    // Act
    let converging = base
        .iter()
        .filter_map(|id| completable.get(id))
        .filter(|prerequisites| prerequisites.len() >= 2)
        .count();

    // Assert
    assert!(
        converging >= 1,
        "本体任务图里没有任何一个节点有两个以上前置——「一个任务可以有\
         多个前置」这条验收标的（quests.json5 文件头「图的形状是刻意的」）\
         已经丢了，这张图退化成了树"
    );
}
