//! 据点名册：一座还有人住的据点，住着**谁**。
//!
//! 项目所有者的裁决把这件事的边界画得很死：
//!
//! > 「有 NPC 的地方必然存在一个据点，不然 NPC 上哪吃饭睡觉呢。」
//! > 「而且要确保 NPC 在未探索区域也能正常运作。」
//! > 「这个可以参考矮人要塞的例子。」
//!
//! 本模块是前两句在代码里的落点，第三句是它的方法论。
//!
//! # 一、形状：默认派生，只存偏差（ADR 0009）
//!
//! 一座据点的人口是历史推演的产物（[`ll_world::chronicle`]），而历史
//! 推演本身是种子的纯函数、**不进存档**。本模块把这条纪律往下推一层：
//!
//! ```text
//! 派生：据点 S 人口 P  →  P 个 NPC 的身份（种族 / 职业 / 名册序号）
//!                        由 (world_seed, S.id, 序号) 完全确定
//! 存偏差：只有真的被物化过的那些据点，其 NPC 才作为 Agent 进存档
//! ```
//!
//! 「未探索区域的 NPC 正常运作」因此不需要任何后台推进：**他们根本不
//! 需要实体化**。一座玩家从没走近过的村子，它有多少人、各自是干什么的，
//! 随时可以由 [`settlement_roster`] 当场算出来（同一颗种子逐位相同），
//! 而世界状态里一个字节都不占——这正是矮人要塞对离场文明单位的做法：
//! 抽象地存在，直到你真的遇见他们。
//!
//! # 二、「重复生成」这个问题在结构上消失了吗：一半是，一半不是
//!
//! **是的那一半**：名册派生本身是纯函数，算多少次都是同一份名册，不存在
//! 「第二次算出不一样的一批人」。
//!
//! **不是的那一半，也是本批次真正要解决的那个问题**：`Agent` 一旦被
//! 物化就进了 `WorldState::actors`，也就进了存档——而它此后会被玩家改变
//! （被杀、被抢、走开）。若「哪些据点已经物化过」这件事本身不记下来，
//! 区块被淘汰再加载时就会照着同一份名册**再生成一批**，把玩家杀掉的人
//! 原样复活。
//!
//! 记这件事需要的最小状态是**一份已物化据点的 id 集合**
//! （[`ll_world::state::WorldState::materialized_settlements`]），
//! 不是一份逐 NPC 的偏差表。理由是逐 NPC 偏差表要先回答一个本批次答不上
//! 的问题：**派生出来的那个人与存档里那个 `Agent` 之间的稳定身份是什么**。
//! `Agent` 上没有「我是 S 号据点名册里的第 7 个」这样的字段，加一个就是
//! 又一次 `WorldState::hash()` 改动 + 存档 remap；而加了之后，读档路径
//! 还要每次重跑一遍派生、逐条与存档比对、把差异合并回去——**换来的能力
//! 与「据点 id 集合」完全相同**（两者都只需要回答「这座据点该不该再生成
//! 一批人」）。ADR 0021 的判据在这里给出的是「不建」。
//!
//! 代价如实标注：已物化的据点从此**不再随人口变化**。一座村子被玩家屠尽
//! 之后不会有新人搬进来，因为「搬进来」需要的是一套据点的运行期演化，而
//! 那是独立一批的工作，不是这一批顺手做半个。
//!
//! # 三、职业是 `ClassDef`，不是一套平行类型
//!
//! `knowledge/design/settlements-structures-and-npc-spawning.md` 已经裁定
//! 「NPC 职业与玩家职业是同一个东西」。本模块因此只**查**
//! [`crate::class::ClassTable`]，一个新注册表都不建；「猎户 / 屠夫 / 农夫 /
//! 据点管理者 / 民兵 / 铁匠」这几条是**内容**
//! （`mods/lostland/classes.json5`），与「战士 / 法师 / 游侠 / 卫兵」走
//! 同一个注册通道。
//!
//! # 四、查不到就是查不到（ADR 0015）
//!
//! [`SettlementRoles`] 的每一项都是 `Option<ContentIndex>`：某条职业内容
//! 没被装载（第三方 mod 组合掉了本体、或本体内容文件被改坏）时，那一档
//! 不参与抽取，而不是 panic、也不是凭空 intern 一条出来。全部落空时名册
//! 里的人一律带 `ContentIndex::default()`（「尚无职业」的既有诚实表达，
//! 与 `ll_game::world::build_player_agent` 对玩家的处理一致）。
//!
//! # 五、确定性（C3 / C5）
//!
//! 随机全部来自 [`DetRng::for_entity`]，三元组是
//! `(world_seed, ROSTER_STREAM_ID, 据点 id × MAX_ROSTER + 名册序号)`——
//! 形状照抄已落地的 [`ll_world::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]。
//! 全模块不含任何 `HashMap`/`HashSet`，权重表是定长数组，遍历顺序由下标
//! 决定。

use std::collections::BTreeMap;

use ll_core::ident::{ContentIndex, NamespacedId, WorldId};
use ll_core::rng::DetRng;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_sim::item::{ItemCatalog, equip_mask_of, outfit_from_inventory};
use ll_world::entity::{Agent, BaseStats};
use ll_world::item::{EquipSlot, ItemStack};
use ll_world::resource::ResourceKind;
use ll_world::settlement::{SettlementSite, SettlementStatus};
use ll_world::space::{Space, ZoneCoord};

use crate::class::ClassTable;
use crate::race::{RaceTable, starting_inventory};
use crate::registry::Registry;

/// 名册派生所用的随机流编号——与据点建筑铺设
/// （[`ll_world::settlement::SETTLEMENT_LAYOUT_STREAM_ID`]）、历史推演
/// （[`ll_world::chronicle::CHRONICLE_STREAM_ID`]）各自分开：改动名册
/// 抽法不会连带改掉房子怎么铺，反之亦然。
pub const ROSTER_STREAM_ID: u64 = 0x004E_5043_5F52_0001;

/// 一座据点最多派生（因而最多物化）多少个 NPC。
///
/// # 为什么需要一个上界，取值为什么是这个
///
/// 实测三个种子共 788 座据点，人口中位数 31、**最大 175**
/// （见 [`ll_world::settlement`] 模块文档「实测」一节）。而 `Agent` 是
/// 厚层实体，其类型文档写明「数百个，有界」——把一座首邑的 175 个人全部
/// 物化，光一座据点就吃掉那个预算的一大半，玩家走过三四座就再没有余量
/// 留给怪物与随从。
///
/// 取 24：一座村子里同时住着二十来个活人，在传统 roguelike 的视野
/// （`BASE_SIGHT_RADIUS` 12 格）下已经是「一眼看不完」的量级；玩家把
/// 一片大陆上的四五座据点都逛遍，总量仍在一百出头，留得住余量。
///
/// **这个上界只截断物化，不篡改人口**：[`SettlementSite::population`]
/// 仍然是历史算出来的那个数，编年史、承载力、覆灭判定读的都还是它。名册
/// 是「这座村子里你能真的遇见谁」的那一层，不是人口普查。
pub const MAX_ROSTER: u32 = 24;

/// 「据点管理者」在名册里的固定序号——每座还有人住的据点恰好一位。
const STEWARD_INDEX: u32 = 0;

/// 每多少个居民配一名守卫（向下取整，另加固定的一名）。
///
/// 取 8：24 人的满员据点因此有 4 名守卫（1 + 24/8），12 人的小村有 2 名。
/// 比例本身没有更深的依据——它要满足的只是「守卫看得见但不至于比村民多」，
/// 与 [`MAX_ROSTER`] 一样属于手感取舍，不影响任何正确性。
const RESIDENTS_PER_GUARD: u32 = 8;

/// 资源画像第一名给对应档位的权重加成，见
/// [`SettlementRoles::commoner_weights`] 文档。
const PRIMARY_RESOURCE_BONUS: u32 = 9;

/// 资源画像第二名的加成——明显小于第一名，让「主业」与「副业」在名册
/// 上真的分得开。
const SECONDARY_RESOURCE_BONUS: u32 = 3;

/// 一条「可抽取的档位」：内容索引 + 权重。
///
/// 权重为 0 或索引为 `None` 的档位不参与抽取——前者是取值的选择，后者是
/// 「这条内容没装载」（ADR 0015，见模块文档四节）。
#[derive(Debug, Clone, Copy)]
struct WeightedSlot {
    content: Option<ContentIndex>,
    weight: u32,
}

/// 本模块按名字引用的那几条职业内容——同时也是
/// `mods/lostland/classes.json5` 必须注册哪几条的清单。
///
/// 抽成常量而不是把字符串散在 [`SettlementRoles::resolve`] 里，理由同
/// [`crate::class`] 的 `BASE_CLASS_IDS`：集成测试要按同一份清单核对内容
/// 真的注册了它们，两处各写一份字面量迟早会分叉。
pub const SETTLEMENT_CLASS_IDS: [&str; 7] = [
    "lostland:steward",
    "lostland:guard",
    "lostland:militia",
    "lostland:farmer",
    "lostland:hunter",
    "lostland:butcher",
    "lostland:blacksmith",
];

/// 本模块按名字引用的那几条资源内容（`mods/lostland/resources.json5`）。
///
/// **水源不在其中**，见 [`SettlementRoles::commoner_weights`] 文档末尾
/// 那条如实标注。
pub const SETTLEMENT_RESOURCE_IDS: [&str; 3] =
    ["lostland:farmland", "lostland:timber", "lostland:iron_vein"];

/// 本模块按名字引用的那几条种族内容（`mods/lostland/races.json5`）。
pub const SETTLEMENT_RACE_IDS: [&str; 3] = ["lostland:human", "lostland:dwarf", "lostland:elf"];

/// 本模块认得的那几条据点职业，以及它们各自与哪种资源相配。
///
/// 全部字段是 `Option`：本结构体由 [`Self::resolve`] 从注册表**查**出来，
/// 查不到就是 `None`（ADR 0015，见模块文档四节）。
#[derive(Debug, Clone)]
pub struct SettlementRoles {
    /// 据点管理者——每座据点恰好一位（名册序号 0）。
    pub steward: Option<ContentIndex>,
    /// 守卫（`lostland:guard`）——按 [`RESIDENTS_PER_GUARD`] 配额。
    ///
    /// **这一条是本模块存在之前就有的内容**，而且它此前是一条真实的
    /// 悬空引用：[`crate::native_behavior`] 的卫兵那棵树第一句就问
    /// 「这个实体是不是 `lostland:guard`」，而全仓库没有任何路径生成过
    /// 带这个职业的实体，那个分支因此恒为假。本模块是它第一次真的可能
    /// 成立的地方。
    pub guard: Option<ContentIndex>,
    /// 民兵——平时务农、战时拿起矛的那一类，无资源亲和。
    pub militia: Option<ContentIndex>,
    /// 农夫——与良田相配。
    pub farmer: Option<ContentIndex>,
    /// 猎户——与木材（林地）相配。
    pub hunter: Option<ContentIndex>,
    /// 屠夫——无资源亲和：屠夫跟着人走，不跟着地走。
    pub butcher: Option<ContentIndex>,
    /// 铁匠——与铁矿相配。
    pub blacksmith: Option<ContentIndex>,

    /// 良田的资源索引，查不到时为 `None`（那一条亲和恒不成立）。
    farmland: Option<ResourceKind>,
    /// 木材的资源索引。
    timber: Option<ResourceKind>,
    /// 铁矿的资源索引。
    iron: Option<ResourceKind>,

    /// 本体三族的索引，按 `[人类, 矮人, 精灵]` 排列。
    races: [Option<ContentIndex>; 3],
}

impl SettlementRoles {
    /// 从注册表解析出本模块要用的那几条内容。
    ///
    /// **只查，不注册**（与 [`crate::class::resolve_base_classes`] 同一条
    /// 纪律，也与 [`crate::native_behavior`] 内部那个 `lookup` 逐字同形）：
    /// 决策层不该凭空造出内容（ADR 0015）。查不到的那一条留 `None`，对应
    /// 的档位从此不参与抽取。
    ///
    /// `classes` 用来做一次「这个索引真的是一条职业吗」的确认：注册表
    /// 里存在同名标识符不等于它被定义成了职业（`ContentIndex` 是全局
    /// 号段，地形/物品/技能共用同一个 `Interner`）。
    pub fn resolve(registry: &Registry, classes: &ClassTable) -> Self {
        let class_of = |id: &str| -> Option<ContentIndex> {
            let index = lookup(registry, id)?;
            classes.is_defined(index).then_some(index)
        };
        SettlementRoles {
            steward: class_of(SETTLEMENT_CLASS_IDS[0]),
            guard: class_of(SETTLEMENT_CLASS_IDS[1]),
            militia: class_of(SETTLEMENT_CLASS_IDS[2]),
            farmer: class_of(SETTLEMENT_CLASS_IDS[3]),
            hunter: class_of(SETTLEMENT_CLASS_IDS[4]),
            butcher: class_of(SETTLEMENT_CLASS_IDS[5]),
            blacksmith: class_of(SETTLEMENT_CLASS_IDS[6]),
            farmland: lookup(registry, SETTLEMENT_RESOURCE_IDS[0]).map(ResourceKind::from_index),
            timber: lookup(registry, SETTLEMENT_RESOURCE_IDS[1]).map(ResourceKind::from_index),
            iron: lookup(registry, SETTLEMENT_RESOURCE_IDS[2]).map(ResourceKind::from_index),
            races: [
                lookup(registry, SETTLEMENT_RACE_IDS[0]),
                lookup(registry, SETTLEMENT_RACE_IDS[1]),
                lookup(registry, SETTLEMENT_RACE_IDS[2]),
            ],
        }
    }

    /// 「普通居民」那几档的权重表，已按这座据点的资源画像调整过。
    ///
    /// # 基础权重与资源加成怎么定
    ///
    /// 基础权重回答的是「一座什么资源都不突出的村子里，这几种人各占
    /// 多少」：农夫最多（谁都得吃饭）、猎户次之、民兵再次、屠夫与铁匠
    /// 各一份（一座村子有一个就够了）。
    ///
    /// 资源加成回答的是项目所有者要的那条：「守着铁矿的据点该有铁匠，
    /// 守着良田的该有农夫」。加成挂在
    /// [`SettlementSite::resource_profile`] 的两个名次上，第一名
    /// [`PRIMARY_RESOURCE_BONUS`]（9）、第二名
    /// [`SECONDARY_RESOURCE_BONUS`]（3）——**第一名的加成大于全部基础
    /// 权重之和（12）的一半**，这是「矿城真的以铁匠为主」而不是「矿城里
    /// 铁匠稍微多一点」的来源。
    ///
    /// 水源（`lostland:fresh_water`）**不出现在这里，这是如实标注不是
    /// 遗漏**：本批次没有渔夫/水匠这类职业，硬把水源挂到某个现有职业上
    /// （挂给屠夫？挂给农夫？）只会是一次没有内容依据的编造。水源因此
    /// 当前不改变职业分布——真要它有后果，落点是新增一条职业内容，不是
    /// 在这里改一个数。
    ///
    /// **这条留白比听起来大，实测数据在此**（种子 20260826，本体默认
    /// 布局，242 座还有人住的据点，按第一名资源分组的职业占比）：
    ///
    /// | 主资源 | 据点数 | 农夫 | 猎户 | 铁匠 |
    /// |---|---|---|---|---|
    /// | 良田 | 10 | **44.4%** | 19.7% | 4.5% |
    /// | 木材 | 104 | 23.9% | **40.4%** | 4.3% |
    /// | 铁矿 | 12 | 13.6% | 19.3% | **36.4%** |
    /// | 水源 | 116 | 34.5% | 21.6% | 5.4% |
    ///
    /// 前三行是「资源真的改变了职业分布」的直接证据（各自的对口职业
    /// 都跳到四成上下）。第四行是这条留白的代价：**近一半的据点主资源
    /// 是水源**，它们拿到的是没有任何倾向的基础分布。要不要为此新增一条
    /// 渔夫职业，是内容裁定，不是代码问题。
    fn commoner_weights(&self, site: &SettlementSite) -> [WeightedSlot; 5] {
        let mut slots = [
            WeightedSlot {
                content: self.farmer,
                weight: 5,
            },
            WeightedSlot {
                content: self.hunter,
                weight: 3,
            },
            WeightedSlot {
                content: self.militia,
                weight: 2,
            },
            WeightedSlot {
                content: self.butcher,
                weight: 1,
            },
            WeightedSlot {
                content: self.blacksmith,
                weight: 1,
            },
        ];
        // 逐档比对而不是查一张「资源 → 职业」的表：档位只有五个、亲和
        // 只有三条，一张表反而要多维护一份下标对应关系，而任何 map 容器
        // 都会把约束 C5 拖进来。
        self.apply_affinity(
            site,
            &mut slots,
            [(self.farmland, 0), (self.timber, 1), (self.iron, 4)],
        );
        slots
    }

    /// 种族权重：与职业同一套「基础 + 资源亲和」手法。
    ///
    /// 铁矿抬矮人、木材抬精灵、良田抬人类——刻板，但正是刻板才让玩家
    /// 走进一座矿城时**看得出来**这是一座矿城。基础权重三族相同：一座
    /// 什么都不突出的村子不该有种族倾向。
    fn race_weights(&self, site: &SettlementSite) -> [WeightedSlot; 3] {
        let mut slots = [
            WeightedSlot {
                content: self.races[0],
                weight: 4,
            },
            WeightedSlot {
                content: self.races[1],
                weight: 4,
            },
            WeightedSlot {
                content: self.races[2],
                weight: 4,
            },
        ];
        self.apply_affinity(
            site,
            &mut slots,
            [(self.farmland, 0), (self.iron, 1), (self.timber, 2)],
        );
        slots
    }

    /// 把资源画像的两个名次折算成权重加成，写进 `slots`。
    ///
    /// `affinity` 是「哪种资源抬第几档」的对应表，定长三条、按下标遍历，
    /// 不涉及任何哈希容器（约束 C5）。
    fn apply_affinity(
        &self,
        site: &SettlementSite,
        slots: &mut [WeightedSlot],
        affinity: [(Option<ResourceKind>, usize); 3],
    ) {
        let _ = self;
        for (rank, entry) in site.resource_profile.iter().enumerate() {
            if entry.is_none() {
                continue;
            }
            let bonus = if rank == 0 {
                PRIMARY_RESOURCE_BONUS
            } else {
                SECONDARY_RESOURCE_BONUS
            };
            for (kind, slot) in affinity {
                if kind.is_some() && kind == *entry {
                    slots[slot].weight = slots[slot].weight.saturating_add(bonus);
                }
            }
        }
    }
}

/// 查一个已知字符串对应的内容索引；没注册就是 `None`（**不 intern**
/// ——ADR 0015，与 [`crate::native_behavior`] 内部那个同名帮手逐字同形）。
fn lookup(registry: &Registry, id: &str) -> Option<ContentIndex> {
    let parsed = NamespacedId::parse(id).ok()?;
    registry.get(&parsed)
}

/// 一个 NPC 的**派生身份**：由种子与据点完全确定，不进存档。
///
/// 这不是一个 [`Agent`]——它没有位置、没有血量、没有背包，那些是物化那
/// 一刻才产生的东西（[`build_npc_agent`]）。把两者分开，「未探索区域的
/// NPC」才谈得上「不需要实体化也存在」：一份 `NpcProfile` 随时可以由
/// [`settlement_roster`] 现算出来，回答「那座村子里有几个铁匠」这类问题
/// 不需要在世界状态里放任何东西。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NpcProfile {
    /// 他住在哪座据点——[`SettlementSite::id`]。
    ///
    /// 「有 NPC 的地方必然存在一个据点」这条裁定在类型上的表达：这个
    /// 字段没有 `Option`，本模块也没有任何不带据点就产出 `NpcProfile`
    /// 的入口。
    pub home: WorldId,
    /// 在这座据点名册里的序号（`0..MAX_ROSTER`）。与 `home` 一起构成
    /// 这个人在派生世界里的稳定身份。
    pub roster_index: u32,
    /// 种族，指向注册表；全部种族内容都没装载时为
    /// `ContentIndex::default()`。
    pub race: ContentIndex,
    /// 职业，指向 [`crate::class::ClassTable`]；对应的职业内容没装载时
    /// 为 `ContentIndex::default()`（「尚无职业」的既有诚实表达）。
    pub profession: ContentIndex,
}

/// 派生一座据点的完整名册。
///
/// **纯函数**：同一 `(world_seed, site)` 恒产出逐位相同的结果，不读也
/// 不写任何世界状态。废墟（[`SettlementStatus::Ruined`]）与零人口据点
/// 产出空名册——「有 NPC 的地方必然存在一个（还有人住的）据点」。
///
/// 名册长度是 `min(site.population, MAX_ROSTER)`，见 [`MAX_ROSTER`] 文档
/// 「这个上界只截断物化，不篡改人口」一节。
pub fn settlement_roster(
    site: &SettlementSite,
    roles: &SettlementRoles,
    world_seed: u64,
) -> Vec<NpcProfile> {
    if site.status != SettlementStatus::Inhabited {
        return Vec::new();
    }
    let residents = site.population.min(MAX_ROSTER);
    if residents == 0 {
        return Vec::new();
    }
    let guards = 1 + residents / RESIDENTS_PER_GUARD;
    let commoners = roles.commoner_weights(site);
    let races = roles.race_weights(site);

    let mut roster = Vec::with_capacity(residents as usize);
    for index in 0..residents {
        let mut rng = roster_rng(world_seed, site.id, index);
        // 抽取顺序（先种族后职业）本身是这条流的一部分：调换顺序会让
        // 同一颗种子产出另一份名册。改动这里等于改动世界，不是重构。
        let race = pick(&races, &mut rng).unwrap_or_default();
        let profession = if index == STEWARD_INDEX {
            roles.steward
        } else if index <= guards {
            roles.guard
        } else {
            pick(&commoners, &mut rng)
        };
        roster.push(NpcProfile {
            home: site.id,
            roster_index: index,
            race,
            profession: profession.unwrap_or_default(),
        });
    }
    roster
}

/// 名册第 `index` 号那一位专属的随机流（C3）。
///
/// 三元组的第三项是 `据点 id × MAX_ROSTER + 序号`——与
/// [`ll_world::settlement::stamp_settlement`] 为每栋建筑派生流时用的
/// `site.id × MAX_BUILDINGS + building` 逐字同形，保证同一座据点的不同
/// 人、不同据点的同一序号，都落在互不重叠的流上。
fn roster_rng(world_seed: u64, site: WorldId, index: u32) -> DetRng {
    DetRng::for_entity(
        world_seed,
        ROSTER_STREAM_ID,
        u64::from(site.get()) * u64::from(MAX_ROSTER) + u64::from(index),
    )
}

/// 按权重抽一档。全部档位都不可用（索引为 `None` 或权重为 0）时返回
/// `None`，**并且一个随机数都不取**——空抽取不该悄悄推进随机流。
fn pick(slots: &[WeightedSlot], rng: &mut DetRng) -> Option<ContentIndex> {
    let total: u64 = slots
        .iter()
        .filter(|slot| slot.content.is_some())
        .map(|slot| u64::from(slot.weight))
        .sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.gen_range(total);
    for slot in slots {
        let Some(content) = slot.content else {
            continue;
        };
        let weight = u64::from(slot.weight);
        if roll < weight {
            return Some(content);
        }
        roll -= weight;
    }
    // 理论不可达：`roll < total` 而循环恰好减掉了全部权重之和。退回第一
    // 个可用档位而不是 panic（规格 §10.2「降级而非崩溃」）。
    slots.iter().find_map(|slot| slot.content)
}

/// 物化一个 NPC 需要的、与「哪一位」无关的那一组输入。
///
/// 打包成结构体而不是继续往参数表上加，理由同
/// [`ll_world::settlement::StampContext`]：这几项恒一起出现，散着传只会
/// 让调用点更容易漏配，也会撞上 `clippy::too_many_arguments`。
pub struct MaterializeContext<'a> {
    /// 种族表——出生携带物品与属性修正都从这里查。
    pub races: &'a RaceTable,
    /// 物品目录——出生装备定耐久初值、以及穿戴决策查装备掩码。
    pub items: &'a dyn ItemCatalog,
    /// 地表空间层属性索引，写进 [`Agent::current_space`]。
    pub surface_profile: ContentIndex,
    /// 这一刻的世界时刻：`spawned_at` 与 `next_action_at` 都取它。
    pub now: Tick,
}

/// 把一份派生身份物化成一个真正被模拟的 [`Agent`]。
///
/// # 属性：与玩家角色同一条烘焙路径
///
/// `stats` 走 [`ll_sim::character::bake_race_stat_modifiers`]，与
/// `ll_game::world::build_player_agent` 是同一个函数——NPC 与玩家在数值
/// 上不是两套东西（`knowledge/design/race-system.md`「二、属性修正」的
/// 烘焙语义对两者一视同仁）。
///
/// # 装备：NPC 自行决策（项目所有者裁定）
///
/// > 「这个如果是 NPC 就是根据 NPC 自行决策，人的话就等玩家自己装备吧」
///
/// 玩家那一半已经落地（出生装备只进背包）。NPC 这一半的落点是
/// [`outfit_decision`]——它在 [`ll_sim::item::outfit_from_inventory`]
/// **之上**加了一层「这个 NPC 会挑哪件穿」，而不是把背包里能穿的一股脑
/// 全套上。
pub fn build_npc_agent(
    profile: &NpcProfile,
    pos: TorusPos,
    zone: ZoneCoord,
    roles: &SettlementRoles,
    ctx: &MaterializeContext<'_>,
) -> Agent {
    let carried = ctx
        .races
        .get(profile.race)
        .map(|view| starting_inventory(&view, ctx.items))
        .unwrap_or_default();
    let (equipment, inventory) = outfit_decision(profile, roles, carried, ctx.items);
    let stats =
        ll_sim::character::bake_race_stat_modifiers(BaseStats::BASELINE, profile.race, ctx.races);
    Agent {
        pos,
        stats,
        // 与玩家同一条纪律（见 `ll_game::world::spawn_player` 对
        // `next_action_at` 的注释）：取当前世界时钟而不是 `Tick(0)`，
        // 否则这个 NPC 一进时间轴就会把世界时钟倒拨回午夜。
        next_action_at: ctx.now,
        health: Agent::STARTING_HEALTH,
        affiliations: Vec::new(),
        wallet: 0,
        profession: profile.profession,
        goals: Vec::new(),
        race: profile.race,
        mana: Agent::STARTING_MANA,
        stamina: Agent::STARTING_STAMINA,
        resource_pools: BTreeMap::new(),
        spent_slots: BTreeMap::new(),
        inventory,
        equipment,
        resting: None,
        unlocked_skills: Vec::new(),
        known_recipes: Vec::new(),
        identified_items: Vec::new(),
        skill_cooldowns: BTreeMap::new(),
        subclasses: Vec::new(),
        subclasses_ever_granted: Vec::new(),
        active_stat_modifiers: BTreeMap::new(),
        current_space: Space::surface(zone, ctx.surface_profile),
        mod_state: BTreeMap::new(),
        creature_kind: None,
        spawned_at: ctx.now,
        remembered_id: None,
        level: Agent::STARTING_LEVEL,
        experience: 0,
        xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
        unspent_attribute_points: 0,
        unspent_skill_points: 0,
        stealthed: false,
    }
}

/// 「这个 NPC 会挑哪件穿」——项目所有者裁定的那一层决策，架在
/// [`ll_sim::item::outfit_from_inventory`] 之上。
///
/// 返回 `(装备栏, 留在背包里的)`，与被它包住的那个函数同一个形状。
///
/// # 规则：拿武器的只有拿武器的职业
///
/// - **武装职业**（守卫、民兵）：能穿的全部穿上——他们的工作就是站在
///   那里让人看见自己带着家伙。
/// - **其余职业**：占用手部槽位（[`EquipSlot::MAIN_HAND`] /
///   [`EquipSlot::OFF_HAND`]）的东西**留在背包**，其余照穿。一个农夫
///   身上有件外衣是常态，举着一把剑站在田里不是。
///
/// # 为什么规则只有这一条
///
/// 因为再多一条就需要 `ItemDef` 上当前不存在的信息。「铁匠该穿皮围裙」
/// 要物品带得出「这是围裙」这个语义——本仓库的物品今天只有堆叠上限、
/// 装备掩码、属性加成、耐久四样，没有任何一样答得上来。按掩码分「手上
/// 拿的 vs 身上穿的」是**用现有数据真答得出来的最强判断**，再往下就是
/// 编造。
///
/// # 一处如实标注：本体内容当前测不出这条规则的差别
///
/// 本体三族的 `starting_items`（`mods/lostland/races.json5`）是亚麻衬衫、
/// 羊毛手套、骨针这类东西，**没有一件占用手部槽位**。因此在本体默认内容
/// 下，两条分支产出的结果恰好相同。这不是规则没落地——是内容还没给它
/// 可分辨的输入；本模块的单元测试
/// `非武装职业把手部装备留在背包而武装职业穿上` 用一份合成物品目录把
/// 两条分支各走一遍，守住这条规则本身。
fn outfit_decision(
    profile: &NpcProfile,
    roles: &SettlementRoles,
    carried: Vec<ItemStack>,
    items: &dyn ItemCatalog,
) -> (BTreeMap<EquipSlot, ItemStack>, Vec<ItemStack>) {
    let profession = Some(profile.profession);
    let armed = (roles.guard.is_some() && profession == roles.guard)
        || (roles.militia.is_some() && profession == roles.militia);
    if armed {
        return outfit_from_inventory(carried, items);
    }
    let hands = EquipSlot::MAIN_HAND
        .mask()
        .union(EquipSlot::OFF_HAND.mask());
    let mut wearable = Vec::new();
    let mut stowed = Vec::new();
    for stack in carried {
        if equip_mask_of(stack.def, items).intersects(hands) {
            stowed.push(stack);
        } else {
            wearable.push(stack);
        }
    }
    let (equipment, mut rest) = outfit_from_inventory(wearable, items);
    rest.append(&mut stowed);
    (equipment, rest)
}

#[cfg(test)]
mod tests {
    use ll_core::ident::Interner;
    use ll_core::torus::TorusSize;
    use ll_sim::item::ItemRule;
    use ll_world::item::SlotMask;
    use ll_world::settlement::SITE_RESOURCE_SLOTS;
    use ll_world::zone::ZoneLayout;

    use ll_sim::combat::Penetration;
    use ll_world::entity::AttributeKind;
    use ll_world::item::WearChannels;

    use crate::class::ClassAttrs;

    use super::*;

    /// 一张现造的、与本体内容无关的角色扮演表：七条职业、三个种族、
    /// 三种资源，全部用真实的本体 id 字符串（[`SettlementRoles::resolve`]
    /// 认的正是它们），但由测试自己注册——本 crate 的单元测试不装载
    /// 本体内容文件，那是 `crates/ll-mod/tests/` 的集成测试的事。
    fn sample_roles() -> (SettlementRoles, Registry, [ContentIndex; 3]) {
        let mut registry = Registry::new();
        let mut classes = ClassTable::new();
        for id in SETTLEMENT_CLASS_IDS {
            let index = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
            classes
                .define(
                    index,
                    ClassAttrs {
                        display_name_key: NamespacedId::parse("lostland:x").expect("合法标识符"),
                        primary_attribute: AttributeKind::Strength,
                        traits: Vec::new(),
                    },
                )
                .expect("首次定义");
        }
        for id in SETTLEMENT_RACE_IDS {
            registry.intern(NamespacedId::parse(id).expect("合法标识符"));
        }
        let mut resources = [ContentIndex::default(); 3];
        for (slot, id) in SETTLEMENT_RESOURCE_IDS.iter().enumerate() {
            resources[slot] = registry.intern(NamespacedId::parse(id).expect("合法标识符"));
        }
        let roles = SettlementRoles::resolve(&registry, &classes);
        (roles, registry, resources)
    }

    /// 一座人口 `population`、资源画像为 `profile` 的据点。
    fn site(
        population: u32,
        profile: [Option<ResourceKind>; SITE_RESOURCE_SLOTS],
    ) -> SettlementSite {
        let layout = ZoneLayout::new(48, TorusSize::new(2, 2).expect("2x2 合法"))
            .expect("48 满足全部对齐与跨度约束");
        let size = layout.tile_size();
        let mut counter = 3u32;
        SettlementSite {
            id: WorldId::next(&mut counter),
            zone: layout.tile_to_zone(size.wrap(10, 10)).0,
            anchor: size.wrap(10, 10),
            status: SettlementStatus::Inhabited,
            founded_epoch: 0,
            abandoned_epoch: None,
            population,
            peak_population: population,
            building_count: 1 + population / 4,
            resource_profile: profile,
        }
    }

    #[test]
    fn 同一颗种子同一座据点派生出逐位相同的名册() {
        // Arrange
        let (roles, _registry, _resources) = sample_roles();
        let site = site(20, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let first = settlement_roster(&site, &roles, 0xABCD_1234);
        let second = settlement_roster(&site, &roles, 0xABCD_1234);

        // Assert
        assert_eq!(first, second);
        assert_eq!(first.len(), 20);
    }

    #[test]
    fn 换一颗种子名册就不同() {
        // Arrange
        let (roles, _registry, _resources) = sample_roles();
        let site = site(24, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let first = settlement_roster(&site, &roles, 1);
        let second = settlement_roster(&site, &roles, 2);

        // Assert
        assert_ne!(first, second);
    }

    #[test]
    fn 废墟派生出空名册() {
        // Arrange
        let (roles, _registry, _resources) = sample_roles();
        let mut ruin = site(0, [None; SITE_RESOURCE_SLOTS]);
        ruin.status = SettlementStatus::Ruined;
        ruin.peak_population = 90;

        // Act
        let roster = settlement_roster(&ruin, &roles, 7);

        // Assert
        assert!(roster.is_empty());
    }

    #[test]
    fn 名册长度被max_roster截断而人口本身不变() {
        // Arrange
        let (roles, _registry, _resources) = sample_roles();
        let big = site(175, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let roster = settlement_roster(&big, &roles, 11);

        // Assert
        assert_eq!(roster.len(), MAX_ROSTER as usize);
        assert_eq!(big.population, 175);
    }

    #[test]
    fn 每座还有人住的据点恰好一位据点管理者() {
        // Arrange
        let (roles, _registry, _resources) = sample_roles();
        let village = site(15, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let roster = settlement_roster(&village, &roles, 5);
        let stewards = roster
            .iter()
            .filter(|npc| Some(npc.profession) == roles.steward)
            .count();

        // Assert
        assert_eq!(stewards, 1);
        assert_eq!(
            roster[0].profession,
            roles.steward.expect("夹具注册了管理者")
        );
    }

    #[test]
    fn 守着铁矿的据点铁匠比守着良田的多() {
        // Arrange
        let (roles, _registry, resources) = sample_roles();
        let farmland = ResourceKind::from_index(resources[0]);
        let iron = ResourceKind::from_index(resources[2]);
        let mining = site(24, [Some(iron), None]);
        let farming = site(24, [Some(farmland), None]);

        // Act：同一颗种子、同一个人口，唯一的差别是资源画像。
        let mining_smiths = count_of(&settlement_roster(&mining, &roles, 99), roles.blacksmith);
        let farming_smiths = count_of(&settlement_roster(&farming, &roles, 99), roles.blacksmith);
        let mining_farmers = count_of(&settlement_roster(&mining, &roles, 99), roles.farmer);
        let farming_farmers = count_of(&settlement_roster(&farming, &roles, 99), roles.farmer);

        // Assert
        assert!(
            mining_smiths > farming_smiths,
            "矿城的铁匠 {mining_smiths} 应多于农业村的 {farming_smiths}"
        );
        assert!(
            farming_farmers > mining_farmers,
            "农业村的农夫 {farming_farmers} 应多于矿城的 {mining_farmers}"
        );
    }

    fn count_of(roster: &[NpcProfile], class: Option<ContentIndex>) -> usize {
        roster
            .iter()
            .filter(|npc| class.is_some() && Some(npc.profession) == class)
            .count()
    }

    #[test]
    fn 一条职业内容都没装载时名册仍然产出但职业是占位索引() {
        // Arrange：空注册表 + 空职业表——第三方 mod 组合掉本体的情形。
        let registry = Registry::new();
        let classes = ClassTable::new();
        let roles = SettlementRoles::resolve(&registry, &classes);
        let village = site(6, [None; SITE_RESOURCE_SLOTS]);

        // Act
        let roster = settlement_roster(&village, &roles, 3);

        // Assert
        assert_eq!(roster.len(), 6);
        assert!(
            roster
                .iter()
                .all(|npc| npc.profession == ContentIndex::default())
        );
    }

    /// 一份只回答装备掩码的合成物品目录。
    struct MaskCatalog(BTreeMap<ContentIndex, SlotMask>);

    impl ItemCatalog for MaskCatalog {
        fn item(&self, item: ContentIndex) -> Option<ItemRule> {
            self.0.get(&item).map(|mask| ItemRule {
                stack_limit: 1,
                equip_mask: *mask,
                stat_bonuses: Vec::new(),
                use_effect: None,
                penetration: Penetration::NONE,
                max_durability: None,
                wear_channels: WearChannels::default(),
                damage_formula: None,
                damage_category: None,
                rule_modifiers: Vec::new(),
                requires_identification: false,
                study_experience: 0,
                blind_box_pool: Vec::new(),
                taught_recipes: Vec::new(),
            })
        }
    }

    #[test]
    fn 非武装职业把手部装备留在背包而武装职业穿上() {
        // Arrange
        let (roles, _registry, _resources) = sample_roles();
        let mut interner = Interner::new();
        let sword = interner.intern(NamespacedId::parse("testmod:sword").expect("合法标识符"));
        let shirt = interner.intern(NamespacedId::parse("testmod:shirt").expect("合法标识符"));
        let mut masks = BTreeMap::new();
        masks.insert(sword, EquipSlot::MAIN_HAND.mask());
        masks.insert(shirt, EquipSlot::BODY.mask());
        let catalog = MaskCatalog(masks);
        let carried = vec![ItemStack::new(sword, 1), ItemStack::new(shirt, 1)];
        let mut counter = 1u32;
        let home = WorldId::next(&mut counter);
        let farmer = NpcProfile {
            home,
            roster_index: 4,
            race: ContentIndex::default(),
            profession: roles.farmer.expect("夹具注册了农夫"),
        };
        let guard = NpcProfile {
            profession: roles.guard.expect("夹具注册了守卫"),
            ..farmer
        };

        // Act
        let (farmer_worn, farmer_packed) =
            outfit_decision(&farmer, &roles, carried.clone(), &catalog);
        let (guard_worn, _guard_packed) = outfit_decision(&guard, &roles, carried, &catalog);

        // Assert
        assert!(
            !farmer_worn.contains_key(&EquipSlot::MAIN_HAND),
            "农夫不该举着剑"
        );
        assert!(farmer_worn.contains_key(&EquipSlot::BODY), "衣服照穿");
        assert!(
            farmer_packed.iter().any(|stack| stack.def == sword),
            "剑应当留在背包里"
        );
        assert!(
            guard_worn.contains_key(&EquipSlot::MAIN_HAND),
            "守卫该带着家伙"
        );
    }
}
