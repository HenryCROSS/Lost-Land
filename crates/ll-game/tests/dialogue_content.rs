//! 对话内容表的**端到端**验收：走生产装载路径
//! （`ll_game::content::load_content` + `ll_game::locale_sources` →
//! `ll_i18n::Catalog::load`），读仓库里真实的 `mods/` 与 `assets/locales/`，
//! 不用任何临时夹具。
//!
//! 本文件咬住计划文档
//! `docs/superpowers/plans/2026-08-31-batch18-dialogue-content.md` 里那几条
//! 「本批必须能证明真的成立」的能力：
//!
//! | 能力 | 本文件里对应的断言 |
//! |---|---|
//! | 本体与 mod 的对话走同一条路装进同两张表 | `本体与示例模组的对话都装进了同两张表` |
//! | **对话图允许有环** | `本体内容里真的存在回环` |
//! | 分支选项与条件谓词真的有内容在用 | `十条谓词在真实内容里全部有用例` |
//! | 说话人按职业匹配、文化收窄优先 | `矿堡卫兵匹配到按文化收窄的那一段` |
//! | 文案一个字都不在 JSON5 里，且**每条键中英文都有** | `每一条对话文案键在中英文下都有精确译文` |
//! | **示例 mod 的文案走它自己的命名空间** | `示例模组的对话文案来自它自己的 ftl`、`对话里故意撞键的两条互不覆盖` |
//!
//! 反例验证（ADR 0022）见计划文档 3.3 与 5.3 两节，两条都实测过。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ll_core::ident::NamespacedId;
use ll_game::content::{BASE_NAMESPACE, LoadedContent, load_content};
use ll_game::{GamePaths, locale_sources};
use ll_i18n::Catalog;
use ll_mod::dialogue::{DialogueCondition, DialogueNext};

/// 仓库根——`ll-game` 位于 `crates/ll-game`，向上两级。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// 按生产路径装一次真实内容。
fn 真实内容() -> LoadedContent {
    let root = repo_root();
    load_content(&root.join("mods"), &root.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功")
}

/// 按生产路径装出 `Catalog`：本体 + 全部带 `locales/` 的 mod。
fn 真实文案() -> Catalog {
    let paths = GamePaths::under(&repo_root());
    Catalog::load(BASE_NAMESPACE, &locale_sources(&paths))
}

fn id(raw: &str) -> NamespacedId {
    NamespacedId::parse(raw).expect("固定字面量标识符恒合法")
}

#[test]
fn 本体与示例模组的对话都装进了同两张表() {
    // 「本体即 Mod」在对话上的检验：两边的声明除了命名空间字符串之外没有
    // 任何结构性差异，因此装完之后落在同一张表里、用同一种方式查得到。
    // Arrange
    let loaded = 真实内容();

    // Act
    let 本体 = loaded
        .registry
        .get(&id("lostland:steward_greeting"))
        .expect("本体的管理者对话必须注册");
    let 模组 = loaded
        .registry
        .get(&id("examplemod:necromancer_greeting"))
        .expect("示例模组的死灵法师对话必须注册");

    // Assert
    assert!(loaded.dialogue_table.get(本体).is_some());
    assert!(loaded.dialogue_table.get(模组).is_some());
    assert!(
        loaded.dialogue_table.defined_indices().len() >= 3,
        "本体两段 + 示例模组一段"
    );
}

#[test]
fn 本体内容里真的存在回环() {
    // 这是「对话图允许有环、不能复用无环校验」那条裁定在真实内容里的钉子。
    // 反例验证（计划文档 3.3）：给节点图加上一份真的无环校验，本体这段内容
    // 必须让装载失败——实测过。
    // Arrange
    let loaded = 真实内容();
    let root = loaded
        .registry
        .get(&id("lostland:steward_root"))
        .expect("管理者的开场节点必须注册");
    let join = loaded
        .registry
        .get(&id("lostland:steward_join"))
        .expect("落脚那一节必须注册");

    // Act：root 能走到 join，join 又能走回 root。
    let root_到_join = loaded
        .dialogue_node_table
        .get(root)
        .expect("已注册")
        .options
        .iter()
        .any(|option| option.next == DialogueNext::Node(join));
    let join_回_root = loaded
        .dialogue_node_table
        .get(join)
        .expect("已注册")
        .options
        .iter()
        .any(|option| option.next == DialogueNext::Node(root));

    // Assert
    assert!(root_到_join && join_回_root, "本体内容里必须真的有一处回环");
}

#[test]
fn 每一段会话的起始节点与每一条跳转都指向真的存在的节点() {
    // 装载期已经校验过（`load_content` 里的 `validate_references`，不通过
    // 就根本返回不了 `LoadedContent`），这里再断言一次是为了让「校验真的在
    // 生产路径上跑」这件事有一条独立于实现的证据：把那次调用注释掉，本条
    // 仍然会红。
    // Arrange
    let loaded = 真实内容();

    // Act & Assert
    for dialogue in loaded.dialogue_table.defined_indices() {
        let view = loaded.dialogue_table.get(dialogue).expect("已注册");
        assert!(
            loaded.dialogue_node_table.is_defined(view.root),
            "会话 {dialogue:?} 的起始节点没有定义"
        );
    }
    for node in loaded.dialogue_node_table.defined_indices() {
        let view = loaded.dialogue_node_table.get(node).expect("已注册");
        for option in view.options {
            if let DialogueNext::Node(target) = option.next {
                assert!(
                    loaded.dialogue_node_table.is_defined(target),
                    "节点 {node:?} 的一条跳转指向未定义的 {target:?}"
                );
            }
        }
    }
}

#[test]
fn 十条谓词在真实内容里全部有用例() {
    // 设计文档四节 4.3 的硬规则：**新增谓词必须同批带一条真实内容用例**。
    // 这条把那条规则从劝告变成机器检查——加一条谓词而不给它写台词，下面
    // 那个 `assert_eq!(10)` 会红。
    // Arrange
    let loaded = 真实内容();

    // Act：把全部节点里出现过的谓词种类收成一个集合。
    let mut kinds: BTreeSet<&'static str> = BTreeSet::new();
    for node in loaded.dialogue_node_table.defined_indices() {
        let view = loaded.dialogue_node_table.get(node).expect("已注册");
        for option in view.options {
            for condition in &option.conditions {
                kinds.insert(match condition {
                    DialogueCondition::Affiliated(_) => "affiliated",
                    DialogueCondition::NotAffiliated(_) => "not-affiliated",
                    DialogueCondition::StandingAtLeast { .. } => "standing-at-least",
                    DialogueCondition::QuestCompleted(_) => "quest-completed",
                    DialogueCondition::QuestNotCompleted(_) => "quest-not-completed",
                    DialogueCondition::FlagSet(_) => "flag-set",
                    DialogueCondition::FlagNotSet(_) => "flag-not-set",
                    DialogueCondition::HasItem { .. } => "has-item",
                    DialogueCondition::WalletAtLeast(_) => "wallet-at-least",
                    DialogueCondition::IsRace(_) => "is-race",
                });
            }
        }
    }

    // Assert
    assert_eq!(
        kinds.len(),
        10,
        "封闭清单十条，真实内容里出现的是 {kinds:?}"
    );
}

#[test]
fn 带org参数的归属条件在真实内容里有用例() {
    // `org` 是那条**可选**参数，最容易变成「有实现没内容」的一个。
    // Arrange
    let loaded = 真实内容();

    // Act
    let 有带org的条件 = loaded
        .dialogue_node_table
        .defined_indices()
        .into_iter()
        .flat_map(|node| {
            loaded
                .dialogue_node_table
                .get(node)
                .expect("已注册")
                .options
                .to_vec()
        })
        .flat_map(|option| option.conditions)
        .any(|condition| match condition {
            DialogueCondition::Affiliated(query) | DialogueCondition::NotAffiliated(query) => {
                query.org.is_some()
            }
            DialogueCondition::StandingAtLeast { query, .. } => query.org.is_some(),
            _ => false,
        });

    // Assert
    assert!(有带org的条件, "可选的 org 参数必须有一条真实内容用例");
}

#[test]
fn 矿堡卫兵匹配到按文化收窄的那一段() {
    // 设计文档 1.3 的裁决顺序：声明了 culture 的胜过只声明 profession 的。
    // Arrange
    let loaded = 真实内容();
    let guard = loaded
        .registry
        .get(&id("lostland:guard"))
        .expect("本体职业「卫兵」必须注册");
    let mining = loaded
        .registry
        .get(&id("lostland:mining_hold"))
        .expect("本体文化「矿堡」必须注册");
    let expected = loaded
        .registry
        .get(&id("lostland:mining_guard_greeting"))
        .expect("矿堡卫兵对话必须注册");

    // Act & Assert
    assert_eq!(
        loaded.dialogue_table.match_speaker(guard, Some(mining)),
        Some(expected)
    );
    // 一个没有文化归属的卫兵匹配不到这一段——本体今天没有第二段卫兵对话，
    // 因此它只能是 None。这条同时说明「按文化收窄」不是摆设。
    assert_eq!(loaded.dialogue_table.match_speaker(guard, None), None);
}

#[test]
fn 每一条对话文案键在中英文下都有精确译文() {
    // 设计文档三节 3.5 建议的覆盖率检查的**测试版**（门禁版仍未做）。
    // `try_resolve` 精确查找、不走语言回退链——否则「只缺了中文」会被一句
    // 英文糊过去（批次 0 的 2.4 定的那条回退链）。
    // Arrange
    let loaded = 真实内容();
    let catalog = 真实文案();

    // Act：把两张表里出现的全部 text_key 收齐。
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for node in loaded.dialogue_node_table.defined_indices() {
        let view = loaded.dialogue_node_table.get(node).expect("已注册");
        keys.insert(view.text_key.to_string());
        for option in view.options {
            keys.insert(option.text_key.to_string());
        }
    }

    // Assert
    assert!(keys.len() >= 30, "真实内容的文案键太少了：{}", keys.len());
    for key in &keys {
        for language in ["zh-CN", "en"] {
            assert!(
                catalog.try_resolve(language, key).is_some(),
                "对话文案键 {key} 在 {language} 下没有译文"
            );
        }
    }
}

#[test]
fn 示例模组的对话文案来自它自己的ftl() {
    // 「本体即 Mod」在对话文案上的检验：示例模组的台词写在
    // mods/example_mod/locales/ 里，本体的 assets/locales/ 一个字都没有它。
    // Arrange
    let catalog = 真实文案();
    let key = "examplemod:dialogue.necromancer.root";

    // Act
    let zh = catalog.resolve("zh-CN", key);
    let en = catalog.resolve("en", key);

    // Assert
    assert_ne!(zh, key, "退化成键名了——它自己的 ftl 没被读到");
    assert_ne!(zh, en);
    assert!(zh.contains("死灵法师"));
    assert!(en.contains("necromancer"));
}

#[test]
fn 对话里故意撞键的两条互不覆盖() {
    // 缺口 ②（批次 0 三节 3.2）在**对话**上的复现：
    // `lostland:dialogue.common.farewell` 与
    // `examplemod:dialogue.common.farewell` 折出同一个 Fluent id
    // （`dialogue-common-farewell`），而 mod 恒在本体之后装载。
    //
    // 反例验证（计划文档 5.3）：把 `ll_i18n::split_key` 的命名空间分流改回
    // 「剥掉前缀」，本条与另外两条一起变红——实测过。**实测到的坍缩方向
    // 是本体赢**（示例模组那一句永远查不到），与批次 0 的
    // `race-elf-display_name` 那次实测（mod 赢）方向相反：谁赢取决于
    // `FluentBundle::add_resource` 撞上重复 id 时那份文件是整份被跳过还是
    // 只跳过冲突条目。**方向不重要，重要的是两条键坍缩成了一条**——那正是
    // 命名空间维度要消灭的东西。
    // Arrange
    let catalog = 真实文案();

    // Act
    let 本体告辞 = catalog
        .try_resolve("zh-CN", "lostland:dialogue.common.farewell")
        .expect("本体的告辞必须有译文");
    let 模组告辞 = catalog
        .try_resolve("zh-CN", "examplemod:dialogue.common.farewell")
        .expect("示例模组的告辞必须有译文");

    // Assert
    assert_ne!(本体告辞, 模组告辞, "两个命名空间的同名键互相覆盖了");
    assert_eq!(本体告辞, "（告辞）");
    assert_eq!(模组告辞, "（拂袖而去）");
}

#[test]
fn 对话内容表进了内容值哈希() {
    // 新增一类内容却忘了接进 `classify_index`，那类内容的全部条目会被判成
    // `ContentTableKind::Opaque`（只混 id、不混字段值）。`ll_game::content`
    // 的覆盖率回归测试守的是「Opaque 集合恰好等于已知例外」那一头，这里
    // 从正面再钉一次：对话的 id 必须被判成对话表。
    // Arrange
    let loaded = 真实内容();
    let tables = loaded.value_tables();

    // Act & Assert
    for (raw, expected) in [
        (
            "lostland:steward_greeting",
            ll_mod::content_hash::ContentTableKind::Dialogue,
        ),
        (
            "lostland:steward_root",
            ll_mod::content_hash::ContentTableKind::DialogueNode,
        ),
        (
            "examplemod:necromancer_greeting",
            ll_mod::content_hash::ContentTableKind::Dialogue,
        ),
    ] {
        let index = loaded.registry.get(&id(raw)).expect("必须注册");
        assert_eq!(
            ll_mod::content_hash::classify_index(index, &tables),
            expected,
            "{raw} 被判成了别的表"
        );
    }
}
