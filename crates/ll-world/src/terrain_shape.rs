//! 地形**形态**参数 [`TerrainShape`]：同一个种子下，这组数值决定世界
//! 长什么样。
//!
//! # 为什么单独一个模块
//!
//! 它本来住在 [`crate::generate`]。气候条带批次给它加上第五个字段
//! （`climate_band_width`）之后，那个文件连同新增的测试涨到九百多行，
//! 越过了本仓库的 800 行上限。拆出来的边界是现成的：这个类型是**参数
//! 声明与校验**，`generate` 是**用这些参数生成地形**，两件事本来就不
//! 共享任何私有状态。
//!
//! 为了不惊动任何调用方，[`crate::generate`] 原样 `pub use` 了这个类型
//! ——`ll_world::generate::TerrainShape` 这条既有路径继续有效，存档里的
//! 字段名与序列化格式一个字节没动。

/// 地形**形态**参数：同一个种子下，这组数值决定世界长什么样——水陆
/// 比例、山地多少、陆地碎成几块。
///
/// 全部取 [`crate::noise::TileableNoise`] 输出区间同一套千分比整数，全程无浮点
/// （[ADR 0020](../../../knowledge/decisions/0020-scripts-may-use-floats-internally-boundary-type-gated.md)
/// 乙区：这些数值直接流进世界状态，必须是量化整数）。
///
/// # 为什么与 `seed` 分成两个类型
///
/// 不是为了对称（[ADR 0021](../../../knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md)
/// 明令禁止那种理由），是因为存档里这两半的归属**本来就不同**：
/// `seed` 早就作为 `ll_world::state::WorldState::seed` 单独持久化了，
/// 而形态参数此前根本没进存档。把形态参数聚成一个类型，
/// `WorldState` 就只需要新增**一个**字段（
/// `ll_world::state::WorldState::terrain_shape`）承接它，
/// 不必把种子再存第二遍、也不必把四个散字段各自铺进存档结构与
/// `remap` 的穷尽解构里。
///
/// # 为什么它必须进存档（ADR 0009「默认派生，只存偏差」）
///
/// ADR 0009 的规则是「能派生的不进存档」。形态参数**不可派生**——它是
/// 玩家在建档那一刻做出的选择，世界建成之后没有任何其它数据能反推出
/// 「玩家当初选的是海平面 400 还是 620」（地形本身是这组参数的函数，
/// 反过来不成立：不同参数组合可能在已常驻的那几块区块上产出相同地形）。
/// 它与 `seed`、世界尺寸、生成期 mod 集合同属
/// `ll_content::world_identity` 描述的「缺一，世界都复现不出来」那一
/// 类，因此进存档是正当的，不是对 ADR 0009 的例外。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TerrainShape {
    /// 深水与浅水的分界高度（千分比）。**水陆比例的主旋钮**：噪声高度
    /// 分布集中在 300～700 之间（见 `knowledge/design/worldgen-parameters.md` 实测直方
    /// 图），这个值每上调 50，全图水域比例约上升 11 个百分点；调到 200
    /// 以下或 700 以上旋钮会饱和（几乎全陆 / 几乎全水）。
    pub sea_level: i32,
    /// 丘陵与山地的分界高度（千分比）。**山地比例的主旋钮**，且与
    /// [`Self::sea_level`] 正交——实测调这个值不改变水陆比例一个百分点。
    pub mountain_level: i32,
    /// 噪声倍频叠加层数，层数越多地形起伏的细节越丰富。**地形破碎程度
    /// 的旋钮**：层数越多，高度分布越向中位数收拢（多层平均的必然结果），
    /// 于是极端高度变少（山地占比从 1 层的 19% 掉到 8 层的 2.6%）、
    /// 海岸线越曲折、独立陆块越多。
    pub octaves: u32,
    /// 大陆尺度缩减档位：每 +1 档，噪声最粗一层的格子边长减半，也就是
    /// 「一块大陆」的典型尺寸减半。见
    /// [`crate::noise::TileableNoise::shrink_continents`]——**这是「群岛」形态唯一
    /// 真正需要的旋钮**，原有三个阈值旋钮表达不了它。
    pub continent_shrink: u32,
    /// 气候条带的**单侧带宽**（千分比），见 [`crate::climate`]。
    /// **气候条带的总开关兼强度旋钮**：干热带占据暖度高于
    /// `1000 - climate_band_width` 的纬度，极地带占据暖度低于
    /// `climate_band_width` 的纬度，其余是温带。
    ///
    /// # 为什么 `0` 必须是**精确的**恒等
    ///
    /// 取 `0` 时两条判据（严格不等号，见
    /// [`crate::climate::band_from_warmth`]）恒假，整图温带，地形分带
    /// 与气候条带落地之前**逐位相同**。这不是巧合而是刻意设计：黄金
    /// 基准重冻纪律的第 ② 步要求「把改动关掉，确认精确回到旧值」，
    /// 有了这个取值，那一步就从一次性的手工验证变成一条长期活着的
    /// 回归测试（`crates/ll-world/tests/determinism.rs` 的
    /// `气候带宽为零时世界摘要回到气候条带落地之前的旧值`）。
    ///
    /// 两个更早批次的验收 demo（`p2_acceptance`/`p5_coordinate_acceptance`）
    /// 也靠它把自己的世界钉在「无气候」上，冻结像素基准因此零漂移。
    #[serde(default = "default_climate_band_width")]
    pub climate_band_width: i32,
}

/// [`TerrainShape::climate_band_width`] 的 serde 缺省值。
///
/// 老存档的 `terrain_shape` 里没有这个键，缺省必须回落到**新默认值**
/// 而不是 `i32::default()`（0）——0 的含义是「关掉气候」，让老存档静默
/// 变成一个没有气候条带的世界，正是 `worldgen-parameters.md` 五节警告
/// 过的「同一张地图在玩家脚下裂成两种地形」。
fn default_climate_band_width() -> i32 {
    TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH
}

impl Default for TerrainShape {
    /// 默认形态：海平面 400、山地起点 750、四层倍频、不缩减大陆尺度、
    /// 气候条带单侧带宽 250‰。
    ///
    /// 这五个值必须**逐位**保持不变——它们是
    /// `crates/ll-world/tests/determinism.rs` 的 `EXPECTED_WORLD_DIGEST`
    /// 与 `crates/ll-sim/tests/replay.rs` 的 `EXPECTED_REPLAY_DIGEST`
    /// 两条黄金基准所固定的那张地图。改动其中任何一个都要走黄金基准
    /// 重冻四步（见 `knowledge/handoff/2026-08-27-session-handoff.md`
    /// 一节第 2 条），不是「跑一下把期望值抄过来」。
    fn default() -> Self {
        TerrainShape {
            sea_level: 400,
            mountain_level: 750,
            octaves: 4,
            continent_shrink: 0,
            climate_band_width: Self::DEFAULT_CLIMATE_BAND_WIDTH,
        }
    }
}

/// [`TerrainShape`] 各字段的合法取值范围——越界即拒绝，见
/// [`TerrainShape::validate`]。
///
/// 越界值不会让生成 panic（阈值链只是让某些地形带变空，
/// `shrink_continents` 自己在 1 处饱和），但会静默产出一张没人想要的
/// 地图——玩家手改配置写错一个零，应该看到一条明确的拒绝日志，而不是
/// 开出一个全是深水的世界还以为是自己运气差。
impl TerrainShape {
    /// 高度千分比的取值上界（含），与 [`crate::noise::TileableNoise`] 输出区间一致。
    pub const HEIGHT_MAX: i32 = 1000;
    /// [`Self::mountain_level`] 至少要比 [`Self::sea_level`] 高出多少
    /// ——阈值链里从海平面到山地之间铺了浅水(+50)/沙地(+100)与
    /// 草地/森林/丘陵三段（`mountain_level - 150` 起算），少于 150 这
    /// 几段就会互相穿插，地形带的先后顺序不再成立。
    pub const MIN_LEVEL_GAP: i32 = 150;
    /// 倍频层数的合法区间（含）。上界取 12：振幅每层减半，第 11 层起
    /// 振幅整数除法归零、[`crate::noise::TileableNoise::octaves`] 自己就会提前跳出，
    /// 再大的值只是无效声明。
    pub const OCTAVES_RANGE: std::ops::RangeInclusive<u32> = 1..=12;
    /// [`Self::climate_band_width`] 的默认值：干热带与极地带各占纬度的
    /// 25%，温带占 50%。
    ///
    /// 取 250 而不是更大：干热带低海拔全是沙漠，带宽每加一分，可耕的
    /// 温带就少一分。四分之一是「一眼看得出气候条带真的存在」与「世界
    /// 仍以温带为主」之间的取舍，不是从任何公式推出来的——要改就改这
    /// 一个常量。
    ///
    /// 公开而不是私有：`ll_content::world_identity::TERRAIN_PRESETS` 是
    /// `const` 上下文，用不了 [`Default::default`]，四档预设必须能在
    /// 常量位置引用同一个值，否则「默认预设与 `TerrainShape::default()`
    /// 逐位相同」这条既有约束就要靠四处手抄的字面量维持。
    pub const DEFAULT_CLIMATE_BAND_WIDTH: i32 = 250;
    /// [`Self::climate_band_width`] 的上界（含）。取 500：干热带与极地带
    /// 各占一侧带宽，两者合计超过全部纬度（`2 × 500 = 1000‰`）之后温带
    /// 就被挤没了，地形分带里「温带那一支」永远走不到——与
    /// [`Self::MIN_LEVEL_GAP`] 拒绝阈值链互相穿插是同一条纪律：参数越界
    /// 不该让某一支静默死掉，该被当场拒绝。
    pub const MAX_CLIMATE_BAND_WIDTH: i32 = 500;
    /// 大陆尺度缩减档位的上界（含）。取 8：本体最大预设尺寸下自动推导
    /// 出的 `coarse_scale` 也不过 32（五档），8 档足以让任何尺寸都饱和
    /// 到 1，再大没有额外效果。
    pub const MAX_CONTINENT_SHRINK: u32 = 8;

    /// 校验这组形态参数是否落在合法区间；不合法时返回一句可直接写进
    /// 日志的中文原因。
    ///
    /// 这是系统边界上的输入校验（玩家手写的配置文件是不可信输入）：
    /// 调用方（`ll_game` 的配置解析）应当在拒绝时记一条日志并退回
    /// [`Self::default`]，与 `ll_platform::config::load_or_default`
    /// 对损坏配置的处理同一条纪律——绝不 panic。
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=Self::HEIGHT_MAX).contains(&self.sea_level) {
            return Err(format!(
                "海平面 {} 超出合法区间 0..={}",
                self.sea_level,
                Self::HEIGHT_MAX
            ));
        }
        if !(0..=Self::HEIGHT_MAX).contains(&self.mountain_level) {
            return Err(format!(
                "山地阈值 {} 超出合法区间 0..={}",
                self.mountain_level,
                Self::HEIGHT_MAX
            ));
        }
        if self.mountain_level - self.sea_level < Self::MIN_LEVEL_GAP {
            return Err(format!(
                "山地阈值 {} 与海平面 {} 的差不足 {}，地形带会互相穿插",
                self.mountain_level,
                self.sea_level,
                Self::MIN_LEVEL_GAP
            ));
        }
        if !Self::OCTAVES_RANGE.contains(&self.octaves) {
            return Err(format!(
                "倍频层数 {} 超出合法区间 {}..={}",
                self.octaves,
                Self::OCTAVES_RANGE.start(),
                Self::OCTAVES_RANGE.end()
            ));
        }
        if self.continent_shrink > Self::MAX_CONTINENT_SHRINK {
            return Err(format!(
                "大陆尺度缩减档位 {} 超出合法区间 0..={}",
                self.continent_shrink,
                Self::MAX_CONTINENT_SHRINK
            ));
        }
        if !(0..=Self::MAX_CLIMATE_BAND_WIDTH).contains(&self.climate_band_width) {
            return Err(format!(
                "气候条带单侧带宽 {} 超出合法区间 0..={}",
                self.climate_band_width,
                Self::MAX_CLIMATE_BAND_WIDTH
            ));
        }
        Ok(())
    }
}
