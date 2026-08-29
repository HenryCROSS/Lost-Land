//! 角色性别 [`Gender`]。
//!
//! # 它是所有者裁定加进来的，今天只有渲染层在读
//!
//! 项目所有者的裁定原文：
//!
//! > 「开始游戏的时候需要玩家设置种族，性别，职业。」
//! >
//! > 「以后可能会加入不同性别的贴图，不过目前先留着个位置默认用其中
//! > 一个好了。」
//!
//! 「留着个位置」在本仓库里有一种唯一正确的实现方式：**贴图查找回退
//! 链**（`<种族>_<职业>_<性别>` → `<种族>_<职业>` → `<种族>` +
//! `<职业>`，见 `ll_game::surface_draw` 模块文档）。今天一张带性别的
//! 图都没有，所有人自然落到最后一段分层合成，**行为与本字段落地之前
//! 逐像素相同**——但槽位是真实存在的，往 `assets/sprites/` 里放一张
//! `lostland_human_lostland_blacksmith_female.png` 就生效，引擎一个字
//! 都不用改。
//!
//! **绝不复制文件**（「默认用其中一个」若实现成把同一张图复制两份，
//! 两份迟早会漂，本仓库有过先例）。
//!
//! # 决策层今天没有消费者，这是登记在案的，不是遗漏
//!
//! 婚配/血缘（`Kinship`）属规格 §15 的 P9。`scripts/ci/check_field_consumers.py`
//! 的 `EXEMPTIONS` 里有一条写明日期与安排的 `Agent.gender` 豁免，
//! 豁免理由是**渲染层今天就在读它**（一个现在就成立的真实消费点），
//! 不是「等 P9」。
//!
//! # 为什么不是内容（不进 `Registry`、不占 `ContentIndex`）
//!
//! 内容注册项的代价是：新增一条会让其后全部条目的 `ContentIndex`
//! 整体平移（气候批次实测过，见
//! `knowledge/handoff/2026-08-28-session-handoff.md` 第二节），因此
//! 每次增删都要走黄金基准重冻。性别的取值集合不是 mod 该扩展的东西
//! ——mod 想表达「第三种性别」时它真正要的是一套新的婚配/繁衍规则，
//! 不是往一个两值枚举里再塞一个变体。做成 Rust 枚举，加变体是纯加法，
//! 也不牵动任何索引。

use ll_core::rng::DetRng;
use serde::{Deserialize, Serialize};

/// 角色性别。
///
/// # 为什么只有两个变体
///
/// 今天唯一的消费者是贴图查找回退链，而已获所有者批准的美术批次是
/// **13 职业 × 9 种族 = 117 张**，性别维度**一张都没有**。多加一个
/// 变体等于凭空要求美术多产一档，而所有者没有裁定过那件事。
///
/// P9 婚配系统真正需要更多变体时，**追加一个变体是纯加法**：
/// [`Agent::gender`] 已经带 `serde(default)`，老存档不受影响；
/// [`Gender::ALL`] 是本文件里唯一那份清单，界面与测试都从它现取，
/// 不存在第二份需要同步的平行表。
///
/// [`Agent::gender`]: crate::entity::Agent::gender
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    /// 男性。
    Male,
    /// 女性。
    Female,
}

impl Gender {
    /// 全部取值，顺序即界面里左右键循环的顺序。
    ///
    /// **唯一的那份清单**：角色创建界面、贴图回退链的测试、NPC 的确定
    /// 性抽取全部从这里现取，不手抄平行表——「两份清单迟早只更新一份」
    /// 是本仓库反复付过代价的失败模式（见
    /// `ll_game::menu_screen::settings_rows` 文档同一条纪律）。
    pub const ALL: [Gender; 2] = [Gender::Male, Gender::Female];

    /// 展示名的 Fluent 键。
    ///
    /// 与种族/职业不同，性别**不是内容**（见模块文档），因此它没有一张
    /// 声明了 `display_name_key` 的内容表可查，键只能长在这里。这是
    /// 「引擎里不出现用户可见字符串」与「引擎里不按内容 id 分支」两条
    /// 纪律之间的正确落点：返回的是**本地化键**，不是文案本身。
    pub fn display_name_key(self) -> &'static str {
        match self {
            Gender::Male => "gender-male-display_name",
            Gender::Female => "gender-female-display_name",
        }
    }

    /// 精灵键里代表这个性别的那一段（小写 ASCII）。
    ///
    /// 与 [`Self::display_name_key`] 分开：前者随语言变化、后者是资产
    /// 文件名的一部分，**永远不许随译文变化**（同
    /// `ll_platform::config::NewGameConfig::terrain_preset`「存标识而不是
    /// 译名」那条既有理由）。
    pub fn sprite_tag(self) -> &'static str {
        match self {
            Gender::Male => "male",
            Gender::Female => "female",
        }
    }
    /// 按 `(世界种子, 实体键, 事件计数)` 确定性地抽一个性别（约束 C3：
    /// 禁止全局随机数流）。
    ///
    /// # 谁在用
    ///
    /// NPC。玩家的性别由角色创建界面给出，不走这里。
    ///
    /// **NPC 必须真的有性别**，否则「渲染层今天就在读它」这条豁免理由
    /// 只在玩家一个实体上成立，太单薄——世界上跑着的几百个 NPC 里
    /// 一个都不读它，那条理由就名不副实。
    ///
    /// # 事件计数是什么
    ///
    /// [`DetRng::for_entity`] 的第三个输入。同一个实体在不同「事件」上
    /// 取到的是互不相关的两条流，因此调用方给一个**本条用途专属**的
    /// 常量即可（见 [`GENDER_EVENT`]），不必与任何回合计数挂钩：性别
    /// 在一个实体的一生里只抽一次。
    pub fn deterministic(world_seed: u64, entity_key: u64, event_counter: u64) -> Gender {
        let mut rng = DetRng::for_entity(world_seed, entity_key, event_counter);
        let pick = rng.gen_range(Gender::ALL.len() as u64) as usize;
        // `gen_range` 的上界恒是 ALL 的长度，因此下标恒合法；`get` 只是
        // 为了不在一条纯派生路径上写 panic（同本仓库其余「查不到就退化」
        // 纪律）。
        Gender::ALL.get(pick).copied().unwrap_or_default()
    }
}

/// [`Gender::deterministic`] 专用的事件计数——一个实体的性别在它一生里
/// 只抽一次，因此这条流与任何回合/事件计数无关，取一个本用途专属的
/// 常量即可。
///
/// 取值本身没有含义，但**一旦改动，全世界 NPC 的性别会重抽**（世界摘要
/// 随之改变，要走黄金基准重冻四步）。
pub const GENDER_EVENT: u64 = 0x6765_6E64_6572_0001;

impl Default for Gender {
    /// 默认取第一个变体。
    ///
    /// **这是占位，不是断言。** 它服务两处：
    ///
    /// 1. 老存档——那些 `Agent` 早于「性别」这个概念，磁盘上没有任何
    ///    可以恢复的真相，`serde(default)` 只能给一个值；
    /// 2. 不经角色创建界面的构造路径（测试夹具、
    ///    `ll_game::world::spawn_player` 的默认那一份）。
    ///
    /// 选 `Male` 没有任何设定上的含义，只是「枚举的第一个」——所有者
    /// 的原话正是「目前先留着个位置默认用其中一个好了」。
    fn default() -> Self {
        Gender::Male
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 全部取值清单与枚举变体一一对应且无重复() {
        // 反例形式：将来加第三个变体时，若忘了往 ALL 里补，这条会红。
        let mut seen = Gender::ALL.to_vec();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), Gender::ALL.len(), "ALL 里有重复项");
        assert!(
            Gender::ALL.contains(&Gender::default()),
            "默认值必须是 ALL 里的一项"
        );
    }

    #[test]
    fn 精灵键片段两两不同且全是小写ascii() {
        // 精灵键片段会被拼进图集条目名，撞名就等于两个性别共用一张图。
        let tags: Vec<_> = Gender::ALL.iter().map(|g| g.sprite_tag()).collect();
        let mut sorted = tags.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), tags.len(), "精灵键片段撞名");
        for tag in tags {
            assert!(
                tag.chars().all(|c| c.is_ascii_lowercase()),
                "精灵键片段 {tag} 不是纯小写 ASCII，会让资产文件名依赖大小写规则"
            );
        }
    }

    #[test]
    fn 同一个种子与实体键恒抽出同一个性别() {
        // 约束 C3：确定性。同一个三元组在任何时候都得到同一个结果。
        let a = Gender::deterministic(20260828, 42, GENDER_EVENT);
        let b = Gender::deterministic(20260828, 42, GENDER_EVENT);
        assert_eq!(a, b);
    }

    #[test]
    fn 两个性别都抽得到不是恒返回同一个() {
        // 反例：若 deterministic 退化成「恒返回默认值」，本条会红。
        let seen: std::collections::BTreeSet<_> = (0..64u64)
            .map(|key| Gender::deterministic(20260828, key, GENDER_EVENT))
            .collect();
        assert_eq!(
            seen.len(),
            Gender::ALL.len(),
            "六十四个实体键里没有把两个性别都抽出来，抽取退化了"
        );
    }

    #[test]
    fn 展示名键两两不同() {
        let keys: Vec<_> = Gender::ALL
            .iter()
            .map(|g| g.display_name_key())
            .collect::<Vec<_>>();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "两个性别共用了同一条本地化键");
    }
}
