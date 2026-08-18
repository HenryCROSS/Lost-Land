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

use ll_core::rng::DetRng;

use crate::entity::{EntityId, FamilyId};

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
