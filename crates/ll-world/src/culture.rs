//! 文化：一座据点**怎么建、谁来建、跟谁不对付**的那一层。
//!
//! # 项目所有者的裁决
//!
//! > 「文化由据点建立时决定，不看种族。选择依据是资源 + 地形 + 邻近
//! > 据点 + 一点随机。建立者种族给权重但不锁死。」
//! > 「一个哥布林营地和一座矮人矿城是同一种东西。」
//! > 「每个聚居地可能组成势力，每个聚居地存在一种文化信仰。」
//!
//! 三条合起来给出的形状是：**文化是据点的一个派生属性**（一座据点
//! 恰好一种文化，见 [`crate::settlement::SettlementSite::culture`]），
//! 在拓荒那一刻按周边条件抽出来，此后这一茬人一直用它；据点覆灭、
//! 另一批人重新拓荒时重新抽一次。
//!
//! # 类型在 `ll-world`、数据由 `ll-mod` 填，理由与 [`crate::resource`] 逐字相同
//!
//! 选址（[`crate::chronicle`]）、铺房子（[`crate::settlement`]）、
//! 战争（同上）全部发生在 `ll-world`，而文化是**内容**（`mods/<id>/
//! cultures.json5`）。`ll-world` 不能反向依赖 `ll-mod`，因此走
//! [`crate::terrain::TerrainTable`]/[`crate::resource::ResourceTable`]
//! 已经走过两次的那条路：**类型定义在这里，装载器在 `ll-mod`，装好之
//! 后整张表注入世界生成**（`ChronicleParams` 那一侧接的是
//! [`CultureTable`]，与它已经在接的 `ResourceTable` 同一种手法）。
//!
//! `ll_mod::roster::settlement_founder_race` 的模块文档此前记录过
//! 「建立者种族之所以是一个 `pub fn` 而不是 `SettlementSite` 的字段，
//! 是因为硬挂就要把种族内容倒灌进 `ll-world`」——**本模块正是那堵墙的
//! 破法**：倒灌进来的不是注册表，是一张按 [`ContentIndex`] 下标索引的
//! 定长表，与地形表、资源表同构。
//!
//! # 五个字段，每一个都指得出自己被哪一行读
//!
//! 本仓库已经找到 31 处以上「声明了但从没接线」的字段，因此这张表的
//! 收录判据是**逐字段的**：说不出「谁在哪一行读它、影响什么可观测
//! 结果」的字段一律不收。五个字段各自的落点见 [`CultureAttrs`] 逐字段
//! 文档的「**谁读它**」段，一句话版本：
//!
//! | 字段 | 谁读它 | 可观测结果 |
//! |---|---|---|
//! | `economy` | `chronicle` 的 `culture_weights` | 守着铁矿的地方长出矿业文化 |
//! | `home_terrain` | 同上 | 山里长出山地文化、草原上长出农耕文化 |
//! | `wall_terrain` | [`crate::settlement`] 的 `house_tiles`/`ruin_tiles` | 矮人矿城是石头的，哥布林营地是木头的 |
//! | `founder_races` | `ll_mod::roster::settlement_founder_race` | 哥布林部落里住的是哥布林 |
//! | `hostility` | `chronicle` 的 `wage_wars`/`pick_target` | 「矮人矿城被哥布林部落攻灭」 |
//!
//! # 文化不进存档（ADR 0009「默认派生，只存偏差」）
//!
//! 文化是 `(世界种子, 据点序号, 纪元, 周边条件)` 的纯函数，与据点本身
//! 同一条纪律——整部编年史都不进存档，读档时 `rebuild_chronicle` 重新
//! 派生一遍。[`crate::settlement::SettlementSite::culture`] 因此是一个
//! **派生快照上的字段**，不是一处存储。
//!
//! # 确定性（约束 C3 / C5）
//!
//! - 唯一的随机来源是 `chronicle` 里那一次 `DetRng::for_entity`
//!   （流编号 [`crate::chronicle::CHRONICLE_CULTURE_STREAM_ID`]），三元组与调用顺序无关。
//! - 遍历文化只走 [`CultureTable::registered`]（注册顺序的 `Vec`），
//!   加权抽取按同一个顺序线性扫描，全程不碰任何 `HashMap`/`HashSet`。
//! - 敌意查表是两次 `Vec` 线性扫描，同样按注册顺序。

use std::fmt;

use ll_core::ident::{ContentIndex, NamespacedId, WorldId};
use ll_core::rng::DetRng;

use crate::resource::ResourceCategory;
use crate::terrain::TerrainKind;

/// 一种文化的内容索引包装——与 [`crate::resource::ResourceKind`]、
/// [`TerrainKind`] 同一种手法（避免「一个文化索引」与「一个资源索引」
/// 在类型上混为一谈）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CultureKind(ContentIndex);

impl CultureKind {
    /// 从一个已经 `intern` 出来的内容索引构造。
    pub const fn from_index(index: ContentIndex) -> Self {
        CultureKind(index)
    }

    /// 取回内部的内容索引。
    pub const fn index(self) -> ContentIndex {
        self.0
    }
}

/// [`CultureAttrs::hostility`] 的取值上界。
///
/// 敌意是战争概率的**加分项**，直接加在 `WAR_NUMERATOR` 上（分母
/// `WAR_DENOMINATOR` = 8）。上界取 7 是因为再高就会让分子达到或超过
/// 分母——那等于「只要够强就必然开战」，把项目所有者点名要守住的
/// 「战争是少数派」这条闸门整个拆掉。注册期当场拒绝越界值
/// （[`CultureError::HostilityOutOfRange`]），不静默截断。
pub const MAX_HOSTILITY: u32 = 7;

/// 一条文化声明——本体与 mod 注册文化时共用的同一个输入形状
/// （「本体即 Mod」，ADR 0018）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CultureAttrs {
    /// 展示名的本地化键，不存字面字符串——与
    /// [`crate::resource::ResourceAttrs::display_name_key`] 同一条既有
    /// 惯例。
    ///
    /// **谁读它**：编年史要能说出「**哥布林部落**攻灭了**矮人矿城**」，
    /// 呈现层由这个键取名字。与 `ResourceAttrs::display_name_key` 同
    /// 一种处境（真正的传说浏览 UI 尚未落地），因此同样走字段门禁
    /// 豁免而不是假装它已接线。
    pub display_name_key: NamespacedId,
    /// 这种文化**靠什么吃饭**：与它相配的资源大类。
    ///
    /// **谁读它**：[`crate::chronicle`] 的 `culture_weights`——一处
    /// 候选点的 [`crate::settlement::SettlementSite::resource_profile`]
    /// 命中哪种大类，就给声明了那个大类的文化加权重。这是项目所有者
    /// 「选择依据是**资源** + 地形 + 邻近据点 + 一点随机」四项里的第
    /// 一项，可观测结果是「守着铁矿的地方长出矿业文化、守着良田的地方
    /// 长出农耕文化」。
    ///
    /// 只声明**一个**大类而不是一张权重表：一张 `Vec<(大类, 权重)>`
    /// 在本批次没有任何一条内容会真的用到第二项（一种文化靠两种大类
    /// 吃饭这件事本身还没有内容设计），按 ADR 0021 那就是为对称而抽象。
    /// 真需要第二项时，把这个字段换成 `Vec` 是一处改动，不是一次迁移。
    pub economy: ResourceCategory,
    /// 这种文化**住在什么地上**：选址地形偏好。
    ///
    /// **谁读它**：[`crate::chronicle`] 的 `culture_weights` 第二项
    /// ——候选点锚点那一格的**基础地形**（噪声算出来的那一层，不是
    /// 已经铺过房子的那一层）与本字段相同就加权重。这是四项依据里的
    /// 「地形」，可观测结果是「山里长出山地文化、草原上长出农耕文化」。
    pub home_terrain: TerrainKind,
    /// 这种文化**用什么盖房子**：有人住的屋子那一圈墙用哪种地形。
    ///
    /// **谁读它**：[`crate::settlement::stamp_settlement`] 经
    /// `house_tiles`/`ruin_tiles` 逐格写进地形层。本字段替换掉的是本
    /// 模块落地之前那两个函数里写死的 `ids.wall_wood`/`ids.wall_stone`
    /// ——在此之前**一座哥布林营地会长得和一座矮人矿城一模一样**。
    ///
    /// 可观测结果最直接：矮人矿城是石头砌的，哥布林营地是木头搭的，
    /// 两者的废墟因此在地上也分得出来。
    pub wall_terrain: TerrainKind,
    /// 这种文化的据点**由谁建立**：候选种族与权重（权重为 0 的档位
    /// 不参与抽取）。
    ///
    /// **谁读它**：`ll_mod::roster::settlement_founder_race` 按这张表
    /// 抽一次，抽出的种族成为整座据点的主体人口
    /// （`ll_mod::roster::OUTSIDER_PERMILLE` 之外的那八成）。
    ///
    /// # 为什么种族权重挂在文化上，而不是反过来
    ///
    /// 这是**本批次的实现判断**，与项目所有者原话的方向相反，理由记在
    /// 这里以便复核：所有者说「文化由据点建立时决定，**不看种族**……
    /// 建立者种族给权重但不锁死」。「不看种族」这一半本字段完全满足
    /// ——文化抽取（`culture_weights`）一个字节的种族数据都不读；
    /// 「种族给文化加权」那一半要求**先有种族再有文化**，而建立者种族
    /// 今天是 `ll-mod` 按资源画像抽的，`ll-world` 拿不到它，于是要么
    /// 再往 `ll-world` 注入第二张「种族↔资源亲和」表（表面积翻倍，
    /// 表达力不增），要么把顺序倒过来。倒过来之后「不锁死」由**权重**
    /// 表达（矿业文化可以是矮人 8 / 人类 3 / 精灵 1，于是「人类开的
    /// 矿城」照样出得来），语义上没有损失。
    ///
    /// 顺带偿还的一笔债：在此之前建立者种族的资源亲和是
    /// `ll_mod::roster` 里写死的三元数组
    /// （`["lostland:human", "lostland:dwarf", "lostland:elf"]`），
    /// **第三方 mod 加一个种族拿不到任何选址亲和，一座据点都不会属于
    /// 它**。改成本字段之后，加一条 `cultures.json5` 就有自己的据点。
    pub founder_races: Vec<(ContentIndex, u32)>,
    /// 这种文化**跟谁不对付**：目标文化的内容索引 + 敌意分
    /// （`0..=`[`MAX_HOSTILITY`]）。
    ///
    /// **谁读它**：[`crate::chronicle`] 的 `wage_wars`（敌意直接加在
    /// 开战概率的分子上）与 `pick_target`（敌意高的目标优先被选为守
    /// 方）。可观测结果就是本批次的验收线：编年史里出现「第 N 纪元，
    /// 某矮人矿城被某哥布林部落攻灭」。
    ///
    /// # 刻意允许不对称
    ///
    /// 「哥布林恨矮人」不蕴含「矮人恨哥布林恨得一样深」。表是有向的：
    /// [`CultureTable::hostility`] 只查 `攻方 → 守方` 这一个方向，不做
    /// 任何对称化。判据与 `race-system.md` 十节给 `race_affinity` 定的
    /// 那条逐字相同。
    ///
    /// # 有向的是**声明**，不是每一层的判定
    ///
    /// 这一条是本表最容易被读错的地方，写清楚免得后人再绕一圈：
    ///
    /// - **本字段与本模块（内容声明层）有向**。上面这段一个字都没改。
    /// - **[`crate::chronicle`] 的战争推演（`hostility_between` /
    ///   `wage_wars` / `pick_target`）也有向**，它直接按 `攻方 → 守方`
    ///   读这张表。「矿邑对哥布林只有 3」在这一层照常起作用：矮人不太
    ///   会主动出兵讨伐。
    /// - **实体级撞格路由（`ll_sim::ai_query::declared_hostile`）对称**
    ///   ——所有者裁定「只要有一方处于敌对状态，另一方也会发起攻击」，
    ///   它取两个方向的最大值。
    ///
    /// 两者不矛盾，因为回答的是不同问题：出兵是集体决策（可以单方面
    /// 隐忍），迎面撞见拔不拔刀不是（一方拔刀，另一方不可能站着挨打）。
    /// **本表是这两个问题共同的输入，不是其中任何一个的答案。**
    ///
    /// # 敌对不是开战的唯一条件
    ///
    /// 人口阈值（`WAR_MIN_POPULATION`）与优势比
    /// （`WAR_DOMINANCE_RATIO`）两条闸门一个字都没动，本字段只是往
    /// 已有的 1/8 掷骰上**加分**——与 `try_found` 已经在用的「四条加分
    /// 互相独立地推高同一个概率分子」是同一个手法。全表敌意为 0 时，
    /// 战争行为与本模块落地之前**逐位相同**（本模块测试
    /// `敌意全为零时开战判定与旧行为逐位相同` 守着这条）。
    pub hostility: Vec<(ContentIndex, u32)>,
}

/// 文化注册期可能出现的错误。ADR 0017「注册期完整校验」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CultureError {
    /// 同一个内容索引被定义了两次——纪律同
    /// [`crate::resource::ResourceError::DuplicateDefinition`]。
    DuplicateDefinition(ContentIndex),
    /// [`CultureAttrs::hostility`] 的某一项超出 `0..=`[`MAX_HOSTILITY`]，
    /// 理由见 [`MAX_HOSTILITY`]。
    HostilityOutOfRange(u32),
    /// [`CultureAttrs::founder_races`] 是空的，或者全部权重都是 0。
    ///
    /// 这条不是吹毛求疵：一份没有任何建立者种族的文化，抽中它的据点
    /// 会拿到一份「谁也不是」的名册。静默产出那种据点比装载期当场
    /// 点名要难查得多（ADR 0017）。
    NoFounderRace(ContentIndex),
}

impl fmt::Display for CultureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CultureError::DuplicateDefinition(index) => {
                write!(f, "文化索引 {} 被重复定义", index.get())
            }
            CultureError::HostilityOutOfRange(value) => {
                write!(f, "文化敌意 {value} 超出 0..={MAX_HOSTILITY} 的合法范围")
            }
            CultureError::NoFounderRace(index) => {
                write!(f, "文化索引 {} 没有任何权重为正的建立者种族", index.get())
            }
        }
    }
}

impl std::error::Error for CultureError {}

/// 文化的列式存储：按 [`ContentIndex`] 下标索引（ADR 0017），形状照抄
/// [`crate::resource::ResourceTable`]——包括那份 [`Self::registered`]
/// 注册顺序列表，理由也逐字相同：文化是需要**遍历全表**的内容表
/// （每次拓荒都要问「有哪些文化可能在这里扎根」），而 `defined` 位图的
/// 下标顺序会随「同一次装载里别的表 intern 了多少条」漂移。
#[derive(Debug, Default, Clone)]
pub struct CultureTable {
    display_name_key: Vec<Option<NamespacedId>>,
    economy: Vec<Option<ResourceCategory>>,
    home_terrain: Vec<Option<TerrainKind>>,
    wall_terrain: Vec<Option<TerrainKind>>,
    founder_races: Vec<Vec<(ContentIndex, u32)>>,
    hostility: Vec<Vec<(ContentIndex, u32)>>,
    defined: Vec<bool>,
    order: Vec<CultureKind>,
    /// 「无文化」哨兵索引（`lostland:cultureless`，由
    /// `ll_mod::base_cultureless` 注册），`None` 表示这次会话没注册过
    /// 它、「无文化」这条判据整个不生效。
    ///
    /// # 为什么它住在这张表里，而不是在判定点另外传一个参数
    ///
    /// 它与 `hostility` 是**同一个问题的两半**：敌意表里那一行
    /// `(哨兵索引, 分数)` 只有配上「哨兵索引是哪一个」才读得懂。表已经
    /// 会跟着编年史一起走（见 `crate::chronicle::WorldChronicle::culture_table`
    /// 的字段注释「跟着编年史一起走，调用方就不需要再从别处凑一张可能
    /// 已经对不上号的表」），哨兵搭同一趟车，判定点因此**不需要任何新
    /// 参数**——`ll_sim::turn` 的撞格路由从 `WorldState` 现有的编年史
    /// 句柄就能把两半一起拿到。
    ///
    /// 它**不是**第二个真相源：真相源是 `Registry`（字符串 id ↔ 索引），
    /// 这里存的是那次解析的结果，与 `crate::settlement::SettlementSite::culture`
    /// 存一个索引而不是一个字符串是同一种缓存。
    cultureless: Option<ContentIndex>,
}

impl CultureTable {
    /// 建立空表。
    ///
    /// **空表是合法的**，且语义明确：「这个世界没有文化这一层」。
    /// 那样一来选址不含文化项、建材退回引擎默认的木/石、战争敌意恒为
    /// 0——三处退化都是**与本模块落地之前逐位相同**的行为，不 panic，
    /// 也不静默变成别的东西。这条性质同时是黄金基准重冻的「把改动关掉」
    /// 那一步所依赖的。
    pub fn new() -> Self {
        Self::default()
    }

    /// 记下这次会话的「无文化」哨兵索引，见 [`Self::cultureless`]。
    ///
    /// 生产调用点只有一处：`ll_mod::load_session::LoadSession::load_all`
    /// 在全部 mod 装载完成之后注册哨兵、随即写进这张表。重复调用直接
    /// 覆盖——没有「已经设过就拒绝」的校验，因为同一次会话里这个值只
    /// 可能来自同一次 `Registry::intern`，覆盖恒是幂等的。
    pub fn set_cultureless(&mut self, index: ContentIndex) {
        self.cultureless = Some(index);
    }

    /// 这次会话的「无文化」哨兵索引，没注册过就是 `None`。
    ///
    /// 消费者是 `ll_sim::ai_query::declared_hostile`：身上找不到
    /// `AffiliationKind::Culture` 归属的实体，判定时回退到这个索引，
    /// 于是「哥布林对无文化敌意 6」这条内容真的能咬到一个没有任何归属
    /// 的玩家。
    pub fn cultureless(&self) -> Option<ContentIndex> {
        self.cultureless
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上文化属性。
    ///
    /// # 校验（ADR 0017「注册期完整校验」）
    ///
    /// 1. **不得重复定义**——[`CultureError::DuplicateDefinition`]。
    /// 2. **敌意必须落在 `0..=`[`MAX_HOSTILITY`]**——
    ///    [`CultureError::HostilityOutOfRange`]。
    /// 3. **至少要有一个权重为正的建立者种族**——
    ///    [`CultureError::NoFounderRace`]。
    pub fn define(&mut self, index: ContentIndex, attrs: CultureAttrs) -> Result<(), CultureError> {
        for (_, hostility) in &attrs.hostility {
            if *hostility > MAX_HOSTILITY {
                return Err(CultureError::HostilityOutOfRange(*hostility));
            }
        }
        if !attrs.founder_races.iter().any(|(_, weight)| *weight > 0) {
            return Err(CultureError::NoFounderRace(index));
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.economy.resize(new_len, None);
            self.home_terrain.resize(new_len, None);
            self.wall_terrain.resize(new_len, None);
            self.founder_races.resize(new_len, Vec::new());
            self.hostility.resize(new_len, Vec::new());
        }

        if self.defined[idx] {
            return Err(CultureError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.economy[idx] = Some(attrs.economy);
        self.home_terrain[idx] = Some(attrs.home_terrain);
        self.wall_terrain[idx] = Some(attrs.wall_terrain);
        self.founder_races[idx] = attrs.founder_races;
        self.hostility[idx] = attrs.hostility;
        self.order.push(CultureKind::from_index(index));
        Ok(())
    }

    /// 给定索引当前是否已经登记为一种文化。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.defined
            .get(index.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 全部已注册文化，**按注册顺序**——遍历唯一允许的来源（约束 C5），
    /// 理由见类型文档。
    pub fn registered(&self) -> &[CultureKind] {
        &self.order
    }

    /// 展示名的本地化键。返回 `Option` 只是因为列式存储需要一个「这一格
    /// 还没被定义」的表示，理由同
    /// [`crate::resource::ResourceTable::display_name_key`]。
    pub fn display_name_key(&self, kind: CultureKind) -> Option<NamespacedId> {
        self.display_name_key
            .get(kind.index().get() as usize)
            .cloned()
            .flatten()
    }

    /// 这种文化靠哪个资源大类吃饭。
    pub fn economy(&self, kind: CultureKind) -> Option<ResourceCategory> {
        self.economy
            .get(kind.index().get() as usize)
            .copied()
            .flatten()
    }

    /// 这种文化的选址地形偏好。
    pub fn home_terrain(&self, kind: CultureKind) -> Option<TerrainKind> {
        self.home_terrain
            .get(kind.index().get() as usize)
            .copied()
            .flatten()
    }

    /// 这种文化盖房子用的墙。
    pub fn wall_terrain(&self, kind: CultureKind) -> Option<TerrainKind> {
        self.wall_terrain
            .get(kind.index().get() as usize)
            .copied()
            .flatten()
    }

    /// 这种文化的建立者种族候选与权重，**按声明顺序**（约束 C5）。
    /// 未定义的索引返回空切片。
    pub fn founder_races(&self, kind: CultureKind) -> &[(ContentIndex, u32)] {
        self.founder_races
            .get(kind.index().get() as usize)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// `attacker` 这种文化对 `defender` 那种文化的敌意分，没有声明就是
    /// 0（互不相干）。
    ///
    /// **有向**：`hostility(a, b)` 与 `hostility(b, a)` 可以不同，理由
    /// 见 [`CultureAttrs::hostility`]「刻意允许不对称」一节。
    ///
    /// 两个 `Option<CultureKind>` 而不是两个 `CultureKind`：调用方
    /// （`chronicle` 的战争推演）手上的据点**可能没有文化**（空文化表
    /// 的世界、或者内容里一条文化都没装载），让调用点各写一遍
    /// `if let Some(..)` 只会让那条 `0` 分散在两处。
    pub fn hostility(&self, attacker: Option<CultureKind>, defender: Option<CultureKind>) -> u32 {
        let (Some(attacker), Some(defender)) = (attacker, defender) else {
            return 0;
        };
        self.hostility
            .get(attacker.index().get() as usize)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|(target, _)| *target == defender.index())
                    .map(|(_, score)| *score)
            })
            .unwrap_or(0)
    }
}

/// 建立者种族抽取所用的随机流编号。
///
/// # 为什么这个常量从 `ll-mod` 搬到了这里
///
/// 它此前叫 `ll_mod::roster::FOUNDER_RACE_STREAM_ID`，与那一侧的
/// `settlement_founder_race` 一起住在名册模块里。**占领批次把「谁建的」
/// 这个问题变成了世界生成期的判据**：[`crate::chronicle`] 的战争结算
/// 要问「攻守双方是不是同一个种族」才能决定这一仗是占领还是毁灭，而
/// 它在 `ll-world` 里、拿不到 `ll-mod`（依赖方向反了）。
///
/// 两条出路：在 `ll-world` 里另写一份「同族判定」，或者把这一份算法
/// 搬下来让两边共用。前者就是 ADR 0021 明令拦下的那种重复——同一个
/// 问题两份实现，一旦权重表的解释方式变了就会分叉，而分叉的表现是
/// 「编年史说这是同族战争，名册里却是两个种族」。因此搬下来：
/// [`founder_race`] 是唯一实现，`ll_mod::roster::settlement_founder_race`
/// 现在只是它的一层薄封装（保留旧签名，调用点一个字没改）。
///
/// **取值一个字节都没变**，因此同一颗种子抽出的建立者种族与搬迁之前
/// 逐位相同——这条由 `ll-mod` 侧既有的名册测试守着。
pub const FOUNDER_RACE_STREAM_ID: u64 = 0x004E_5043_5F46_0001;

/// 这座据点的**建立者种族**——`(世界种子, 据点 id, 这座据点信的文化)`
/// 的纯函数，同一组入参恒产出同一个答案（约束 C3）。
///
/// 抽取按 [`CultureAttrs::founder_races`] 的**声明顺序**线性扫描加权
/// 命中，不碰任何哈希容器（约束 C5）。
///
/// # 三种返回 `None` 的情形，都不是错误
///
/// 1. 据点没有文化（`culture` 为 `None`）——空文化表的世界。
/// 2. 这条文化没被定义（索引越界）——递进来的表与产出这份据点快照的
///    不是同一张。
/// 3. 候选名单里全部权重都是 0——注册期校验
///    （[`CultureError::NoFounderRace`]）挡掉了正常路径上的这一种，
///    留着分支是因为类型上仍然表达得出来。
///
/// 三种都**一个随机数都不取**：空抽取不该悄悄推进随机流（判据与
/// `ll_mod::roster` 的 `pick` 逐字相同，这条正是搬迁必须保持逐位相同
/// 的那个细节）。
///
/// # 谁读它
///
/// - [`crate::chronicle`] 的战争结算：同族倾向占领、异族倾向毁灭。
/// - `ll_mod::roster::settlement_founder_race`：整座据点的主体人口。
///
/// 两个消费者读的是**同一个**答案，这正是它搬到这一层的理由。
pub fn founder_race(
    cultures: &CultureTable,
    culture: Option<CultureKind>,
    site: WorldId,
    world_seed: u64,
) -> Option<ContentIndex> {
    let slots = match culture {
        Some(kind) => cultures.founder_races(kind),
        None => &[],
    };
    let total: u64 = slots.iter().map(|(_, weight)| u64::from(*weight)).sum();
    if total == 0 {
        return None;
    }
    let mut rng = DetRng::for_entity(world_seed, FOUNDER_RACE_STREAM_ID, u64::from(site.get()));
    let mut roll = rng.gen_range(total);
    for (race, weight) in slots {
        let weight = u64::from(*weight);
        if roll < weight {
            return Some(*race);
        }
        roll -= weight;
    }
    // 理论不可达（`roll < total` 而循环恰好减掉了全部权重之和）。退回
    // 第一个候选而不是 panic，规格 §10.2「降级而非崩溃」。
    slots.first().map(|(race, _)| *race)
}

/// 单元测试用的一张小文化表：两种互相敌对的文化。
///
/// 与 [`crate::resource::base_resource_fixture`] 同一条既有惯例——
/// 测试与 demo 需要一张「像那么回事」的表，但它们拿不到 `ll-mod` 的
/// 装载器（依赖方向反了）。
///
/// 返回 `(表, [山地文化, 部落文化])`。山地文化守金属、住山里、砌石墙；
/// 部落文化守食物、住草地、搭木墙；**部落对山地有敌意
/// [`MAX_HOSTILITY`]，反向为 0**（刻意不对称，见
/// [`CultureAttrs::hostility`]）。
pub fn base_culture_fixture(
    mut intern: impl FnMut(&str) -> ContentIndex,
    metal_race: ContentIndex,
    tribal_race: ContentIndex,
    stone_wall: TerrainKind,
    wood_wall: TerrainKind,
    mountain: TerrainKind,
    grass: TerrainKind,
) -> (CultureTable, [CultureKind; 2]) {
    let mountain_id = intern("fixture:mountainfolk");
    let tribe_id = intern("fixture:tribe");
    let mut table = CultureTable::new();
    table
        .define(
            mountain_id,
            CultureAttrs {
                display_name_key: NamespacedId::parse("fixture:culture.mountainfolk.display_name")
                    .expect("夹具字面量合法"),
                economy: ResourceCategory::Metal,
                home_terrain: mountain,
                wall_terrain: stone_wall,
                founder_races: vec![(metal_race, 10)],
                hostility: Vec::new(),
            },
        )
        .expect("夹具文化互不重复");
    table
        .define(
            tribe_id,
            CultureAttrs {
                display_name_key: NamespacedId::parse("fixture:culture.tribe.display_name")
                    .expect("夹具字面量合法"),
                economy: ResourceCategory::Food,
                home_terrain: grass,
                wall_terrain: wood_wall,
                founder_races: vec![(tribal_race, 10)],
                hostility: vec![(mountain_id, MAX_HOSTILITY)],
            },
        )
        .expect("夹具文化互不重复");
    (
        table,
        [
            CultureKind::from_index(mountain_id),
            CultureKind::from_index(tribe_id),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{Interner, NamespacedId};

    fn fixture() -> (CultureTable, [CultureKind; 2]) {
        let mut interner = Interner::new();
        let mut next = |id: &str| interner.intern(NamespacedId::parse(id).expect("合法"));
        let race_a = next("fixture:dwarf");
        let race_b = next("fixture:goblin");
        let stone = TerrainKind::from_index(next("fixture:wall_stone"));
        let wood = TerrainKind::from_index(next("fixture:wall_wood"));
        let mountain = TerrainKind::from_index(next("fixture:mountain"));
        let grass = TerrainKind::from_index(next("fixture:grass"));
        base_culture_fixture(|id| next(id), race_a, race_b, stone, wood, mountain, grass)
    }

    #[test]
    fn 空表上一切查询都退化成中性答案而不是panic() {
        // Arrange
        let table = CultureTable::new();
        let kind = CultureKind::from_index(ContentIndex::default());

        // Act / Assert：空表 = 「这个世界没有文化这一层」，全部退化。
        assert!(table.registered().is_empty());
        assert_eq!(table.display_name_key(kind), None);
        assert_eq!(table.economy(kind), None);
        assert_eq!(table.home_terrain(kind), None);
        assert_eq!(table.wall_terrain(kind), None);
        assert!(table.founder_races(kind).is_empty());
        assert_eq!(table.hostility(Some(kind), Some(kind)), 0);
    }

    #[test]
    fn 敌意是有向的反向查不到() {
        // Arrange
        let (table, [mountainfolk, tribe]) = fixture();

        // Act
        let forward = table.hostility(Some(tribe), Some(mountainfolk));
        let backward = table.hostility(Some(mountainfolk), Some(tribe));

        // Assert：「哥布林恨矮人」不蕴含「矮人恨哥布林恨得一样深」。
        assert_eq!(forward, MAX_HOSTILITY);
        assert_eq!(backward, 0);
    }

    #[test]
    fn 没有文化的一方敌意恒为零() {
        // Arrange
        let (table, [mountainfolk, _]) = fixture();

        // Act / Assert
        assert_eq!(table.hostility(None, Some(mountainfolk)), 0);
        assert_eq!(table.hostility(Some(mountainfolk), None), 0);
        assert_eq!(table.hostility(None, None), 0);
    }

    #[test]
    fn 注册顺序被原样保留() {
        // Arrange / Act
        let (table, [mountainfolk, tribe]) = fixture();

        // Assert：遍历顺序是注册顺序，不是索引顺序，也不是哈希顺序。
        assert_eq!(table.registered(), &[mountainfolk, tribe]);
    }

    #[test]
    fn 重复定义同一个索引被拒绝() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("fixture:one").expect("合法"));
        let race = interner.intern(NamespacedId::parse("fixture:race").expect("合法"));
        let terrain = TerrainKind::from_index(
            interner.intern(NamespacedId::parse("fixture:terrain").expect("合法")),
        );
        let attrs = || CultureAttrs {
            display_name_key: NamespacedId::parse("fixture:culture.one.display_name")
                .expect("合法"),
            economy: ResourceCategory::Food,
            home_terrain: terrain,
            wall_terrain: terrain,
            founder_races: vec![(race, 1)],
            hostility: Vec::new(),
        };
        let mut table = CultureTable::new();
        table.define(index, attrs()).expect("第一次定义应当成功");

        // Act
        let second = table.define(index, attrs());

        // Assert
        assert_eq!(second, Err(CultureError::DuplicateDefinition(index)));
    }

    #[test]
    fn 敌意越界在注册期就被拒绝() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("fixture:one").expect("合法"));
        let other = interner.intern(NamespacedId::parse("fixture:two").expect("合法"));
        let race = interner.intern(NamespacedId::parse("fixture:race").expect("合法"));
        let terrain = TerrainKind::from_index(
            interner.intern(NamespacedId::parse("fixture:terrain").expect("合法")),
        );
        let mut table = CultureTable::new();

        // Act
        let result = table.define(
            index,
            CultureAttrs {
                display_name_key: NamespacedId::parse("fixture:culture.one.display_name")
                    .expect("合法"),
                economy: ResourceCategory::Food,
                home_terrain: terrain,
                wall_terrain: terrain,
                founder_races: vec![(race, 1)],
                hostility: vec![(other, MAX_HOSTILITY + 1)],
            },
        );

        // Assert：敌意达到分母就等于「够强必然开战」，闸门形同虚设。
        assert_eq!(
            result,
            Err(CultureError::HostilityOutOfRange(MAX_HOSTILITY + 1))
        );
    }

    #[test]
    fn 没有建立者种族的文化在注册期就被拒绝() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("fixture:one").expect("合法"));
        let race = interner.intern(NamespacedId::parse("fixture:race").expect("合法"));
        let terrain = TerrainKind::from_index(
            interner.intern(NamespacedId::parse("fixture:terrain").expect("合法")),
        );
        let mut table = CultureTable::new();

        // Act：权重全为 0 与「一条都没写」是同一件事。
        let result = table.define(
            index,
            CultureAttrs {
                display_name_key: NamespacedId::parse("fixture:culture.one.display_name")
                    .expect("合法"),
                economy: ResourceCategory::Food,
                home_terrain: terrain,
                wall_terrain: terrain,
                founder_races: vec![(race, 0)],
                hostility: Vec::new(),
            },
        );

        // Assert
        assert_eq!(result, Err(CultureError::NoFounderRace(index)));
    }
}
