//! 名字生成：纯函数，零存储。
//!
//! # 为什么名字必须是纯函数
//!
//! 每个 NPC 都有名字，而 NPC 可达数百万——若把名字存成字符串，光名字
//! 就要占几十兆，且要跟进存档迁移。名字是纯函数则零存储：
//!
//! ```text
//! 名 = 音素表(文化, hash(种子, entity_id))
//! 姓 = 音素表(文化, hash(种子, family_id))     ← 同族同姓，白送
//! 全名 = 按文化的姓名顺序拼接
//! ```
//!
//! 任何时候都能重算，同一个 NPC 每次算出来永远一样。矮人要塞用几万个
//! 矮人验证过这条：起名字本身极便宜，与性能瓶颈毫无关系。
//!
//! **三个白送的效果**：每个文化一套音素表，「这名字听起来像山地族」
//! 不需额外设计；姓氏随家族 ID 派生，联姻改姓与子女继承都是自然结果；
//! 玩家改名或剧情赐名存成偏移（不在本模块范围内），未改过的不占存储
//! ——与钱包同一个模式（见 `crate::entity::ThinPopulation::wallet_of`）。
//!
//! # 为什么必须用 `DetRng::for_entity`
//!
//! 若用任何全局随机流，同一个 NPC 的名字会因调用顺序而变——今天先给
//! A 起名再给 B 起名，明天顺序反过来，两人名字全乱。`DetRng::for_entity`
//! 只由 `(种子, ID, 事件计数)` 三元组决定，与调用顺序无关。

use std::collections::BTreeMap;

use ll_core::rng::DetRng;

use crate::culture::CultureTable;
use crate::entity::{Affiliation, AffiliationKind, Agent, EntityId, FamilyId, OrgRef};

/// 名字生成规则：本体即 mod——不同文化各提供一套。
///
/// 完整的文化系统冻结在 `knowledge/design/society-and-affiliation.md`
/// 第二节，`NamingRules` 是其中 `CultureDef::naming` 字段引用的类型；
/// 本任务只建这一个独立可用的命名规则类型，不建完整的 `CultureDef`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamingRules {
    /// 音节声母表。
    pub onsets: Vec<String>,
    /// 音节韵腹表。
    pub nuclei: Vec<String>,
    /// 音节韵尾表。
    pub codas: Vec<String>,
    /// 音节数的 `(下限, 上限)`，闭区间。上限不大于下限时恒取下限。
    pub syllables: (u8, u8),
    /// 姓在名前（如「张三」）取 `true`；名在姓前（如「John Smith」）取
    /// `false`。
    pub surname_first: bool,
}

/// 生成失败时的占位名：三张音素表都为空——多半是 mod 提供了残缺的
/// 命名规则——此时没有任何音素可拼，与其崩溃，不如给一个能一眼看出
/// 「这是占位符」的名字，让内容作者能立刻定位到是命名规则配错了。
const PLACEHOLDER_NAME: &str = "无名氏";

/// 给定名（不含姓）之间互相区分的事件计数，用来把「起名」这条随机流
/// 与 [`surname`] 的随机流分开——否则 `entity.as_u64()` 与
/// `family.0` 数值恰好相同时，两处会算出同一个名字，姓名无端绑死。
const GIVEN_NAME_EVENT: u64 = 0;

/// 姓氏专用的事件计数，理由同 [`GIVEN_NAME_EVENT`]。
const SURNAME_EVENT: u64 = 1;

/// 由实体标识派生的给定名。
///
/// 同一个 `(rules, seed, entity)` 任何时候调用都得到相同结果——这正是
/// 「零存储、可重算」的字面含义。
pub fn given_name(rules: &NamingRules, seed: u64, entity: EntityId) -> String {
    let mut rng = DetRng::for_entity(seed, entity.as_u64(), GIVEN_NAME_EVENT);
    build_name(rules, &mut rng)
}

/// 由家族标识派生的姓氏。
///
/// 只依赖家族号，不依赖具体是哪个实体在查——这正是「同族同姓」白送的
/// 由来：同一家族的任意成员查到的都是同一个姓。
pub fn surname(rules: &NamingRules, seed: u64, family: FamilyId) -> String {
    let mut rng = DetRng::for_entity(seed, family.0 as u64, SURNAME_EVENT);
    build_name(rules, &mut rng)
}

/// 给定名与姓氏按文化的姓名顺序拼接成全名。
pub fn full_name(rules: &NamingRules, seed: u64, entity: EntityId, family: FamilyId) -> String {
    let given = given_name(rules, seed, entity);
    let last = surname(rules, seed, family);
    if rules.surname_first {
        format!("{last}{given}")
    } else {
        format!("{given}{last}")
    }
}

/// 按音节数拼接声母/韵腹/韵尾，得到一个名字（给定名或姓氏共用这套
/// 算法，区别只在传入的 `rng` 由哪个标识派生）。
fn build_name(rules: &NamingRules, rng: &mut DetRng) -> String {
    let syllable_count = syllable_count(rules, rng);
    let mut name = String::new();
    for _ in 0..syllable_count {
        push_phoneme(&mut name, rng, &rules.onsets);
        push_phoneme(&mut name, rng, &rules.nuclei);
        push_phoneme(&mut name, rng, &rules.codas);
    }
    if name.is_empty() {
        PLACEHOLDER_NAME.to_string()
    } else {
        name
    }
}

/// 算出这次要拼几个音节：`[下限, 上限]` 闭区间内的一个确定性随机值。
/// 上限不大于下限时恒取下限（含两者都为零的情况——此时不消耗 `rng`，
/// 因为区间只有一个可能取值，无需随机）。
fn syllable_count(rules: &NamingRules, rng: &mut DetRng) -> u8 {
    let (min, max) = rules.syllables;
    if max <= min {
        return min;
    }
    let span = (max - min) as u64 + 1;
    min + rng.gen_range(span) as u8
}

/// 从音素表里按 `rng` 选一个音素追加到 `name`；表为空时什么都不做，
/// 而不是索引越界崩溃——mod 可能只提供部分音素类别（例如某文化没有
/// 韵尾）。
fn push_phoneme(name: &mut String, rng: &mut DetRng, table: &[String]) {
    if table.is_empty() {
        return;
    }
    let index = rng.gen_range(table.len() as u64) as usize;
    name.push_str(&table[index]);
}

/// 一种语言下的三张音素表。
///
/// 「一种语言一份」是 `knowledge/design/naming-and-localization.md`
/// 三节冻结的裁定：中文版与英文版必然是两张不同的表（英文用拉丁字母
/// 拼音节，中文用汉字），但**同一下标必须是同一个音**——
/// `onsets[7]` 在英文版是 `thr`、在中文版是「斯」，指的是同一个音位、
/// 同一个人。`hash → 下标` 这一步与显示语言无关，于是切语言只换字形、
/// 不换名字，种子分享才成立。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhonemeTables {
    /// 音节声母表。
    pub onsets: Vec<String>,
    /// 音节韵腹表。
    pub nuclei: Vec<String>,
    /// 音节韵尾表；允许为空（某些文化没有韵尾）。
    pub codas: Vec<String>,
}

/// 一份文化声明的构词材料——[`crate::culture::CultureAttrs::naming`]
/// 的类型。
///
/// # 为什么是「一份音节数区间 + 每种语言一张音素表」
///
/// 音节数**不分语言**：它是这个名字有几个音的事实，与用什么字形写下来
/// 无关。音素表分语言，理由见 [`PhonemeTables`]。这样切分的直接好处是
/// 「表长必须对齐」这条约束**只需要检查音素表**，音节数不可能对不齐。
///
/// # 与 [`NamingRules`] 的关系
///
/// [`NamingRules`] 是**已经选好语言之后**的那一份规则，也是
/// [`given_name`]/[`surname`]/[`full_name`] 三个既有纯函数认的输入。
/// 本类型经 [`Self::rules_for`] 产出它——**不重写一套生成算法**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CultureNaming {
    /// 音节数的 `(下限, 上限)`，闭区间，语义同 [`NamingRules::syllables`]。
    pub syllables: (u8, u8),
    /// 语言标签（`"zh-CN"`/`"en"`……）到那种语言的音素表。
    ///
    /// **`BTreeMap` 而不是 `HashMap`**（约束 C5）：它的遍历顺序进内容
    /// 哈希，也决定 [`Self::rules_for`] 在查不到目标语言时回落到哪一条。
    /// `HashMap` 的遍历顺序不确定，两处都会变成不可复现的。
    pub phonemes: BTreeMap<String, PhonemeTables>,
}

/// [`CultureNaming`] 自查出来的形状问题，由
/// [`crate::culture::CultureTable::define`] 在注册期翻译成
/// [`crate::culture::CultureError`] 当场拒绝（ADR 0017）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamingProblem {
    /// 一种语言都没声明——那样这份文化的每个 NPC 都只能拿到占位名。
    NoLanguage,
    /// 某种语言的声母表或韵腹表是空的（载荷是语言标签）。没有声母与
    /// 韵腹就拼不出任何音节，[`build_name`] 会返回
    /// [`PLACEHOLDER_NAME`]——那是给「mod 配错了」准备的兜底，不该是
    /// 本体内容的常态。
    EmptyPhonemeTable(String),
    /// 两种语言的某一张表长度不同，载荷是
    /// `(语言甲, 语言乙, 表名)`。这正是
    /// `naming-and-localization.md` 三节点名的那条：表长一旦对不齐，
    /// 玩家切语言时全世界 NPC 集体改名。
    LengthMismatch(String, String, &'static str),
}

impl CultureNaming {
    /// 注册期自查，见 [`NamingProblem`]。
    ///
    /// 遍历顺序取 `BTreeMap` 的键序，因此报出来的第一条问题是确定的
    /// ——同一份内容在任何机器上都得到同一条错误消息（约束 C5）。
    pub fn problem(&self) -> Option<NamingProblem> {
        let mut iter = self.phonemes.iter();
        let Some((first_lang, first)) = iter.next() else {
            return Some(NamingProblem::NoLanguage);
        };
        if first.onsets.is_empty() || first.nuclei.is_empty() {
            return Some(NamingProblem::EmptyPhonemeTable(first_lang.clone()));
        }
        for (lang, tables) in iter {
            if tables.onsets.is_empty() || tables.nuclei.is_empty() {
                return Some(NamingProblem::EmptyPhonemeTable(lang.clone()));
            }
            for (name, left, right) in [
                ("onsets", first.onsets.len(), tables.onsets.len()),
                ("nuclei", first.nuclei.len(), tables.nuclei.len()),
                ("codas", first.codas.len(), tables.codas.len()),
            ] {
                if left != right {
                    return Some(NamingProblem::LengthMismatch(
                        first_lang.clone(),
                        lang.clone(),
                        name,
                    ));
                }
            }
        }
        None
    }

    /// 取某种显示语言下的那一份 [`NamingRules`]。
    ///
    /// 该语言没有声明时**回落到 `BTreeMap` 的第一条**，而不是返回
    /// `None`：各语言版本按下标对齐（[`PhonemeTables`]），回落拿到的是
    /// **同一个人的另一套转写**，不是另一个人。让它返回 `None` 会把
    /// 「这个 mod 没写日文音素表」升级成「日文玩家看到的全是占位名」。
    ///
    /// 一条语言都没有时返回 `None`——那种 [`CultureNaming`] 过不了
    /// [`Self::problem`]，正常内容里不存在。
    ///
    /// `surname_first` 恒取 `false`：本批只派生给定名，而
    /// [`given_name`] **一个字节都不读它**（只有 [`full_name`] 读），
    /// 因此取任何值结果相同。家族系统落地那天，姓氏顺序连同
    /// [`surname`] 一起接线，届时它才需要进内容声明。
    pub fn rules_for(&self, language: &str) -> Option<NamingRules> {
        let tables = self
            .phonemes
            .get(language)
            .or_else(|| self.phonemes.values().next())?;
        Some(NamingRules {
            onsets: tables.onsets.clone(),
            nuclei: tables.nuclei.clone(),
            codas: tables.codas.clone(),
            syllables: self.syllables,
            surname_first: false,
        })
    }

    /// 这份命名规则在 `language` 下**能拼出来的最长名字**。
    ///
    /// 不是取样，是上界：音节数取区间上限，每个位置取该表里最长的那个
    /// 音素。用途是排版门禁——`{ $npc_name }` 归
    /// `宽度判据::参数化`（拿模式串去量没有意义），真实宽度由
    /// 「拼好的整行」那条断言覆盖，而那条断言要的正是最坏情况，
    /// 不是随手挑一个名字。
    ///
    /// 三张表都为空、或该语言与全部回落都查不到时返回 `None`。
    pub fn longest_name(&self, language: &str) -> Option<String> {
        let rules = self.rules_for(language)?;
        let longest = |table: &[String]| -> String {
            table
                .iter()
                .max_by_key(|phoneme| phoneme.len())
                .cloned()
                .unwrap_or_default()
        };
        let syllable = format!(
            "{}{}{}",
            longest(&rules.onsets),
            longest(&rules.nuclei),
            longest(&rules.codas)
        );
        if syllable.is_empty() {
            return None;
        }
        Some(syllable.repeat(rules.syllables.1.max(rules.syllables.0) as usize))
    }
}

/// 一个 [`Agent`] 在 `language` 下的给定名——**渲染期现算，不进世界
/// 状态、不进存档**。
///
/// # 为什么名字不是 `Agent` 的一个字段
///
/// ADR 0009「默认派生，只存偏差」，而命名是其中**最极端的一例**：
/// 它连偏移都不允许，因为「同一个 NPC 永远同名」是设计要求本身
/// （`knowledge/design/naming-and-localization.md` 一节）。存一份字符串
/// 等于把纯函数的结果落盘，还要为它写迁移、进世界哈希、进 remap，
/// 换来零新增能力。
///
/// # 它凭什么是确定的
///
/// 三样输入全部是确定的：世界种子、实体号、以及**这个 NPC 属于哪份
/// 文化**（`AffiliationKind::Culture` 那条归属，由
/// `ll_mod::roster::build_npc_agent` 在物化时挂上）。底层是
/// [`given_name`]，它走 [`DetRng::for_entity`]——与调用顺序无关，
/// 因此「这一帧先画谁」不可能改变任何人的名字。
///
/// **它不消耗任何全局随机流**：`DetRng::for_entity` 由
/// `(种子, 实体号, 事件计数)` 三元组直接构造，不从世界里取任何东西，
/// 也不往世界里写任何东西（签名里没有 `&mut`）。
///
/// # 查不到就是查不到
///
/// 身上没有文化归属（玩家就是这一档，见
/// `ll_game::world::build_player_agent`）、归属指向的不是一份已定义的
/// 文化、或者那份文化没声明 `naming` 时返回 `None`。调用方回落到职业
/// 显示名——ADR 0015：诚实表达「这个人没有名字」，不伪造一个。
pub fn agent_given_name(
    agent: &Agent,
    entity: EntityId,
    cultures: &CultureTable,
    language: &str,
    seed: u64,
) -> Option<String> {
    // 线性扫 `Vec`，不碰任何哈希容器（约束 C5）。一个 `Agent` 身上
    // 至多一条文化归属（`build_npc_agent` 只挂一条），取第一条即可。
    let culture = agent.affiliations.iter().find_map(|affiliation| {
        let Affiliation {
            kind: AffiliationKind::Culture,
            org: OrgRef::Def(index),
            ..
        } = affiliation
        else {
            return None;
        };
        Some(crate::culture::CultureKind::from_index(*index))
    })?;
    let rules = cultures.naming(culture)?.rules_for(language)?;
    Some(given_name(&rules, seed, entity))
}

/// 测试夹具：一份**刚好合法**的命名声明（一种语言、各一个音素）。
///
/// 与 [`crate::building::bare_building_fixture`] 完全同一种用途与同一条
/// 理由：[`crate::culture::CultureAttrs`] 新增必填字段之后，几十处只关心
/// 别的字段的夹具需要一个「最小可用值」，各写各的会漂移。
pub fn bare_naming_fixture() -> CultureNaming {
    let mut phonemes = BTreeMap::new();
    phonemes.insert(
        "en".to_string(),
        PhonemeTables {
            onsets: vec!["k".into()],
            nuclei: vec!["a".into()],
            codas: Vec::new(),
        },
    );
    CultureNaming {
        syllables: (2, 2),
        phonemes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mountain_folk_rules() -> NamingRules {
        NamingRules {
            onsets: vec!["k".into(), "t".into(), "g".into(), "b".into(), "d".into()],
            nuclei: vec!["a".into(), "o".into(), "u".into(), "i".into(), "e".into()],
            codas: vec!["r".into(), "n".into(), "g".into(), String::new()],
            syllables: (2, 3),
            surname_first: false,
        }
    }

    fn empty_rules() -> NamingRules {
        NamingRules {
            onsets: Vec::new(),
            nuclei: Vec::new(),
            codas: Vec::new(),
            syllables: (2, 3),
            surname_first: false,
        }
    }

    #[test]
    fn 同一实体每次生成的名字相同() {
        // Arrange
        let rules = mountain_folk_rules();
        let entity = EntityId::new(7, 0);

        // Act
        let first = given_name(&rules, 42, entity);
        let second = given_name(&rules, 42, entity);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 不同实体生成不同的名字() {
        // 用一批实体而非仅两个，避免恰好撞名导致测试偶发失败——只要
        // 这批里出现一个以上的不同名字，就说明实体标识确实参与了生成。
        // Arrange
        let rules = mountain_folk_rules();

        // Act
        let names: std::collections::HashSet<String> = (0..20)
            .map(|index| given_name(&rules, 42, EntityId::new(index, 0)))
            .collect();

        // Assert
        assert!(names.len() > 1);
    }

    #[test]
    fn 同一家族的成员姓氏相同() {
        // Arrange
        let rules = mountain_folk_rules();
        let family = FamilyId(3);

        // Act
        let first = surname(&rules, 42, family);
        let second = surname(&rules, 42, family);

        // Assert：不同实体查同一个家族号，姓氏必须一致。
        assert_eq!(first, second);
    }

    #[test]
    fn 不同家族的姓氏不同() {
        // Arrange
        let rules = mountain_folk_rules();

        // Act
        let names: std::collections::HashSet<String> = (0..20u32)
            .map(|family| surname(&rules, 42, FamilyId(family)))
            .collect();

        // Assert
        assert!(names.len() > 1);
    }

    #[test]
    fn 姓在前的文化按其顺序拼接() {
        // Arrange
        let mut rules = mountain_folk_rules();
        rules.surname_first = true;
        let entity = EntityId::new(1, 0);
        let family = FamilyId(1);

        // Act
        let full = full_name(&rules, 42, entity, family);
        let expected = format!(
            "{}{}",
            surname(&rules, 42, family),
            given_name(&rules, 42, entity)
        );

        // Assert
        assert_eq!(full, expected);
    }

    #[test]
    fn 名在前的文化按其顺序拼接() {
        // Arrange
        let rules = mountain_folk_rules();
        let entity = EntityId::new(1, 0);
        let family = FamilyId(1);

        // Act
        let full = full_name(&rules, 42, entity, family);
        let expected = format!(
            "{}{}",
            given_name(&rules, 42, entity),
            surname(&rules, 42, family)
        );

        // Assert
        assert_eq!(full, expected);
    }

    #[test]
    fn 音素表为空时不崩溃而返回占位名() {
        // mod 可能提供空表——不能因此索引越界崩溃。
        // Arrange
        let rules = empty_rules();
        let entity = EntityId::new(1, 0);

        // Act
        let name = given_name(&rules, 42, entity);

        // Assert
        assert_eq!(name, PLACEHOLDER_NAME);
    }

    #[test]
    fn 名字生成不依赖调用顺序() {
        // 若用了任何全局随机流而非 DetRng::for_entity，先算 A 再算 B
        // 与先算 B 再算 A 会得到不同结果。
        // Arrange
        let rules = mountain_folk_rules();
        let a = EntityId::new(1, 0);
        let b = EntityId::new(2, 0);

        // Act：先 A 后 B
        let a_first = given_name(&rules, 42, a);
        let b_first = given_name(&rules, 42, b);

        // Act：先 B 后 A
        let b_second = given_name(&rules, 42, b);
        let a_second = given_name(&rules, 42, a);

        // Assert
        assert_eq!(a_first, a_second);
        assert_eq!(b_first, b_second);
    }
}
