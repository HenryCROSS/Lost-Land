//! 薄层容器：列式（SoA）人口，容纳数十万到数百万背景 NPC。

use ll_core::ident::ContentIndex;
use ll_core::rng::DetRng;
use ll_core::time::{TICKS_PER_DAY, Tick};
use ll_core::torus::TorusPos;

use super::{Agent, BaseStats, EntityId, FamilyId};

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
/// 不派生 `serde`：`profession`/`race` 两列都是 `Vec<ContentIndex>`，
/// `ll_core::ident` 模块文档明确写着 `ContentIndex` 不可持久化（依赖
/// mod 加载顺序）。真正持久化薄层人口需要把每个 `ContentIndex` 解析回
/// `NamespacedId` 字符串，这是内容注册表存档格式的职责，不在本任务
/// 范围内——[`crate::entity::Arena`] 的序列化往返测试已经覆盖了「实体
/// 存储机制本身」这条要验证的东西，不依赖 `ContentIndex`。
#[derive(Debug, Clone)]
pub struct ThinPopulation {
    generation: Vec<u32>,
    settlement: Vec<u16>,
    profession: Vec<ContentIndex>,
    /// 种族，指向注册表。与 `profession` 同一模式——见
    /// [`crate::entity::Agent::race`] 文档。
    race: Vec<ContentIndex>,
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
    /// 种族，指向注册表。
    pub race: ContentIndex,
    /// 所属家族编号。
    pub family: FamilyId,
    /// 钱包的基准值。重定基准时刷新。
    pub wallet_rebase: i64,
    /// 钱包相对公式结果的偏移量。玩家给钱、抢劫只改这个。
    pub wallet_delta: i64,
    /// 上次重定基准的时刻。
    pub rebase_at: Tick,
}

impl ThinPopulation {
    /// 建一个空的薄层人口。
    pub fn new() -> Self {
        ThinPopulation {
            generation: Vec::new(),
            settlement: Vec::new(),
            profession: Vec::new(),
            race: Vec::new(),
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
    /// **八个列必须在这一处、且只在这一处一起 push**——分散到多处各自
    /// push 正是列式存储最容易出的错：某一列忘了同步，各列长度就此
    /// 错位，后续任何按下标读取都会读到别的 NPC 的数据。
    pub fn spawn(
        &mut self,
        settlement: u16,
        profession: ContentIndex,
        race: ContentIndex,
        family: FamilyId,
        wallet_baseline: i64,
        now: Tick,
    ) -> EntityId {
        let index = self.generation.len() as u32;
        self.generation.push(0);
        self.settlement.push(settlement);
        self.profession.push(profession);
        self.race.push(race);
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
            race: self.race[index],
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
    pub fn promote(&self, id: EntityId, at: TorusPos, seed: u64, now: Tick) -> Option<Agent> {
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
            race: self.race[index],
            // 薄层不追踪幸运，升格时取零——见 Agent::luck 文档。
            luck: 0,
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
    fn 新生成的背景npc可以按标识取回() {
        // Arrange
        let mut population = ThinPopulation::new();

        // Act
        let id = population.spawn(1, farmer(), human(), FamilyId(1), 100, Tick(0));

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
            population.spawn(1, farmer(), human(), FamilyId(1), 0, Tick(0));
        }

        // Assert
        let lengths = [
            population.generation.len(),
            population.settlement.len(),
            population.profession.len(),
            population.race.len(),
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
        let id = population.spawn(1, farmer(), human(), FamilyId(1), 0, Tick(0));
        let stale = EntityId::new(id.index(), id.generation() + 1);

        // Act & Assert
        assert!(population.get_slot(stale).is_none());
    }

    #[test]
    fn 钱包由基准值与偏移量共同决定() {
        // Arrange：同一时刻查询（elapsed 为零），公式贡献恒为零，
        // 于是钱包值必然等于基准值加偏移量。
        let mut population = ThinPopulation::new();
        let id = population.spawn(1, farmer(), human(), FamilyId(1), 1000, Tick(0));

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
        let id = population.spawn(1, farmer(), human(), FamilyId(1), 1000, Tick(0));
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
        let id = population.spawn(1, profession, human(), FamilyId(1), 500, Tick(0));
        let world = ll_core::torus::TorusSize::new(16, 16).expect("常量非零");
        let at = world.wrap(3, 3);

        // Act
        let agent = population
            .promote(id, at, 42, Tick(0))
            .expect("有效标识必然能升格");

        // Assert
        assert_eq!(agent.profession, profession);
    }

    #[test]
    fn 升格后的agent携带薄层记录的种族() {
        // Arrange
        let mut population = ThinPopulation::new();
        let race = human();
        let id = population.spawn(1, farmer(), race, FamilyId(1), 500, Tick(0));
        let world = ll_core::torus::TorusSize::new(16, 16).expect("常量非零");
        let at = world.wrap(3, 3);

        // Act
        let agent = population
            .promote(id, at, 42, Tick(0))
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
        let agent = population.promote(bogus, world.wrap(0, 0), 42, Tick(0));

        // Assert
        assert!(agent.is_none());
    }
}
