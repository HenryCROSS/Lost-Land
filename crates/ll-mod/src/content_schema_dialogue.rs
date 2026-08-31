//! `dialogues.json5` 的反序列化 schema 与装载——[`crate::dialogue`] 那两张
//! 内容表的输入侧。
//!
//! 分成独立一个模块而不是塞进 [`crate::content_schema`]，理由与
//! [`crate::content_schema_gear`]/[`crate::content_schema_world`] 逐字相同：
//! 一个 JSON5 文件对应一个 schema 模块，`content_schema.rs` 已经是这条切分
//! 线的产物。本模块的三条硬要求（未知字段报错、缺必填字段报错、错误带行列
//! 位置）沿用 [`crate::content_schema`] 模块文档，一个字不改。
//!
//! # 一个文件两张名册
//!
//! 先例是 `crafting.json5`（配方类别 + 配方）。`dialogues` 是会话入口，
//! `nodes` 是节点，两者同住 `dialogues.json5`，一遍读完。
//!
//! # 只 intern 还是必须已定义
//!
//! | 引用 | 语义 | 理由 |
//! |---|---|---|
//! | `speaker.profession` / `speaker.culture` | **必须已定义** | 只 get 不 intern。职业表与文化表都排在 `dialogues.json5` **之前**装载（[`crate::content_data`] 的 `CONTENT_FILES`），拼错当场报错 |
//! | 条件里的 `quest` / `item` / `race` / `org` | **必须已定义** | 同上，四张表全部排在前面 |
//! | `root` / 选项的 `next` | intern | **必须允许前向引用**：同一个文件里回环是常态，`next` 指向的节点几乎一定写在后面。真正的「指向的节点存在吗」由 [`crate::dialogue::validate_references`] 在全部 mod 装完之后一次性做 |
//!
//! # 条件为什么是带标签的对象，不是 `serde(untagged)`
//!
//! `{ kind: "…", … }` 与 [`crate::content_schema::RawQuestCondition`]、
//! `RawSkillEffect` 同一个写法。本 crate 里唯一一处 `untagged` 是
//! [`crate::content_expr`] 的 `RawExpr`，它成立是因为三个变体的 JSON 表示
//! 天然不重叠；对话条件的十条 `kind` 参数大量重叠（好几条都只有一个
//! `value`），不满足那个前提。
//!
//! # 一个字面文案都不进 JSON5
//!
//! `text_key` 走 [`parse_id`]（**不 intern**），见 [`crate::dialogue`] 模块
//! 文档末节。

use serde::Deserialize;

use ll_core::ident::ContentIndex;
use ll_world::entity::AffiliationKind;

use crate::content_schema::{Applied, intern_id, parse_id, required_id};
use crate::dialogue::{
    AffiliationQuery, DialogueAttrs, DialogueCondition, DialogueNext, DialogueNodeAttrs,
    DialogueNodeTable, DialogueOption, DialogueSpeaker, DialogueTable,
};
use crate::registry::Registry;

/// 选项 `next` 字段的保留字：结束会话。
///
/// 是一个**保留字**而不是「留空表示结束」：空字符串在 JSON5 里看不出意图，
/// 而且会与「作者忘了填」无法区分。写死一个词，拼错就报错。
pub const END_OF_DIALOGUE: &str = "end";

/// `dialogues.json5` 的顶层形状。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueFile {
    /// 会话入口名册，按书写顺序注册。
    #[serde(default)]
    pub dialogues: Vec<RawDialogue>,
    /// 节点名册，按书写顺序注册。
    #[serde(default)]
    pub nodes: Vec<RawDialogueNode>,
}

/// 一段会话入口声明。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDialogue {
    /// 完整命名空间标识符。
    pub id: String,
    /// 这段对话认谁说。
    pub speaker: RawDialogueSpeaker,
    /// 起始节点的完整标识符。
    pub root: String,
}

/// 说话人匹配条件，见 [`DialogueSpeaker`]。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDialogueSpeaker {
    /// 职业标识符，必填。
    pub profession: String,
    /// 文化标识符，缺省表示不按文化收窄。
    #[serde(default)]
    pub culture: Option<String>,
}

/// 一个对话节点声明。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDialogueNode {
    /// 完整命名空间标识符。
    pub id: String,
    /// NPC 这一句说什么——**本地化键**，不是文案。
    pub text_key: String,
    /// 玩家能选的行，按书写顺序；缺省表示死路一条。
    #[serde(default)]
    pub options: Vec<RawDialogueOption>,
}

/// 一条选项声明。
///
/// **本批次没有 `outcomes` 字段**，理由见 [`DialogueOption`] 文档。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDialogueOption {
    /// 这一行显示什么——**本地化键**。
    pub text_key: String,
    /// 全部满足才显示；缺省 = 无条件显示。
    #[serde(default)]
    pub conditions: Vec<RawDialogueCondition>,
    /// 选中之后跳到哪：节点标识符，或保留字 [`END_OF_DIALOGUE`]。
    pub next: String,
}

/// 一条显示条件。形状与理由同
/// [`crate::content_schema::RawQuestCondition`]：一个 `kind` 标签 + 一组
/// 可选参数，`resolve` 里逐 `kind` 校验「该有的必须有、不该有的必须没有」。
///
/// **「不该有的必须没有」这一半不能省**：`{ kind: "wallet-at-least",
/// count: 3 }` 若被静默接受，作者会以为自己写的是「有 3 个」，而实际生效的
/// 是一条缺 `value` 的错误条件。这与 `RawQuestCondition` 拒绝
/// `script` + `count` 的组合是同一条纪律。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDialogueCondition {
    /// 十条之一，见 [`DialogueCondition`]。
    pub kind: String,
    /// 归属类别：`faction` / `religion` / `guild` / `culture` / `family`。
    #[serde(default)]
    pub affiliation: Option<String>,
    /// 具体组织的内容标识符（今天只有文化能满足，见 [`AffiliationQuery`]）。
    #[serde(default)]
    pub org: Option<String>,
    /// 任务标识符。
    #[serde(default)]
    pub quest: Option<String>,
    /// 对话标志标识符。
    #[serde(default)]
    pub flag: Option<String>,
    /// 物品标识符。
    #[serde(default)]
    pub item: Option<String>,
    /// 种族标识符。
    #[serde(default)]
    pub race: Option<String>,
    /// `has-item` 的数量下界，恒 ≥ 1。
    #[serde(default)]
    pub count: Option<u32>,
    /// `standing-at-least` / `wallet-at-least` 的数值下界。
    #[serde(default)]
    pub value: Option<i64>,
}

/// 一条条件里「哪些参数该出现」的清单——`resolve` 用它一次性把不该出现的
/// 参数挑出来报错，避免十条 `kind` 各写一遍五行 `if x.is_some()`。
#[derive(Debug, Clone, Copy, Default)]
struct Allowed {
    affiliation: bool,
    org: bool,
    quest: bool,
    flag: bool,
    item: bool,
    race: bool,
    count: bool,
    value: bool,
}

impl RawDialogueCondition {
    /// 报出「这个 kind 缺了某个必填参数」。
    fn missing(&self, field: &str) -> String {
        format!("对话条件 kind {:?} 缺少必填字段 {field:?}", self.kind)
    }

    /// 校验「不该出现的参数确实没出现」，见本类型文档。
    fn reject_extras(&self, allowed: Allowed) -> Result<(), String> {
        let extras = [
            (
                "affiliation",
                self.affiliation.is_some(),
                allowed.affiliation,
            ),
            ("org", self.org.is_some(), allowed.org),
            ("quest", self.quest.is_some(), allowed.quest),
            ("flag", self.flag.is_some(), allowed.flag),
            ("item", self.item.is_some(), allowed.item),
            ("race", self.race.is_some(), allowed.race),
            ("count", self.count.is_some(), allowed.count),
            ("value", self.value.is_some(), allowed.value),
        ];
        for (name, present, permitted) in extras {
            if present && !permitted {
                return Err(format!("对话条件 kind {:?} 不接受字段 {name:?}", self.kind));
            }
        }
        Ok(())
    }

    /// 解析归属查询（`affiliation` 必填、`org` 可选）。
    fn affiliation_query(&self, registry: &Registry) -> Result<AffiliationQuery, String> {
        let raw = self
            .affiliation
            .as_deref()
            .ok_or_else(|| self.missing("affiliation"))?;
        let kind = match raw {
            "faction" => AffiliationKind::Faction,
            "religion" => AffiliationKind::Religion,
            "guild" => AffiliationKind::Guild,
            "culture" => AffiliationKind::Culture,
            "family" => AffiliationKind::Family,
            other => {
                return Err(format!(
                    "未知的归属类别 {other:?}（只认 faction / religion / guild / culture / family）"
                ));
            }
        };
        let org = match self.org.as_deref() {
            None => None,
            Some(raw) => Some(required_id(registry, raw, "归属组织标识符")?),
        };
        Ok(AffiliationQuery { kind, org })
    }

    /// 解析成一条 [`DialogueCondition`]。
    ///
    /// `registry` 只读：本函数用到的四类引用（任务/物品/种族/组织）全部
    /// 走 [`required_id`]，一条都不 intern。
    fn resolve(&self, registry: &Registry) -> Result<DialogueCondition, String> {
        let affiliation_shape = Allowed {
            affiliation: true,
            org: true,
            ..Allowed::default()
        };
        match self.kind.as_str() {
            "affiliated" | "not-affiliated" => {
                self.reject_extras(affiliation_shape)?;
                let query = self.affiliation_query(registry)?;
                Ok(if self.kind == "affiliated" {
                    DialogueCondition::Affiliated(query)
                } else {
                    DialogueCondition::NotAffiliated(query)
                })
            }
            "standing-at-least" => {
                self.reject_extras(Allowed {
                    value: true,
                    ..affiliation_shape
                })?;
                let query = self.affiliation_query(registry)?;
                let raw = self.value.ok_or_else(|| self.missing("value"))?;
                let value = i32::try_from(raw).map_err(|_| {
                    format!("声望下界 {raw} 超出千分比的表示范围（i32），见 Affiliation::standing")
                })?;
                Ok(DialogueCondition::StandingAtLeast { query, value })
            }
            "quest-completed" | "quest-not-completed" => {
                self.reject_extras(Allowed {
                    quest: true,
                    ..Allowed::default()
                })?;
                let raw = self.quest.as_deref().ok_or_else(|| self.missing("quest"))?;
                let quest = required_id(registry, raw, "任务标识符")?;
                Ok(if self.kind == "quest-completed" {
                    DialogueCondition::QuestCompleted(quest)
                } else {
                    DialogueCondition::QuestNotCompleted(quest)
                })
            }
            "flag-set" | "flag-not-set" => {
                self.reject_extras(Allowed {
                    flag: true,
                    ..Allowed::default()
                })?;
                let raw = self.flag.as_deref().ok_or_else(|| self.missing("flag"))?;
                // 标志没有内容表可查（它是对话系统自己在 `Agent.mod_state`
                // 里写的一条记录），因此只 `parse_id` 成 `NamespacedId`，
                // 与 `QuestCondition::Script` 携带一个 id 是同一种情形。
                let flag = parse_id(raw, "对话标志标识符")?;
                Ok(if self.kind == "flag-set" {
                    DialogueCondition::FlagSet(flag)
                } else {
                    DialogueCondition::FlagNotSet(flag)
                })
            }
            "has-item" => {
                self.reject_extras(Allowed {
                    item: true,
                    count: true,
                    ..Allowed::default()
                })?;
                let raw = self.item.as_deref().ok_or_else(|| self.missing("item"))?;
                let item = required_id(registry, raw, "物品标识符")?;
                let count = self.count.ok_or_else(|| self.missing("count"))?;
                if count == 0 {
                    return Err(
                        "对话条件 has-item 的 count 必须至少是 1——0 件等于没写这条条件".to_string(),
                    );
                }
                Ok(DialogueCondition::HasItem { item, count })
            }
            "wallet-at-least" => {
                self.reject_extras(Allowed {
                    value: true,
                    ..Allowed::default()
                })?;
                Ok(DialogueCondition::WalletAtLeast(
                    self.value.ok_or_else(|| self.missing("value"))?,
                ))
            }
            "is-race" => {
                self.reject_extras(Allowed {
                    race: true,
                    ..Allowed::default()
                })?;
                let raw = self.race.as_deref().ok_or_else(|| self.missing("race"))?;
                Ok(DialogueCondition::IsRace(required_id(
                    registry,
                    raw,
                    "种族标识符",
                )?))
            }
            other => Err(format!(
                "未知的对话条件 kind {other:?}（只认 affiliated / not-affiliated / \
                 standing-at-least / quest-completed / quest-not-completed / flag-set / \
                 flag-not-set / has-item / wallet-at-least / is-race）"
            )),
        }
    }
}

/// 把一个文件里的会话入口与节点写进注册表与两张对话表。
///
/// 引用完整性（`root`/`next` 指向的节点真的存在吗）**不在这里校验**——
/// 那是 [`crate::dialogue::validate_references`] 在全部 mod 装载完毕之后的
/// 事，理由同 [`crate::content_schema::apply_quests`] 把环检查交给
/// `validate_no_cycles`。
///
/// 先写节点再写入口：两者之间没有顺序约束（`root` 走 intern），这个次序
/// 只是为了让「节点里的条件解析失败」这类更常见的错误先报出来。
pub fn apply_dialogues(
    registry: &mut Registry,
    dialogues: &mut DialogueTable,
    nodes: &mut DialogueNodeTable,
    file: &DialogueFile,
) -> Applied {
    for node in &file.nodes {
        let index = intern_id(registry, &node.id, "对话节点标识符")?;
        let text_key = parse_id(&node.text_key, "本地化键标识符")?;
        let mut options = Vec::with_capacity(node.options.len());
        for option in &node.options {
            let mut conditions = Vec::with_capacity(option.conditions.len());
            for condition in &option.conditions {
                conditions.push(condition.resolve(registry)?);
            }
            let next = resolve_next(registry, &option.next)?;
            options.push(DialogueOption {
                text_key: parse_id(&option.text_key, "本地化键标识符")?,
                conditions,
                next,
            });
        }
        nodes
            .define(index, DialogueNodeAttrs { text_key, options })
            .map_err(|err| err.to_string())?;
    }

    for dialogue in &file.dialogues {
        let index = intern_id(registry, &dialogue.id, "对话标识符")?;
        let profession = required_id(registry, &dialogue.speaker.profession, "职业标识符")?;
        let culture = match dialogue.speaker.culture.as_deref() {
            None => None,
            Some(raw) => Some(required_id(registry, raw, "文化标识符")?),
        };
        let root = intern_id(registry, &dialogue.root, "对话节点标识符")?;
        dialogues
            .define(
                index,
                DialogueAttrs {
                    speaker: DialogueSpeaker {
                        profession,
                        culture,
                    },
                    root,
                },
            )
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// 把 `next` 字段解析成 [`DialogueNext`]：保留字或一个节点标识符。
fn resolve_next(registry: &mut Registry, raw: &str) -> Result<DialogueNext, String> {
    if raw == END_OF_DIALOGUE {
        return Ok(DialogueNext::End);
    }
    Ok(DialogueNext::Node(intern_id(
        registry,
        raw,
        "对话节点标识符",
    )?))
}

/// 供 [`crate::content_hash`]/[`crate::content_audit`] 之外的调用方判断一条
/// 引用是不是「结束会话」——`DialogueNext` 自身是 `Copy` 的公开枚举，这里
/// 只留一个便捷判定，避免每个使用点各写一次 `matches!`。
pub fn is_end(next: DialogueNext) -> bool {
    matches!(next, DialogueNext::End)
}

/// 供 [`crate::content_hash`] 与 [`crate::content_audit`] 复用：一条
/// `DialogueNext` 里携带的节点索引（`End` 没有）。
pub fn next_target(next: DialogueNext) -> Option<ContentIndex> {
    match next {
        DialogueNext::End => None,
        DialogueNext::Node(index) => Some(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;

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
            crate::dialogue::validate_references(&dialogues, &nodes),
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
}
