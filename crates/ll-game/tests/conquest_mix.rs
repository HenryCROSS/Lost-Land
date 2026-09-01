//! 编年史结局的**构成比**：三百年跑完之后，打了几仗、谁被占了、谁被
//! 铲平了、地上还剩多少人、七份文化各占几座城。
//!
//! # 这个文件替代的是一个已经不存在的 example
//!
//! `knowledge/handoff/2026-08-27-session-handoff.md` 与
//! `2026-08-28-session-handoff.md` 都点名「加种族之后**必须重跑
//! `probe_conquest`**」——`crates/ll-game/examples/probe_conquest.rs`
//! 是气候条带批次用来测「战争总场次 / 存活据点 / 活人口」的那个探针。
//!
//! **它已经不存在了**：2026-08-29 所有者裁定删除全部 `examples/`
//! （ADR 0030），门禁 `scripts/ci/check_no_examples.sh` 的判据是「工作区
//! 一个 example target 都不许有」，因此它既不能被找回来、也不能被重写
//! 成 example。证据见 `crates/ll-world/tests/climate_terrain_mix.rs` 头
//! 注释与 `knowledge/design/worldgen-parameters.md`「它连带改变了什么」
//! 一节的两段更正。
//!
//! 落点因此照抄 `climate_terrain_mix.rs` 已经验证过的那条路：**把一次性
//! 测量写成会断言的测试**。同一批数字于是同时是两样东西——
//!
//! - `cargo test -p ll-game --test conquest_mix -- --nocapture` 打出的
//!   那张表，就是「改前/改后」两列的来源；
//! - 表里那几条**不随内容漂移的结构性性质**被断言下来，下一次有人改坏
//!   战争推演时它会红，而不是像 example 那样一声不吭地打印错的数。
//!
//! # 为什么断言的是性质、不是具体数字
//!
//! 战争场次与人口是**内容敏感**的：加一份文化、改一条敌意、动一次地形
//! 都会让它们变。把具体数字钉死等于造第四条黄金基准，而本仓库已经有
//! 三条、每条都要走四步重冻——再加一条只会让每个内容批次多付一次代价，
//! 换不到任何新的鉴别力（那三条已经覆盖了「世界逐位稳定」这件事）。
//!
//! 这里断言的是**推演本身没坏**：仗真的打过、两种结局都真的出现过、
//! 地上真的还有活人。这些性质在任何一份合理的内容下都成立，因此它们
//! 变红只可能是推演坏了。
//!
//! # 它真的复现了那个已删除的探针吗——交叉核对
//!
//! 本文件第一次跑出来的三个种子（**内容一个字都还没改**，主干
//! `04885c9`）与 `knowledge/design/worldgen-parameters.md`
//! 「它连带改变了什么（实测，不是估计）」那张表的「带宽 250」一行
//! **逐个数字相同**：
//!
//! | | 战争总场次 | 存活据点 | 活人口 |
//! |---|---|---|---|
//! | 那张表（`probe_conquest`，气候批次实测） | 37 / 41 / 30 | 231 / 232 / 240 | 9796 / 8825 / 9967 |
//! | 本文件首跑 | 37 / 41 / 30 | 231 / 232 / 240 | 9796 / 8825 / 9967 |
//!
//! 三列九个数字全中。这不是巧合能解释的——它证明本文件量的确实是那个
//! 探针量的那件事，因此「改前」那一列可以直接与历史记录接上。
//!
//! # 批次 24（五个新种族 + 沙漠文化）的改前／改后（本机实测）
//!
//! 三个种子按 `SEEDS` 顺序（20260826 / 7 / 99）：
//!
//! | | 存活据点 | 活人口 | 战争总场次 | 占领 | 毁灭 |
//! |---|---|---|---|---|---|
//! | 改前 | 231 / 232 / 240 | 9796 / 8825 / 9967 | 37 / 41 / 30 | 7 / 9 / 5 | 30 / 32 / 25 |
//! | 改后 | 232 / 225 / 245 | 9996 / 8872 / 9966 | 34 / 43 / 18 | 3 / 9 / 5 | 31 / 34 / 13 |
//!
//! 存活据点与人口基本持平（±3%），**战争场次的方差明显变大**（种子 99
//! 从 30 掉到 18）。原因是文化分布被第七份文化重新洗过一遍，而战争的
//! 三道闸门里有一道是敌意——只有部落声明敌意，部落的城少了，仗就少了：
//!
//! | 文化（种子 20260826，还有人住的据点） | 改前 | 改后 |
//! |---|---|---|
//! | 农庄 | 38 | 30 |
//! | 矿邑 | 17 | 25 |
//! | 林居 | 44 | 37 |
//! | 渔港 | 76 | 65 |
//! | 石砦 | 11 | 20 |
//! | **部落** | **45** | **20** |
//! | 沙民（本批新增） | — | 35 |
//!
//! 沙民凭空多出 35 座，其余六份各让出一些——**部落让得最多**（45 → 20）。
//! 这是 `pick_culture` 的加权抽取该有的行为（多一个候选，每个候选的
//! 期望份额下降），只是部落原先靠「木材 × 丘陵」占着的那批点位，现在
//! 有一部分被「食物 × 沙漠」抢走了。
//!
//! **交接文档点名要看的那一条**（「九个种族后同族碰撞概率明显下降」）
//! 也实测到了：三个种子合计的易主里，同族／异族从 **9 / 12** 变成
//! **6 / 11**——同族占比 43% → 35%。注意这两列是**代理量**不是判定当时
//! 的那个布尔，别拿它当闸门，理由见本文件末尾那一大段。
//!
//! # ADR 0025：不启动窗口，不盲注输入
//!
//! 与 `culture_and_war.rs`/`populated_determinism.rs` 同一条纪律：全程
//! 不碰 GPU、不模拟键盘。这里连 `WorldState` 都不造——只跑
//! `WorldChronicle::generate`，也就是 `ll_game::world::build_new_world`
//! 里产出这批数字的那一步，输入逐字段与它一致（同一份 `build_zone_layout`、
//! 同一份 `build_zone_noise`、同一份 `ChronicleParams::default()`）。
//! 铺地形与物化 NPC 与本文件要测的东西无关，跳过它们让三个种子跑得完。

use ll_game::content::LoadedContent;
use ll_game::world::build_zone_layout;
use ll_world::chronicle::{ChronicleInput, ChronicleParams, WorldChronicle};
use ll_world::culture::{CultureKind, CultureTable, founder_race};
use ll_world::generate::{GenParams, build_zone_noise};
use ll_world::history::{HistoricalEvent, HistoricalEventKind, SettlementDemise};
use ll_world::settlement::{SettlementSite, SettlementStatus};

/// 测量用的三个种子——与 `crates/ll-world/tests/climate_terrain_mix.rs`
/// 的 `SEEDS` 逐字相同，也就是当年 `probe_conquest` 用的那一批。同一批
/// 世界，才谈得上「地形怎么变了」与「战争结局怎么变了」说的是同一件事。
const SEEDS: [u64; 3] = [20260826, 7, 99];

/// 测试用内容装载——走与本体二进制完全相同的通道，`mods_root` 指向仓库
/// 真实的 `mods/` 目录。写法与 `culture_and_war.rs`/`populated_determinism.rs`
/// 的同名帮手一致；集成测试之间看不见彼此的私有帮手，因此这几行在这里
/// 重来一遍。
fn test_content() -> LoadedContent {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ll-game-conquest-mix-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let mods_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods");
    let content = ll_game::content::load_content(&mods_root, &dir.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功");
    let _ = std::fs::remove_dir_all(&dir);
    content
}

/// 一个种子跑出来的一整份编年史结局构成。
///
/// 字段顺序即打印顺序；全部是计数，不含任何浮点——比值在打印时才算，
/// 避免把一个会随内容漂移的浮点数写进断言。
#[derive(Debug, Default, Clone, Copy)]
struct 结局构成 {
    /// 最终快照里还有人住的据点数。
    存活据点: usize,
    /// 最终快照里的废墟数。
    废墟: usize,
    /// 存活据点的人口总和。
    活人口: u64,
    /// 打过的仗（以毁灭收场的 + 以易主收场的）。
    战争总场次: usize,
    /// 其中以**易主**收场的。
    占领: usize,
    /// 其中以**毁灭**收场的。
    毁灭: usize,
    /// 易主里攻守双方建立者种族相同的那些。
    同族占领: usize,
    /// 易主里攻守双方建立者种族不同的那些。
    异族占领: usize,
    /// 易主里连**文化**都换了的那些（同文化互相吞并在地上看不出区别）。
    换了文化的占领: usize,
}

/// 数一遍这份编年史。
///
/// # 确定性（约束 C5）
///
/// 只线性扫 `events` 与 `sites` 两个 `Vec`，不经任何哈希容器。
fn 统计(
    sites: &[SettlementSite],
    events: &[HistoricalEvent],
    cultures: &CultureTable,
    seed: u64,
) -> 结局构成 {
    let mut out = 结局构成::default();
    for site in sites {
        match site.status {
            SettlementStatus::Inhabited => {
                out.存活据点 += 1;
                out.活人口 += u64::from(site.population);
            }
            SettlementStatus::Ruined => out.废墟 += 1,
        }
    }
    for event in events {
        match &event.kind {
            HistoricalEventKind::SettlementAbandoned(record) => {
                if matches!(record.cause, SettlementDemise::War { .. }) {
                    out.毁灭 += 1;
                }
            }
            HistoricalEventKind::SettlementConquered(record) => {
                out.占领 += 1;
                if record.former_culture != record.new_culture {
                    out.换了文化的占领 += 1;
                }
                // 与 `ll_world::chronicle` 的 `occupation_numerator`
                // 判定时用的是同一个函数、同一条随机流：受害方按**自己
                // 的据点 id** + 易主前的文化，攻方按**它自己的据点 id**
                // + 易主后的文化（后者就是攻方那一份）。
                let 受害方 = founder_race(
                    cultures,
                    Some(CultureKind::from_index(record.former_culture)),
                    record.site,
                    seed,
                );
                let 攻方 = founder_race(
                    cultures,
                    Some(CultureKind::from_index(record.new_culture)),
                    record.conqueror,
                    seed,
                );
                if 受害方.is_some() && 受害方 == 攻方 {
                    out.同族占领 += 1;
                } else {
                    out.异族占领 += 1;
                }
            }
            HistoricalEventKind::Kill(_) | HistoricalEventKind::SettlementFounded(_) => {}
        }
    }
    out.战争总场次 = out.占领 + out.毁灭;
    out
}

/// 跑一个种子的编年史——输入逐字段与
/// `ll_game::world::build_new_world_with_mode` 里那一处 `generate` 一致。
fn 跑一个种子(content: &LoadedContent, seed: u64) -> WorldChronicle {
    let layout = build_zone_layout().expect("本体默认区块布局恒合法");
    let params = GenParams {
        seed,
        ..GenParams::default()
    };
    let noise = build_zone_noise(&layout, &params).expect("本体默认布局下噪声源恒能建立");
    WorldChronicle::generate(
        &ChronicleInput {
            layout: &layout,
            noise: &noise,
            params: &params,
            terrain_ids: &content.terrain_ids,
            terrain_table: &content.terrain_table,
            resources: &content.resource_table,
            cultures: &content.culture_table,
        },
        ChronicleParams::default(),
    )
}

#[test]
fn 三百年历史的结局构成比() {
    // Arrange
    let content = test_content();

    // Act
    let mut 全部 = Vec::new();
    for seed in SEEDS {
        let chronicle = 跑一个种子(&content, seed);
        let 构成 = 统计(
            chronicle.sites(),
            chronicle.events(),
            &content.culture_table,
            seed,
        );
        // 这一串就是「改前/改后」两列的来源。用 `--nocapture` 看。
        println!(
            "种子 {seed}：存活据点 {} · 废墟 {} · 活人口 {} · 战争 {}（占领 {} / 毁灭 {}）· \
             占领里同族 {} / 异族 {} · 其中真的换了文化的 {}",
            构成.存活据点,
            构成.废墟,
            构成.活人口,
            构成.战争总场次,
            构成.占领,
            构成.毁灭,
            构成.同族占领,
            构成.异族占领,
            构成.换了文化的占领,
        );
        全部.push((seed, 构成));
    }

    // 文化分布：每一份已注册的文化各占几座还有人住的城。这一列回答的是
    // 「新加的文化真的会被 `pick_culture` 抽中吗」——只打印不断言，
    // 断言在 `每一份本体文化都真的建得起城` 那一条。
    let chronicle = 跑一个种子(&content, SEEDS[0]);
    for kind in content.culture_table.registered() {
        let 座数 = chronicle
            .sites()
            .iter()
            .filter(|site| {
                site.status == SettlementStatus::Inhabited && site.culture == Some(*kind)
            })
            .count();
        let 名字 = content
            .registry
            .resolve(kind.index())
            .map(|id| id.to_string())
            .unwrap_or_else(|| "<解析不出>".to_string());
        println!("种子 {} 的文化分布：{名字} {座数} 座", SEEDS[0]);
        assert!(
            座数 > 0,
            "文化 {名字} 在种子 {} 的三百年历史里一座城都没建起来——             `mods/lostland/cultures.json5` 顶上那条判据（「每一条都必须回答             它在世界生成里真的会被抽中吗」）对它不成立了",
            SEEDS[0]
        );
    }

    // 建立者种族分布：每一个**已定义**的本体种族都必须真的当过某座
    // 还有人住的据点的建立者。
    //
    // 这一条回答的是本批最实在的那个问题：**「只加种族不加
    // `founder_races`，它们一座据点都不会属于」**（2026-08-28 交接文档
    // 第六节的落点表就是为这句话写的）。加完五族之后，如果哪一族的
    // 落点写漏了，它在世界上就是不存在的——而那不会有任何东西报错。
    //
    // 清单从注册表现查，因此**下一次加种族时这一条自动开始管它**，
    // 与 `crates/ll-game/tests/npc_appearance.rs` 那条合成图断言同一
    // 条纪律。只数本体命名空间：`example_mod` 的种族没有落点是它自己
    // 的内容决定。
    let 本体 = content
        .registry
        .resolve(content.race_ids.human)
        .expect("本体人类恒解析得到")
        .namespace()
        .to_string();
    for race in content.registry.snapshot() {
        if race.namespace() != 本体 {
            continue;
        }
        let Some(index) = content.registry.get(&race) else {
            continue;
        };
        if !content.race_table.is_defined(index) {
            // 占位种族（`lostland:placeholder_race`）刻意没有 `RaceDef`，
            // 它不该、也不可能建城。
            continue;
        }
        let 座数 = chronicle
            .sites()
            .iter()
            .filter(|site| {
                site.status == SettlementStatus::Inhabited
                    && founder_race(&content.culture_table, site.culture, site.id, SEEDS[0])
                        == Some(index)
            })
            .count();
        println!("种子 {} 的建立者种族分布：{race} {座数} 座", SEEDS[0]);
        assert!(
            座数 > 0,
            "本体种族 {race} 在种子 {} 的三百年历史里一座据点都没建起来——             多半是 `mods/lostland/cultures.json5` 里漏了它的 `founder_races`              落点，而漏了不会有任何东西报错：那一族在世界上就是不存在的",
            SEEDS[0]
        );
    }

    // Assert：只断言不随内容漂移的性质，见模块文档「为什么断言的是性质」。
    for (seed, 构成) in &全部 {
        assert!(
            构成.存活据点 > 0,
            "种子 {seed} 的三百年历史一座还有人住的据点都没剩下——\
             那不是「内容变了」，是选址或承载力坏了"
        );
        assert!(
            构成.活人口 > 0,
            "种子 {seed} 的存活据点加起来一个活人都没有——\
             `SettlementStatus::Inhabited` 的定义要求人口 > 0"
        );
        assert!(
            构成.战争总场次 > 0,
            "种子 {seed} 的三百年里一仗都没打过——人口阈值、优势比、\
             开战掷骰这三道闸门里有一道恒假"
        );
        assert_eq!(
            构成.战争总场次,
            构成.占领 + 构成.毁灭,
            "一仗必须恰好落进「占领」或「毁灭」之一"
        );
    }

    // 两种结局都必须真的出现过（三个种子合起来看）。分开看太脆：一个
    // 种子里恰好没有异族战争是合理的内容波动，而「全世界三个种子都
    // 没有占领」只可能是 `occupation_numerator` 恒返回 `None`。
    let 总占领: usize = 全部.iter().map(|(_, c)| c.占领).sum();
    let 总毁灭: usize = 全部.iter().map(|(_, c)| c.毁灭).sum();
    assert!(
        总占领 > 0,
        "三个种子里一次易主都没有——`occupation_numerator` 恒 `None` \
         （没有文化）或占领掷骰恒假时正是这个症状"
    );
    assert!(
        总毁灭 > 0,
        "三个种子里一座被打没的城都没有——占领掷骰恒真时正是这个症状"
    );

    // 同族／异族那两列**只打印，不断言**——这是本批开工时实测出来的
    // 结论，记在这里免得下一个人再写一次同样的错断言。
    //
    // 本文件第一版断言的是「易主里同族占多数」，理由是
    // `SAME_RACE_OCCUPATION_NUMERATOR > CROSS_RACE_OCCUPATION_NUMERATOR`
    // （所有者：「同种族的话更倾向于占领而不是毁灭」）。**那条断言在
    // 本批一个字都还没改的主干上就已经是红的**（三个种子合计同族 9、
    // 异族 12）。
    //
    // 红的原因不是推演坏了，是**这两列根本不是 `occupation_numerator`
    // 当时读到的那两个值**：`founder_race` 的三个输入是（文化、**据点
    // id**、种子），占领之后受害方的文化被改写成攻方那一份，于是它的
    // 建立者种族按**自己的据点 id** 重抽，与攻方那座城抽出的种族只是
    // 大概率相同、不是必然相同。事后从 `SettlementConqueredRecord`
    // 反推出来的「同族」因此是一个**代理量**，不是判定当时的那个布尔。
    //
    // 这正是 `knowledge/handoff/2026-08-28-session-handoff.md` 四节第 4
    // 条登记着、**仍等所有者裁定**的那个问题（「占领之后 NPC 名册的
    // 种族跟着重抽，对吗」），也是 `culture_and_war.rs` 的 `find_conquest`
    // 里那一大段注释说的同一件事。裁定之前，把一个代理量断言成设计意图
    // 的证据是错的——它会在下一次内容变动时以「设计意图消失了」这种
    // 完全误导的措辞变红。
    //
    // 两列仍然打印：本批要回答的正是「九个种族之后同族碰撞概率降了
    // 多少」，那是一个要被看见的数，只是不该被当成闸门。
}
