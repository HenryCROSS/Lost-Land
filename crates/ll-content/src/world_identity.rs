//! 世界身份四要素：种子 + 尺寸 + 地形形态 + 生成期 mod 集合。
//!
//! # 为什么是四个，不再是三个
//!
//! 地形形态参数（[`ll_world::generate::TerrainShape`]）与另外三个完全
//! 同性质：玩家在建档那一刻做出的选择，事后没有任何数据能反推，缺了它
//! 同一个世界就复现不出来（`knowledge/design/worldgen-parameters.md`
//! 五节「为什么形态参数必须进存档」）。它此前只住在存档**主体**
//! （`ll_world::state::WorldState::terrain_shape`），既不在本类型里、
//! 也不在存档头部——同一份文档把这条记为「一处已知的不对齐」。生成期
//! mod 集合修正批次把身份要素收拢成单一真相源时一并收进来，否则收拢
//! 出来的仍然是一个缺了一角的身份。
//!
//! # 单一真相源：绑定一次，此后只被搬运
//!
//! 四要素在**世界创建那一刻**由 [`WorldIdentity::bind`] 绑定一次，写进
//! 存档头；读档时由 [`WorldIdentity::restore_from_header`] 把存档头那
//! 一份**原样接回来**，存档时原样写回。存档路径上没有任何一处重新推导
//! 身份——尤其是生成期 mod 集合，重算它等于用「玩家现在开着哪些 mod」
//! 覆盖掉「这个世界当初是用哪些 mod 生成的」，这正是
//! `knowledge/audit/2026-08-26-phase-reckoning-p6-p8.md` 三节第 9 项
//! 记录的那条缺陷。
//!
//! 这条纪律不靠注释维持，靠类型：[`crate::header::SaveHeader`] 的四个
//! 身份字段是 `pub(crate)` 的，crate 外唯一能填它们的入口是
//! [`crate::header::SaveHeader::new`]，而它只接受一个已经绑好的
//! `&WorldIdentity`。存档路径（`ll_game::save::save_game`）因此**写不出**
//! 「现场重算一份生成期集合塞进头部」这行代码——不是不该写，是编译
//! 不过。
//!
//! # 为什么尺寸也是身份的一部分
//!
//! 项目所有者已定：地图大小在开局建档前由玩家选择，世界可以是长方形
//! （区块与瓦片本身是正方形，世界总体不必是）。种子相同、mod 集合相同
//! 但尺寸不同，噪声场采样的周期（[`ll_world::noise::TileableNoise`]）
//! 跟着变，产出的不是同一张地形——尺寸因此和种子、生成期 mod 集合一样
//! 「缺一，世界都复现不出来」，见 `knowledge/design/identity-and-ids.md`
//! 六节与本模块最初的会话记录。
//!
//! # 绑定时机：世界创建时刻
//!
//! 三要素的绑定不等待任何生成器——`ll_mod::mod_set` 模块文档「绑定
//! 时机」一节已经更正了「留给 P6 世界生成器」这句过期注释：规格插入
//! 新 P6（物品与装备）后，真正的历史世界生成器现排到 P7,而世界创建
//! （地形本身，从 P2 起就存在）不需要等它。[`WorldIdentity::bind`] 是
//! 这个绑定时机在类型层面的落点——调用它的地方就是"世界创建"这一刻，
//! 不应该在任何更晚的时间点（例如每次读档）重新调用。
//!
//! # 本模块不设计开局 UI
//!
//! `ll-ui` 完整控件库在 P7，本模块只交付"给定一个尺寸候选，返回是否
//! 安全"的纯函数校验（[`validate_size_choice`]）与一份推荐预设表
//! （[`RECOMMENDED_PRESETS`]），供未来 P7 UI 直接引用。

use ll_core::error::CoreError;
use ll_core::torus::TorusSize;
use ll_mod::manifest::mod_self_id;
use ll_mod::mod_set::{GenerationModSet, ModSetEntry};
use ll_world::WorldError;
use ll_world::generate::TerrainShape;
use ll_world::zone::ZoneLayout;

use crate::header::{ModHeaderEntry, SaveHeader};
use crate::mode::SaveMode;

/// 一档推荐的地图尺寸预设：区块边长（固定 48，与
/// [`ZoneLayout::default_config`] 一致）+ 世界区块数。
///
/// 四档预设全部选长方形（`zone_count` 的宽高不相等）——不是因为正方形
/// 必然踩雷（`safe_coarse_scale` 已经是通用算法级修复,见
/// `crates/ll-world/src/noise.rs` 模块文档「一个更隐蔽的退化」，任何
/// 尺寸修复后都安全），而是长方形天然远离「两轴周期相等」这个退化
/// 触发条件，不需要依赖减半分支就能确认安全,见
/// `crates/ll-world/tests/noise_presets.rs` 的多样性回归测试。
///
/// # 为什么区块边长从 128 改成 48
///
/// 见 [`ZoneLayout::default_config`] 文档：项目所有者裁定区块边长默认
/// 改为 48（`= CELL_SIZE * 3`，奇数倍数），这类取值下任何 `zone_count`
/// 都不会触发噪声大陆尺度层退化（同一份文档给出证明），比旧值 128
/// （`= CELL_SIZE * 8`，纯 2 的幂）更不容易踩雷——四档预设因此不需要
/// 重新论证一遍是否落在退化区间，`crates/ll-world/tests/noise_presets.rs`
/// 仍然保留实测多样性回归，作为独立于这条数学证明的经验性验证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizePreset {
    /// 供 UI 展示的标签，非最终文案（P7 UI 落地时按 Fluent 本地化）。
    pub label: &'static str,
    /// 区块边长（格）。
    pub zone_span: u32,
    /// 世界区块数 `(宽, 高)`。
    pub zone_count: (u32, u32),
}

/// 四档推荐预设：小/中/大/巨，区块边长固定 48。
///
/// 「标准」正是 [`ZoneLayout::default_config`] 给出的默认配置（96×64
/// 区块）——推荐预设表不是凭空另起一套数值，是把设计文档十一节的默认
/// 值纳入同一张表，与其余三档并列展示。其余三档在「标准」基础上按
/// 相同的宽高比例（4:3 / 3:2 交替，与旧版预设表同一种排布习惯）伸缩。
pub const RECOMMENDED_PRESETS: &[SizePreset] = &[
    SizePreset {
        label: "小陆地",
        zone_span: 48,
        zone_count: (64, 48),
    },
    SizePreset {
        label: "标准",
        zone_span: 48,
        zone_count: (96, 64),
    },
    SizePreset {
        label: "广阔",
        zone_span: 48,
        zone_count: (128, 96),
    },
    SizePreset {
        label: "浩瀚",
        zone_span: 48,
        zone_count: (192, 128),
    },
];

/// 一档推荐的**地形形态**预设：稳定标识 + 两个 Fluent 键 + 一组
/// [`TerrainShape`]。
///
/// # 为什么是 Rust 常量，不是 JSON5 内容（这是本批次的一处判断，不是
/// 所有者原话）
///
/// 本项目的架构是「JSON5 内容 + Rust 行为」，一张只有整数的表看起来
/// 天然属于内容侧。三条理由让它留在 Rust：
///
/// 1. **它与既有的尺寸预设表是同一件东西。**[`RECOMMENDED_PRESETS`]
///    （地图尺寸预设，玩家在同一个开局界面上做的同一类选择）从落地
///    起就是本文件里的 Rust 常量。把地形形态预设做成内容，会让开局
///    界面上并排的两组选项来自两套完全不同的机制。
/// 2. **这些数值的正当性由引擎自己的实测测试背书。**
///    `crates/ll-content/tests/terrain_presets.rs` 直接断言「群岛预设的
///    水域比例必须显著高于大陆预设」这类性质。那条测试住在 `ll-world`，
///    而 `ll-world` 不能反向依赖 `ll-mod`/`ll-content`——预设一旦搬进
///    JSON5，测试就只能把同一批数字再抄一遍，凭空造出一处必须手工保持
///    同步的重复（`crates/ll-world/tests/noise_presets.rs` 模块文档已经
///    如实记录过一次同样的重复，那是被依赖方向逼出来的，不该主动再造
///    第二处）。
/// 3. **做成内容要付的代价与收益不成比例。**那意味着一套新的 JSON5
///    schema、一个注册期加载器，以及
///    `ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION` 递增——
///    换来的只是「第三方 mod 能加一档地形预设」。而 mod 真正想改地形
///    时要的是自己的生成算法，不是往一张四整数表里再加一行。
///
/// 这条判断随时可以推翻：真有 mod 需要声明自己的地形预设时，把这张表
/// 搬进内容侧是一次机械改动，届时再付那套代价也不迟（YAGNI）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainPreset {
    /// 稳定标识——玩家在配置文件里写的就是这个字符串，永远不随译文变化。
    pub id: &'static str,
    /// 供 UI/日志展示的名字，走 Fluent（`assets/locales/*.ftl`）。
    pub display_name_key: &'static str,
    /// 一句话说明这档预设是什么样的世界，同样走 Fluent。
    pub description_key: &'static str,
    /// 这档预设对应的地形形态参数。
    pub shape: TerrainShape,
}

/// [`TERRAIN_PRESETS`] 里默认那一档的标识——找不到玩家指定的标识时
/// 退回到它，也是配置文件缺省时的取值。
///
/// 它对应的 [`TerrainShape`] 必须与 [`TerrainShape::default`] **逐位
/// 相同**：两条黄金基准（`crates/ll-world/tests/determinism.rs` 的
/// `EXPECTED_WORLD_DIGEST`、`crates/ll-sim/tests/replay.rs` 的
/// `EXPECTED_REPLAY_DIGEST`）固定的正是那张地图，
/// `crates/ll-content/src/world_identity.rs` 的测试
/// `大陆预设与地形形态默认值逐位相同` 把这条约束钉死。
pub const DEFAULT_TERRAIN_PRESET_ID: &str = "continent";

/// 四档推荐地形形态预设：大陆 / 群岛 / 山地 / 内陆。
///
/// # 数值从哪来（全部实测，不是拍脑袋）
///
/// 每一档的数值都在本体「标准」尺寸（96×64 区块 = 4608×3072 格）下实测过
/// 水域比例、深水比例、山地比例、独立陆块数、最大陆块占全部陆地的比例、
/// 海岸线占陆地的比例六项（「大陆」那一档取十个种子的均值，其余三档取
/// 五个种子——种子数按行不同，完整数据与调参过程见
/// `knowledge/design/worldgen-parameters.md`）。名字与实测数据必须对得上——
/// `crates/ll-content/tests/terrain_presets.rs` 逐条断言这件事。
///
/// | 预设 | 水域 | 深水 | 山地 | 独立陆块 | 最大陆块占陆地 |
/// |---|---|---|---|---|---|
/// | 大陆（10 种子） | 37.3% | 25.5% | 3.0% | 9.4 | 98.9% |
/// | 群岛（5 种子） | 72.6% | 60.2% | 1.6% | 251.6 | 8.2% |
/// | 山地（5 种子） | 25.0% | 15.9% | 24.4% | 6.2 | 98.7% |
/// | 内陆（5 种子） | 15.9% | 9.0% | 3.0% | 3.8 | 99.8% |
///
/// # 为什么「群岛」必须同时动 `continent_shrink`
///
/// 光把海平面调高**得不到群岛**，得到的是「一块被淹得只剩边角的大陆」：
/// 实测 `sea_level = 600`（水域 82.2%）时，最大的那一块陆地仍占全部
/// 陆地的 40.3%。原因是噪声的大陆尺度由世界尺寸自动推导，与阈值无关，
/// 抬高海平面只是把同一批大陆的低处淹掉，不会把它们切碎。真正把陆地
/// 切成群岛的是 [`TerrainShape::continent_shrink`]（见
/// `ll_world::noise::TileableNoise::shrink_continents`）——实测它把独立
/// 陆块数从 10.8 抬到 88.2（缩两档）而水域比例几乎不动（35.5% →
/// 37.1%），这正是「碎」与「淹」两件事被拆开的证据。
pub const TERRAIN_PRESETS: &[TerrainPreset] = &[
    TerrainPreset {
        id: DEFAULT_TERRAIN_PRESET_ID,
        display_name_key: "lostland:worldgen.preset.continent.display_name",
        description_key: "lostland:worldgen.preset.continent.description",
        // 必须与 TerrainShape::default() 逐位相同，见
        // DEFAULT_TERRAIN_PRESET_ID 文档。
        //
        // 四档预设的 climate_band_width 全部取默认值：气候条带是**纬度**
        // 的函数，与「水陆比例/山地多少/陆地碎成几块」这三件由高度决定
        // 的事正交，没有哪一档地形形态天然该配一条更宽或更窄的气候带。
        // 要单独调，改 config.json5 的 new_game.climate_band_width。
        shape: TerrainShape {
            sea_level: 400,
            mountain_level: 750,
            octaves: 4,
            continent_shrink: 0,
            climate_band_width: TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH,
        },
    },
    TerrainPreset {
        id: "archipelago",
        display_name_key: "lostland:worldgen.preset.archipelago.display_name",
        description_key: "lostland:worldgen.preset.archipelago.description",
        shape: TerrainShape {
            sea_level: 540,
            mountain_level: 780,
            octaves: 4,
            continent_shrink: 2,
            climate_band_width: TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH,
        },
    },
    TerrainPreset {
        id: "highland",
        display_name_key: "lostland:worldgen.preset.highland.display_name",
        description_key: "lostland:worldgen.preset.highland.description",
        shape: TerrainShape {
            sea_level: 350,
            mountain_level: 620,
            octaves: 4,
            continent_shrink: 0,
            climate_band_width: TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH,
        },
    },
    TerrainPreset {
        id: "inland",
        display_name_key: "lostland:worldgen.preset.inland.display_name",
        description_key: "lostland:worldgen.preset.inland.description",
        shape: TerrainShape {
            sea_level: 300,
            mountain_level: 760,
            octaves: 4,
            continent_shrink: 0,
            climate_band_width: TerrainShape::DEFAULT_CLIMATE_BAND_WIDTH,
        },
    },
];

/// 按稳定标识查一档地形形态预设；标识不认识时返回 [`None`]，由调用方
/// 决定是记日志退回默认还是报错——本函数不替调用方做那个决定。
///
/// 线性扫描而非 `HashMap`：四条记录，且约束 C5 明令逻辑判断不得依赖
/// 哈希容器迭代顺序，这里连引入的理由都没有。
pub fn terrain_preset(id: &str) -> Option<&'static TerrainPreset> {
    TERRAIN_PRESETS.iter().find(|preset| preset.id == id)
}

/// 校验一组尺寸选择是否能构造出合法的 [`ZoneLayout`]。
///
/// 两步校验：`zone_count` 本身必须是合法的 [`TorusSize`]（非零、不超过
/// [`TorusSize::MAX_EXTENT`]），再交给 [`ZoneLayout::new`] 校验区块边长
/// 的对齐约束——不重新实现任何一层校验规则，只是把两层串起来给一个
/// 统一的入口，供未来 P7 开局界面直接调用。
///
/// `zone_count` 不合法（零或溢出）时按 [`WorldError::WorldTooSmall`]
/// 报告——`TorusSize::new` 本身不携带失败原因，而「宽高任一维为零」与
/// 「尺寸小于视口所需跨度」在用户可见的意义上是同一类问题（尺寸选得
/// 不合理），复用这个既有变体不需要为一个不会被任何推荐预设触发的
/// 边界新增变体。
pub fn validate_size_choice(
    zone_span: u32,
    zone_count: (u32, u32),
) -> Result<ZoneLayout, WorldError> {
    let count = TorusSize::new(zone_count.0, zone_count.1).ok_or(WorldError::WorldTooSmall {
        width: zone_count.0,
        height: zone_count.1,
    })?;
    ZoneLayout::new(zone_span, count)
}

/// 世界身份四要素——种子、尺寸、地形形态、生成期 mod 集合——捆绑在
/// 一起的类型，四者缺一，同一个世界都无法复现（见模块文档）。
///
/// # 为什么字段是私有的
///
/// 身份的每一个要素都只在世界创建那一刻确定一次，此后只被搬运。公开
/// 字段等于公开一条「事后改一改」的通路，而这条缺陷（存档时把生成期
/// 集合重算一遍）的形状恰恰就是「事后改了一改」。私有字段 + 两个具名
/// 构造器（[`Self::bind`] 建档、[`Self::restore_from_header`] 读档搬运）
/// 让「什么时候允许决定身份」这件事在类型层面只有两个答案。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldIdentity {
    seed: u64,
    zone_layout: ZoneLayout,
    terrain_shape: TerrainShape,
    generation_mods: GenerationModSet,
    mode: SaveMode,
}

impl WorldIdentity {
    /// 在世界创建时刻一次性捆绑四要素。
    ///
    /// 本方法本身就是"绑定时机"的落点：调用它的地方就是"世界创建"这
    /// 一刻——不应该在任何更晚的时间点（例如每次读档、每次存档）重新
    /// 调用，读档走 [`Self::restore_from_header`] 把存档头那一份原样接
    /// 回来，不是重新推导。
    pub fn bind(
        seed: u64,
        zone_layout: ZoneLayout,
        terrain_shape: TerrainShape,
        generation_mods: GenerationModSet,
        mode: SaveMode,
    ) -> Self {
        WorldIdentity {
            seed,
            zone_layout,
            terrain_shape,
            generation_mods,
            mode,
        }
    }

    /// 读档时把存档头记录的世界身份**原样接回来**。
    ///
    /// # 四个要素各自从哪里来（不是随便挑的）
    ///
    /// - **生成期 mod 集合、种子**：取自存档头。这两项在头部有权威记录，
    ///   且生成期集合**只有**头部这一份——本方法存在的全部理由就是把它
    ///   接回来而不是重算。
    /// - **尺寸**：取自调用方传入的 `zone_layout`。存档头只记了区块数
    ///   （[`SaveHeader::world_size`]），不记区块边长（见该字段文档），
    ///   而存档主体里的 `ZoneLayout` 两者都全，是更完整的同一个值。
    /// - **地形形态**：取自调用方传入的 `terrain_shape`，来源是存档
    ///   主体（`WorldState::terrain_shape`）——主体那一份是流式生成真正
    ///   读的权威副本，且**一定存在**；头部那一份是本批次新增的展示用
    ///   副本，本批次之前写出的存档里没有（见
    ///   [`SaveHeader::terrain_shape`]）。取主体因此既权威又不会在老存档
    ///   上退化。
    ///
    /// # 错误
    ///
    /// 存档头里的命名空间字符串拼不出合法的 [`ll_core::ident::NamespacedId`]
    /// 时返回 [`CoreError`]——正常路径下不会发生（写出时这些命名空间都
    /// 是从合法 `NamespacedId` 取出来的），但存档是外部数据，不做「一定
    /// 合法」的假设。
    pub fn restore_from_header(
        header: &SaveHeader,
        zone_layout: ZoneLayout,
        terrain_shape: TerrainShape,
    ) -> Result<Self, CoreError> {
        let mut entries = Vec::with_capacity(header.generation_mods.len());
        for entry in &header.generation_mods {
            entries.push(ModSetEntry {
                id: mod_self_id(&entry.namespace)?,
                version: entry.version.clone(),
                content_hash: entry.content_hash,
            });
        }
        Ok(WorldIdentity {
            seed: header.world_seed,
            zone_layout,
            terrain_shape,
            generation_mods: GenerationModSet(entries),
            // 模式取自存档头——它是这条事实**唯一**的持久记录，与生成期
            // mod 集合同性质：读档时只能搬运，不能重新推导。重新推导的话
            // 「这局是不是肉鸽」就变成了「玩家现在的偏好设置是什么」，
            // 单向不可逆当场失效。
            mode: header.mode,
        })
    }

    /// 生成本世界地形所用的种子。
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// 世界尺寸（区块边长 + 区块数）。
    pub fn zone_layout(&self) -> &ZoneLayout {
        &self.zone_layout
    }

    /// 建档时选定的地形形态参数。
    pub fn terrain_shape(&self) -> TerrainShape {
        self.terrain_shape
    }

    /// 生成期 mod 集合快照——绑定后永久不变。
    pub fn generation_mods(&self) -> &GenerationModSet {
        &self.generation_mods
    }

    /// 这个世界的存档模式：肉鸽（[`SaveMode::Permadeath`]）还是普通
    /// （[`SaveMode::FreeSave`]）。
    pub fn mode(&self) -> SaveMode {
        self.mode
    }

    /// 玩家可以手动存档吗——肉鸽模式下只有自动保存，没有手动存档入口。
    ///
    /// 判据收在这里而不是让每个 UI 各写一次 `matches!`：暂停菜单要用它
    /// 决定「保存」那一行在不在，将来的任何入口也该问同一个问题。
    pub fn allows_manual_save(&self) -> bool {
        matches!(self.mode, SaveMode::FreeSave { .. })
    }

    /// **唯一**允许的模式变化：肉鸽 → 普通。返回真表示这次调用真的改变
    /// 了模式。
    ///
    /// # 反向为什么写不出来
    ///
    /// 本方法内部就是 [`SaveMode::downgrade`]，而那个函数的 `match` 里
    /// **没有任何一个分支返回 [`SaveMode::Permadeath`]**，且
    /// `FreeSave` 的「曾经降级过」标记是 `crate::mode` 模块私有的。因此
    /// 「普通 → 肉鸽」不是「不该写」，是**写不出来**——本类型上也没有
    /// 任何 `set_mode`，字段是私有的，唯一能产出 `Permadeath` 的入口是
    /// [`Self::bind`]，而调用它的地方按定义就是「创建一个新世界」。
    ///
    /// 判据不在这里重写一份：本方法只是把那个单向转换接到世界身份上。
    ///
    /// ```compile_fail
    /// # use ll_content::world_identity::WorldIdentity;
    /// # use ll_content::mode::SaveMode;
    /// fn 把普通档改回肉鸽(identity: &mut WorldIdentity) {
    ///     // 没有 set_mode，字段也是私有的——这一行编译不过。
    ///     identity.set_mode(SaveMode::Permadeath);
    /// }
    /// ```
    pub fn downgrade_mode(&mut self) -> bool {
        match self.mode.downgrade() {
            Some(relaxed) => {
                self.mode = relaxed;
                true
            }
            None => false,
        }
    }
}

/// 把 [`GenerationModSet`]（`ll_mod::mod_set`）转换成
/// [`crate::header::SaveHeader::generation_mods`] 可以直接使用的
/// `Vec<ModHeaderEntry>`。
///
/// # 断链三修复（P5-A 任务 14）
///
/// `ll_mod::mod_set::ModSetEntry` 与 `crate::header::ModHeaderEntry`
/// 字段形状几乎相同（命名空间 + 版本号 + 内容哈希），但分属两个不同
/// crate 的类型，此前没有任何生产代码把两者接起来——`ll-content` 全部
/// 现存测试（含 P5 批次 E、L6 端到端脚手架）都是直接手写
/// `Vec<ModHeaderEntry>` 或干脆留空，验收 demo（任务 13）为了走通
/// `WorldIdentity::bind` 到「可以写进存档头」这一环，临时在 demo 自己
/// 的代码里补了一份等价的转换逻辑（不是生产代码），并如实记录了这处
/// 缺口。本函数是补上的那一环——`ModHeaderEntry` 只用 `String`/整数/
/// 枚举这类原始类型（见 [`crate::header`] 模块文档「为什么头部不能
/// 引用 `ContentIndex`」），转换本身只是把 `NamespacedId` 取出命名空间
/// 部分、版本号与内容哈希原样搬过来，不涉及任何需要额外校验或推导的
/// 逻辑。
///
/// 调用点：见 [`crate::save_file`] 的存档写出流程测试与
/// `crates/ll-content/examples/p5_save_acceptance.rs`——两处都已经改为
/// 调用这个函数，不再各自重新发明一份等价的搬运代码。
pub fn generation_mods_to_header_entries(set: &GenerationModSet) -> Vec<ModHeaderEntry> {
    set.0
        .iter()
        .map(|entry| ModHeaderEntry {
            namespace: entry.id.namespace().to_string(),
            version: entry.version.clone(),
            content_hash: entry.content_hash,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::NamespacedId;
    use ll_mod::manifest::ModManifest;
    use ll_mod::registry::Registry;

    fn manifest(namespace: &str, version: &str) -> ModManifest {
        ModManifest {
            id: NamespacedId::parse(&format!("{namespace}:self")).expect("测试用命名空间恒合法"),
            version: version.to_string(),
            dependencies: Vec::new(),
        }
    }

    fn id(raw: &str) -> NamespacedId {
        NamespacedId::parse(raw).expect("测试用标识符恒合法")
    }

    #[test]
    fn generationmodset一旦封存后与后续currentmodset的变化无关() {
        // 类型上两者已经隔离（GenerationModSet/CurrentModSet 是不同
        // 类型，见 ll_mod::mod_set 模块文档的 compile_fail 示例），这里
        // 补运行期断言：捆绑进 WorldIdentity 之后，registry 继续变化
        // 不会让已经绑定的三要素跟着漂移。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let manifests = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);
        let identity = WorldIdentity::bind(
            42,
            ZoneLayout::default_config(),
            TerrainShape::default(),
            generation.clone(),
            SaveMode::fresh_free_save(),
        );

        // Act：世界创建之后 registry 继续变化。
        registry.intern(id("lostland:river"));

        // Assert：已绑定的三要素原样不变。
        assert_eq!(identity.generation_mods(), &generation);
    }

    #[test]
    fn 清单里的版本号原样进存档头且改一个字符就打不开() {
        // 项目所有者裁决：「新版本不兼容旧版本存档就是了，版本不对就
        // 打不开。」这条测试把那句话钉成一条可执行的断言，钉的是整条
        // 链——`mod.json5` 的 `version` → `ModHeaderEntry.version` →
        // `check_mod_set` 硬门禁——而不是链上任何单独一环。
        //
        // 为什么值得单独钉：链上每一环各自都有测试，但「改 mod.json5
        // 的版本号会让此前的存档全部打不开」这个**后果**此前没有任何
        // 一条测试直说。它是一颗定时炸弹还是一条有意的策略，区别只在
        // 于有没有人把它写下来——策略见
        // knowledge/design/save-and-mod-version-policy.md。
        // Arrange：生成期的 mod 清单里版本是 0.1.0。
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let 生成期清单 = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &生成期清单);
        let 存档头条目 = generation_mods_to_header_entries(&generation);

        // Assert 其一：版本号原样搬进存档头，没有任何规范化。
        assert_eq!(存档头条目[0].version, "0.1.0");

        // Act & Assert 其二：清单版本没动 → 放行。
        assert!(crate::load_error::check_mod_set(&存档头条目, &生成期清单).is_ok());

        // Act & Assert 其三：只改末尾一个字符 → 硬门禁拒绝。
        // 不做语义化版本解析，"0.1.1 是 0.1.0 的兼容升级"这种判断在这
        // 里不存在，也不该存在。
        let 改过版本的清单 = vec![manifest("lostland", "0.1.1")];
        let err = crate::load_error::check_mod_set(&存档头条目, &改过版本的清单)
            .expect_err("版本号改了就该打不开");
        assert!(
            matches!(err, crate::load_error::LoadError::ModSetMismatch(_)),
            "实际是 {err:?}"
        );
    }

    #[test]
    fn generation_mods_to_header_entries产出的条目字段与源数据逐一对应() {
        // 断链三修复的核心验证：GenerationModSet -> Vec<ModHeaderEntry>
        // 这次转换本身只是原样搬运,不丢字段、不改数值。
        // Arrange
        let mut registry = Registry::new();
        registry.intern(id("lostland:mountain"));
        let manifests = vec![manifest("lostland", "0.1.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);

        // Act
        let entries = generation_mods_to_header_entries(&generation);

        // Assert
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].namespace, "lostland");
        assert_eq!(entries[0].version, "0.1.0");
        assert_eq!(
            entries[0].content_hash,
            registry.content_hash_of("lostland")
        );
    }

    #[test]
    fn generation_mods_to_header_entries对未贡献内容的mod保留空哈希() {
        // 裁定 P5-8 配套：「在场但从未贡献内容」的 content_hash 是
        // None,转换过程不能把它折叠成任何裸整数（例如 0）。
        // Arrange
        let registry = Registry::new();
        let manifests = vec![manifest("emptymod", "1.0.0")];
        let generation = GenerationModSet::capture(&registry, &manifests);

        // Act
        let entries = generation_mods_to_header_entries(&generation);

        // Assert
        assert_eq!(entries[0].content_hash, None);
    }

    #[test]
    fn generation_mods_to_header_entries对空集合产出空列表() {
        // Arrange
        let generation = GenerationModSet(Vec::new());

        // Act
        let entries = generation_mods_to_header_entries(&generation);

        // Assert
        assert!(entries.is_empty());
    }

    #[test]
    fn 每个推荐预设满足zonelayout现有构造约束() {
        // Arrange & Act & Assert
        for preset in RECOMMENDED_PRESETS {
            let result = validate_size_choice(preset.zone_span, preset.zone_count);
            assert!(
                result.is_ok(),
                "预设 {} 未能构造出合法的 ZoneLayout: {:?}",
                preset.label,
                result
            );
        }
    }

    #[test]
    fn validate_size_choice对不满足cell_size整除约束的尺寸返回错误() {
        // Arrange：50 不是 CELL_SIZE(16) 的整数倍。
        // Act
        let result = validate_size_choice(50, (4, 4));

        // Assert
        assert!(matches!(
            result,
            Err(WorldError::ZoneSpanNotAligned { zone_span: 50 })
        ));
    }

    #[test]
    fn validate_size_choice对零区块数返回错误而不panic() {
        // Arrange & Act
        let result = validate_size_choice(128, (0, 32));

        // Assert
        assert!(matches!(result, Err(WorldError::WorldTooSmall { .. })));
    }

    #[test]
    fn 标准预设与zonelayout默认配置产出相同的区块布局() {
        // 「中」档预设不是另起一套数值,是纳入了设计文档十一节的默认值
        // ——这里锁住两者确实一致。
        // Arrange
        let standard = RECOMMENDED_PRESETS[1];

        // Act
        let from_preset =
            validate_size_choice(standard.zone_span, standard.zone_count).expect("标准预设恒合法");

        // Assert
        assert_eq!(from_preset, ZoneLayout::default_config());
    }
}
