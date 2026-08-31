//! 势力播种：把编年史**已经在推演**的占领链物化成真正的
//! [`crate::entity::OrgInstance`]。
//!
//! # 这一层不发明任何新机制
//!
//! 编年史（[`crate::chronicle`]）早就在推演据点建立、战争与**占领**：
//! 一条 [`crate::history::SettlementConqueredRecord`] 就是一句「谁统治
//! 谁」——`conqueror` 那座据点所属的势力，从此多统治一座 `site`。
//! 把整部事件日志按发生顺序折叠一遍，落下来的正是项目所有者要的那句
//! 话：**「Faction 应该是真的势力，下属很多据点。」**
//!
//! 本模块因此**没有**任何选址、掷骰或平衡逻辑，它是 `&[HistoricalEvent]`
//! 的一个纯函数（[`seed_factions`]）。
//!
//! # 四条设计裁定（完整论证在
//! `docs/superpowers/plans/2026-08-29-batch14-faction-seeding.md` 二节）
//!
//! 1. **每一次据点建立当场立一个势力**，因此一座活着的据点恒属于且只属于
//!    一个势力，「无势力的活据点」不合法。一座从未打过仗的孤立据点是一个
//!    只有它自己的城邦——它**不是**「拿据点 `WorldId` 冒充势力」（所有者
//!    否掉的那条）：势力有自己独立分配的号、自己的成员表，会随占领长大或
//!    覆灭。
//! 2. **身份不存副本**：势力只记首邑（[`Faction::seat`]），文化、建立者
//!    种族、展示名全部由首邑现算（[`seat_culture`]/[`founder_race_of`]/
//!    [`display_name_key`]）。把文化拷进势力就是本仓库反复出事的那种
//!    「真相源之外的副本」——而文化**会随占领改变**。
//! 3. **据点 → 势力严格一对一**，由 [`FactionTable`] 的倒排索引在类型层
//!    表达：同一座据点出现在两个势力的成员表里是一个**构造错误**
//!    （[`FactionTableError::SiteRuledTwice`]），读档路径也走这条校验。
//! 4. **势力被灭不删除记录**，转成 [`FactionStatus::Fallen`]。三条理由缺
//!    一不可：`OrgInstance::id` 的既有文档写死「永不复用——王朝覆灭后历史
//!    事件仍要能解析回它」；玩家的归属是
//!    `Affiliation { org: OrgRef::Instance(号) }` 而
//!    `ll_content::remap::remap_affiliations` 对 `Instance` **不做重映射**，
//!    条目一消失那个号就指向空气**且没有任何东西会报错**；编年史本来就在
//!    记覆灭。**这一条直接回答「玩家加入的势力被灭了会怎样」：归属仍然
//!    解析得到，解析到的是一个已覆灭的势力——玩家是亡国之人，不是一个
//!    悬空指针。**
//!
//! # 确定性（约束 C3 / C5）
//!
//! - **C5**：全程只有 `Vec` + 二分 + 升序插入，一个 `HashMap`/`HashSet`
//!   都不碰。输入 `events` 本身已按发生顺序（纪元升序，同纪元内按候选点
//!   光栅序）排好，那是编年史既有的确定性顺序。
//! - **C3**：这一折叠里**没有任何随机决策**，它是事件日志的纯函数。本模块
//!   唯一的随机来源是建立者种族，而它走的是既有的
//!   [`crate::culture::founder_race`]（`DetRng::for_entity` +
//!   [`crate::culture::FOUNDER_RACE_STREAM_ID`]）——与 `ll_mod::roster` 排
//!   名册、与编年史判「这是不是一场同族战争」用的是同一条流，因此三处说
//!   的是同一件事。**刻意不为势力另开一条随机流**：硬塞一次掷骰只会给世界
//!   摘要多一个没有语义的自由度（ADR 0021 拦的正是这种为对称而加的东西）。
//!
//! # `WorldId` 从哪来
//!
//! 继续用编年史自己的计数器（[`seed_factions`] 的 `next_world_id` 参数），
//! 与据点号、历史事件号**同一个号段**、同一个 `WorldId::next` 惯例
//! （`knowledge/design/identity-and-ids.md` 三）。两条白送的后果：势力号
//! 与据点号**永不相等**（「拿据点号冒充势力」在号段层面就不可能）；
//! `WorldChronicle::next_world_id()` 自然把势力用掉的号算进去，
//! `ll_game::world` 那句 `world.next_world_id = max(...)` 一个字不用改。

use ll_core::hashing::StateHasher;
use ll_core::ident::{ContentIndex, NamespacedId, WorldId};

use crate::culture::{CultureKind, CultureTable, founder_race};
use crate::entity::OrgInstance;
use crate::history::{HistoricalEvent, HistoricalEventKind, SettlementConqueredRecord};
use crate::settlement::SettlementSite;

/// 一个势力还在不在。
///
/// 只有两态，且**单调**：成员表一旦归零就再也不可能回升（只有据点建立
/// 与占领会增加成员，而据点建立恒创建**新**势力）。因此这个枚举不需要
/// 「复国」那一支——真要有复国，那是另一个机制、另一批。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FactionStatus {
    /// 还统治着至少一座活着的据点。
    Active,
    /// 最后一座据点在这个纪元没了——被铲平，或被别人占走。
    ///
    /// 记纪元而不是只记一个布尔：编年史的其余覆灭记录（
    /// [`crate::history::SettlementAbandonedRecord::epoch`]）都记，
    /// 「这个王朝亡于第几纪元」是同一类问题的同一种答案。
    Fallen {
        /// 第几个纪元。
        epoch: u32,
    },
}

/// 一个势力：一份 [`OrgInstance`] 身份 + 它统治的那些据点。
///
/// # 为什么内嵌 `OrgInstance` 而不是把它的三个字段摊平
///
/// [`OrgInstance`] 是「组织实例」这个概念的类型本体（势力、宗教、行会、
/// 家族共用），`identity-and-ids.md` 二把「mod 定义种类、世界生成造个体」
/// 冻结在那里。摊平等于把 `def`/`authored` 这两个**mod 直接定义具体势力**
/// 那条路（同文档四）要用的字段复制一份，下一个组织类型落地时再复制一份。
/// 内嵌之后 `faction.org` 就是那条路的现成入口。
///
/// 播种出来的势力 `def`/`authored` **恒为 `None`**（纯生成，没有 mod 模板，
/// 也没有 mod 给它起过名）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Faction {
    /// 组织实例身份，见类型文档。
    pub org: OrgInstance,
    /// **首邑**：立国的那座据点。
    ///
    /// 首邑被打没了而势力还有别的据点时，改指**现存成员里 `WorldId` 最小**
    /// 的那座——一条与遍历顺序无关的确定性规则（约束 C5），不是「随便挑
    /// 一个」。势力覆灭（成员归零）时这个字段**不清空**：它是「这个王朝
    /// 起于何处」这条历史，与 [`crate::chronicle`] 对废墟文化「覆灭时不
    /// 清零」的既有取舍逐字同源。
    pub seat: WorldId,
    /// 立国于第几个纪元。
    pub founded_epoch: u32,
    /// 还在不在，见 [`FactionStatus`]。
    pub status: FactionStatus,
    /// 它统治的据点，**升序去重**。
    ///
    /// 势力覆灭后为空。`Vec` 保序、不涉及 `HashMap`/`HashSet` 迭代顺序
    /// （约束 C5），与 [`crate::state::WorldState::materialized_settlements`]
    /// 的既有取舍逐字相同。
    pub members: Vec<WorldId>,
}

impl Faction {
    /// 这个势力的号——[`OrgInstance::id`] 的转发，省得到处写 `.org.id`。
    pub fn id(&self) -> WorldId {
        self.org.id
    }

    /// 还统治着至少一座据点吗。
    pub fn is_active(&self) -> bool {
        matches!(self.status, FactionStatus::Active)
    }
}

/// [`FactionTable`] 的构造校验失败原因。
///
/// 走 ADR 0011 的 `try_from` 模式：这些都是**无上下文**的结构不变式
/// （不需要注册表、不需要世界），因此可以、也必须在反序列化那一刻就拒绝。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactionTableError {
    /// 同一座据点出现在两个势力的成员表里——「据点 → 势力一对一」被破坏。
    SiteRuledTwice {
        /// 被两个势力同时声称统治的那座据点。
        site: WorldId,
    },
    /// 两个势力用了同一个号。
    DuplicateFactionId {
        /// 重复的那个号。
        id: WorldId,
    },
    /// 势力表没有按号升序排列——二分查找依赖这一点。
    NotSortedById,
    /// 某个势力的成员表没有按号升序去重排列。
    MembersNotSorted {
        /// 出问题的那个势力。
        faction: WorldId,
    },
    /// 一个还活着的势力却没有任何成员，或一个已覆灭的势力却还有成员。
    StatusMembersMismatch {
        /// 出问题的那个势力。
        faction: WorldId,
    },
}

impl std::fmt::Display for FactionTableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FactionTableError::SiteRuledTwice { site } => {
                write!(f, "据点 {} 同时被两个势力统治", site.get())
            }
            FactionTableError::DuplicateFactionId { id } => {
                write!(f, "势力号 {} 重复", id.get())
            }
            FactionTableError::NotSortedById => write!(f, "势力表没有按号升序排列"),
            FactionTableError::MembersNotSorted { faction } => {
                write!(f, "势力 {} 的成员表没有升序去重", faction.get())
            }
            FactionTableError::StatusMembersMismatch { faction } => {
                write!(f, "势力 {} 的存续状态与成员数不符", faction.get())
            }
        }
    }
}

/// 全世界的势力表 + 「这座据点归谁」的倒排索引。
///
/// # 为什么倒排索引不进存档
///
/// 它是 `factions` 的纯函数。存两份就要回答「两份对不上时信谁」，而
/// 存档主体走 `postcard`——一份被外部改坏的存档能轻易造出一份自相矛盾的
/// 索引。反序列化时**重算**并顺带跑完整校验（[`FactionTable::rebuild`]），
/// 「一对一」这条不变式因此在读档那一刻就被强制，而不是等到某个查询给出
/// 错误答案。这与 `WorldState` 自己的 `#[serde(try_from = "…Repr")]`
/// 是同一条既有模式（ADR 0011）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "FactionTableRepr")]
pub struct FactionTable {
    /// 按 [`Faction::id`] 升序（也就是分配顺序）。
    factions: Vec<Faction>,
    /// `(据点号, 势力号)`，按据点号升序且唯一——见类型文档。
    #[serde(skip)]
    by_site: Vec<(WorldId, WorldId)>,
}

/// [`FactionTable`] 的线格式中转类型，见该类型文档「为什么倒排索引不进
/// 存档」。
#[derive(serde::Deserialize)]
struct FactionTableRepr {
    factions: Vec<Faction>,
}

impl TryFrom<FactionTableRepr> for FactionTable {
    type Error = FactionTableError;

    fn try_from(repr: FactionTableRepr) -> Result<Self, Self::Error> {
        FactionTable::rebuild(repr.factions)
    }
}

impl Default for FactionTable {
    fn default() -> Self {
        FactionTable::new()
    }
}

impl FactionTable {
    /// 一张空表——这个世界还没有任何势力。
    ///
    /// 合法且常见：空文化表、零纪元推演、绝大多数单元测试的世界都没有
    /// 据点，因此也没有势力（ADR 0015「尚无内容」的既有表达，不是错误）。
    pub fn new() -> FactionTable {
        FactionTable {
            factions: Vec::new(),
            by_site: Vec::new(),
        }
    }

    /// 从一批势力建表：校验全部不变式，并重算倒排索引。
    ///
    /// 这是**唯一**的有内容构造路径（[`seed_factions`] 与反序列化都走
    /// 它），因此「一对一」这条不变式没有旁路。
    pub fn rebuild(factions: Vec<Faction>) -> Result<FactionTable, FactionTableError> {
        let mut by_site: Vec<(WorldId, WorldId)> = Vec::new();
        let mut previous_id: Option<WorldId> = None;
        for faction in &factions {
            let id = faction.id();
            match previous_id {
                Some(before) if before == id => {
                    return Err(FactionTableError::DuplicateFactionId { id });
                }
                Some(before) if before > id => return Err(FactionTableError::NotSortedById),
                _ => {}
            }
            previous_id = Some(id);
            if faction.members.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(FactionTableError::MembersNotSorted { faction: id });
            }
            if faction.is_active() != !faction.members.is_empty() {
                return Err(FactionTableError::StatusMembersMismatch { faction: id });
            }
            for site in &faction.members {
                by_site.push((*site, id));
            }
        }
        by_site.sort_unstable();
        if let Some(pair) = by_site.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(FactionTableError::SiteRuledTwice { site: pair[0].0 });
        }
        Ok(FactionTable { factions, by_site })
    }

    /// 全部势力，按号升序（含已覆灭的——见模块文档裁定 4）。
    pub fn factions(&self) -> &[Faction] {
        &self.factions
    }

    /// 有几个势力（含已覆灭的）。
    pub fn len(&self) -> usize {
        self.factions.len()
    }

    /// 这个世界一个势力都没有吗。
    pub fn is_empty(&self) -> bool {
        self.factions.is_empty()
    }

    /// 按号查一个势力。表按号升序，走二分。
    pub fn get(&self, id: WorldId) -> Option<&Faction> {
        self.factions
            .binary_search_by_key(&id, |faction| faction.id())
            .ok()
            .map(|position| &self.factions[position])
    }

    /// **这座据点归谁**——废墟与从不存在的号返回 `None`。
    ///
    /// 这是对话的「加入」那一支要问的那个问题（`dialogue-system.md` 5.1）：
    /// 玩家跟某座据点的管理者说要加入，加入的是这座据点**所属的势力**，
    /// 不是据点本身。走二分，不碰哈希容器（约束 C5）。
    pub fn faction_of(&self, site: WorldId) -> Option<WorldId> {
        self.by_site
            .binary_search_by_key(&site, |(at, _)| *at)
            .ok()
            .map(|position| self.by_site[position].1)
    }

    /// 混进世界状态哈希（ADR 0022）。
    ///
    /// 只写 `factions`——倒排索引是它的纯函数，写两遍不会多抓到任何东西。
    /// 全程 `Vec` 顺序，不涉及 `HashMap`/`HashSet` 迭代顺序（约束 C5）。
    pub(crate) fn write_hash(&self, hasher: &mut StateHasher) {
        hasher.write_u64(self.factions.len() as u64);
        for faction in &self.factions {
            hasher.write_u64(u64::from(faction.org.id.get()));
            match faction.org.def {
                None => hasher.write_u64(0),
                Some(def) => {
                    hasher.write_u64(1);
                    hasher.write_u64(u64::from(def.get()));
                }
            }
            // `authored` 是一个命名空间标识符；混入它的字符串字节而不是
            // 跳过——「mod 给这个势力起过名」与「没起过」是两个不同的
            // 世界，播种路径今天恒为 `None`，但哈希不该对将来的那条路
            // 视而不见。
            match &faction.org.authored {
                None => hasher.write_u64(0),
                Some(authored) => {
                    hasher.write_u64(1);
                    let text = authored.to_string();
                    hasher.write_u64(text.len() as u64);
                    for byte in text.as_bytes() {
                        hasher.write_u64(u64::from(*byte));
                    }
                }
            }
            hasher.write_u64(u64::from(faction.seat.get()));
            hasher.write_u64(u64::from(faction.founded_epoch));
            match faction.status {
                FactionStatus::Active => hasher.write_u64(0),
                FactionStatus::Fallen { epoch } => {
                    hasher.write_u64(1);
                    hasher.write_u64(u64::from(epoch));
                }
            }
            hasher.write_u64(faction.members.len() as u64);
            for site in &faction.members {
                hasher.write_u64(u64::from(site.get()));
            }
        }
    }
}

/// 首邑那座据点现在信什么文化——**势力的文化就是首邑的文化**，不存副本。
///
/// 见模块文档裁定 2。`sites` 传 [`crate::chronicle::WorldChronicle::sites`]。
/// 查不到（首邑已成废墟被移出快照、或空表）时为 `None`，ADR 0015。
pub fn seat_culture(faction: &Faction, sites: &[SettlementSite]) -> Option<CultureKind> {
    sites
        .iter()
        .find(|site| site.id == faction.seat)
        .and_then(|site| site.culture)
}

/// 这个势力**由哪一族当家**——首邑的文化决定，走
/// [`crate::culture::founder_race`]。
///
/// 与 `ll_mod::roster` 给这座据点排名册、与 [`crate::chronicle`] 判「这是
/// 不是一场同族战争」用的是**同一个函数、同一条随机流**，因此三处说的是
/// 同一件事——这正是本批不给势力另存一个种族字段的理由。
pub fn founder_race_of(
    faction: &Faction,
    sites: &[SettlementSite],
    cultures: &CultureTable,
    world_seed: u64,
) -> Option<ContentIndex> {
    founder_race(
        cultures,
        seat_culture(faction, sites),
        faction.seat,
        world_seed,
    )
}

/// 这个势力的**展示名本地化键**——取首邑文化的
/// [`crate::culture::CultureAttrs::display_name_key`]。
///
/// # 为什么这里没有字符串
///
/// 本模块一个用户可见字面量都不新增（`scripts/ci/check_i18n_strings.py`）。
/// 文化表里那个键已经是唯一真相源，拷一份进 [`Faction`] 就是第二份副本，
/// 而文化会随占领改变。真正的专名（「卡拉克第三王朝」这类）要等
/// [`crate::naming::NamingRules`] 接进文化表——那条今天**全仓库零生产
/// 消费点**，是另一批的事，本批不预支。
pub fn display_name_key(
    faction: &Faction,
    sites: &[SettlementSite],
    cultures: &CultureTable,
) -> Option<NamespacedId> {
    cultures.display_name_key(seat_culture(faction, sites)?)
}

/// 把一部编年史的事件日志折叠成势力表——本模块的全部算法。
///
/// `next_world_id` 是编年史自己的那个计数器，势力号从它继续分配（见模块
/// 文档「`WorldId` 从哪来」）。`events` 必须是编年史产出的原序。
///
/// 三种事件各对应一条规则，完整论证见模块文档与计划文档三节：
///
/// - **建立** → 立一个新势力，成员只有它自己；
/// - **易主** → 把**那一座**据点从旧主搬到新主（占领事件的字面语义只说了
///   一座城换手，没说整个王国易主）；
/// - **覆灭** → 从所属势力里除名。
///
/// 后两条都可能让某个势力的成员归零，那一刻它转
/// [`FactionStatus::Fallen`]。
pub fn seed_factions(events: &[HistoricalEvent], next_world_id: &mut u32) -> FactionTable {
    let mut factions: Vec<Faction> = Vec::new();
    // `(据点号, factions 下标)`，按据点号升序——折叠过程中的可变倒排
    // 索引。用 `Vec` + 二分而不是 `HashMap`：约束 C5。
    let mut owner: Vec<(WorldId, usize)> = Vec::new();

    for event in events {
        match &event.kind {
            HistoricalEventKind::SettlementFounded(record) => {
                found_faction(
                    &mut factions,
                    &mut owner,
                    record.site,
                    record.epoch,
                    next_world_id,
                );
            }
            HistoricalEventKind::SettlementConquered(record) => {
                transfer_site(&mut factions, &mut owner, record);
            }
            HistoricalEventKind::SettlementAbandoned(record) => {
                drop_site(&mut factions, &mut owner, record.site, record.epoch);
            }
            HistoricalEventKind::Kill(_) => {}
        }
    }

    FactionTable::rebuild(factions)
        .expect("折叠过程逐步维护了全部不变式：号升序唯一、成员升序去重、存续状态与成员数一致")
}

/// **建立**那一条规则：立一个新势力，成员只有这座据点自己。
///
/// 势力号从编年史的计数器继续分配——「拿据点号冒充势力」在号段层面
/// 因此不可能发生（模块文档「`WorldId` 从哪来」）。
fn found_faction(
    factions: &mut Vec<Faction>,
    owner: &mut Vec<(WorldId, usize)>,
    site: WorldId,
    epoch: u32,
    next_world_id: &mut u32,
) {
    let position = factions.len();
    factions.push(Faction {
        org: OrgInstance {
            id: WorldId::next(next_world_id),
            def: None,
            authored: None,
        },
        seat: site,
        founded_epoch: epoch,
        status: FactionStatus::Active,
        members: vec![site],
    });
    insert_owner(owner, site, position);
}

/// **易主**那一条规则：把**那一座**据点从旧主搬到征服者的势力。
///
/// 只搬一座，不是整个王国易主——占领事件的字面语义只说了一座城换手
/// （[`crate::history::SettlementConqueredRecord`] 只记 `site` 一座）。
fn transfer_site(
    factions: &mut [Faction],
    owner: &mut [(WorldId, usize)],
    record: &SettlementConqueredRecord,
) {
    let (Some(from), Some(to)) = (
        lookup_owner(owner, record.site),
        lookup_owner(owner, record.conqueror),
    ) else {
        // 一座没有归属的据点被占——只可能出现在手工构造的残缺事件流里。
        // 跳过而不是 panic：ADR 0015 的既有表达，且编年史自己的生产路径
        // 不可能走到这里（占领双方都必然先有建立事件）。
        return;
    };
    if from == to {
        // 自家人「占」自家城：不改变任何东西，也不重复计数。
        return;
    }
    remove_member(&mut factions[from], record.site, record.epoch);
    insert_member(&mut factions[to], record.site);
    set_owner(owner, record.site, to);
}

/// **覆灭**那一条规则：据点成了废墟，从所属势力里除名。
fn drop_site(
    factions: &mut [Faction],
    owner: &mut Vec<(WorldId, usize)>,
    site: WorldId,
    epoch: u32,
) {
    let Some(from) = lookup_owner(owner, site) else {
        return;
    };
    remove_member(&mut factions[from], site, epoch);
    clear_owner(owner, site);
}

/// 把一座据点从势力的成员表里除名；除名后若空了就转
/// [`FactionStatus::Fallen`]，若丢的是首邑而还有别的据点就改指现存成员里
/// 号最小的那座（确定性规则，见 [`Faction::seat`] 字段文档）。
fn remove_member(faction: &mut Faction, site: WorldId, epoch: u32) {
    if let Ok(position) = faction.members.binary_search(&site) {
        faction.members.remove(position);
    }
    if faction.members.is_empty() {
        faction.status = FactionStatus::Fallen { epoch };
    } else if faction.seat == site {
        faction.seat = faction.members[0];
    }
}

/// 把一座据点加进势力的成员表，保持升序去重。
fn insert_member(faction: &mut Faction, site: WorldId) {
    if let Err(position) = faction.members.binary_search(&site) {
        faction.members.insert(position, site);
    }
}

/// 折叠过程中那份可变倒排索引的三个操作，都走二分、都保持升序。
fn lookup_owner(owner: &[(WorldId, usize)], site: WorldId) -> Option<usize> {
    owner
        .binary_search_by_key(&site, |(at, _)| *at)
        .ok()
        .map(|position| owner[position].1)
}

fn insert_owner(owner: &mut Vec<(WorldId, usize)>, site: WorldId, faction: usize) {
    match owner.binary_search_by_key(&site, |(at, _)| *at) {
        Ok(position) => owner[position].1 = faction,
        Err(position) => owner.insert(position, (site, faction)),
    }
}

fn set_owner(owner: &mut [(WorldId, usize)], site: WorldId, faction: usize) {
    if let Ok(position) = owner.binary_search_by_key(&site, |(at, _)| *at) {
        owner[position].1 = faction;
    }
}

fn clear_owner(owner: &mut Vec<(WorldId, usize)>, site: WorldId) {
    if let Ok(position) = owner.binary_search_by_key(&site, |(at, _)| *at) {
        owner.remove(position);
    }
}
