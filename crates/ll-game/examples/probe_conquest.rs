//! 战争结局探针：跑一遍生产建档路径（`ll_game::world::build_new_world`），
//! 把每个种子的**战争场次 / 被毁据点 / 被占领据点 / 存活据点**原样打
//! 出来，同族与异族分开计。
//!
//! **这是测量工具，不是产品代码**——与 `probe_content_hash.rs` 同一个
//! 定位。存在的理由是：占领这条结局落地之后，「战争多了」这句话的含义
//! 变了（战争多不再等于世界被打空），而在此之前没有一个不用起游戏就能
//! 拿到这组对照数的办法。项目所有者要给战争频率定口径时读的就是这张表。
//!
//! ```text
//! cargo run --release --example probe_conquest -p ll-game
//! cargo run --release --example probe_conquest -p ll-game -- 20260826 7 99
//! ```
//!
//! 不给参数时跑 [`DEFAULT_SEEDS`]（与
//! `crates/ll-game/tests/culture_and_war.rs` 用的是同一批种子，因此这张
//! 表里的数字与那边断言的是同一批世界）。
//!
//! # 「查不到」那一列是什么
//!
//! 毁灭事件只记下攻方的 `WorldId`，双方的**种族**要顺着号码回到最终
//! 快照里查文化才算得出来。一处候选点可以被反复拓荒，早期某一场战争
//! 的守方（或攻方）因此可能在最终快照里根本不存在——那种事件归进这一
//! 列，不硬凑进同族/异族任何一边。占领事件不受影响：它的记录自带易主
//! 前后两份文化。

use ll_world::culture::{CultureKind, founder_race};
use ll_world::history::{HistoricalEventKind, SettlementDemise};
use ll_world::settlement::SettlementStatus;

/// 默认测量的三个种子——与端到端验收测试用的是同一批。
const DEFAULT_SEEDS: [u64; 3] = [20260826, 7, 99];

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let loaded = match ll_game::content::load_content(&root.join("mods"), &root.join("assets")) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("装载失败：{err}");
            std::process::exit(1);
        }
    };

    let parsed: Vec<u64> = std::env::args()
        .skip(1)
        .filter_map(|arg| arg.parse().ok())
        .collect();
    let seeds = if parsed.is_empty() {
        DEFAULT_SEEDS.to_vec()
    } else {
        parsed
    };

    println!("种子\t战争\t毁灭\t占领\t同族占\t同族毁\t异族占\t异族毁\t查不到\t存活\t废墟\t活人口");
    for seed in seeds {
        let world = match ll_game::world::build_new_world(
            &loaded,
            ll_world::generate::GenParams {
                seed,
                ..ll_world::generate::GenParams::default()
            },
        ) {
            Ok(world) => world,
            Err(err) => {
                eprintln!("种子 {seed} 建档失败：{err:?}");
                continue;
            }
        };
        let chronicle = world
            .world
            .terrain
            .chronicle()
            .expect("新游戏必然装上了编年史");
        let cultures = chronicle.culture_table();

        let mut destroyed = 0usize;
        let mut occupied = 0usize;
        let mut same_occupied = 0usize;
        let mut same_destroyed = 0usize;
        let mut cross_occupied = 0usize;
        let mut cross_destroyed = 0usize;
        let mut unresolved = 0usize;

        for event in chronicle.events() {
            match &event.kind {
                HistoricalEventKind::SettlementAbandoned(record) => {
                    let SettlementDemise::War { aggressor } = record.cause else {
                        continue;
                    };
                    destroyed += 1;
                    // 顺着号码回最终快照查双方的文化，再各抽一次建立者
                    // 种族——与 `chronicle` 判定同族时用的是同一个函数。
                    let victim = chronicle.sites().iter().find(|s| s.id == record.site);
                    let attacker = chronicle.sites().iter().find(|s| s.id == aggressor);
                    match (victim, attacker) {
                        (Some(victim), Some(attacker)) => {
                            let victim_race =
                                founder_race(cultures, victim.culture, victim.id, seed);
                            let attacker_race =
                                founder_race(cultures, attacker.culture, attacker.id, seed);
                            if victim_race.is_some() && victim_race == attacker_race {
                                same_destroyed += 1;
                            } else {
                                cross_destroyed += 1;
                            }
                        }
                        _ => unresolved += 1,
                    }
                }
                HistoricalEventKind::SettlementConquered(record) => {
                    occupied += 1;
                    // 占领记录自带易主前后两份文化，不需要回查快照。
                    let before = founder_race(
                        cultures,
                        Some(CultureKind::from_index(record.former_culture)),
                        record.site,
                        seed,
                    );
                    let after = founder_race(
                        cultures,
                        Some(CultureKind::from_index(record.new_culture)),
                        record.conqueror,
                        seed,
                    );
                    if before == after {
                        same_occupied += 1;
                    } else {
                        cross_occupied += 1;
                    }
                }
                _ => {}
            }
        }

        let inhabited = chronicle
            .sites()
            .iter()
            .filter(|site| site.status == SettlementStatus::Inhabited)
            .count();
        let ruined = chronicle
            .sites()
            .iter()
            .filter(|site| site.status == SettlementStatus::Ruined)
            .count();
        let population: u32 = chronicle.sites().iter().map(|site| site.population).sum();
        let wars = destroyed + occupied;
        println!(
            "{seed}\t{wars}\t{destroyed}\t{occupied}\t{same_occupied}\t{same_destroyed}\
             \t{cross_occupied}\t{cross_destroyed}\t{unresolved}\t{inhabited}\t{ruined}\t{population}"
        );
    }
}
