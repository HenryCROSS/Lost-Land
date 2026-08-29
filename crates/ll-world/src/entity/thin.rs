//! 薄层容器：列式（SoA）人口，容纳数十万到数百万背景 NPC。

use ll_core::ident::ContentIndex;
use ll_core::rng::DetRng;
use ll_core::time::{TICKS_PER_DAY, Tick};
use ll_core::torus::TorusPos;
use serde::{Deserialize, Serialize};

use crate::space::Space;

use super::{Agent, BaseStats, EntityId, FamilyId, Gender};

/// 钱包公式每「天」的基础收入。P3 阶段的占位取值——真实的经济平衡
/// （职业、聚落健康度、供需）属于 P8，见
/// `knowledge/design/agent-goals-and-economy.md` 第七之二节。
const DAILY_INCOME: i64 = 10;

/// 薄层人口：字段是固定模式，人人一样，故用列式（SoA）排布。
///
/// 每个 `Vec` 是一列，同一下标对应同一个 NPC。批量更新钱包时遍历的是
/// 一条连续的 `Vec<i64>`，完全可向量化——若改成 `Vec<Agent>` 行式排布，
/// 只读钱包一个字段也会把整条缓存行拉进来，浪费十倍内存带宽（见
/// `knowledge/design/agent-goals-and-economy.md` 「性能纪律」一节）。
///
/// **不支持销毁/复用槽位**：与厚层 [`crate::entity::Arena`] 不同，薄层
/// 人口目前只增不减——个体死亡在这一档是 cohort 层的批量人口更替
/// （见同一份设计文档「群体过程」一节），不是逐个体销毁，P8 落地前
/// 没有单个体销毁的需求，故 `generation` 列当前恒为零，只是为未来
/// cohort 批量更替预留的字段位——加字段现在零成本，P8 再加就要写
/// 存档迁移链。
///
/// # 可直接派生 `serde`（P5 批次 B，偿还历史债务）
///
/// 曾经不派生 `serde`：`profession`/`race` 两列都是 `Vec<ContentIndex>`，
/// 当时 `ContentIndex` 还没有可直接使用的序列化实现。这条障碍已解除
/// ——`ContentIndex` 现在直接派生 `Serialize`/`Deserialize`（结构合法
/// 与已注册是两件事，见 [`crate::entity::Agent`] 模块文档「可派生
/// `serde`」一节的完整论证，以及 `ll_core::ident` 模块文档），每一列
/// 因此都可以直接派生，不需要额外的解析上下文。真正把索引解析回
/// `NamespacedId` 字符串、核对当前会话是否仍注册着这条内容，是存档
/// 主体读写管线（任务 9）拿到注册表之后才能做的独立步骤。
/// # `race` 列已经还掉了（文化批次）
///
/// 这里曾经有第八列 `race: Vec<ContentIndex>`，带着一条明写的实现
/// 债务：`knowledge/design/race-system.md` 八节的设计是薄层**零列**
/// ——种族该由「出生聚落 + 聚落种族权重表」现算派生，不该单独存一列；
/// 当时不修的理由是「权重表还没落地」。
///
/// **那条理由现在没有了。** 权重表落地了，而且落在内容里：一座据点的
/// 文化（[`crate::settlement::SettlementSite::culture`]）带着一份
/// [`crate::culture::CultureAttrs::founder_races`]，
/// `ll_mod::roster::settlement_founder_race` 按它抽一次就是这座据点的
/// 主体种族。而 `settlement` 那一列本来就是「出生聚落」——
/// `birth_settlement` 不需要新增，它一直在。
///
/// 于是这一列被删掉：**能派生的不进存档**（ADR 0009）。
/// [`Self::promote`] 改为由调用方递一个 `race` 进来，与 `at`/`zone`/
/// `surface_profile` 三个参数同一条既有理由——薄层不持有注册表、也不
/// 持有文化表，那份上下文只有持有 `WorldState` 与内容表的一方才有。
///
/// 这次改动**不触发任何存档迁移**：薄层在生产路径上从来没有被写入过
/// 一次（`spawn` 的全部调用点都在测试里），且 `population` 不参与
/// [`crate::state::WorldState::hash`]——两条黄金基准都因此逐位不变，
/// 已实测。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinPopulation {
    generation: Vec<u32>,
    settlement: Vec<u16>,
    profession: Vec<ContentIndex>,
    family: Vec<FamilyId>,
    /// 钱包的基准值。重定基准时刷新。
    wallet_rebase: Vec<i64>,
    /// 钱包相对公式结果的偏移量。玩家给钱、抢劫只改这个。
    wallet_delta: Vec<i64>,
    /// 上次重定基准的时刻。
    rebase_at: Vec<Tick>,
}

/// [`ThinPopulation::get_slot`] 返回的只读快照：把某个下标的各列取值
/// 拼成一个值，供调用方一次性读取，不必分别调用七个列访问器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinSlot {
    /// 所属聚落编号。
    pub settlement: u16,
    /// 当前职业，指向注册表。
    pub profession: ContentIndex,
    /// 所属家族编号。
    pub family: FamilyId,
    /// 钱包的基准值。重定基准时刷新。
    pub wallet_rebase: i64,
    /// 钱包相对公式结果的偏移量。玩家给钱、抢劫只改这个。
    pub wallet_delta: i64,
    /// 上次重定基准的时刻。
    pub rebase_at: Tick,
}

/// 升格一个薄层 NPC 所需的、**薄层自己拿不到**的那一组上下文。
///
/// 打包成结构体而不是继续往参数表上加，理由与
/// [`crate::settlement::StampContext`] 逐字相同：这四项恒一起出现，
/// 散着传只会让调用点更容易漏配，也会撞上
/// `clippy::too_many_arguments`（`race` 从列改成参数正是压垮那条闸门
/// 的第八个参数）。
#[derive(Debug, Clone, Copy)]
pub struct PromotionContext {
    /// 升格后落在哪一格——通常是该 NPC 所属聚落的位置。
    pub at: TorusPos,
    /// 升格后 `Agent::current_space` 的区块坐标。
    pub zone: crate::space::ZoneCoord,
    /// 地表层属性的内容索引。
    pub surface_profile: ContentIndex,
    /// 种族——由调用方按 `ThinSlot::settlement` 那座据点的文化派生，
    /// 见 [`ThinPopulation`] 文档「`race` 列已经还掉了」。
    pub race: ContentIndex,
}

impl ThinPopulation {
    /// 建一个空的薄层人口。
    pub fn new() -> Self {
        ThinPopulation {
            generation: Vec::new(),
            settlement: Vec::new(),
            profession: Vec::new(),
            family: Vec::new(),
            wallet_rebase: Vec::new(),
            wallet_delta: Vec::new(),
            rebase_at: Vec::new(),
        }
    }

    /// 放入一个新背景 NPC，返回其标识。
    ///
    /// `wallet_baseline` 是钱包公式的起点（见 [`Self::wallet_of`]），
    /// `now` 同时作为 `rebase_at` 的初始值。
    ///
    /// **七个列必须在这一处、且只在这一处一起 push**——分散到多处各自
    /// push 正是列式存储最容易出的错：某一列忘了同步，各列长度就此
    /// 错位，后续任何按下标读取都会读到别的 NPC 的数据。
    ///
    /// **没有 `race` 参数**：种族由 `settlement` 现算派生，见本类型
    /// 文档「`race` 列已经还掉了」一节。
    pub fn spawn(
        &mut self,
        settlement: u16,
        profession: ContentIndex,
        family: FamilyId,
        wallet_baseline: i64,
        now: Tick,
    ) -> EntityId {
        let index = self.generation.len() as u32;
        self.generation.push(0);
        self.settlement.push(settlement);
        self.profession.push(profession);
        self.family.push(family);
        self.wallet_rebase.push(wallet_baseline);
        self.wallet_delta.push(0);
        self.rebase_at.push(now);
        EntityId::new(index, 0)
    }

    /// 按标识校验并取出下标；世代不符或越界均返回 [`None`]。
    fn index_of(&self, id: EntityId) -> Option<usize> {
        let index = id.index() as usize;
        if self.generation.get(index).copied()? == id.generation() {
            Some(index)
        } else {
            None
        }
    }

    /// 取出某个 NPC 全部列的只读快照。
    pub fn get_slot(&self, id: EntityId) -> Option<ThinSlot> {
        let index = self.index_of(id)?;
        Some(ThinSlot {
            settlement: self.settlement[index],
            profession: self.profession[index],
            family: self.family[index],
            wallet_rebase: self.wallet_rebase[index],
            wallet_delta: self.wallet_delta[index],
            rebase_at: self.rebase_at[index],
        })
    }

    /// 算出某个 NPC 当前的钱包值：`公式(种子, id, 距 rebase_at 的时长)
    /// + wallet_delta`。
    ///
    /// 钱包不直接存值，正是「棘轮问题」的解法（见
    /// `knowledge/design/agent-goals-and-economy.md` 「棘轮问题」一节）：
    /// 被玩家动过的 NPC 只多存一个偏移量，仍能立刻回到批量公式，不必
    /// 永久占用昂贵的模拟槽位。
    pub fn wallet_of(&self, id: EntityId, seed: u64, now: Tick) -> Option<i64> {
        let index = self.index_of(id)?;
        let elapsed = now.0 - self.rebase_at[index].0;
        Some(
            wallet_formula(seed, id, elapsed)
                + self.wallet_rebase[index]
                + self.wallet_delta[index],
        )
    }

    /// 给全体 NPC 的钱包偏移量统一加上 `delta`（可为负）。
    ///
    /// 这是薄层列式排布的直接回报：遍历的是一条连续的 `Vec<i64>`，
    /// 不会像行式排布那样把每个 NPC 无关的其余字段一并拖进缓存行。
    pub fn batch_update_wallets(&mut self, delta: i64) {
        for value in &mut self.wallet_delta {
            *value += delta;
        }
    }

    /// 把某个背景 NPC 升格为可供厚层模拟的 [`Agent`] 快照。
    ///
    /// 薄层不追踪逐个体的位置与属性，`at` 由调用方提供——通常是该 NPC
    /// 所属聚落的位置。返回的 `Agent` 是升格那一刻的快照，此后不再与
    /// 薄层的公式挂钩；调用方需要自行决定何时（以及是否）通过
    /// [`Self::rebase`] 把它交还给薄层。
    ///
    /// `zone`/`surface_profile`/`race` 由调用方提供，用于构造升格后
    /// `Agent::current_space` 的初始值（恒为 `Space::Surface`——薄层
    /// NPC 只在地表活动，见 [`crate::entity::Agent::current_space`]
    /// 文档）与 `Agent::race`：薄层本身不持有 `ZoneLayout`/层属性
    /// 注册表/文化表，这三样上下文只有调用方（持有 `WorldState` 与
    /// 内容表的一方）才有。
    ///
    /// `race` 从参数进来而不是从列里读，是文化批次还掉的那笔债——
    /// 调用方按 `ThinSlot::settlement` 查那座据点的文化、再按文化抽
    /// 建立者种族（`ll_mod::roster::settlement_founder_race`），与
    /// 名册派生走的是同一条路。见本类型文档「`race` 列已经还掉了」。
    pub fn promote(
        &self,
        id: EntityId,
        seed: u64,
        now: Tick,
        ctx: PromotionContext,
    ) -> Option<Agent> {
        let PromotionContext {
            at,
            zone,
            surface_profile,
            race,
        } = ctx;
        let index = self.index_of(id)?;
        let wallet = self.wallet_of(id, seed, now)?;
        Some(Agent {
            pos: at,
            stats: BaseStats::BASELINE,
            next_action_at: now,
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet,
            profession: self.profession[index],
            goals: Vec::new(),
            // 性别由 `(世界种子, 实体 id, 本用途专属事件计数)` 确定性
            // 抽出（约束 C3），不占薄层的一列：薄层的设计纪律是「只存
            // 公式算不出来的东西」，而性别恰好算得出来——同一个薄层
            // NPC 无论升格多少次都得到同一个性别。
            gender: Gender::deterministic(seed, id.as_u64(), super::GENDER_EVENT),
            race,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            known_recipes: Vec::new(),
            identified_items: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            subclasses_ever_granted: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: Space::surface(zone, surface_profile),
            // 薄层本就不支持脚本状态存储（设计文档三、3 节：只限厚层
            // `Arena<Agent>`），升格这一刻自然是空的。
            mod_state: std::collections::BTreeMap::new(),
            // 薄层不追踪生物类型（薄层 NPC 全部走 `race`，见
            // Agent::creature_kind 文档「绝大多数……不需要设置」）。
            creature_kind: None,
            // 升格这一刻就是这个厚层实体真正开始存在的时刻。
            spawned_at: now,
            // 升格本身不是"值得被记住"的事件——薄层 NPC 数量巨大，
            // 升格是常规的前景层调度,不该顺手给每一个都发一个
            // WorldId（见 Agent::remembered_id 文档「懒分配」）。
            remembered_id: None,
            // 薄层不追踪逐个体等级/经验——升格这一刻总是从 1 级、零
            // 经验开始，与生命/法力/耐力同一条占位纪律，见
            // Agent::STARTING_LEVEL/STARTING_XP_TO_NEXT_LEVEL 文档。
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        })
    }

    /// 重定基准：把当前公式算出的钱包值快照成新的基准，偏移量归零。
    ///
    /// 长期未交互的 NPC 靠这一步回到「纯公式驱动」——见
    /// [`Self::wallet_of`] 文档引用的棘轮问题一节。重定基准前后
    /// [`Self::wallet_of`] 的返回值必须不变，这是它正确性的判据。
    pub fn rebase(&mut self, id: EntityId, seed: u64, now: Tick) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let current = wallet_formula(seed, id, now.0 - self.rebase_at[index].0)
            + self.wallet_rebase[index]
            + self.wallet_delta[index];
        self.wallet_rebase[index] = current;
        self.wallet_delta[index] = 0;
        self.rebase_at[index] = now;
        true
    }

    /// 把 `profession` 列的 `ContentIndex` 原地重映射——存档读入后的
    /// 重映射步骤（`ll-content` 任务 9）需要。
    ///
    /// 曾经是**两**列（`profession` 与 `race`）。`race` 列随文化批次
    /// 删掉了（见 [`ThinPopulation`] 文档「`race` 列已经还掉了」），
    /// 因此这里少了一趟——**能派生的东西不需要重映射**，这正是
    /// ADR 0009 那条原则顺手省下来的成本之一。
    ///
    /// 泛型的错误类型 `E`——本 crate 不知道、也不该知道调用方（`ll-content`）
    /// 会怎么报错，只负责在闭包报错时立即中止并把错误原样透传，不吞掉
    /// 也不猜测该包成什么错误类型。
    pub fn try_remap_content_indices<E>(
        &mut self,
        mut remap: impl FnMut(ContentIndex) -> Result<ContentIndex, E>,
    ) -> Result<(), E> {
        for slot in &mut self.profession {
            *slot = remap(*slot)?;
        }
        Ok(())
    }

    /// 当前人口数量。
    pub fn len(&self) -> usize {
        self.generation.len()
    }

    /// 人口是否为空。
    pub fn is_empty(&self) -> bool {
        self.generation.is_empty()
    }
}

impl Default for ThinPopulation {
    fn default() -> Self {
        ThinPopulation::new()
    }
}

/// 钱包公式：`距 rebase_at 的时长` 换算成天数，天数为零时恒返回零
/// （这保证了「重定基准后偏移归零而钱包值不变」——刚重定基准那一刻
/// `elapsed` 恰为零），此后每天一份确定性波动的收入。
///
/// 全整数、由 [`DetRng::for_entity`] 派生，不依赖任何全局随机流——同一
/// `(种子, 实体, 天数)` 三元组在任何时候、任何线程上都得到相同结果，
/// 批量结算天然可并行。真实的经济平衡属于 P8，这里只是占位公式。
fn wallet_formula(seed: u64, id: EntityId, elapsed_ticks: i64) -> i64 {
    let days = elapsed_ticks.div_euclid(TICKS_PER_DAY);
    if days == 0 {
        return 0;
    }
    let mut rng = DetRng::for_entity(seed, id.as_u64(), days as u64);
    let variance = rng.gen_range(DAILY_INCOME as u64 + 1) as i64 - DAILY_INCOME / 2;
    days * DAILY_INCOME + variance
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    fn farmer() -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:farmer").expect("合法标识符"))
    }

    fn human() -> ContentIndex {
        let mut interner = Interner::new();
        interner.intern(NamespacedId::parse("lostland:human").expect("合法标识符"))
    }

    #[test]
    fn thinpopulation序列化往返后各列长度与内容一致() {
        // 直接对应 P5 批次 B 存在的理由：ThinPopulation 摘掉
        // `#[serde(skip)]` 之前，这条断言无法成立——薄层压根不参与
        // 序列化。这里放入两个不同的 NPC（职业/种族/家族/钱包各不
        // 相同），确认往返后每一列的每一个下标都对应回原来的值，而不
        // 只是长度凑巧相等。
        // Arrange
        let mut original = ThinPopulation::new();
        let first = original.spawn(1, farmer(), FamilyId(10), 500, Tick(0));
        let second = original.spawn(2, farmer(), FamilyId(20), 800, Tick(5));

        // Act
        let encoded = serde_json::to_string(&original).expect("全部列均已可派生序列化");
        let decoded: ThinPopulation =
            serde_json::from_str(&encoded).expect("刚序列化的数据必然合法");

        // Assert
        assert_eq!(decoded.len(), original.len());
        assert_eq!(decoded.get_slot(first), original.get_slot(first));
        assert_eq!(decoded.get_slot(second), original.get_slot(second));
    }

    #[test]
    fn 新生成的背景npc可以按标识取回() {
        // Arrange
        let mut population = ThinPopulation::new();

        // Act
        let id = population.spawn(1, farmer(), FamilyId(1), 100, Tick(0));

        // Assert
        assert!(population.get_slot(id).is_some());
    }

    #[test]
    fn 薄层各列的长度恒相等() {
        // 列式存储最容易出的错：某一列忘了同步 push。
        // Arrange
        let mut population = ThinPopulation::new();

        // Act
        for _ in 0..5 {
            population.spawn(1, farmer(), FamilyId(1), 0, Tick(0));
        }

        // Assert
        let lengths = [
            population.generation.len(),
            population.settlement.len(),
            population.profession.len(),
            population.family.len(),
            population.wallet_rebase.len(),
            population.wallet_delta.len(),
            population.rebase_at.len(),
        ];
        assert!(lengths.iter().all(|&len| len == 5));
    }

    #[test]
    fn 世代不符的标识取不到槽位() {
        // Arrange
        let mut population = ThinPopulation::new();
        let id = population.spawn(1, farmer(), FamilyId(1), 0, Tick(0));
        let stale = EntityId::new(id.index(), id.generation() + 1);

        // Act & Assert
        assert!(population.get_slot(stale).is_none());
    }

    #[test]
    fn 钱包由基准值与偏移量共同决定() {
        // Arrange：同一时刻查询（elapsed 为零），公式贡献恒为零，
        // 于是钱包值必然等于基准值加偏移量。
        let mut population = ThinPopulation::new();
        let id = population.spawn(1, farmer(), FamilyId(1), 1000, Tick(0));

        // Act
        population.batch_update_wallets(50);
        let wallet = population
            .wallet_of(id, 42, Tick(0))
            .expect("刚生成的标识必然有效");

        // Assert
        assert_eq!(wallet, 1000 + 50);
    }

    #[test]
    fn 重定基准后偏移归零而钱包值不变() {
        // Arrange
        let mut population = ThinPopulation::new();
        let id = population.spawn(1, farmer(), FamilyId(1), 1000, Tick(0));
        population.batch_update_wallets(200);
        let before = population
            .wallet_of(id, 42, Tick(3 * TICKS_PER_DAY))
            .expect("有效标识");

        // Act
        population.rebase(id, 42, Tick(3 * TICKS_PER_DAY));
        let after = population
            .wallet_of(id, 42, Tick(3 * TICKS_PER_DAY))
            .expect("有效标识");
        let slot = population.get_slot(id).expect("有效标识");

        // Assert
        assert_eq!(after, before);
        assert_eq!(slot.wallet_delta, 0);
    }

    #[test]
    fn 升格后的agent携带薄层记录的职业() {
        // Arrange
        let mut population = ThinPopulation::new();
        let profession = farmer();
        let id = population.spawn(1, profession, FamilyId(1), 500, Tick(0));
        let world = ll_core::torus::TorusSize::new(16, 16).expect("常量非零");
        let at = world.wrap(3, 3);

        // Act
        let agent = population
            .promote(id, 42, Tick(0), promotion_context(at, human()))
            .expect("有效标识必然能升格");

        // Assert
        assert_eq!(agent.profession, profession);
    }

    #[test]
    fn 升格后的agent携带调用方递进来的种族() {
        // 文化批次之前这条测试叫「携带**薄层记录的**种族」，读的是已经
        // 删掉的 `race` 列。现在种族由调用方派生后递进来（据点 →
        // 文化 → 建立者种族），本条守的是「递进来的那个值真的落到了
        // `Agent::race` 上」，不是「薄层存了它」。
        // Arrange
        let mut population = ThinPopulation::new();
        let race = human();
        let id = population.spawn(1, farmer(), FamilyId(1), 500, Tick(0));
        let world = ll_core::torus::TorusSize::new(16, 16).expect("常量非零");
        let at = world.wrap(3, 3);

        // Act
        let agent = population
            .promote(id, 42, Tick(0), promotion_context(at, race))
            .expect("有效标识必然能升格");

        // Assert
        assert_eq!(agent.race, race);
    }

    #[test]
    fn 无效标识升格返回空() {
        // Arrange
        let population = ThinPopulation::new();
        let bogus = EntityId::new(0, 0);
        let world = ll_core::torus::TorusSize::new(16, 16).expect("常量非零");

        // Act
        let agent = population.promote(
            bogus,
            42,
            Tick(0),
            promotion_context(world.wrap(0, 0), human()),
        );

        // Assert
        assert!(agent.is_none());
    }

    /// 测试用升格上下文：升格相关测试只关心落点与种族，另外两样取一个
    /// 合法占位值。
    fn promotion_context(at: TorusPos, race: ContentIndex) -> PromotionContext {
        PromotionContext {
            at,
            zone: zone_fixture(),
            surface_profile: ContentIndex::default(),
            race,
        }
    }

    /// 测试用区块坐标：升格相关测试不关心具体落在哪个区块，只需要一个
    /// 合法值。
    fn zone_fixture() -> crate::space::ZoneCoord {
        ll_core::torus::TorusSize::new(48, 32)
            .expect("48x32 是合法的区块尺寸")
            .wrap(0, 0)
    }
}
