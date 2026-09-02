//! 层/空间属性——「本体即 Mod」在 [`crate::space::Space`] 层面的落点。
//!
//! # 照抄 `terrain.rs` 已验证的模式
//!
//! [`crate::terrain`] 把地形从硬编码常量迁入内容注册表，验证过一套
//! 「私有字段 + `Table::define` 注册期校验 + `materialize_*` 本体注册
//! 入口 + `*_fixture` 测试夹具」的模式（见其模块文档）。`SpaceProfile`
//! 走同一条路径：一种空间类型（地表、洞窟、地下城、建筑内部……）是
//! `mod` 可以扩展的内容，不是编译期写死的枚举。
//!
//! # 与 Registry 的关系（依赖方向）
//!
//! 依赖顺序是 `ll-world` ← `ll-sim` ← `ll-script` ← `ll-mod`（规格
//! §5）：定义 `Registry` 的 `ll-mod` 在 `ll-world` **下游**，本 crate
//! 绝不能反过来依赖它。本模块因此不认识 `Registry` 这个类型，只依赖
//! `ll-core` 已有的 [`ContentIndex`]/[`NamespacedId`]。
//!
//! [`materialize_base_space_profiles`] 是本体层属性注册的唯一入口，
//! 签名接受一个 `&mut dyn FnMut(NamespacedId) -> ContentIndex` 回调，
//! 而不是接受一个具体的 `Registry`/`Interner` 类型——与
//! [`crate::terrain::materialize_base_terrain`] 完全同构：
//!
//! - 生产路径（`ll-mod`）传入 `|id| registry.intern(id)`，与 mod 注册
//!   内容走完全相同的一条代码路径。
//! - 测试/demo 路径用 [`base_space_profile_fixture`]——内部现造一个空
//!   [`Interner`]，不牵扯任何 mod 加载或 `Registry`。
//!
//! # 物化为列式数据，注册期完整校验（ADR 0016 / 0017）
//!
//! [`SpaceProfileTable`] 按属性分列，不按内容分结构——与
//! [`crate::terrain::TerrainTable`] 同一套道理。[`SpaceProfileTable::define`]
//! 是注册期入口，校验放在这里完整做一次，错误在加载时就报出来，而不是
//! 等玩到某个场景才表现成怪行为。
//!
//! # 与 `Space::profile` 的关系：无需一个额外的索引新类型
//!
//! [`crate::terrain`] 里 `TerrainKind` 是包了一层的 `ContentIndex`
//! 新类型，因为 `ChunkGrid` 需要一个专属类型区分"这是地形索引"。
//! `Space::Surface`/`Space::Interior` 的 `profile` 字段直接就是裸的
//! [`ContentIndex`]（见 [`crate::space`] 的形状），没有类似的存储专属
//! 语境需要额外包一层——因此本模块的查询方法直接接受 `ContentIndex`，
//! 不新增一个 `SpaceProfileKind` 包装类型。

use std::fmt;

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::time::Tick;

use crate::light::{LightLevel, ambient_light_under};
use crate::temperature::{Temperature, temperature_under};
use crate::weather::Weather;

/// 一种空间类型的静态属性，本体与 mod 注册层属性时共用的同一个输入
/// 形状——与 [`crate::terrain::TerrainDef`] 是同一个模式（本体的声明
/// 与未来 mod 的声明除了 `id` 里的命名空间字符串不同之外，不存在任何
/// 结构性差异）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceProfile {
    /// 命名空间标识符，例如 `lostland:surface`、`yourmod:abyss`。
    pub id: NamespacedId,
    /// 环境光基准，千分比（`0..=1000`），与
    /// [`crate::light::LightLevel`] 同一量纲。**仅在 `exposed_to_sky`
    /// 为假时生效**——见模块文档「与光照系统的组合」。
    pub ambient_light_floor: i32,
    /// 是否露天：为真时受昼夜、四季与**天气**影响（消费方走
    /// [`effective_ambient_light`]，天气那一层由 [`effective_weather`]
    /// 把关）；为假时环境光恒等于 `ambient_light_floor`，与世界时钟、
    /// 与外面在不在下雨都无关。
    pub exposed_to_sky: bool,
    /// 温度基准，十分之一摄氏度（`200` = 20℃），与
    /// [`crate::temperature::Temperature`] 同一量纲。
    ///
    /// 露天空间（`exposed_to_sky` 为真）把它当作全年均温，季节/天气/
    /// 昼夜三个偏移叠加在它之上；非露天空间的温度**恒等于**它。唯一
    /// 正确的查询入口是 [`effective_temperature`]。
    pub base_temperature: i32,
    /// 是否允许挖掘。
    pub diggable: bool,
    /// 是否允许建造。
    pub buildable: bool,
    /// 音效环境标签（回响等）。**仅预留字段，不在本次任务展开**——
    /// 点光源、声效系统如何消费它是后续批次的设计范围。
    pub reverb_tag: Option<NamespacedId>,
}

/// [`SpaceProfileTable::define`] 实际存进列式存储的属性子集——不含
/// `id`（`id` 只在注册那一刻用于换取 [`ContentIndex`]，换到之后就不再
/// 需要），与 [`crate::terrain::TerrainAttrs`] 相对 [`crate::terrain::TerrainDef`]
/// 同一个理由。**必须公开**：这是 [`SpaceProfileTable::define`] 唯一的
/// 参数类型，任何想直接调用 `define`（而不是走
/// [`materialize_base_space_profiles`] 那条便捷路径）的调用方——包括
/// 未来 mod 自己的层属性注册函数——都需要能构造这个类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceProfileAttrs {
    /// 环境光基准，千分比。
    pub ambient_light_floor: i32,
    /// 是否露天。
    pub exposed_to_sky: bool,
    /// 温度基准。
    pub base_temperature: i32,
    /// 是否允许挖掘。
    pub diggable: bool,
    /// 是否允许建造。
    pub buildable: bool,
    /// 音效环境标签，仅预留。
    pub reverb_tag: Option<NamespacedId>,
}

/// 层属性注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些
/// 错误在加载时就报出来，而不是等到查询某个具体空间时才表现成怪行为。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceProfileError {
    /// 同一个内容索引被定义了两次。
    ///
    /// 与 [`crate::terrain::TerrainError::DuplicateDefinition`] 同一条
    /// 纪律：`Interner::intern` 对同一个 `NamespacedId` 重复调用是
    /// 幂等的（返回同一个索引），但幂等的是「索引分配」，不是「这个
    /// 索引对应的层属性」——两个不同的 mod（或某 mod 与本体）若都尝试
    /// 给同一个 `id` 定义层属性，第二次必须报错，不能静默覆盖第一次的
    /// 结果。
    DuplicateDefinition(ContentIndex),
    /// `ambient_light_floor` 超出了 `0..=1000` 这个与
    /// [`crate::light::LightLevel`] 一致的千分比范围。
    ///
    /// 越界值不会在查询时立刻崩溃（消费方多半会做 clamp），但会让
    /// 「地下层环境光基准」这个数值失去与光照系统其余部分共用的量纲
    /// 含义，属于填错数据的信号，必须在注册期拦下，而不是留到玩家
    /// 走进这个空间时才表现成诡异的亮度。
    AmbientLightFloorOutOfRange(i32),
}

impl fmt::Display for SpaceProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpaceProfileError::DuplicateDefinition(index) => {
                write!(f, "空间层属性索引 {} 被重复定义", index.get())
            }
            SpaceProfileError::AmbientLightFloorOutOfRange(value) => {
                write!(f, "环境光基准 {value} 超出 0..=1000 的合法千分比范围")
            }
        }
    }
}

impl std::error::Error for SpaceProfileError {}

/// 层属性的列式存储：按 [`ContentIndex`] 下标索引，不按内容分结构
/// （ADR 0017），与 [`crate::terrain::TerrainTable`] 同一个理由。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分，不是「空间层属性
/// 专属」的连续编号——本体内容与地形、技能、物品等其他内容类型共享
/// 同一个 `Interner`/`Registry`。因此这里额外维护一份 `defined` 位图：
/// 数组下标落在表范围内不代表「这是一条层属性」，只有 `defined[idx]`
/// 为真才是。
#[derive(Debug, Default, Clone)]
pub struct SpaceProfileTable {
    ambient_light_floor: Vec<i32>,
    exposed_to_sky: Vec<bool>,
    base_temperature: Vec<i32>,
    diggable: Vec<bool>,
    buildable: Vec<bool>,
    reverb_tag: Vec<Option<NamespacedId>>,
    defined: Vec<bool>,
}

impl SpaceProfileTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上层属性。
    ///
    /// # 校验（ADR 0017「注册期完整校验」）
    ///
    /// 1. **不得重复定义**——见 [`SpaceProfileError::DuplicateDefinition`]
    ///    文档。
    /// 2. **`ambient_light_floor` 必须落在 `0..=1000`**——见
    ///    [`SpaceProfileError::AmbientLightFloorOutOfRange`] 文档。
    pub fn define(
        &mut self,
        index: ContentIndex,
        attrs: SpaceProfileAttrs,
    ) -> Result<(), SpaceProfileError> {
        if !(0..=1000).contains(&attrs.ambient_light_floor) {
            return Err(SpaceProfileError::AmbientLightFloorOutOfRange(
                attrs.ambient_light_floor,
            ));
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.ambient_light_floor.resize(new_len, 0);
            self.exposed_to_sky.resize(new_len, false);
            self.base_temperature.resize(new_len, 0);
            self.diggable.resize(new_len, false);
            self.buildable.resize(new_len, false);
            self.reverb_tag.resize(new_len, None);
        }

        if self.defined[idx] {
            return Err(SpaceProfileError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.ambient_light_floor[idx] = attrs.ambient_light_floor;
        self.exposed_to_sky[idx] = attrs.exposed_to_sky;
        self.base_temperature[idx] = attrs.base_temperature;
        self.diggable[idx] = attrs.diggable;
        self.buildable[idx] = attrs.buildable;
        self.reverb_tag[idx] = attrs.reverb_tag;
        Ok(())
    }

    /// [`effective_temperature`] 的**按索引**版本：只握着一个
    /// [`ContentIndex`]（而不是一份完整的 [`SpaceProfile`]）的调用方
    /// 用这一个（温度系统批次新增）。
    ///
    /// # 为什么需要这个入口
    ///
    /// 结算侧（`ll_sim::resolve`）从 `Agent::current_space` 拿到的正是
    /// 一个裸的 `ContentIndex`。走自由函数版本就必须先现拼一份完整的
    /// [`SpaceProfile`]——那需要一个 `id`，而结算侧根本没有理由知道
    /// 这个空间叫什么，只能像 `ll_game::app::space_profile_of` 那样
    /// 编造一个 `lostland:runtime_profile` 占位 id。为一次温度查询编造
    /// 一个假身份是纯粹的噪音。
    ///
    /// 两个入口共用同一份实现（模块私有的 `temperature_of`），「露天与
    /// 否」这条分支只写一处，不可能漂移。
    ///
    /// 未登记的索引在调试构建下触发断言（与本表其余访问器一致），发布
    /// 构建下按 `exposed_to_sky` 的兜底值（假）走非露天分支，得到
    /// `Temperature(0)`——0℃ 恰在冰点上、不触发暴露惩罚，是这里唯一
    /// 说得通的降级取值。**调用方应当先用 [`Self::is_defined`] 把关**，
    /// 见 `ll_sim::exposure::AmbientSource::temperature_in`。
    pub fn effective_temperature(
        &self,
        index: ContentIndex,
        tick: Tick,
        weather: Weather,
    ) -> Temperature {
        temperature_of(
            self.exposed_to_sky(index),
            self.base_temperature(index),
            tick,
            weather,
        )
    }

    /// 给定的层属性索引当前是否已经登记过。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.defined
            .get(index.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 环境光基准。未登记索引兜底为 0（视为全黑，安全侧——不会让一个
    /// 损坏/缺失 mod 的空间意外变得比设计更亮）。
    pub fn ambient_light_floor(&self, index: ContentIndex) -> i32 {
        debug_assert!(self.is_defined(index), "查询未注册的空间层属性: {index:?}");
        self.ambient_light_floor
            .get(index.get() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// 是否露天。未登记索引兜底为 `true`（视为露天，安全侧——套用既有
    /// `ambient_light(tick)` 曲线，不会让一个损坏/缺失 mod 的空间意外
    /// 陷入无法解释的永久黑暗）。
    pub fn exposed_to_sky(&self, index: ContentIndex) -> bool {
        debug_assert!(self.is_defined(index), "查询未注册的空间层属性: {index:?}");
        self.exposed_to_sky
            .get(index.get() as usize)
            .copied()
            .unwrap_or(true)
    }

    /// 温度基准。未登记索引兜底为 0。
    pub fn base_temperature(&self, index: ContentIndex) -> i32 {
        debug_assert!(self.is_defined(index), "查询未注册的空间层属性: {index:?}");
        self.base_temperature
            .get(index.get() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// 是否允许挖掘。未登记索引兜底为 `false`（安全侧——不给一个损坏/
    /// 缺失 mod 的空间意外开放破坏性操作）。
    pub fn diggable(&self, index: ContentIndex) -> bool {
        debug_assert!(self.is_defined(index), "查询未注册的空间层属性: {index:?}");
        self.diggable
            .get(index.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 是否允许建造。未登记索引兜底为 `false`，理由同 [`Self::diggable`]。
    pub fn buildable(&self, index: ContentIndex) -> bool {
        debug_assert!(self.is_defined(index), "查询未注册的空间层属性: {index:?}");
        self.buildable
            .get(index.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 音效环境标签。未登记索引兜底为 `None`。
    pub fn reverb_tag(&self, index: ContentIndex) -> Option<NamespacedId> {
        debug_assert!(self.is_defined(index), "查询未注册的空间层属性: {index:?}");
        self.reverb_tag.get(index.get() as usize).cloned().flatten()
    }
}

/// 本体四个基础空间类型在当前注册表里的索引缓存。
///
/// 由 [`materialize_base_space_profiles`] 在启动时一次性物化，与
/// [`crate::terrain::BaseTerrainIds`] 同一个模式：调用方此后按字段
/// 访问，是常量级开销。
#[derive(Debug, Clone, Copy)]
pub struct BaseSpaceProfileIds {
    /// 地表：露天，环境光跟随世界时钟。
    pub surface: ContentIndex,
    /// 洞窟：不露天，可挖掘，天然形成，不可建造。
    pub cave: ContentIndex,
    /// 地下城：不露天，结构化生成，不可挖掘也不可建造。
    pub dungeon: ContentIndex,
    /// 建筑内部：不露天，可建造（摆放家具等），不可挖掘。
    pub building_interior: ContentIndex,
}

/// 本体层属性注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调（生产路径是 `|id| registry.intern(id)`，
/// 测试/demo 路径是本模块的 [`base_space_profile_fixture`]）——本函数
/// 只管「拿到一个索引后，声明它的层属性」，不关心索引从哪个具体类型来,
/// 这正是保持 `ll-world` 不反向依赖 `ll-mod` 的关键（见模块文档「与
/// Registry 的关系」）。
///
/// 四种基础空间类型的具体数值是内容设计取舍，不是本文档的结构性
/// 论证——`ambient_light_floor`/`base_temperature` 的具体大小可以在
/// 后续批次调整，这里给出内部自洽的默认值：地表全程跟随世界时钟
/// （`exposed_to_sky = true`，`ambient_light_floor` 不生效，取 0）；
/// 洞窟、地下城环境光基准取 0（伸手不见五指，暗视/火把才有意义，见
/// 设计文档七节「连锁效果」）；建筑内部给一点非零的基准光（想象窗户/
/// 天窗漏进来的光），与纯粹的地下空间区分开。
///
/// # 温度基准（温度系统批次）
///
/// 四个取值分别是地表 200（20℃，全年均温）、洞窟 100（10℃）、地下城
/// 80（8℃）、建筑内部 220（22℃），单位见
/// [`crate::temperature`]。三个**非露天**空间的取值有一条硬约束：
/// **必须明确高于冰点**。它们的温度恒定不变（见
/// [`effective_temperature`]），一旦低于冰点就等于让玩家一进洞窟就
/// 永久挨罚——那正是 `ll_sim::exposure` 「只在极端条件下产生后果」这
/// 条纪律最不该出现的失效方式（一个躲不掉、也解释不清的常驻惩罚）。
/// 反过来，这条约束让「冬夜进洞躲一晚」成为一个真实有效的玩家决策，
/// 有测试钉住（见本模块 `三种非露天空间的温度恒定且明确高于冰点`）。
pub fn materialize_base_space_profiles(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseSpaceProfileIds, SpaceProfileTable), SpaceProfileError> {
    let mut table = SpaceProfileTable::new();

    let surface = define_base(
        &mut table,
        intern,
        "lostland:surface",
        0,
        true,
        200,
        true,
        true,
    )?;
    let cave = define_base(
        &mut table,
        intern,
        "lostland:cave",
        0,
        false,
        100,
        true,
        false,
    )?;
    let dungeon = define_base(
        &mut table,
        intern,
        "lostland:dungeon",
        0,
        false,
        80,
        false,
        false,
    )?;
    let building_interior = define_base(
        &mut table,
        intern,
        "lostland:building_interior",
        50,
        false,
        220,
        false,
        true,
    )?;

    Ok((
        BaseSpaceProfileIds {
            surface,
            cave,
            dungeon,
            building_interior,
        },
        table,
    ))
}

/// [`materialize_base_space_profiles`] 的内部帮手：把一条声明的字面量
/// 字段拆开传入，换取一次 `intern` + 一次 [`SpaceProfileTable::define`]。
/// 与 `terrain::define_base`（模块私有，无法作为 rustdoc 链接目标，
/// 是 `crates/ll-world/src/terrain.rs` 里同名的内部帮手）同一个理由
/// 抽成函数，避免四份几乎相同的样板代码互相漂移。`reverb_tag` 本次
/// 全部传 `None`——「仅预留字段，不展开」，见模块文档。
#[allow(clippy::too_many_arguments)]
fn define_base(
    table: &mut SpaceProfileTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    id: &str,
    ambient_light_floor: i32,
    exposed_to_sky: bool,
    base_temperature: i32,
    diggable: bool,
    buildable: bool,
) -> Result<ContentIndex, SpaceProfileError> {
    let index = intern(NamespacedId::parse(id).expect("本体空间层属性 id 字面量恒合法"));
    table.define(
        index,
        SpaceProfileAttrs {
            ambient_light_floor,
            exposed_to_sky,
            base_temperature,
            diggable,
            buildable,
            reverb_tag: None,
        },
    )?;
    Ok(index)
}

/// 求某个空间在某一世界时刻的有效环境光——`SpaceProfile` 与既有光照
/// 曲线（[`crate::light`]）的**组合点**，不是第二套光照实现。
///
/// # 为什么不能对 `Interior` 直接调用 [`ambient_light_under`]
///
/// [`ambient_light_under`] 只依赖世界时钟与天气，不知道调用它的是露天
/// 地表还是伸手不见五指的地下城——直接对 `Interior` 调用会让地下城在
/// 正午呈现满光照，这是纯粹的接线错误，不是设计冲突。任何消费光照的
/// 调用方都必须先经过本函数，不能绕过去直接调用
/// `ambient_light_under(tick, weather)`。
///
/// # 不是第二个真相源
///
/// [`crate::light`] 模块文档「光照是纯函数派生，绝不进世界状态」的
/// 纪律在这里继续成立，也是 ADR 0010「白昼判定收敛为同一份真相源」
/// 教训的直接延续：本函数不重新定义昼夜曲线、季节缩放或天气乘数的任何
/// 一段，`exposed_to_sky` 为真时原样转发给既有的
/// [`ambient_light_under`]；为假时改用 `profile.ambient_light_floor`，
/// 与世界时钟、与天气都完全无关——**这条地板值路径不知道、也不需要
/// 知道时钟走到哪、外面在下什么**，不存在与 [`ambient_light_under`]
/// 各自维护一份边界、彼此可能矛盾的风险。
///
/// # 天气（天气系统批次新增）
///
/// `weather` 是纯派生值（[`crate::weather::Weather::derive`]），不进
/// 世界状态。它只在露天分支生效，且先经 [`effective_weather`] 过一道
/// ——洞窟/地下城因此完全不受天气影响，这条语义与本函数原有的
/// `exposed_to_sky` 判定共用同一个字段，不是新增的第二条判据。
/// 只想要昼夜四季、不关心天气的调用方（测试、验收 demo）显式传
/// [`crate::weather::Weather::CLEAR`]。
///
/// 见 `knowledge/design/coordinate-system-and-layers.md` 七节「与既有
/// 光照系统的组合，而非替换」。
pub fn effective_ambient_light(profile: &SpaceProfile, tick: Tick, weather: Weather) -> LightLevel {
    if profile.exposed_to_sky {
        ambient_light_under(tick, effective_weather(profile, weather))
    } else {
        LightLevel(profile.ambient_light_floor)
    }
}

/// 天气在这个空间里**实际**是什么——非露天空间恒为
/// [`Weather::CLEAR`]，露天空间原样透传。
///
/// # 为什么需要这一步，而不是让每个消费者自己判断
///
/// 天气有两个独立的旋钮（光照与视野，见
/// [`crate::weather::WeatherDef::sight_scale`]），而
/// [`effective_ambient_light`] 只吃得下光照那一个。视野那一个由
/// `ll_game::layout::effective_sight_radius` 在另一处消费——若「洞窟不
/// 受天气影响」这条判断在两处各写一遍，迟早出现「洞窟不会变暗但会因为
/// 起雾而看不远」这种自相矛盾的行为。本函数是这条判断的**唯一**真相
/// 源：两个消费者都先经过它，`exposed_to_sky` 的语义因此只定义一次。
///
/// 判断依据只有 `exposed_to_sky` 一个字段——与
/// [`effective_ambient_light`] 完全一致，不引入第二条「算不算露天」的
/// 判据（ADR 0010 的教训）。
pub fn effective_weather(profile: &SpaceProfile, weather: Weather) -> Weather {
    if profile.exposed_to_sky {
        weather
    } else {
        Weather::CLEAR
    }
}

/// 这个空间在某一世界时刻、某种天气下**实际**的温度——温度系统唯一
/// 正确的查询入口（温度系统批次新增）。
///
/// - `exposed_to_sky` 为真：走 [`crate::temperature::temperature_under`]，
///   在 `profile.base_temperature` 之上叠加季节、天气、昼夜三个偏移。
/// - 为假：恒等于 `Temperature(profile.base_temperature)`，与世界时钟、
///   与外面在不在下雪都无关。
///
/// # 与 [`effective_ambient_light`] 逐字同构，不是第二条判据
///
/// 本函数的形状与 [`effective_ambient_light`] 完全一致，且**走的是同
/// 一个 [`effective_weather`]**——「这个空间受不受外界影响」这句话在
/// 整个仓库里只有那一处定义（见 [`effective_weather`] 文档「为什么需要
/// 这一步」一节）。温度是它的第三个消费者（前两个是环境光与视野半径），
/// 加进来时没有新增任何一条「算不算露天」的判断，这正是 ADR 0010
/// 「白昼判定收敛为同一份真相源」那条教训要求的做法。
///
/// 非露天分支为什么不叫 `temperature_floor`：`ambient_light_floor` 是
/// 一个**下限**（露天时不生效，非露天时兜底），而 `base_temperature`
/// 是一个**基准**（露天时是四个加数里的第一个，非露天时就是全部）。
/// 两个字段在露天分支里的角色不同，命名如实反映这一点。
///
/// 洞窟冬暖夏凉因此是免费得到的：本体洞窟 `base_temperature = 80`
/// （8℃）恒定，冬夜地表已经跌到 -4℃ 时，钻进洞窟就等于回到 8℃——
/// 「进洞躲一夜」这个玩家决策不需要任何额外建模就成立了。
pub fn effective_temperature(profile: &SpaceProfile, tick: Tick, weather: Weather) -> Temperature {
    temperature_of(
        profile.exposed_to_sky,
        profile.base_temperature,
        tick,
        weather,
    )
}

/// [`effective_temperature`] 与 [`SpaceProfileTable::effective_temperature`]
/// 共用的**唯一**一份实现——「露天与否」这条分支只写这一处。
///
/// # 为什么这里不像光照那样再调一次 [`effective_weather`]
///
/// [`effective_ambient_light`] 在露天分支里调用
/// `effective_weather(profile, weather)`，是因为它随后要把结果交给
/// [`ambient_light_under`] 这个**不认识 profile** 的函数；那次调用在
/// 露天分支里恒等于原样透传（见 [`effective_weather`] 实现），真正起
/// 作用的是非露天分支的 `Weather::CLEAR`。
///
/// 温度这一路的非露天分支**根本不走到天气**——它直接返回
/// `Temperature(base_temperature)`，三个偏移量一个都不叠。因此在露天
/// 分支里插一次恒等透传只是装饰，且会让两条路径在阅读时看起来像是
/// 「各自做了一次判断」。判据仍然只有 `exposed_to_sky` 这**一个字段**
/// ——与 [`effective_weather`]/[`effective_ambient_light`] 读的是同一个
/// ——没有引入第二条「算不算露天」的判断，ADR 0010 那条教训要防的正是
/// 「同一个问题有两个答案」，不是「同一个字段被读了两次」。
fn temperature_of(
    exposed_to_sky: bool,
    base_temperature: i32,
    tick: Tick,
    weather: Weather,
) -> Temperature {
    if exposed_to_sky {
        temperature_under(base_temperature, tick, weather)
    } else {
        Temperature(base_temperature)
    }
}

/// 供测试与验收 demo 使用：现造一个空 [`Interner`]，注册本体全部四个
/// 空间类型，返回可用的 `(BaseSpaceProfileIds, SpaceProfileTable)`。
///
/// **不是生产路径**——生产路径必须经过 `ll-mod::Registry::intern`（见
/// 模块文档「与 Registry 的关系」）。
pub fn base_space_profile_fixture() -> (BaseSpaceProfileIds, SpaceProfileTable) {
    let mut interner = Interner::new();
    materialize_base_space_profiles(&mut |id| interner.intern(id))
        .expect("本体空间层属性声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

    /// 一个仅在测试里现造的露天 profile：不经过表/注册表，直接构造
    /// `SpaceProfile`，因为 `effective_ambient_light` 只依赖这个结构体
    /// 本身的字段，不需要牵扯 `SpaceProfileTable`/`ContentIndex`。
    fn surface_like_profile() -> SpaceProfile {
        SpaceProfile {
            id: NamespacedId::parse("lostland:surface").expect("合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 200,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        }
    }

    /// 一个仅在测试里现造的不露天 profile，地板值取一个非零、能与
    /// `ambient_light` 曲线在正午/午夜的取值明显区分开的数。
    fn underground_like_profile() -> SpaceProfile {
        SpaceProfile {
            id: NamespacedId::parse("lostland:dungeon").expect("合法"),
            ambient_light_floor: 50,
            exposed_to_sky: false,
            base_temperature: 80,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        }
    }

    #[test]
    fn 露天profile的有效光照随时钟变化() {
        // Arrange
        let profile = surface_like_profile();
        let midnight = Tick(0);
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

        // Act
        let midnight_light = effective_ambient_light(&profile, midnight, Weather::CLEAR);
        let noon_light = effective_ambient_light(&profile, summer_noon, Weather::CLEAR);

        // Assert
        assert_ne!(midnight_light.0, noon_light.0);
    }

    #[test]
    fn 地下profile的有效光照恒为地板值不随时钟变化() {
        // Arrange
        let profile = underground_like_profile();
        let midnight = Tick(0);
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

        // Act
        let midnight_light = effective_ambient_light(&profile, midnight, Weather::CLEAR);
        let noon_light = effective_ambient_light(&profile, summer_noon, Weather::CLEAR);

        // Assert：两个相差极大的时刻算出同一个值，且这个值就是地板值——
        // 一次 assert_eq! 同时钉住「恒定」与「等于地板值」两条，不拆成
        // 两条各自平凡的断言。
        assert_eq!(
            (midnight_light.0, noon_light.0),
            (profile.ambient_light_floor, profile.ambient_light_floor)
        );
    }

    #[test]
    fn 露天profile的有效光照随天气变化() {
        // 天气接进光照管线这件事的直接落点：同一时刻、同一空间，换一种
        // 天气就该换一个亮度。
        // Arrange
        let profile = surface_like_profile();
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);
        let stormy = Weather {
            kind: None,
            light_scale: 600,
            sight_scale: 1000,
            temperature_offset: 0,
        };

        // Act
        let clear_light = effective_ambient_light(&profile, summer_noon, Weather::CLEAR);
        let stormy_light = effective_ambient_light(&profile, summer_noon, stormy);

        // Assert
        assert!(stormy_light < clear_light);
    }

    #[test]
    fn 非露天profile的有效光照不受天气影响() {
        // 洞窟里下不下雨都一样黑——`exposed_to_sky` 的语义，天气必须
        // 落在露天那一侧。
        // Arrange
        let profile = underground_like_profile();
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);
        let stormy = Weather {
            kind: None,
            light_scale: 100,
            sight_scale: 100,
            temperature_offset: 0,
        };

        // Act
        let clear_light = effective_ambient_light(&profile, summer_noon, Weather::CLEAR);
        let stormy_light = effective_ambient_light(&profile, summer_noon, stormy);

        // Assert
        assert_eq!(
            (clear_light.0, stormy_light.0),
            (profile.ambient_light_floor, profile.ambient_light_floor)
        );
    }

    #[test]
    fn 非露天profile把天气整个换成晴空基准() {
        // effective_weather 是「洞窟不受天气影响」这条判断的唯一真相源
        // ——视野那一路（ll_game::layout）也走它，两个乘数必须一起被
        // 中和掉，否则会出现「洞窟不变暗但因为起雾看不远」的矛盾行为。
        // Arrange
        let outdoor = surface_like_profile();
        let indoor = underground_like_profile();
        let foggy = Weather {
            kind: None,
            light_scale: 900,
            sight_scale: 300,
            temperature_offset: 0,
        };

        // Act & Assert
        assert_eq!(effective_weather(&outdoor, foggy), foggy);
        assert_eq!(effective_weather(&indoor, foggy), Weather::CLEAR);
    }

    #[test]
    fn 地表的exposed_to_sky为真() {
        // Arrange
        let (ids, table) = base_space_profile_fixture();

        // Act & Assert
        assert!(table.exposed_to_sky(ids.surface));
    }

    #[test]
    fn 地下城的exposed_to_sky为假() {
        // Arrange
        let (ids, table) = base_space_profile_fixture();

        // Act & Assert
        assert!(!table.exposed_to_sky(ids.dungeon));
    }

    #[test]
    fn 洞窟允许挖掘() {
        // Arrange
        let (ids, table) = base_space_profile_fixture();

        // Act & Assert
        assert!(table.diggable(ids.cave));
    }

    #[test]
    fn 地下城不允许挖掘() {
        // 结构化生成的内容，不是天然洞穴——不应该被随意挖穿。
        // Arrange
        let (ids, table) = base_space_profile_fixture();

        // Act & Assert
        assert!(!table.diggable(ids.dungeon));
    }

    #[test]
    fn 建筑内部允许建造() {
        // Arrange
        let (ids, table) = base_space_profile_fixture();

        // Act & Assert
        assert!(table.buildable(ids.building_interior));
    }

    #[test]
    fn 地下城的环境光基准为零() {
        // 地下城不露天（exposed_to_sky 为假），环境光完全由这个基准值
        // 决定，与世界时钟无关——设计文档七节「连锁效果」要求它接近
        // 零，视野半径完全靠光源与暗视撑起来。
        // Arrange
        let (ids, table) = base_space_profile_fixture();

        // Act
        let dungeon_floor = table.ambient_light_floor(ids.dungeon);

        // Assert
        assert_eq!(dungeon_floor, 0);
    }

    #[test]
    fn 重复定义同一个索引返回错误而非静默覆盖() {
        // 与 TerrainTable 同一条纪律：两个 mod（或某 mod 与本体）都
        // 尝试给同一个内容索引定义层属性时，第二次必须报错。
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("lostland:surface").expect("合法"));
        let mut table = SpaceProfileTable::new();
        table
            .define(
                index,
                SpaceProfileAttrs {
                    ambient_light_floor: 0,
                    exposed_to_sky: true,
                    base_temperature: 200,
                    diggable: true,
                    buildable: true,
                    reverb_tag: None,
                },
            )
            .expect("首次定义应当成功");

        // Act
        let result = table.define(
            index,
            SpaceProfileAttrs {
                ambient_light_floor: 0,
                exposed_to_sky: false,
                base_temperature: 80,
                diggable: false,
                buildable: false,
                reverb_tag: None,
            },
        );

        // Assert
        assert_eq!(result, Err(SpaceProfileError::DuplicateDefinition(index)));
    }

    #[test]
    fn 环境光基准超出千分比范围时注册失败() {
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("yourmod:broken").expect("合法"));
        let mut table = SpaceProfileTable::new();

        // Act
        let result = table.define(
            index,
            SpaceProfileAttrs {
                ambient_light_floor: 1001,
                exposed_to_sky: false,
                base_temperature: 0,
                diggable: false,
                buildable: false,
                reverb_tag: None,
            },
        );

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn 本体profile与假想mod的profile调用同一个公开define函数完成注册() {
        // 结构等价的最小验收：本体注册完之后，用同一个 Interner
        // 追加一条假想 mod 风格的层属性声明，验证走的是同一个
        // intern + define 调用组合，没有任何本体专属的特权通道。
        //
        // 边界：本测试只证明本体与 mod（这里甚至只是假想的、直接用
        // Interner 构造的 mod 风格 id，不经过 ll-mod 的 Registry 或
        // 任何脚本）走同一条注册路径，不能证明 mod 脚本调得到这套
        // API——`ll-world` 本身也不认识脚本这个概念（见本模块「与
        // Registry 的关系」一节）。真正的「脚本能注册」证据在
        // `ll-mod` 的 `crate::pipeline` 脚本装载测试与
        // `mods/example_mod/gameplay.scm`。
        // Arrange
        let mut interner = Interner::new();
        let (_ids, mut table) = materialize_base_space_profiles(&mut |id| interner.intern(id))
            .expect("本体空间层属性声明表内部一致");
        let mod_index = interner.intern(NamespacedId::parse("yourmod:abyss").expect("合法"));

        // Act
        let result = table.define(
            mod_index,
            SpaceProfileAttrs {
                ambient_light_floor: 0,
                exposed_to_sky: false,
                base_temperature: -50,
                diggable: false,
                buildable: false,
                reverb_tag: Some(NamespacedId::parse("yourmod:abyssal_echo").expect("合法")),
            },
        );

        // Assert
        assert!(result.is_ok());
    }

    #[test]
    #[should_panic(expected = "查询未注册的空间层属性")]
    fn 未登记的索引查询在调试构建下触发断言() {
        // 与 TerrainTable 同一套开发期安全网：提示本体新增空间类型却
        // 忘了登记，或调用方传入了垃圾索引。`cargo test` 默认是 debug
        // 构建（debug_assertions 开启），这里直接验证断言确实会触发。
        // Arrange
        let mut interner = Interner::new();
        let unregistered =
            interner.intern(NamespacedId::parse("yourmod:never_defined").expect("合法"));
        let table = SpaceProfileTable::new();

        // Act
        let _ = table.exposed_to_sky(unregistered);
    }

    #[test]
    fn 露天空间的温度随昼夜与季节变化() {
        // effective_temperature 露天分支的存在意义：同一个空间在不同
        // 时刻应当有不同温度。
        // Arrange
        let profile = surface_like_profile();
        let summer_noon = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);
        let winter_midnight = Tick(90 * TICKS_PER_DAY);

        // Act
        let hot = effective_temperature(&profile, summer_noon, Weather::CLEAR);
        let cold = effective_temperature(&profile, winter_midnight, Weather::CLEAR);

        // Assert
        assert!(hot > cold, "夏季正午 {hot:?} 应当明显热于冬季午夜 {cold:?}");
        assert!(!hot.is_freezing());
        assert!(cold.is_freezing());
    }

    #[test]
    fn 非露天空间的温度恒等于基准与时钟天气都无关() {
        // 与「洞窟里下不下雨都一样黑」逐字同构的一条：非露天分支既不看
        // 时钟也不看天气。
        // Arrange
        let mut profile = surface_like_profile();
        profile.exposed_to_sky = false;
        profile.base_temperature = 100;
        let (ids, table) = crate::weather::base_weather_fixture();
        let snow = Weather {
            kind: Some(ids.snowfall),
            light_scale: table.light_scale(ids.snowfall),
            sight_scale: table.sight_scale(ids.snowfall),
            temperature_offset: table.temperature_offset(ids.snowfall),
        };

        // Act
        let winter_midnight = effective_temperature(&profile, Tick(90 * TICKS_PER_DAY), snow);
        let summer_noon = effective_temperature(
            &profile,
            Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR),
            Weather::CLEAR,
        );

        // Assert
        assert_eq!(winter_midnight, Temperature(100));
        assert_eq!(summer_noon, Temperature(100));
    }

    #[test]
    fn 温度与光照读的是同一个露天判据() {
        // 「这个空间受不受天气影响」只有一条判据（exposed_to_sky）：给
        // 非露天空间传一个温度偏移极大的天气，温度不受任何影响——与
        // effective_weather 对光照那一路的把关结论完全一致。
        // Arrange
        let mut indoor = surface_like_profile();
        indoor.exposed_to_sky = false;
        indoor.base_temperature = 220;
        let outdoor = surface_like_profile();
        let blizzard = Weather {
            kind: None,
            light_scale: 1000,
            sight_scale: 1000,
            temperature_offset: -400,
        };
        let tick = Tick(30 * TICKS_PER_DAY + 12 * TICKS_PER_HOUR);

        // Act
        let inside = effective_temperature(&indoor, tick, blizzard);
        let outside = effective_temperature(&outdoor, tick, blizzard);

        // Assert
        assert_eq!(inside, Temperature(220), "非露天空间必须完全无视天气");
        assert!(
            outside.0 < 220,
            "露天空间必须真的被这场天气压冷，否则本测试无法区分两条分支"
        );
    }

    #[test]
    fn 按索引与按结构体两个温度入口结论恒一致() {
        // 两个公开入口共用同一份实现，这条测试是那句话的机器检查——
        // 将来若有人给其中一个入口单独加了分支，这里立刻变红。
        // Arrange
        let (ids, table) = base_space_profile_fixture();
        let (weather_ids, weather_table) = crate::weather::base_weather_fixture();
        let snow = Weather {
            kind: Some(weather_ids.snowfall),
            light_scale: weather_table.light_scale(weather_ids.snowfall),
            sight_scale: weather_table.sight_scale(weather_ids.snowfall),
            temperature_offset: weather_table.temperature_offset(weather_ids.snowfall),
        };

        // Act & Assert
        for index in [ids.surface, ids.cave, ids.dungeon, ids.building_interior] {
            let profile = SpaceProfile {
                id: NamespacedId::parse("lostland:probe").expect("合法"),
                ambient_light_floor: table.ambient_light_floor(index),
                exposed_to_sky: table.exposed_to_sky(index),
                base_temperature: table.base_temperature(index),
                diggable: table.diggable(index),
                buildable: table.buildable(index),
                reverb_tag: table.reverb_tag(index),
            };
            for hour in [0, 6, 12, 18] {
                let tick = Tick(90 * TICKS_PER_DAY + hour * TICKS_PER_HOUR);
                assert_eq!(
                    table.effective_temperature(index, tick, snow),
                    effective_temperature(&profile, tick, snow),
                );
            }
        }
    }

    #[test]
    fn 三种非露天空间的温度恒定且明确高于冰点() {
        // materialize_base_space_profiles 文档「温度基准」一节那条硬
        // 约束：一个低于冰点的恒温空间等于永久惩罚，是 exposure 那条
        // 「只在极端条件下」纪律最不该出现的失效方式。
        // Arrange
        let (ids, table) = base_space_profile_fixture();
        let indoor_indices = [ids.cave, ids.dungeon, ids.building_interior];

        // Act & Assert
        for index in indoor_indices {
            assert!(!table.exposed_to_sky(index));
            let base = table.base_temperature(index);
            assert!(
                base > Temperature::FREEZING.0,
                "非露天空间的温度基准 {base} 必须明确高于冰点"
            );
        }
    }
}
