//! 天气——晴/阴/雨/大风/雾/雪，以及它们对光照、视野与温度的影响。
//!
//! # 天气是纯派生的，零存档状态
//!
//! 本模块**没有任何字段进 [`crate::state::WorldState`]**。「此刻是什么
//! 天气」由 `(世界种子, 世界时钟)` 这一对输入现算得出，与
//! [`crate::light`] 模块开篇「光照是纯函数派生，绝不进世界状态」是同
//! 一条纪律的延续，理由也完全相同，只是后果更严重一层：
//!
//! 1. **零存档字段、零同步问题**。天气若存进 `WorldState`，就必须有人
//!    负责在时钟推进时把它改掉；漏改一次就表现成「时间过了三天还在下
//!    同一场雨」，而查代码时时钟和天气各自都对，只有两者一起看才发现
//!    矛盾——与光照缓存失同步是同一类缺陷。
//! 2. **约束 C3/C4 天然满足**。天气的随机性全部来自
//!    [`DetRng::for_entity`]（C3），而它的三个输入里没有任何一个依赖
//!    「谁先算」「算过几次」——同一个 `(种子, 刻度)` 在任何线程、任何
//!    平台、重放的任何一遍都得到同一个答案（C4）。后台推进到确定 tick
//!    之后再问天气，答案与从头逐 tick 走过来完全一致。
//! 3. **内容哈希与存档格式不受影响**。天气本身不进 `WorldState::hash()`；
//!    进内容值哈希的是[`WeatherDef`]这张**内容表**的字段值（见
//!    `ll_mod::content_hash`），那是「装了哪些 mod」这条维度，与「这局
//!    第 37 天早上下不下雨」无关。
//!
//! 代价是**天气不可被改写**：没有「求雨术」这种能改变天气的效果。这不
//! 是死路——本项目已经反复复用的「默认派生，只存偏差」模式（地形、
//! 探索记忆、脚本状态都是这个形状）正是为此准备的：将来真要做求雨，
//! 就在 `WorldState` 里加一张**稀疏的**「某段刻度区间被改写成某种天气」
//! 偏差表，查询时先查偏差、查不到再落回本模块的派生基线。本批次刻意
//! 不做偏差那一半（没有任何消费者，做了就是又一处「声明了没人读」），
//! 但 [`Weather::derive`] 的形状容得下它：它是一个纯查询函数，将来在
//! 它前面插一层偏差查询，不需要改动任何调用点。
//!
//! # 为什么天气表定义在 `ll-world`，而不是 `ll-mod`
//!
//! 与 [`crate::terrain`]/[`crate::space_profile`] 同一个理由：天气的唯
//! 一强制消费者是 [`crate::light`] 的环境光管线，而 `ll-light` 就在本
//! crate。若把表放进下游的 `ll-mod`，`ll-world` 就得反向依赖它（规格
//! §5 的依赖顺序不允许）。本模块因此不认识 `Registry`，
//! [`materialize_base_weathers`] 只接受一个
//! `&mut dyn FnMut(NamespacedId) -> ContentIndex` 解析回调——生产路径
//! （`ll_mod::base_weather`）传 `|id| registry.intern(id)`，测试/demo
//! 走 [`base_weather_fixture`]。
//!
//! # 物化为扁平列，注册期完整校验（ADR 0016 / 0017）
//!
//! 天气是静态声明，落 ADR 0016/0017 的**第一档**：注册期物化成按
//! [`ContentIndex`] 下标索引的扁平列，查询是常量级下标访问，没有任何
//! 跨脚本边界调用。这一条对天气比对别的表更要紧——环境光是**每帧每格**
//! 都要算的热路径，一次跨 Steel 边界调用要 326ns，放进去会直接毁掉
//! 帧率。[`Weather::derive`] 每帧只调用一次（不是每格一次），随后各格
//! 复用同一个 [`Weather`] 值。
//!
//! # 约束 C5：按权重选取必须走确定的遍历顺序
//!
//! 加权选取要遍历「已注册的全部天气」。这里**不用 `HashMap`/`HashSet`**
//! ——[`WeatherTable`] 额外维护一份 [`WeatherTable::registered`] 注册
//! 顺序列表，加权选取只沿这个 `Vec` 走。用哈希容器的遍历顺序做加权
//! 选取，会让同一个种子在不同运行里选出不同天气，是 C5 想拦的正是这
//! 一类缺陷。
//!
//! # 全程整数（ADR 0020 乙区）
//!
//! 光照乘数、视野乘数、温度偏移、季节权重全部是整数（前两者是千分比，
//! 与 [`crate::light::LightLevel`] 同一量纲；温度偏移是十分之一摄氏度，
//! 与 [`crate::temperature::Temperature`] 同一量纲）。天气经环境光影响
//! 视野半径、经温度影响结算，两者都是玩法量，必须量化，一个浮点都不能有。

use ll_core::ident::{ContentIndex, Interner, NamespacedId};
use ll_core::rng::DetRng;
use ll_core::time::{Season, TICKS_PER_HOUR, Tick};

use std::fmt;

/// 千分比乘数的「不缩放」基准值。
pub const WEATHER_SCALE_ONE: i32 = 1000;

/// [`WeatherDef::temperature_offset`] 绝对值的上界，十分之一摄氏度
/// （500 = 50℃）。
///
/// # 为什么要有上界，为什么取这个数
///
/// 与两个乘数的 `0..=1000` 同一条 ADR 0017「注册期完整校验」纪律：一条
/// 越界的声明应当在**装载时**报错，而不是等玩到某个下雪的冬夜才表现成
/// 「玩家一出门就冻僵」这种查不出来源的怪行为。
///
/// 上界不卡得更死（例如 ±100）是刻意的：本体六种天气的偏移全部落在
/// ±80 以内，但「岩浆喷发的灰烬雨让地表升温 30℃」「魔法极寒降下 40℃」
/// 这类 mod 设定都在合理的设计空间里，把上界压到本体用量附近等于用
/// 校验替设计做主。±50℃ 足够容纳任何说得通的天气，同时挡住把这一列当
/// 成开关乱填一个七位数的笔误——那正是校验该拦的东西。
///
/// 上下界对称（不像 `sight_scale` 只封上界）：天气**变暖**与**变冷**
/// 在语义上完全对等（焚风、热浪 vs 寒潮），没有任何一侧需要被特殊对待。
pub const WEATHER_TEMPERATURE_OFFSET_LIMIT: i32 = 500;

/// 一段天气持续多少刻度。
///
/// 取四小时（[`TICKS_PER_HOUR`] × 4 = 144000 刻度）：一天分成六段，
/// 一段约等于 1440 次基础行动（`BASE_ACTION_COST` 为 100 刻度）。再短
/// 会让天气频繁跳变、每次跳变都伴随一次可见的亮度台阶；再长则一整个
/// 游戏日可能只有一种天气，玩家感受不到「天气会变」这件事。
///
/// **这是一个玩法手感取值，不是结构性常量**——调整它会改变同一个种子
/// 下的天气序列（周期序号变了，[`DetRng`] 的事件计数也就变了），但不
/// 会破坏任何确定性保证：改完之后同一个种子仍然稳定复现同一串天气，
/// 只是与改之前那一串不同。
pub const WEATHER_PERIOD_TICKS: i64 = 4 * TICKS_PER_HOUR;

/// 天气这条随机流在 [`DetRng::for_entity`] 里占用的「实体 ID」。
///
/// [`DetRng::for_entity`] 的三元组是 `(世界种子, 实体 ID, 事件计数)`。
/// 天气不属于任何实体，但它需要一条**与任何实体都不会撞车**的独立流
/// ——撞车会让某个实体的行动随机数与天气产生可察觉的关联（例如「每次
/// 下雨时那个哥布林都恰好暴击」）。
///
/// 取 ASCII 字符串 `"WEATHER"` 的字节值（`0x0057_4541_5448_4552`，约
/// 2.4×10^16）：实体 ID 由 `ll_world::entity::EntityId` 从 0 单调递增
/// 分配，一局游戏不可能生成两千万亿个实体，因此这个取值与实体号段永远
/// 不相交；同时它是可读的字面量，一眼能看出这条流是给谁的。
pub const WEATHER_STREAM_ID: u64 = 0x0057_4541_5448_4552;

/// 一种天气的静态属性——本体与 mod 注册天气时共用的同一个输入形状，
/// 与 [`crate::space_profile::SpaceProfile`]/[`crate::terrain::TerrainDef`]
/// 是同一个模式（本体声明与 mod 声明除了 `id` 里的命名空间字符串之外
/// 不存在任何结构性差异）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherDef {
    /// 命名空间标识符，例如 `lostland:rain`、`yourmod:ashfall`。
    pub id: NamespacedId,
    /// 展示名的 Fluent 本地化键，例如 `lostland:weather.rain.display_name`。
    /// 状态栏（`ll_ui::hud::status_bar`）拿它查出「雨」两个字给玩家看。
    pub display_name_key: NamespacedId,
    /// 环境光乘数，千分比（`0..=1000`）。晴天为
    /// [`WEATHER_SCALE_ONE`]，阴雨天更低。是
    /// [`crate::light::ambient_light_under`] 的第三个因子，只对露天空间
    /// 生效（见 [`crate::space_profile::effective_weather`]）。
    pub light_scale: i32,
    /// 视野半径乘数，千分比（`0..=1000`）。与 [`Self::light_scale`] 分
    /// 成两个旋钮而不是一个：雾**不怎么变暗但极大缩短能看多远**，阴天
    /// 则相反（明显变暗但看得一样远）。只有一个乘数的话，这两种天气在
    /// 玩法上就只能是同一种东西的强弱版本。
    pub sight_scale: i32,
    /// 天气对温度的**增量**偏移，十分之一摄氏度，必须落在
    /// `-WEATHER_TEMPERATURE_OFFSET_LIMIT..=WEATHER_TEMPERATURE_OFFSET_LIMIT`
    /// （见 [`WEATHER_TEMPERATURE_OFFSET_LIMIT`]）。
    ///
    /// # 为什么是增量而不是乘数
    ///
    /// [`Self::light_scale`]/[`Self::sight_scale`] 是**乘数**，因为光照
    /// 与视野都有一个天然的零点（全黑、看不见），天气「打折」这件事
    /// 说得通。温度没有这样的零点——摄氏零度只是水的相变点，不是「没有
    /// 温度」，把 20℃ 乘以 0.8 得 16℃ 而把 -20℃ 乘以同一个 0.8 却得
    /// -16℃（**变暖了**）显然荒谬。因此这一列是加法项，与
    /// [`crate::temperature`] 里季节/昼夜两个偏移量同一种形状，三者
    /// 直接相加。
    ///
    /// 取 0 表示这种天气不影响温度（本体的「晴」就是 0）。
    pub temperature_offset: i32,
    /// 四季各自的出现权重，下标由 [`season_slot`] 给出（春/夏/秋/冬）。
    ///
    /// 权重是相对值，不必加起来等于任何特定的数；某一季全部天气权重之
    /// 和为 0 时[`weather_kind_at`]退化成「无天气」（晴空基准）。取 0
    /// 表示这种天气在这一季**绝不出现**——本体的雪就是靠这个把自己钉在
    /// 秋冬两季。
    pub season_weights: [u32; 4],
}

/// [`WeatherTable::define`] 实际存进列式存储的属性子集——不含 `id`
/// （`id` 只在注册那一刻用于换取 [`ContentIndex`]），与
/// [`crate::space_profile::SpaceProfileAttrs`] 相对 `SpaceProfile` 同一
/// 个理由。**必须公开**：这是 [`WeatherTable::define`] 唯一的参数类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherAttrs {
    /// 展示名的 Fluent 本地化键。
    pub display_name_key: NamespacedId,
    /// 环境光乘数，千分比。
    pub light_scale: i32,
    /// 视野半径乘数，千分比。
    pub sight_scale: i32,
    /// 温度增量偏移，十分之一摄氏度，见 [`WeatherDef::temperature_offset`]。
    pub temperature_offset: i32,
    /// 四季出现权重，下标见 [`season_slot`]。
    pub season_weights: [u32; 4],
}

/// 天气注册期可能出现的错误。ADR 0017「注册期完整校验」要求这些错误在
/// 加载时就报出来，而不是等玩到某个下雨的下午才表现成怪行为。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherError {
    /// 同一个内容索引被定义了两次。纪律与
    /// [`crate::space_profile::SpaceProfileError::DuplicateDefinition`]
    /// 完全一致：`Interner::intern` 幂等的是「索引分配」，不是「这个索引
    /// 对应的天气属性」，第二次定义必须报错而不是静默覆盖。
    DuplicateDefinition(ContentIndex),
    /// [`WeatherDef::light_scale`] 超出 `0..=1000` 这个与
    /// [`crate::light::LightLevel`] 一致的千分比范围。
    LightScaleOutOfRange(i32),
    /// [`WeatherDef::sight_scale`] 超出 `0..=1000`。
    ///
    /// 上界卡在 1000（而不是允许 >1000 的「放大视野」）是刻意的：视野
    /// **放大**是暗视、望远镜这类**观察者**属性该做的事，不是天气该做
    /// 的——天气对所有人一视同仁地遮挡视线，让某种天气反而让所有人看
    /// 得更远，在玩法语义上说不通。
    SightScaleOutOfRange(i32),
    /// [`WeatherDef::temperature_offset`] 的绝对值超出
    /// [`WEATHER_TEMPERATURE_OFFSET_LIMIT`]，见该常量文档。
    TemperatureOffsetOutOfRange(i32),
}

impl fmt::Display for WeatherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WeatherError::DuplicateDefinition(index) => {
                write!(f, "天气索引 {} 被重复定义", index.get())
            }
            WeatherError::LightScaleOutOfRange(value) => {
                write!(f, "天气光照乘数 {value} 超出 0..=1000 的合法千分比范围")
            }
            WeatherError::SightScaleOutOfRange(value) => {
                write!(f, "天气视野乘数 {value} 超出 0..=1000 的合法千分比范围")
            }
            WeatherError::TemperatureOffsetOutOfRange(value) => {
                write!(
                    f,
                    "天气温度偏移 {value} 超出 ±{WEATHER_TEMPERATURE_OFFSET_LIMIT} 的合法范围（单位：十分之一摄氏度）"
                )
            }
        }
    }
}

impl std::error::Error for WeatherError {}

/// 把 [`Season`] 映射成 [`WeatherDef::season_weights`] 的下标。
///
/// 单独成函数而不是让调用方各自写 `match`：四季与下标的对应关系一旦有
/// 两处各写一份，就会出现「春天用了冬天的权重」这种查起来极痛苦的错位。
pub const fn season_slot(season: Season) -> usize {
    match season {
        Season::Spring => 0,
        Season::Summer => 1,
        Season::Autumn => 2,
        Season::Winter => 3,
    }
}

/// 天气的列式存储：按 [`ContentIndex`] 下标索引（ADR 0017），与
/// [`crate::space_profile::SpaceProfileTable`] 同一套形状。
///
/// 下标空间是**全局** `ContentIndex` 号段的一部分，因此额外维护一份
/// `defined` 位图：下标落在表范围内不代表「这是一种天气」。
///
/// 比 `SpaceProfileTable` 多一份 [`Self::registered`] 注册顺序列表——
/// 天气是唯一一张需要**按权重遍历全表**的内容表（其余表都是按索引点查），
/// 而 `defined` 位图的下标顺序虽然也是确定的，却会随着「同一次装载里
/// 别的内容表 intern 了多少条」而变化。注册顺序列表把「遍历哪些、按什么
/// 顺序」这件事钉死在天气自己的注册顺序上，见模块文档「约束 C5」一节。
#[derive(Debug, Default, Clone)]
pub struct WeatherTable {
    display_name_key: Vec<Option<NamespacedId>>,
    light_scale: Vec<i32>,
    sight_scale: Vec<i32>,
    temperature_offset: Vec<i32>,
    season_weights: Vec<[u32; 4]>,
    defined: Vec<bool>,
    order: Vec<ContentIndex>,
}

impl WeatherTable {
    /// 建立空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册期入口：给一个已经 `intern` 出来的索引附上天气属性。
    ///
    /// # 校验（ADR 0017「注册期完整校验」）
    ///
    /// 1. **不得重复定义**——见 [`WeatherError::DuplicateDefinition`]。
    /// 2. **两个乘数都必须落在 `0..=1000`**——见
    ///    [`WeatherError::LightScaleOutOfRange`]/
    ///    [`WeatherError::SightScaleOutOfRange`]。
    ///
    /// 季节权重不校验：任意 `u32` 都是合法权重（含 0，见
    /// [`WeatherDef::season_weights`] 文档），没有可拒绝的取值。
    pub fn define(&mut self, index: ContentIndex, attrs: WeatherAttrs) -> Result<(), WeatherError> {
        if !(0..=WEATHER_SCALE_ONE).contains(&attrs.light_scale) {
            return Err(WeatherError::LightScaleOutOfRange(attrs.light_scale));
        }
        if !(0..=WEATHER_SCALE_ONE).contains(&attrs.sight_scale) {
            return Err(WeatherError::SightScaleOutOfRange(attrs.sight_scale));
        }
        if attrs.temperature_offset.abs() > WEATHER_TEMPERATURE_OFFSET_LIMIT {
            return Err(WeatherError::TemperatureOffsetOutOfRange(
                attrs.temperature_offset,
            ));
        }

        let idx = index.get() as usize;
        if idx >= self.defined.len() {
            let new_len = idx + 1;
            self.defined.resize(new_len, false);
            self.display_name_key.resize(new_len, None);
            self.light_scale.resize(new_len, WEATHER_SCALE_ONE);
            self.sight_scale.resize(new_len, WEATHER_SCALE_ONE);
            self.temperature_offset.resize(new_len, 0);
            self.season_weights.resize(new_len, [0; 4]);
        }

        if self.defined[idx] {
            return Err(WeatherError::DuplicateDefinition(index));
        }

        self.defined[idx] = true;
        self.display_name_key[idx] = Some(attrs.display_name_key);
        self.light_scale[idx] = attrs.light_scale;
        self.sight_scale[idx] = attrs.sight_scale;
        self.temperature_offset[idx] = attrs.temperature_offset;
        self.season_weights[idx] = attrs.season_weights;
        self.order.push(index);
        Ok(())
    }

    /// 给定索引当前是否已经登记为一种天气。
    pub fn is_defined(&self, index: ContentIndex) -> bool {
        self.defined
            .get(index.get() as usize)
            .copied()
            .unwrap_or(false)
    }

    /// 全部已注册天气，**按注册顺序**——加权选取唯一允许的遍历来源，
    /// 见模块文档「约束 C5」一节。
    pub fn registered(&self) -> &[ContentIndex] {
        &self.order
    }

    /// 展示名的本地化键。未登记索引兜底为 `None`（没有名字可显示）。
    ///
    /// 返回 `Option` 只是因为列式存储需要一个「这一格还没被定义」的
    /// 表示，**不代表这个字段是可选的**——[`WeatherAttrs::display_name_key`]
    /// 是必填参数，任何一条真实注册出来的天气都有名字。
    pub fn display_name_key(&self, index: ContentIndex) -> Option<NamespacedId> {
        debug_assert!(self.is_defined(index), "查询未注册的天气: {index:?}");
        self.display_name_key
            .get(index.get() as usize)
            .cloned()
            .flatten()
    }

    /// 环境光乘数，千分比。未登记索引兜底为 [`WEATHER_SCALE_ONE`]
    /// （安全侧——损坏/缺失 mod 的天气退化成「不影响光照」，不会让画面
    /// 意外陷入无法解释的黑暗）。
    pub fn light_scale(&self, index: ContentIndex) -> i32 {
        debug_assert!(self.is_defined(index), "查询未注册的天气: {index:?}");
        self.light_scale
            .get(index.get() as usize)
            .copied()
            .unwrap_or(WEATHER_SCALE_ONE)
    }

    /// 视野半径乘数，千分比。未登记索引兜底为 [`WEATHER_SCALE_ONE`]，
    /// 理由同 [`Self::light_scale`]。
    pub fn sight_scale(&self, index: ContentIndex) -> i32 {
        debug_assert!(self.is_defined(index), "查询未注册的天气: {index:?}");
        self.sight_scale
            .get(index.get() as usize)
            .copied()
            .unwrap_or(WEATHER_SCALE_ONE)
    }

    /// 温度增量偏移，十分之一摄氏度。未登记索引兜底为 0（安全侧——
    /// 损坏/缺失 mod 的天气退化成「不影响温度」，不会让玩家在一个说不
    /// 清来源的低温里冻僵，与 [`Self::light_scale`] 同一条降级纪律）。
    pub fn temperature_offset(&self, index: ContentIndex) -> i32 {
        debug_assert!(self.is_defined(index), "查询未注册的天气: {index:?}");
        self.temperature_offset
            .get(index.get() as usize)
            .copied()
            .unwrap_or(0)
    }

    /// 四季出现权重，下标见 [`season_slot`]。未登记索引兜底为全 0
    /// （「任何季节都不出现」——安全侧：一条坏数据不该被抽中）。
    pub fn season_weights(&self, index: ContentIndex) -> [u32; 4] {
        debug_assert!(self.is_defined(index), "查询未注册的天气: {index:?}");
        self.season_weights
            .get(index.get() as usize)
            .copied()
            .unwrap_or([0; 4])
    }
}

/// 某一世界时刻、某个世界种子下**派生**出来的天气。
///
/// 纯值类型，不进 `WorldState`，不参与任何序列化——见模块文档开篇。
/// 消费方（光照、视野、画面色调）每帧拿 [`Weather::derive`] 现算一次，
/// 随后各格复用同一个值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weather {
    /// 天气类型的内容索引。`None` 表示「没有任何天气被注册」或「本季
    /// 全部天气权重之和为 0」，取晴空基准，见 [`Weather::CLEAR`]。
    pub kind: Option<ContentIndex>,
    /// 环境光乘数，千分比——从 [`WeatherTable::light_scale`] 取出后随
    /// 本结构体一起传递，让热路径不必反复回表查。
    pub light_scale: i32,
    /// 视野半径乘数，千分比。
    pub sight_scale: i32,
    /// 温度增量偏移，十分之一摄氏度——从
    /// [`WeatherTable::temperature_offset`] 取出后随本结构体一起传递，
    /// 消费者是 [`crate::temperature::temperature_under`]。
    pub temperature_offset: i32,
}

impl Weather {
    /// 晴空基准：两个乘数都不缩放，也不指向任何一条天气内容。
    ///
    /// 三种用途，都是「这里不该有天气」而不是「这里的天气恰好是晴」：
    /// 非露天空间（[`crate::space_profile::effective_weather`]）、还没有
    /// 注册任何天气的世界、以及只关心昼夜四季的测试/demo。
    pub const CLEAR: Weather = Weather {
        kind: None,
        light_scale: WEATHER_SCALE_ONE,
        sight_scale: WEATHER_SCALE_ONE,
        temperature_offset: 0,
    };

    /// 派生某一世界时刻的天气——本模块的唯一入口。
    ///
    /// 确定性：唯一的随机来源是 [`DetRng::for_entity`]（约束 C3），三个
    /// 输入分别是世界种子、固定的 [`WEATHER_STREAM_ID`]、以及
    /// [`weather_period_index`] 算出的周期序号——全部由 `(world_seed,
    /// tick)` 唯一确定，不依赖调用次数、调用顺序或任何可变状态。同一对
    /// 输入在任何线程、任何平台、重放的任何一遍都得到同一个结果。
    ///
    /// **将来接入「求雨术」这类天气改写时，改的是本函数**：在派生之前
    /// 先查一层存进 `WorldState` 的稀疏偏差表，查不到再走这里的基线。
    /// 调用点不需要跟着改，见模块文档开篇最后一段。
    pub fn derive(world_seed: u64, tick: Tick, table: &WeatherTable) -> Weather {
        let Some(kind) = weather_kind_at(world_seed, tick, table) else {
            return Weather::CLEAR;
        };
        Weather {
            kind: Some(kind),
            light_scale: table.light_scale(kind),
            sight_scale: table.sight_scale(kind),
            temperature_offset: table.temperature_offset(kind),
        }
    }
}

/// 某个世界时刻落在第几个天气周期——[`DetRng`] 的「事件计数」输入。
///
/// 用 `div_euclid` 而不是 `/`：世界时钟理论上不会为负，但读档迁移或
/// 时间倒流类效果可能产生负值，而 `/` 对负数向零取整会让 `-1` 与 `0`
/// 落进同一个周期（`Tick(-1)` 与 `Tick(0)` 本该属于**相邻两个**周期），
/// 与 [`Tick::day_of_year`] 用 `div_euclid` 是同一条理由。
pub fn weather_period_index(tick: Tick) -> i64 {
    tick.0.div_euclid(WEATHER_PERIOD_TICKS)
}

/// 按季节权重选出这一刻的天气类型；一条天气都没有（或本季权重全为 0）
/// 时返回 `None`。
///
/// # 选取算法
///
/// 1. 沿 [`WeatherTable::registered`]（注册顺序，**不是**哈希遍历顺序，
///    约束 C5）累加本季权重，得到权重总和。
/// 2. 总和为 0 时返回 `None`——没有可抽的天气不是错误，是「这个世界
///    这一季就没有天气」，调用方退化成晴空基准。
/// 3. 用 `DetRng::gen_range(总和)` 掷一个点，再沿同一个顺序做前缀和
///    walk，落在哪一段就是哪种天气。
///
/// 每次调用都**新建**一条 `DetRng`（而不是复用一条长流）：这正是
/// `ll_core::rng` 模块文档「让随机数由三元组计算得出而非从共享流中
/// 取出」的用法——本函数因此可以被任意多次、任意顺序地调用，答案不变。
///
/// # 玩家能预测季节规律，但不能预测具体某天
///
/// 权重按季节给出，所以「冬天多半下雪」是玩家学得会的规律；但具体哪
/// 一段时间下雪由 splitmix64 决定，肉眼不可预测。这是刻意的设计取舍：
/// 完全不可学习的天气是纯噪音，完全可推算的天气则不需要随机数。
pub fn weather_kind_at(world_seed: u64, tick: Tick, table: &WeatherTable) -> Option<ContentIndex> {
    let slot = season_slot(tick.season());
    let mut total: u64 = 0;
    for index in table.registered() {
        total += u64::from(table.season_weights(*index)[slot]);
    }
    if total == 0 {
        return None;
    }

    let mut rng = DetRng::for_entity(
        world_seed,
        WEATHER_STREAM_ID,
        weather_period_index(tick) as u64,
    );
    let mut roll = rng.gen_range(total);
    for index in table.registered() {
        let weight = u64::from(table.season_weights(*index)[slot]);
        if roll < weight {
            return Some(*index);
        }
        roll -= weight;
    }

    // 理论不可达：`roll < total` 恒成立（`gen_range` 的契约），而循环
    // 累减的正是同一个 `total`。真走到这里说明前缀和与总和算得不一致，
    // 与其 panic，不如落在最后一条已注册天气上——与本 crate 其余表
    // 「查不到时给一个明确、可预期的兜底值」同一条降级纪律。
    table.registered().last().copied()
}

/// 本体六种天气在当前注册表里的索引缓存，由
/// [`materialize_base_weathers`] 在启动时一次性物化。
#[derive(Debug, Clone, Copy)]
pub struct BaseWeatherIds {
    /// 晴：不影响光照，不影响视野。
    pub clear: ContentIndex,
    /// 阴：明显变暗，几乎不挡视线。
    pub overcast: ContentIndex,
    /// 雨：变暗且略微挡视线。
    pub rain: ContentIndex,
    /// 大风：略微变暗，略微挡视线（扬尘）。
    pub wind: ContentIndex,
    /// 雾：不太暗，但极大缩短能看多远——与阴天正好互补，见
    /// [`WeatherDef::sight_scale`] 文档。
    pub fog: ContentIndex,
    /// 下雪：变暗且挡视线，几乎只在秋冬出现。
    ///
    /// # 为什么 id 是 `lostland:snowfall` 而不是 `lostland:snow`
    ///
    /// **`lostland:snow` 这个 id 已经被地形表占了**——雪地
    /// （[`crate::terrain::BaseTerrainIds::snow`]，本体地形第 8 条）。
    /// [`crate::terrain`] 的本体注册跑在本模块**之前**，两者共用同一个
    /// [`crate::weather`] 之外的东西：`ll_mod::registry::Registry` 是
    /// **一个** id ↔ `ContentIndex` 空间，`intern` 对同一个字符串返回
    /// 同一个索引。天气曾经真的叫 `lostland:snow`，于是「雪地」与
    /// 「下雪」在整个 2026-08 天气批次到 2026-09-01 之间一直共用索引 7：
    /// 两张表各查各的、运行期看不出毛病，但值哈希只认第一张表
    /// （`ll_mod::content_hash::classify_index` 取首个命中者），
    /// **下雪那六个字段值从此完全不进内容哈希**——实测把
    /// `light_scale` 从 720 改成 721，`lostland` 命名空间的内容哈希
    /// 一位都不变；换成不撞名的雾做同样的改动，哈希立刻变。
    ///
    /// 改名的方向照 `ll_mod::tree` 的 `TIMBER_ID` 定下的先例：**先来的
    /// 那张表保留原 id，后加的一方改名**（地形远早于天气）。撞名本身
    /// 现在由 `ll_mod::content_audit::detect_table_define_collisions`
    /// 在装载后阻断，不再依赖谁恰好注意到一个对不上的索引数字。
    pub snowfall: ContentIndex,
}

/// 本体天气注册的唯一入口：本体与 mod 共用的注册路径。
///
/// `intern` 是外部传入的解析回调（生产路径是 `|id| registry.intern(id)`，
/// 见 `ll_mod::base_weather`；测试/demo 路径见 [`base_weather_fixture`]）
/// ——与 [`crate::space_profile::materialize_base_space_profiles`] 完全
/// 同构，理由见模块文档「为什么天气表定义在 `ll-world`」一节。
///
/// # 六种天气的数值取舍
///
/// 具体数值是内容设计取舍，可以在后续批次调整，这里给出一组内部自洽
/// 的默认值，两条硬约束：
///
/// 1. **白昼在任何天气下都明显亮于夜晚**——最暗的雪天（光照乘数 720）
///    叠上最暗的冬季（季节乘数 750）之后，正午仍有 540‰，远高于午夜
///    的 100‰ 基准；画面亮度（`ll_game::layout::effective_tint`）因此
///    恒高于 `MIN_VISIBLE_TINT`（0.4），不会靠下限兜底。
/// 2. **视野在任何天气下都不低于观察者的夜间下限**（未声明暗视时是
///    [`crate::light::DEFAULT_NIGHT_SIGHT_RADIUS`]，声明了暗视的种族
///    是它自己声明的格数）——这条由
///    [`crate::light::sight_radius_under_weather`] 的下限保证，本表的
///    取值只需保证「夏季正午的最差天气仍明显好于夜晚」，见
///    `crate::light` 的组合断言。
/// 3. **新游戏开局（春季早八点）在任何可能出现的天气下，视野仍不低于
///    基准半径的一半**——这是本表数值唯一一次被既有保证反过来约束：
///    雾的视野乘数最初取 650，实测让开局视野从 12 掉到 5，跌破
///    `ll_game::layout` 那条「开局至少要有基准半径的一半，否则开局仍然
///    近乎瞎」的既有断言（该断言是项目所有者当初为「黑夜看不见」定的
///    同一批要求之一）。改成 700 之后开局最差是 6，恰好守住这条线，同时
///    雾与阴天的互补关系（雾更亮但看得更近）完全不受影响。这条约束只
///    管**开局那一刻**：冬季雪天的正午视野仍会掉到下限附近，那是玩家
///    已经在世界里、有准备的时刻，与「开局就近乎瞎」不是一回事。
///
/// 季节权重让四季各有性格：春多雨、夏多晴、秋多风多雾、冬多雪。雪在
/// 春夏两季权重为 0（绝不出现），是 [`WeatherDef::season_weights`] 那
/// 条「取 0 表示这一季绝不出现」在本体内容里的真实用例。
///
/// # 温度偏移的取舍（温度系统批次）
///
/// 晴 0、雾 -10、阴 -20、大风 -30、雨 -40、雪 -80（十分之一摄氏度）。
/// 三条内部约束：
///
/// 1. **晴取 0**——它是 [`Weather::CLEAR`] 这个「这里不该有天气」的
///    基准所对应的那一条内容，两者的温度语义必须一致，否则「洞窟里
///    的天气」（恒为 `CLEAR`）与「外面恰好是晴」会算出不同的温度。
/// 2. **雪严格最冷**——「冬季雪夜」是本体内容里唯一会触发
///    `ll_sim::exposure` 惩罚的极端组合（见
///    [`crate::temperature::SEASON_TEMPERATURE_OFFSETS`] 的取值表），
///    雪若不是最冷的那一种，这句话就名不副实。有测试钉住。
/// 3. **全部落在 ±80 以内**——远小于
///    [`WEATHER_TEMPERATURE_OFFSET_LIMIT`]（±500）。天气是这套加法里
///    幅度最小的一项（季节 -180、昼夜 ±60），它的角色是「让同一个冬夜
///    比另一个冬夜更难熬」，不是自己单独把温度推过冰点——一场夏天的雪
///    不该让人冻僵，那是季节该管的事。
///
/// 排序刻意与 [`WeatherDef::light_scale`] **不完全一致**：雾比阴天亮
/// （850 > 800）却也比阴天暖（-10 > -20），而大风比雨亮（900 > 700）
/// 也比雨暖（-30 > -40）——三个旋钮各自独立，不是同一个「恶劣程度」
/// 标量的三份拷贝，这与 `light_scale`/`sight_scale` 当初分成两个旋钮
/// 是同一条理由。
pub fn materialize_base_weathers(
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
) -> Result<(BaseWeatherIds, WeatherTable), WeatherError> {
    let mut table = WeatherTable::new();

    let clear = define_base(&mut table, intern, "clear", 1000, 1000, 0, [45, 55, 45, 35])?;
    let overcast = define_base(
        &mut table,
        intern,
        "overcast",
        800,
        950,
        -20,
        [25, 20, 25, 25],
    )?;
    let rain = define_base(&mut table, intern, "rain", 700, 900, -40, [20, 15, 15, 3])?;
    let wind = define_base(&mut table, intern, "wind", 900, 950, -30, [15, 8, 20, 12])?;
    let fog = define_base(&mut table, intern, "fog", 850, 700, -10, [12, 4, 15, 10])?;
    let snowfall = define_base(&mut table, intern, "snowfall", 720, 800, -80, [0, 0, 2, 30])?;

    Ok((
        BaseWeatherIds {
            clear,
            overcast,
            rain,
            wind,
            fog,
            snowfall,
        },
        table,
    ))
}

/// [`materialize_base_weathers`] 的内部帮手：把一条声明的字面量字段拆
/// 开传入，换取一次 `intern` + 一次 [`WeatherTable::define`]。与
/// `space_profile` 里同名的内部帮手同一个理由抽成函数，避免六份几乎
/// 相同的样板代码互相漂移。
///
/// `local` 只是本地名（例如 `"rain"`）：内容 id 与本地化键都由它拼出
/// （`lostland:rain` 与 `lostland:weather.rain.display_name`），两者
/// 因此不可能拼错到互相对不上。
#[allow(clippy::too_many_arguments)]
fn define_base(
    table: &mut WeatherTable,
    intern: &mut dyn FnMut(NamespacedId) -> ContentIndex,
    local: &str,
    light_scale: i32,
    sight_scale: i32,
    temperature_offset: i32,
    season_weights: [u32; 4],
) -> Result<ContentIndex, WeatherError> {
    let id = NamespacedId::parse(&format!("lostland:{local}")).expect("本体天气 id 字面量恒合法");
    let display_name_key = NamespacedId::parse(&format!("lostland:weather.{local}.display_name"))
        .expect("本体天气本地化键字面量恒合法");
    let index = intern(id);
    table.define(
        index,
        WeatherAttrs {
            display_name_key,
            light_scale,
            sight_scale,
            temperature_offset,
            season_weights,
        },
    )?;
    Ok(index)
}

/// 供测试与验收 demo 使用：现造一个空 [`Interner`]，注册本体全部六种
/// 天气，返回可用的 `(BaseWeatherIds, WeatherTable)`。
///
/// **不是生产路径**——生产路径必须经过 `ll-mod::Registry::intern`，与
/// [`crate::space_profile::base_space_profile_fixture`] 同一条纪律。
pub fn base_weather_fixture() -> (BaseWeatherIds, WeatherTable) {
    let mut interner = Interner::new();
    materialize_base_weathers(&mut |id| interner.intern(id))
        .expect("本体天气声明表内部一致，注册恒不失败")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::time::{TICKS_PER_DAY, TICKS_PER_HOUR};

    #[test]
    fn 同一种子与时刻恒派生出同一种天气() {
        // 这是天气「零存档状态」得以成立的基石：既然任何时候都能算出
        // 同一个答案，就没有必要把答案存进世界状态。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let tick = Tick(37 * TICKS_PER_DAY + 9 * TICKS_PER_HOUR);

        // Act
        let first = Weather::derive(0xDEAD_BEEF, tick, &table);
        let second = Weather::derive(0xDEAD_BEEF, tick, &table);

        // Assert
        assert_eq!(first, second);
    }

    #[test]
    fn 同一周期内的每一刻度都是同一种天气() {
        // 消费方（光照）在一段周期内必须看到恒定的天气，否则同一场雨
        // 会在相邻两刻度之间闪烁。周期边界内的任意采样点都应一致。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let period_start = Tick(6 * WEATHER_PERIOD_TICKS);

        // Act
        let at_start = Weather::derive(7, period_start, &table);
        let at_middle = Weather::derive(7, Tick(period_start.0 + WEATHER_PERIOD_TICKS / 2), &table);
        let at_end = Weather::derive(7, Tick(period_start.0 + WEATHER_PERIOD_TICKS - 1), &table);

        // Assert
        assert_eq!((at_start, at_middle), (at_end, at_end));
    }

    #[test]
    fn 不同种子在同一时刻多半派生出不同的天气序列() {
        // 若两个种子给出逐段相同的天气，说明种子没有真的参与派生。
        // 逐段比较一整天六个周期，而不是只比一个点——单点相同是正常的
        // 概率事件（只有六种天气）。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let periods: Vec<Tick> = (0..6).map(|i| Tick(i * WEATHER_PERIOD_TICKS)).collect();

        // Act
        let first: Vec<Option<ContentIndex>> = periods
            .iter()
            .map(|t| Weather::derive(1, *t, &table).kind)
            .collect();
        let second: Vec<Option<ContentIndex>> = periods
            .iter()
            .map(|t| Weather::derive(2, *t, &table).kind)
            .collect();

        // Assert
        assert_ne!(first, second);
    }

    #[test]
    fn 相邻两个周期之间天气会变化() {
        // 若周期序号没有真的进 DetRng 的事件计数，整局游戏会是同一种
        // 天气。扫一整年找出至少一次变化即可证明周期确实参与了派生。
        // Arrange
        let (_ids, table) = base_weather_fixture();

        // Act
        let kinds: Vec<Option<ContentIndex>> = (0..120)
            .map(|i| Weather::derive(99, Tick(i * WEATHER_PERIOD_TICKS), &table).kind)
            .collect();

        // Assert
        assert!(
            kinds.windows(2).any(|pair| pair[0] != pair[1]),
            "一整年 120 个周期里天气从未变化，说明周期序号没有进随机流"
        );
    }

    #[test]
    fn 空表派生出晴空基准而不是崩溃() {
        // mod 全部被禁用、或某个 mod 声明了天气却加载失败时的降级路径。
        // Arrange
        let table = WeatherTable::new();

        // Act
        let weather = Weather::derive(1, Tick(0), &table);

        // Assert
        assert_eq!(weather, Weather::CLEAR);
    }

    #[test]
    fn 本季权重全为零时派生出晴空基准() {
        // Arrange：只注册一种「只在冬天出现」的天气，然后在夏天查询。
        let mut interner = Interner::new();
        let mut table = WeatherTable::new();
        let index = interner.intern(NamespacedId::parse("yourmod:blizzard").expect("合法"));
        table
            .define(
                index,
                WeatherAttrs {
                    display_name_key: NamespacedId::parse("yourmod:weather.blizzard.display_name")
                        .expect("合法"),
                    light_scale: 500,
                    sight_scale: 400,
                    temperature_offset: 0,
                    season_weights: [0, 0, 0, 10],
                },
            )
            .expect("合法声明应当注册成功");
        // 第 40 天落在夏季（每季 30 天，第 30..60 天是夏）。
        let summer = Tick(40 * TICKS_PER_DAY);

        // Act
        let weather = Weather::derive(1, summer, &table);

        // Assert
        assert_eq!(weather, Weather::CLEAR);
    }

    #[test]
    fn 权重为零的天气在该季节绝不被抽中() {
        // 本体的雪在春夏两季权重为 0——扫过整个春季的全部周期，一次都
        // 不该抽中雪。这是「季节倾向」这条配置维度真的生效的证据。
        // Arrange
        let (ids, table) = base_weather_fixture();
        // 春季是每年的第 0..30 天。
        let spring_periods = 30 * TICKS_PER_DAY / WEATHER_PERIOD_TICKS;

        // Act & Assert
        for seed in 0..8u64 {
            for period in 0..spring_periods {
                let tick = Tick(period * WEATHER_PERIOD_TICKS);
                let weather = Weather::derive(seed, tick, &table);
                assert_ne!(
                    weather.kind,
                    Some(ids.snowfall),
                    "春季权重为 0 的雪不该被抽中（种子 {seed}，周期 {period}）"
                );
            }
        }
    }

    #[test]
    fn 冬季能抽中雪() {
        // 与上一条互为反面：权重非零的天气必须真的会出现，否则「季节
        // 倾向」只是单向地把东西关掉，从来没有真的打开过。
        // Arrange
        let (ids, table) = base_weather_fixture();
        // 冬季是每年的第 90..120 天。
        let winter_start = 90 * TICKS_PER_DAY;
        let winter_periods = 30 * TICKS_PER_DAY / WEATHER_PERIOD_TICKS;

        // Act
        let mut saw_snow = false;
        for seed in 0..8u64 {
            for period in 0..winter_periods {
                let tick = Tick(winter_start + period * WEATHER_PERIOD_TICKS);
                if Weather::derive(seed, tick, &table).kind == Some(ids.snowfall) {
                    saw_snow = true;
                }
            }
        }

        // Assert
        assert!(saw_snow, "扫过八个种子的整个冬季都没有下过雪");
    }

    #[test]
    fn 负时刻不与零时刻落进同一个周期() {
        // div_euclid 而非 `/`：向零取整会让 -1 与 0 同属周期 0。
        // Arrange & Act
        let before = weather_period_index(Tick(-1));
        let at_zero = weather_period_index(Tick(0));

        // Assert
        assert_eq!((before, at_zero), (-1, 0));
    }

    #[test]
    fn 光照乘数越界时注册失败而不是静默钳位() {
        // Arrange
        let mut interner = Interner::new();
        let mut table = WeatherTable::new();
        let index = interner.intern(NamespacedId::parse("yourmod:toobright").expect("合法"));

        // Act
        let result = table.define(
            index,
            WeatherAttrs {
                display_name_key: NamespacedId::parse("yourmod:weather.toobright.display_name")
                    .expect("合法"),
                light_scale: 1001,
                sight_scale: 1000,
                temperature_offset: 0,
                season_weights: [1, 1, 1, 1],
            },
        );

        // Assert
        assert_eq!(result, Err(WeatherError::LightScaleOutOfRange(1001)));
    }

    #[test]
    fn 视野乘数不允许超过一千() {
        // 视野「放大」是观察者属性（暗视）该做的事，不是天气，见
        // WeatherError::SightScaleOutOfRange 文档。
        // Arrange
        let mut interner = Interner::new();
        let mut table = WeatherTable::new();
        let index = interner.intern(NamespacedId::parse("yourmod:farsight").expect("合法"));

        // Act
        let result = table.define(
            index,
            WeatherAttrs {
                display_name_key: NamespacedId::parse("yourmod:weather.farsight.display_name")
                    .expect("合法"),
                light_scale: 1000,
                sight_scale: 1500,
                temperature_offset: 0,
                season_weights: [1, 1, 1, 1],
            },
        );

        // Assert
        assert_eq!(result, Err(WeatherError::SightScaleOutOfRange(1500)));
    }

    #[test]
    fn 同一个索引重复定义返回错误而非静默覆盖() {
        // Arrange
        let (ids, mut table) = base_weather_fixture();

        // Act
        let result = table.define(
            ids.rain,
            WeatherAttrs {
                display_name_key: NamespacedId::parse("lostland:weather.rain.display_name")
                    .expect("合法"),
                light_scale: 1000,
                sight_scale: 1000,
                temperature_offset: 0,
                season_weights: [1, 1, 1, 1],
            },
        );

        // Assert
        assert_eq!(result, Err(WeatherError::DuplicateDefinition(ids.rain)));
    }

    #[test]
    fn 注册顺序列表按声明顺序排列且不含未注册项() {
        // 加权选取沿这个列表遍历（约束 C5），顺序必须是确定的注册顺序。
        // Arrange
        let (ids, table) = base_weather_fixture();

        // Act
        let order = table.registered();

        // Assert
        assert_eq!(
            order,
            &[
                ids.clear,
                ids.overcast,
                ids.rain,
                ids.wind,
                ids.fog,
                ids.snowfall
            ]
        );
    }

    #[test]
    fn 本体六种天气的字段都能查回声明值() {
        // Arrange
        let (ids, table) = base_weather_fixture();

        // Act & Assert
        assert_eq!(table.light_scale(ids.clear), WEATHER_SCALE_ONE);
        assert_eq!(table.sight_scale(ids.clear), WEATHER_SCALE_ONE);
        assert_eq!(table.season_weights(ids.snowfall), [0, 0, 2, 30]);
        assert_eq!(
            table.display_name_key(ids.fog),
            Some(NamespacedId::parse("lostland:weather.fog.display_name").expect("合法"))
        );
        // 雾是「不太暗但极难看远」，阴天是「明显变暗但看得一样远」——
        // 两个旋钮分开才表达得出这对互补关系，见 WeatherDef::sight_scale。
        assert!(table.light_scale(ids.fog) > table.light_scale(ids.overcast));
        assert!(table.sight_scale(ids.fog) < table.sight_scale(ids.overcast));
    }

    #[test]
    fn 派生结果携带的乘数与表里查到的一致() {
        // Weather 把两个乘数复制出来供热路径复用，复制不能跑偏。
        // Arrange
        let (_ids, table) = base_weather_fixture();

        // Act
        let weather = Weather::derive(12345, Tick(3 * TICKS_PER_DAY), &table);

        // Assert
        let kind = weather.kind.expect("本体六种天气非空，必然抽中一种");
        assert_eq!(
            (weather.light_scale, weather.sight_scale),
            (table.light_scale(kind), table.sight_scale(kind))
        );
    }

    #[test]
    fn 季节下标与四季一一对应且不重复() {
        // 一处写错就会出现「春天用了冬天的权重」这种极难查的错位。
        // Arrange & Act
        let slots = [
            season_slot(Season::Spring),
            season_slot(Season::Summer),
            season_slot(Season::Autumn),
            season_slot(Season::Winter),
        ];

        // Assert
        assert_eq!(slots, [0, 1, 2, 3]);
    }

    #[test]
    fn 本体天气的温度偏移全部落在合法上下界内并且至少有一条非零() {
        // 「至少有一条非零」不是形式要求：本体若全填 0，这一列就成了
        // 又一处没有任何内容真正用上的旋钮（`content_audit` 的字段覆盖
        // 检查也会当场报出来）。
        // Arrange
        let (_ids, table) = base_weather_fixture();

        // Act
        let offsets: Vec<i32> = table
            .registered()
            .iter()
            .map(|index| table.temperature_offset(*index))
            .collect();

        // Assert
        assert!(offsets.iter().any(|offset| *offset != 0));
        for offset in offsets {
            assert!(offset.abs() <= WEATHER_TEMPERATURE_OFFSET_LIMIT);
        }
    }

    #[test]
    fn 雪是本体六种天气里最冷的一种() {
        // 内容自洽：雪的温度偏移必须严格低于其余五种，否则「冬季雪夜」
        // 这条唯一会触发惩罚的组合就名不副实。
        // Arrange
        let (ids, table) = base_weather_fixture();

        // Act
        let snowfall = table.temperature_offset(ids.snowfall);
        let others: Vec<i32> = table
            .registered()
            .iter()
            .filter(|index| **index != ids.snowfall)
            .map(|index| table.temperature_offset(*index))
            .collect();

        // Assert
        for other in others {
            assert!(
                snowfall < other,
                "雪的温度偏移 {snowfall} 应当严格低于 {other}"
            );
        }
    }

    #[test]
    fn 温度偏移越界时注册期就报错() {
        // ADR 0017「注册期完整校验」：越界必须在装载时报出来，而不是
        // 等玩到某个冬夜才表现成「一出门就冻僵」。
        // Arrange
        let mut interner = Interner::new();
        let index = interner.intern(NamespacedId::parse("yourmod:absurd").expect("合法"));
        let mut table = WeatherTable::new();

        // Act
        let result = table.define(
            index,
            WeatherAttrs {
                display_name_key: NamespacedId::parse("yourmod:weather.absurd.display_name")
                    .expect("合法"),
                light_scale: 1000,
                sight_scale: 1000,
                temperature_offset: WEATHER_TEMPERATURE_OFFSET_LIMIT + 1,
                season_weights: [1, 1, 1, 1],
            },
        );

        // Assert
        assert_eq!(
            result,
            Err(WeatherError::TemperatureOffsetOutOfRange(
                WEATHER_TEMPERATURE_OFFSET_LIMIT + 1
            ))
        );
    }

    #[test]
    fn 温度偏移的上下界对称() {
        // 上下界对称是 WEATHER_TEMPERATURE_OFFSET_LIMIT 文档最后一段的
        // 断言：变暖与变冷在语义上完全对等。
        // Arrange
        let mut interner = Interner::new();
        let mut table = WeatherTable::new();
        let warm = interner.intern(NamespacedId::parse("yourmod:foehn").expect("合法"));
        let cold = interner.intern(NamespacedId::parse("yourmod:coldsnap").expect("合法"));
        let key = NamespacedId::parse("yourmod:weather.x.display_name").expect("合法");

        // Act
        let warm_result = table.define(
            warm,
            WeatherAttrs {
                display_name_key: key.clone(),
                light_scale: 1000,
                sight_scale: 1000,
                temperature_offset: WEATHER_TEMPERATURE_OFFSET_LIMIT,
                season_weights: [1, 1, 1, 1],
            },
        );
        let cold_result = table.define(
            cold,
            WeatherAttrs {
                display_name_key: key,
                light_scale: 1000,
                sight_scale: 1000,
                temperature_offset: -WEATHER_TEMPERATURE_OFFSET_LIMIT,
                season_weights: [1, 1, 1, 1],
            },
        );

        // Assert
        assert_eq!((warm_result, cold_result), (Ok(()), Ok(())));
        assert_eq!(
            table.temperature_offset(warm),
            WEATHER_TEMPERATURE_OFFSET_LIMIT
        );
        assert_eq!(
            table.temperature_offset(cold),
            -WEATHER_TEMPERATURE_OFFSET_LIMIT
        );
    }

    #[test]
    fn 派生出来的天气把温度偏移一并带出来() {
        // Weather 这个值类型存在的意义是"热路径不必反复回表查"，温度
        // 偏移必须与两个乘数一样被带出来，否则消费者仍然要拿着表。
        // Arrange
        let (_ids, table) = base_weather_fixture();
        let tick = Tick(11 * WEATHER_PERIOD_TICKS);

        // Act
        let weather = Weather::derive(0x5EED, tick, &table);

        // Assert
        let kind = weather.kind.expect("本体六种天气在任何季节权重和都非零");
        assert_eq!(weather.temperature_offset, table.temperature_offset(kind));
    }
}
