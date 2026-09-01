//! `mods/<id>/dialogues.json5` 的 **schema 解析**验收：十条谓词、四种
//! 后果、跨表引用与「不该有的参数必须没有」。
//!
//! # 为什么从 `content_schema_dialogue.rs` 搬出来
//!
//! 〔2026-08-31，批次 29〕批次 4 的两条后果（`complete-quest` /
//! `give-item`）落地后，那个文件的代码行冲到 806，越过了规格 §13 的 800
//! 上限。门禁的原话是「**按职责拆开**它（不是按行数切）」——本文件就是
//! 那一刀：`content_schema_dialogue.rs` 只留 schema 与解析实现，「这份
//! schema 认不认这段 JSON5」的验收整体搬到这里。批次 26 拆
//! `crates/ll-sim/tests/affiliation_apply.rs` 时用的是同一条纪律。
//!
//! **搬家不改一条断言**（除了把 `use super::*` 换成公开路径、以及本地
//! 重写一份 `Applied` 类型别名）。这些用例本来就只用公开入口
//! （`apply_dialogues` / `DialogueFile` / 两张表），因此从单元测试变成
//! 集成测试是零损失的。

use ll_core::ident::NamespacedId;
use ll_mod::content_schema_dialogue::{DialogueFile, apply_dialogues, is_end, next_target};
use ll_mod::dialogue::{DialogueNodeTable, DialogueTable};
use ll_mod::registry::Registry;
use ll_sim::dialogue::DialogueOutcome;

/// `apply_dialogues` 的返回类型（`Result<(), String>`）——原来在
/// `content_schema_dialogue.rs` 里叫 `Applied`，那是个模块私有别名，
/// 搬出来之后本文件自己写一份同形状的。
type Applied = Result<(), String>;

/// 装一份最小的前置内容（职业/文化/任务/物品/种族各一条），再解析一份
/// 对话文件。前置全部 `intern` 进去，因为条件里的引用走 `required_id`。
fn 解析(source: &str) -> (Registry, DialogueTable, DialogueNodeTable, Applied) {
    let mut registry = Registry::new();
    for raw in [
        "lostland:steward",
        "lostland:mining_hold",
        "lostland:main_quest_1",
        "lostland:iron_ingot",
        "lostland:dwarf",
    ] {
        registry.intern(NamespacedId::parse(raw).expect("固定字面量恒合法"));
    }
    let mut dialogues = DialogueTable::new();
    let mut nodes = DialogueNodeTable::new();
    let result = match json5::from_str::<DialogueFile>(source) {
        Ok(file) => apply_dialogues(&mut registry, &mut dialogues, &mut nodes, &file),
        Err(err) => Err(err.to_string()),
    };
    (registry, dialogues, nodes, result)
}

#[test]
fn 一段带回环的对话解析成功且引用校验通过() {
    // Arrange & Act
    let (_registry, dialogues, nodes, result) = 解析(
        r#"{
          dialogues: [ { id: "lostland:greet",
                         speaker: { profession: "lostland:steward" },
                         root: "lostland:root" } ],
          nodes: [
            { id: "lostland:root", text_key: "lostland:dialogue.root",
              options: [ { text_key: "lostland:dialogue.more", next: "lostland:more" },
                         { text_key: "lostland:dialogue.bye", next: "end" } ] },
            { id: "lostland:more", text_key: "lostland:dialogue.more_text",
              options: [ { text_key: "lostland:dialogue.back", next: "lostland:root" } ] },
          ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    assert_eq!(
        ll_mod::dialogue::validate_references(&dialogues, &nodes),
        Ok(())
    );
}

#[test]
fn 前向引用的节点合法因为next走intern() {
    // `root` 与第一条 `next` 都指向写在后面的节点。
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [
            { id: "lostland:a", text_key: "lostland:dialogue.a",
              options: [ { text_key: "lostland:dialogue.go", next: "lostland:b" } ] },
            { id: "lostland:b", text_key: "lostland:dialogue.b" },
          ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn 说话人的职业拼错在装载期当场报错() {
    // Arrange & Act：只 get 不 intern，拼错就是拼错。
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          dialogues: [ { id: "lostland:greet",
                         speaker: { profession: "lostland:stewrad" },
                         root: "lostland:root" } ],
        }"#,
    );

    // Assert
    assert!(
        result.is_err_and(|err| err.contains("lostland:stewrad") && err.contains("尚未注册")),
        "错误必须点名拼错的那个 id"
    );
}

#[test]
fn 未知的条件kind报错并列出全部认得的kind() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  conditions: [ { kind: "mood-good" } ] } ] } ],
        }"#,
    );

    // Assert
    assert!(
        result.is_err_and(|err| err.contains("mood-good") && err.contains("wallet-at-least")),
        "未知 kind 必须连同封闭清单一起报出来"
    );
}

#[test]
fn 条件带上不属于它的参数当场报错() {
    // 这条守的是「不该有的必须没有」那一半：wallet-at-least 只认
    // value，写了 count 说明作者以为自己写的是 has-item。
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  conditions: [ { kind: "wallet-at-least", value: 10,
                                                  count: 3 } ] } ] } ],
        }"#,
    );

    // Assert
    assert!(
        result.is_err_and(|err| err.contains("不接受字段") && err.contains("count")),
        "多余参数必须点名"
    );
}

#[test]
fn 缺必填参数的条件报错并点名字段() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  conditions: [ { kind: "has-item",
                                                  item: "lostland:iron_ingot" } ] } ] } ],
        }"#,
    );

    // Assert
    assert!(
        result.is_err_and(|err| err.contains("缺少必填字段") && err.contains("count")),
        "缺参数必须点名字段"
    );
}

#[test]
fn has_item的数量为零当场报错() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  conditions: [ { kind: "has-item",
                                                  item: "lostland:iron_ingot",
                                                  count: 0 } ] } ] } ],
        }"#,
    );

    // Assert
    assert!(result.is_err_and(|err| err.contains("至少是 1")));
}

#[test]
fn 未知的归属类别报错并列出五类() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  conditions: [ { kind: "affiliated",
                                                  affiliation: "profession" } ] } ] } ],
        }"#,
    );

    // Assert
    assert!(
        result.is_err_and(|err| err.contains("profession") && err.contains("family")),
        "归属类别是封闭的五类"
    );
}

#[test]
fn 未知字段报错且错误信息带行列位置() {
    // 与 content_schema 的同名钉子同一条理由：deny_unknown_fields 的
    // 错误必须能定位到行列，否则内容作者只知道「某处拼错了」。
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     outcomes: [] } ],
        }"#,
    );

    // Assert：`outcomes` 是批次 2 才会有的字段，今天写它必须当场红。
    let err = result.expect_err("未知字段必须报错");
    assert!(err.contains("outcomes"), "错误里没点名字段：{err}");
    assert!(err.contains("line"), "错误里没有行列位置：{err}");
}

#[test]
fn 十条kind全部能解析出对应的条件() {
    // 这条把封闭清单的十条一次跑遍——新增一条 kind 而忘了在这里加
    // 一行，覆盖率不会掉，但下面的条数断言会红。
    // Arrange & Act
    let (_registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a", options: [
            { text_key: "lostland:dialogue.x", next: "end", conditions: [
              { kind: "affiliated", affiliation: "faction" },
              { kind: "not-affiliated", affiliation: "culture",
                org: "lostland:mining_hold" },
              { kind: "standing-at-least", affiliation: "faction", value: 250 },
              { kind: "quest-completed", quest: "lostland:main_quest_1" },
              { kind: "quest-not-completed", quest: "lostland:main_quest_1" },
              { kind: "flag-set", flag: "lostland:dialogue_flag.seen" },
              { kind: "flag-not-set", flag: "lostland:dialogue_flag.seen" },
              { kind: "has-item", item: "lostland:iron_ingot", count: 2 },
              { kind: "wallet-at-least", value: 50000 },
              { kind: "is-race", race: "lostland:dwarf" },
            ] } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let index = nodes.defined_indices()[0];
    let view = nodes.get(index).expect("刚定义过");
    assert_eq!(view.options[0].conditions.len(), 10, "封闭清单是十条");
}

#[test]
fn 保留字end解析成结束会话() {
    // Arrange & Act
    let (_registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end" } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let index = nodes.defined_indices()[0];
    let view = nodes.get(index).expect("刚定义过");
    assert!(is_end(view.options[0].next));
    assert_eq!(next_target(view.options[0].next), None);
}

#[test]
fn set_flag后果解析成一条对话标志() {
    // Arrange & Act
    let (_registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "set-flag",
                                                flag: "lostland:dialogue_flag.seen" } ] } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let index = nodes.defined_indices()[0];
    let view = nodes.get(index).expect("刚定义过");
    assert_eq!(
        view.options[0].outcomes,
        vec![DialogueOutcome::SetFlag(
            NamespacedId::parse("lostland:dialogue_flag.seen").expect("固定字面量恒合法")
        )],
    );
}

#[test]
fn join_settlement后果解析成不带参数的变体() {
    // 加入哪座据点由**说话人**回答（他的 `Agent::home`），内容文件
    // 里写不出 `WorldId`——因此这条后果一个参数都没有，见
    // `DialogueOutcome::JoinSettlement` 文档。
    //
    // 反例验证（ADR 0022）：把 `resolve` 里 `"join-settlement"` 那一支
    // 改回并进「尚未实现」那一支，本条当场红。
    // Arrange & Act
    let (_registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "join-settlement" } ] } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let index = nodes.defined_indices()[0];
    let view = nodes.get(index).expect("刚定义过");
    assert_eq!(
        view.options[0].outcomes,
        vec![DialogueOutcome::JoinSettlement]
    );
}

#[test]
fn join_settlement带多余的flag参数报错() {
    // 「不该有的必须没有」——静默接受会让作者以为那个 `flag` 起了
    // 作用，与 `RawDialogueCondition` 拒绝多余参数同一条纪律。
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "join-settlement",
                                                flag: "lostland:dialogue_flag.x" } ] } ] } ],
        }"#,
    );

    // Assert
    let err = result.expect_err("多余参数必须报错");
    assert!(
        err.contains("join-settlement"),
        "错误信息要点名是哪一种：{err}"
    );
    assert!(err.contains("flag"), "错误信息要点名是哪个参数：{err}");
}

#[test]
fn 不写outcomes的选项是纯导航选项() {
    // 老 mod 不写这个字段照样装得进来（`#[serde(default)]`），且解析
    // 出来是空数组而不是「一条什么都不做的后果」。
    // Arrange & Act
    let (_registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end" } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let index = nodes.defined_indices()[0];
    assert!(
        nodes.get(index).expect("刚定义过").options[0]
            .outcomes
            .is_empty()
    );
}

#[test]
fn 尚未实现的后果报明确错误而不是静默接受() {
    // 〔2026-08-31，批次 26〕`join-settlement` 已经实现，从这份清单里
    // 挪走了——它现在由 `join_settlement后果解析成不带参数的变体`
    // 与 `join_settlement带多余的flag参数报错` 两条守着。
    // 〔2026-08-31，批次 29〕`complete-quest` 与 `give-item` 同样挪走了，
    // 由 `complete_quest后果解析成一条任务引用`、
    // `give_item后果解析成一条物品引用` 等几条守着。**清单里只剩
    // `open-trade` 一种**，因此这里不再是一个循环（`clippy` 会把只有一个
    // 元素的 `for` 判成 `single_element_loop`）；批次 5 落地那天这条测试
    // 整条删掉，而不是把最后一个元素也拿走留一个空循环。
    let kind = "open-trade";

    // Arrange & Act
    let source = format!(
        r#"{{
          nodes: [ {{ id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ {{ text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ {{ kind: "{kind}" }} ] }} ] }} ],
        }}"#
    );
    let (_registry, _dialogues, _nodes, result) = 解析(&source);

    // Assert
    let err = result.expect_err("尚未实现的后果必须报错");
    assert!(err.contains("尚未实现"), "错误信息要说清楚为什么：{err}");
    assert!(err.contains(kind), "错误信息要点名是哪一种：{err}");
}

#[test]
fn complete_quest后果解析成一条任务引用() {
    // 反例验证（ADR 0022）：把 `resolve` 里 `"complete-quest"` 那一支
    // 挪回「尚未实现」那一支，本条当场红。
    // Arrange & Act
    let (registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "complete-quest",
                                                quest: "lostland:main_quest_1" } ] } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let quest = registry
        .get(&NamespacedId::parse("lostland:main_quest_1").expect("固定字面量恒合法"))
        .expect("夹具里已经 intern 过");
    let index = nodes.defined_indices()[0];
    let view = nodes.get(index).expect("刚定义过");
    assert_eq!(
        view.options[0].outcomes,
        vec![DialogueOutcome::CompleteQuest(quest)]
    );
}

/// 拼错的任务 id **在装载期**当场报错，不是等到运行期静默无效——
/// `required_id`（只 get 不 intern）的既有纪律。
#[test]
fn complete_quest的任务id没注册过时装载期报错() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "complete-quest",
                                                quest: "lostland:no_such_quest" } ] } ] } ],
        }"#,
    );

    // Assert
    let err = result.expect_err("没注册过的任务 id 必须报错");
    assert!(
        err.contains("lostland:no_such_quest"),
        "错误信息要点名是哪一条：{err}"
    );
}

#[test]
fn complete_quest缺少quest参数报错() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "complete-quest" } ] } ] } ],
        }"#,
    );

    // Assert
    let err = result.expect_err("缺必填参数必须报错");
    assert!(err.contains("complete-quest"), "要点名是哪一种：{err}");
    assert!(err.contains("quest"), "要点名是哪个参数：{err}");
}

/// 「不该有的必须没有」这一半对**两个方向**都成立：`complete-quest`
/// 不认 `flag`，`set-flag` 也不认 `quest`。
#[test]
fn 后果参数用错了种类当场报错() {
    for (kind, extra) in [
        ("complete-quest", r#"flag: "lostland:dialogue_flag.x""#),
        ("set-flag", r#"quest: "lostland:main_quest_1""#),
        ("give-item", r#"quest: "lostland:main_quest_1""#),
        ("complete-quest", r#"item: "lostland:iron_ingot""#),
    ] {
        // Arrange & Act
        let source = format!(
            r#"{{
              nodes: [ {{ id: "lostland:a", text_key: "lostland:dialogue.a",
                         options: [ {{ text_key: "lostland:dialogue.x", next: "end",
                                      outcomes: [ {{ kind: "{kind}", {extra} }} ] }} ] }} ],
            }}"#
        );
        let (_registry, _dialogues, _nodes, result) = 解析(&source);

        // Assert
        let err = result.expect_err("多余参数必须报错");
        assert!(err.contains(kind), "要点名是哪一种：{err}");
        assert!(err.contains("不接受字段"), "要说清楚为什么：{err}");
    }
}

/// `give-item` 携带的是一条**有内容表**的物品引用。
#[test]
fn give_item后果解析成一条物品引用() {
    // 反例验证（ADR 0022）：把 `resolve` 里 `"give-item"` 那一支挪回
    // 「尚未实现」那一支，本条当场红。
    // Arrange & Act
    let (registry, _dialogues, nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "give-item",
                                                item: "lostland:iron_ingot" } ] } ] } ],
        }"#,
    );

    // Assert
    assert_eq!(result, Ok(()));
    let item = registry
        .get(&NamespacedId::parse("lostland:iron_ingot").expect("固定字面量恒合法"))
        .expect("夹具里已经 intern 过");
    let index = nodes.defined_indices()[0];
    let view = nodes.get(index).expect("刚定义过");
    assert_eq!(
        view.options[0].outcomes,
        vec![DialogueOutcome::GiveItem(item)]
    );
}

/// `give-item` **不认 `count`**：schema 里压根没有这个字段，写了会被
/// `deny_unknown_fields` 当场拒掉。这一条钉住的是「一次一件」这个
/// 刻意的收窄，见 `DialogueOutcome::GiveItem` 文档。
#[test]
fn give_item写count当场报错而不是被静默忽略() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "give-item",
                                                item: "lostland:iron_ingot",
                                                count: 3 } ] } ] } ],
        }"#,
    );

    // Assert
    let err = result.expect_err("未知字段必须报错");
    assert!(err.contains("count"), "错误里没点名字段：{err}");
}

#[test]
fn give_item缺少item参数报错() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "give-item" } ] } ] } ],
        }"#,
    );

    // Assert
    let err = result.expect_err("缺必填参数必须报错");
    assert!(err.contains("give-item"), "要点名是哪一种：{err}");
    assert!(err.contains("item"), "要点名是哪个参数：{err}");
}

#[test]
fn set_flag缺少flag参数报错() {
    // Arrange & Act
    let (_registry, _dialogues, _nodes, result) = 解析(
        r#"{
          nodes: [ { id: "lostland:a", text_key: "lostland:dialogue.a",
                     options: [ { text_key: "lostland:dialogue.x", next: "end",
                                  outcomes: [ { kind: "set-flag" } ] } ] } ],
        }"#,
    );

    // Assert
    let err = result.expect_err("缺必填参数必须报错");
    assert!(err.contains("flag"), "错误信息要点名缺的是哪个字段：{err}");
}
