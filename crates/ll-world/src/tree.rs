//! 树木：**默认派生，只存偏差**（[ADR 0009]）。
//!
//! # 这个模块解决的是一个规模问题，不是一个玩法问题
//!
//! 世界 3072×2304 ＝ 7 077 888 格。森林哪怕只占 15%，也是**一百万格以上**。
//! 「每棵树一个实体」在这个规模上既存不下也算不动——这正是 [ADR 0009] 记录的
//! 那堵墙，钱包、姓名、人际关系、性格已经各自撞过一次。树是第六次复用。
//!
//! ```text
//! 这一格有没有树、是什么树
//!     = 派生(种子, 位置, 地形, 气候)          ← 零存储，用时现算
//!     ⊕ 偏差(仅被砍/被种/被采过的那些格)      ← 进存档
//! ```
//!
//! 两层的**唯一合流点**是 [`tree_at`]。任何「这一格现在有没有树」的问题都必须
//! 经它，不许有第二处各自拼一遍两层的合并规则——那正是 [ADR 0021] 要拦的形状。
//!
//! # 项目所有者知情并接受的代价
//!
//! **不能给一棵没动过的树单独设属性。它在存档里根本不存在。**
//!
//! 一棵长在 `(1234, 567)` 的橡树，在你砍它/种它/采它之前，世界状态里**没有任何
//! 一个字节**属于它——它是 [`derived_tree_at`] 每次被问到时现算出来的。因此：
//!
//! - **单棵想特殊化：可以。** 给它写一条 [`TreeDeviation`] 记录，那一格从此
//!   有存储、有身份。
//! - **成片想特殊化：不行。** 「把这片林子里所有树都标成受诅咒的」等于给
//!   十万格各写一条记录，派生带来的收益当场消失（[ADR 0009]「适用条件」第 2
//!   条：偏移必须是稀疏事件）。
//!
//! 这条代价是 2026-09-01 项目所有者亲自裁定并接受的，不是实现者选的。
//! **写在这里是为了让下一个人不必重新推导。** 要推翻它，推翻的是 ADR 0009 在
//! 树上的这次应用，不是改一个函数。
//!
//! # 派生层为什么不碰 `DetRng`（约束 C3）
//!
//! [`derived_tree_at`] 在**渲染期**每帧被调用的次数取决于「这一帧画了几格」
//! ——摄像机缩放、窗口大小、视野半径都会改变它。让这种量参与随机流，等于把
//! 确定性重放交给显示器分辨率。批次 28 的地形变体在同一处做过同一条裁定
//! （见 `ll_game::layout::terrain_variant_at` 文档「为什么不用 `DetRng`」）。
//!
//! 因此派生层走**位置哈希**：[`ll_core::hashing::StateHasher`]（FNV-1a），
//! 它的模块文档写明「完全由整数运算构成、由规范唯一确定，因而跨平台跨版本
//! 恒定」——正是这里需要的性质。**本模块一次 `DetRng` 调用都没有**，由
//! `crates/ll-world/tests/tree_derivation.rs` 的一条读源码断言钉住。
//!
//! [ADR 0009]: ../../../knowledge/decisions/0009-derive-by-default-store-only-deviation.md
//! [ADR 0021]: ../../../knowledge/decisions/0021-abstraction-requires-shared-algorithm-not-symmetry.md

use std::collections::BTreeMap;

use ll_core::hashing::StateHasher;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use serde::{Deserialize, Serialize};

use crate::climate::{ClimateBand, band_at};
use crate::state::WorldState;
use crate::terrain::TerrainKind;

/// 森林里有树的格子占千分之几。
///
/// # 这个数字是怎么定的
///
/// 传统 roguelike 的林地要能走得进去：满格都是树等于一堵墙，玩家只会绕着走。
/// 620‰ 意味着大约每五格里有两格是空地，成片时读起来是「树多但有路」。
///
/// **它不是平衡数值，是密度旋钮**：调它只改变派生层，不动任何存储；调完三条
/// 黄金基准**不会**变（派生层不进世界状态，见本模块文档），但屏幕上的林子会
/// 明显变疏或变密。
const TREE_DENSITY_PERMILLE: u32 = 620;

/// [`TREE_DENSITY_PERMILLE`] 的折算基数（千分比）。
const PERMILLE_TOTAL: u32 = 1000;

/// 一条气候带上三种树的权重之和，也是树种折算的基数。
///
/// 取 16 而不是 100：折算走 Lemire 乘法取高位（见 [`fold`]），基数大小不影响
/// 精度，小基数让 [`species_weights`] 那张表一眼能算清比例。
const BAND_WEIGHT_TOTAL: u32 = 16;

/// 果子采摘之后重新长好需要多少 tick。
///
/// # 为什么要有这个，而不是「采过就永远没了」
///
/// 「采过就没了」会让 [`TreeDeviation`] 变成**单向棘轮**：玩家每采一棵树就
/// 永久多一条记录，长期游玩后偏差表无界增长——[ADR 0009] 的「棘轮问题」一节
/// 记录的正是这类返工（升格曾经也是单向的）。果子会长回来，意味着一棵树被采
/// 一百次也只占**一条**记录，且那条记录在果子长好之后语义上退回「与派生一致」。
///
/// 取值 `86_400`：`ll_core::time` 的一天是 86400 tick（一 tick 一秒），
/// 即**一天长一次果**。这是内容参数，不是平衡结论。
///
/// [ADR 0009]: ../../../knowledge/decisions/0009-derive-by-default-store-only-deviation.md
pub const FRUIT_REGROW_TICKS: i64 = 86_400;

/// 树种。**引擎侧静态枚举，不是内容表。**
///
/// # 为什么在引擎侧（两条路的代价对照）
///
/// 完整对照见 `docs/superpowers/plans/2026-09-01-batch32-trees.md` 第五节，
/// 这里留结论与被否掉那条路的**致命伤**：
///
/// - **内容侧声明**（新开 `mods/lostland/trees.json5`，配一整套 schema、
///   内容哈希写入器、注册表与审计）：mod 作者能自带树种是真收益，但今天没有任何 mod 需要它
///   （YAGNI）；代价是 `ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION`
///   要升，并且——**这一条才是致命的**——[`derived_tree_at`] 是每帧上千次的
///   纯函数，让它去查一张 `ContentIndex` 表意味着它要么持有注册表引用、要么
///   被迫变成有状态的东西，本模块「无依赖纯函数」这条性质当场没了。
/// - **本模块（选中）**：一个三变体枚举 + 一张常量权重表，与
///   `ll_game::layout::terrain_variant_count` 那张引擎侧静态表逐字同构
///   （一串常量 + `match`，不经任何哈希容器，约束 C5）。
///
/// **留给「mod 真的要自带树种」那天再走内容侧**，那时这段文档就是落点。
///
/// # 声明侧与资产侧是两份清单，会漂
///
/// 本枚举有三个变体，`assets` 里就得有三张贴图。漂了当场红，两个方向都锁：
/// `crates/ll-game/tests/atlas_coverage.rs` 的
/// `每一种树在真实图集里都查得到条目`（多声明了、图没画）与
/// `图集里不许有声明侧数不出来的树贴图`（图画了、枚举没加）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TreeSpecies {
    /// 橡树：温带的主力，木料产量最高。
    Oak,
    /// 松树：极地带的主力。
    Pine,
    /// 棕榈：干热带的主力，木料产量最低（细杆，没多少可用木）。
    Palm,
}

impl TreeSpecies {
    /// 全部变体，**顺序固定**（数组字面量，不经任何哈希容器，约束 C5）。
    ///
    /// 门禁与分布测试遍历的是这个数组而不是点名三个具体变体——加第四种树时
    /// 那些判据自动开始管它，不会出现 [ADR 0022] 点名的「判据适用面被新代码
    /// 绕过」。
    ///
    /// [ADR 0022]: ../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md
    pub const ALL: [TreeSpecies; 3] = [TreeSpecies::Oak, TreeSpecies::Pine, TreeSpecies::Palm];

    /// 图集条目名的**裸名**（不带命名空间前缀）。
    ///
    /// 与 `tools/ll-artgen` 那三条配方的名字、与 `draw_entry` 的三支派发
    /// **三处逐字一致**。运行期真正被查的键是带前缀的
    /// （`ll_mod::asset_vfs` 把清单条目名与所属命名空间拼起来），拼接由
    /// 调用方做——本模块在 `ll-world`，不认识命名空间这一层。
    pub fn sprite_stem(self) -> &'static str {
        match self {
            TreeSpecies::Oak => "tree_oak",
            TreeSpecies::Pine => "tree_pine",
            TreeSpecies::Palm => "tree_palm",
        }
    }

    /// 砍倒一棵这种树产出几份木料。
    ///
    /// **这是「多树种」在玩法上唯一真实的差异**（贴图之外）。三个数字刻意
    /// 不同：写成一样的话，「不同树种砍出的木料数量不同」那条判据会退化成
    /// 恒真——[ADR 0022] 点名的「生产数据恰好让判据退化」正是这个形状。
    ///
    /// [ADR 0022]: ../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md
    pub fn timber_yield(self) -> u32 {
        match self {
            TreeSpecies::Oak => 3,
            TreeSpecies::Pine => 2,
            TreeSpecies::Palm => 1,
        }
    }

    /// 哈希判别值——[`TreeDeviations::write_hash`] 与任何需要把树种混进
    /// 摘要的地方共用这一处。
    ///
    /// 不用 `self as u64`：那会让「往枚举中间插一个变体」静默改变全部既有
    /// 摘要，而插变体这件事本身不该是一次黄金基准重冻。显式的常量让「改了
    /// 判别值 = 主动重冻」变成一个看得见的动作。
    fn hash_tag(self) -> u64 {
        match self {
            TreeSpecies::Oak => 1,
            TreeSpecies::Pine => 2,
            TreeSpecies::Palm => 3,
        }
    }
}

/// 一条气候带上三种树的权重，与 [`TreeSpecies::ALL`] **同序**，和恒为
/// [`BAND_WEIGHT_TOTAL`]。
///
/// | 气候带 | Oak | Pine | Palm |
/// |---|---|---|---|
/// | `Hot`（干热带） | 3 | 0 | 13 |
/// | `Temperate`（温带） | 11 | 4 | 1 |
/// | `Polar`（极地带） | 2 | 14 | 0 |
///
/// # 为什么是**权重混合**而不是「一条带一种树」
///
/// 硬映射（热带只有棕榈、温带只有橡树、极地只有松树）会让「树种受气候影响」
/// 这条判据退化成「查一次 `match`」：把权重表整个改坏，测试照样绿——因为
/// 判据只看得见「三条带的树种不同」，而硬映射下那永远成立。权重混合让每条带
/// 变成一个**可测的比例**，`三条气候带的树种分布互不相同` 才真的咬得住。
///
/// **含 0 权重是刻意的**（热带无松、极地无棕榈）：它让「某条带真的一棵某种树
/// 都没有」成为一条可以被断言的事实，而不是一个碰巧没抽到的巧合。
fn species_weights(band: ClimateBand) -> [u32; 3] {
    match band {
        ClimateBand::Hot => [3, 0, 13],
        ClimateBand::Temperate => [11, 4, 1],
        ClimateBand::Polar => [2, 14, 0],
    }
}

/// Lemire 乘法折算：把一个 64 位摘要均匀折进 `[0, n)`。
///
/// # 为什么取高位而不是取余
///
/// FNV-1a 的**低位雪崩弱**：相邻格子的摘要低几位相关性明显，直接 `% n` 会在
/// 规则网格上留下肉眼可见的条纹。乘法取高位是仓库已有的写法
/// （`ll_core::rng::DetRng::gen_range` 逐字同一条），没有理由在这里另发明一个。
fn fold(digest: u64, n: u32) -> u32 {
    ((u128::from(digest) * u128::from(n)) >> 64) as u32
}

/// 位置哈希：`seed ‖ x ‖ y ‖ 域名`。
///
/// # 混入次序为什么是「域名放最后」
///
/// **这条是批次 28 实测出来的，不是设计出来的。** FNV-1a 逐字节
/// 「异或 + 乘质数」，最后混进去的那几个字节没有足够轮数被摊开；而
/// `write_i64` 写的是**小端 8 字节**，世界坐标那种小整数**高 7 字节全是 0**
/// ——相邻两格只差最低那一个字节，之后只剩 7 轮「异或 0 再乘」。
///
/// 后果是**最后混进去的那个维度，相邻格子会明显倾向取到同一个结果**。批次 28
/// 在地形变体上实测到：把 `y` 放最后，纵向相邻同变体的比例从 32% 跳到 47%，
/// 画面上就是一条条竖纹（见 `ll_game::layout::terrain_variant_at` 文档那张表）。
///
/// 本函数把长字符串域名放在最后，恰好给 `seed`/`x`/`y` 那 24 个字节补足混合
/// 轮数。**本批自己重新实测了一遍，没有假设它会自动成立**，数字见
/// `crates/ll-world/tests/tree_derivation.rs` 的
/// `相邻格子长树与否的相关性接近相互独立`。
///
/// # 为什么两个域各哈希一次，而不是切同一个摘要的两段位域
///
/// 「有没有树」与「是什么树」必须互相独立。切位域会让两者共享同一次雪崩的
/// 结果，而 FNV-1a 的低位雪崩本来就弱——高位那一段给了密度，低位那一段给
/// 树种，树种就会跟着密度走。两次哈希各 24 字节输入，仍是常数时间。
fn position_digest(seed: u64, pos: TorusPos, domain: &str) -> u64 {
    let mut hasher = StateHasher::new();
    hasher.write_u64(seed);
    hasher.write_i64(i64::from(pos.x()));
    hasher.write_i64(i64::from(pos.y()));
    // **次序不是随手写的**，见本函数文档。域名（长字符串）恒在最后。
    hasher.write_len_prefixed_bytes(domain.as_bytes());
    hasher.finish()
}

/// 「这一格有没有树」那一路的域名。
const DOMAIN_PRESENCE: &str = "lostland:tree/presence";
/// 「这一格是什么树」那一路的域名。
const DOMAIN_SPECIES: &str = "lostland:tree/species";

/// **派生层**：这一格按公式该长什么树，`None` 表示按公式这里没有树。
///
/// **这个函数不知道偏差层的存在**，也不该知道——它就是「世界原本长什么样」。
/// 要问「这一格现在有没有树」，问 [`tree_at`]。
///
/// # 判据（三步，全部 O(1)）
///
/// 1. **只有 `forest` 地形长树。** `forest` 保留当底图是项目所有者的要求原话。
///    一次 [`TerrainKind`] 相等比较。
/// 2. **疏密**：`fold(H("presence"), 1000) < ` [`TREE_DENSITY_PERMILLE`]。
/// 3. **树种**：按 [`species_weights`] 在这一格的气候带上加权抽一个。
///
/// # O(1) 的证据
///
/// 结构上：两次定长哈希（各 24 字节输入）+ 两次乘法折算 + 一次地形比较 +
/// 一段最多三次迭代的定长权重扫描。**没有随世界规模增长的任何东西，没有分配。**
///
/// 判据上：`crates/ll-world/tests/tree_derivation.rs` 的
/// `派生层里没有循环也没有随机流` 读本模块源码，禁止出现 `while` 与 `DetRng`。
/// **这条判据的局限如实登记**：它证的是「没有回潮」，不是渐近复杂度本身
/// ——真正的 O(1) 论证是上一段那个结构性论证。
///
/// # 环面：一行取模都没有
///
/// 入参 `pos` 的类型是 [`TorusPos`]，字段私有、只能经
/// `ll_core::torus::TorusSize::wrap` 构造，不变式是「坐标恒被规范化到
/// `[0, width) × [0, height)`」。**绕回这件事在类型边界上就已经做完了**：
/// 环面上同一个物理格子，无论从哪个方向走到它、坐标算出来是 `-1` 还是
/// `width - 1`，`TorusPos` 都是同一个值，因此派生结果必然相同。本函数因此
/// 不需要、也不允许自己写 `%` 或 `rem_euclid`（与仓库那条「禁止手写欧氏距离」
/// 的门禁同一条精神）。
///
/// `world_height` 与 `band_width` 只喂给 `climate::band_at`——那个函数**刻意**
/// 接受未环绕的原始 `y`（见它自己的文档），本函数传进去的 `pos.y()` 已经规范化，
/// 恒落在合法域内。
pub fn derived_tree_at(
    seed: u64,
    pos: TorusPos,
    kind: TerrainKind,
    forest: TerrainKind,
    world_height: u32,
    band_width: i32,
) -> Option<TreeSpecies> {
    if kind != forest {
        return None;
    }
    if fold(position_digest(seed, pos, DOMAIN_PRESENCE), PERMILLE_TOTAL) >= TREE_DENSITY_PERMILLE {
        return None;
    }
    Some(derived_species_at(seed, pos, world_height, band_width))
}

/// 这一格的**气候**会长出什么树——与「这一格有没有树」是两个独立的问题。
///
/// 培植（`Intent::TendTree` 的 `Plant` 那一支）走的正是这个函数：**种下去
/// 长出什么由那块地的气候决定，不由种子决定**。这与「分布由气候决定」是
/// 同一条规则的两次应用，不是两套逻辑——所以这里是一个函数，不是两个。
pub fn derived_species_at(
    seed: u64,
    pos: TorusPos,
    world_height: u32,
    band_width: i32,
) -> TreeSpecies {
    let weights = species_weights(band_at(pos.y(), world_height, band_width));
    let mut roll = fold(
        position_digest(seed, pos, DOMAIN_SPECIES),
        BAND_WEIGHT_TOTAL,
    );
    for (index, weight) in weights.iter().enumerate() {
        if roll < *weight {
            return TreeSpecies::ALL[index];
        }
        roll -= *weight;
    }
    // 到不了这里：权重和恒为 BAND_WEIGHT_TOTAL，而 `fold` 的值域是
    // `[0, BAND_WEIGHT_TOTAL)`。写成显式回落而不是 `unreachable!`：树种是
    // 渲染与玩法都要用的东西，宁可退回一种树，也不该因为一次算术意外把整局
    // 游戏打断。这条回落由 `权重表的和恒等于折算基数` 那条断言从另一侧堵住。
    TreeSpecies::ALL[0]
}

/// **偏差层**的一条记录：这一格被玩家动过之后现在是什么样。
///
/// # 为什么 `species` 是 `Option` 而不是另开一个 `Felled` 变体
///
/// 「这一格现在站着什么」是**一个**问题，答案要么是某种树、要么是没有。
/// 拆成两个字段（`felled: bool` + `species: TreeSpecies`）会造出
/// `felled: true, species: Oak` 这种无意义组合，然后每个消费点都要决定
/// 「这种组合算什么」——那正是本仓库反复付过代价的形状。
///
/// # 一格一条记录，砍/种/采共用
///
/// 一格被反复动过（砍掉、又种回来、再采果）**不会累积多条记录**，永远只有
/// 最新的那一条。这是 [ADR 0009]「适用条件」第 3 条（偏移必须可以有界存储）
/// 在树上的落点。
///
/// [ADR 0009]: ../../../knowledge/decisions/0009-derive-by-default-store-only-deviation.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeDeviation {
    /// 现在这一格站着什么树；`None` = **被砍掉了**（派生说有、现在没有）。
    pub species: Option<TreeSpecies>,
    /// 最近一次采果的世界时刻；`None` = 从未采过（果子按派生挂着）。
    pub harvested_at: Option<Tick>,
}

impl TreeDeviation {
    /// 一棵刚被砍掉的树。
    pub fn felled() -> Self {
        TreeDeviation {
            species: None,
            harvested_at: None,
        }
    }

    /// 一棵刚被种下的树（果子还没长）。
    ///
    /// `harvested_at` 取种下的那一刻而不是 `None`：新种的树立刻就能采果会让
    /// 「种一棵、采一次、砍掉、再种」变成一条零成本的刷种子路径。
    pub fn planted(species: TreeSpecies, at: Tick) -> Self {
        TreeDeviation {
            species: Some(species),
            harvested_at: Some(at),
        }
    }
}

/// 全世界被动过的那些格子。**这就是「只存偏差」里的「偏差」。**
///
/// # 想给一棵树设属性，就得给它写一条记录；成片不行
///
/// 见本模块文档「项目所有者知情并接受的代价」一节。一百万棵派生出来的树在
/// 这张表里**一条记录都没有**；只有玩家真正动过的那些格子才占一条。
///
/// # 为什么是 `BTreeMap` 不是 `HashMap`（约束 C5）
///
/// 这张表要参与 [`WorldState::hash`](crate::state::WorldState::hash) 的遍历。
/// 哈希容器的桶序在不同运行、不同平台之间不保证一致，会让**同一逻辑状态产出
/// 不同摘要**——而这一类分叉往往要等到几百 tick 之后才暴露成「黄金基准悄悄
/// 对不上」，没有一个明确的第一次出错点可供定位（约束 C5 文档原话）。
/// [`TorusPos`] 已派生 `Ord`（`crates/ll-core/src/torus.rs`），直接可用。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeDeviations {
    by_pos: BTreeMap<TorusPos, TreeDeviation>,
}

impl TreeDeviations {
    /// 一张空表——**世界生成之后的正常状态**：一棵树都没被动过。
    pub fn new() -> Self {
        TreeDeviations::default()
    }

    /// 这一格有没有偏差记录。
    pub fn get(&self, pos: TorusPos) -> Option<TreeDeviation> {
        self.by_pos.get(&pos).copied()
    }

    /// 写入/覆盖这一格的偏差记录。
    ///
    /// **只应由 `ll_sim::apply` 调用**（约束 C1：`apply` 是全局唯一写入口）。
    pub fn set(&mut self, pos: TorusPos, deviation: TreeDeviation) {
        self.by_pos.insert(pos, deviation);
    }

    /// 有多少格被动过。
    pub fn len(&self) -> usize {
        self.by_pos.len()
    }

    /// 一格都没被动过。
    pub fn is_empty(&self) -> bool {
        self.by_pos.is_empty()
    }

    /// 按 [`TorusPos`] 的自然顺序遍历（约束 C5：不涉及任何哈希容器的桶序）。
    pub fn iter(&self) -> impl Iterator<Item = (TorusPos, TreeDeviation)> + '_ {
        self.by_pos.iter().map(|(pos, dev)| (*pos, *dev))
    }

    /// 把这张表混进世界状态摘要。
    ///
    /// 编码写在这里而不是 `state.rs`：那个文件已经在行数棘轮快照里，
    /// 与 `FactionTable::write_hash` 同一条先例（「这个文件已经 3700+ 行，
    /// 新代码不再往里堆」）。
    ///
    /// **空表也写一个长度 0**：否则「没有任何偏差」与「偏差恰好编码成空」
    /// 在摘要上不可区分。
    pub(crate) fn write_hash(&self, hasher: &mut StateHasher) {
        hasher.write_u64(self.by_pos.len() as u64);
        for (pos, deviation) in &self.by_pos {
            hasher.write_i64(i64::from(pos.x()));
            hasher.write_i64(i64::from(pos.y()));
            // `None` 也写一个判别值，否则「被砍掉了」与「站着判别值恰好
            // 编码成 0 的那种树」不可区分——走 `write_optional_world_id`
            // 那条既有纪律的同一个形状。
            match deviation.species {
                None => hasher.write_u64(0),
                Some(species) => hasher.write_u64(species.hash_tag()),
            }
            match deviation.harvested_at {
                None => hasher.write_u64(0),
                Some(at) => {
                    hasher.write_u64(1);
                    hasher.write_i64(at.0);
                }
            }
        }
    }
}

/// 一棵**现在真的站在那儿**的树——[`derived_tree_at`] 与
/// [`TreeDeviations`] 合流之后的答案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tree {
    /// 是什么树。
    pub species: TreeSpecies,
    /// 果子长好了没有——`false` 时 `Harvest` 那一支的闸门不过。
    pub fruit_ready: bool,
}

/// **解析层**：这一格现在有没有树、是什么树、果子长好了没有。
///
/// **偏差覆盖派生**，不是反过来：查到偏差记录就用它（`species: None` ⇒
/// 这里没有树，**派生说有也没有**），查不到才现算。
///
/// 次序反过来会让「砍掉的树下一帧又长回来」——那是这整套架构最容易出、
/// 也最难在测试里发现的一种错（渲染看起来正常，只是树不会消失）。
/// `crates/ll-world/tests/tree_deviation.rs` 的
/// `砍掉的树存读一轮之后仍然不在` 钉住它。
///
/// # 为什么是自由函数而不是 `WorldState` 的方法
///
/// `crates/ll-world/src/state.rs` 在行数棘轮快照里（本批只往它加 5 行：
/// 一个字段、一个 repr 字段、一行搬运、一行初始化、一行哈希混入）。
/// 树的逻辑住在本模块，`state.rs` 只持有那张表。
///
/// `forest` 由调用方给：`WorldState` 不持有 `BaseTerrainIds`
/// （`ContentIndex` 依赖注册表加载顺序，不可持久化——与 `terrain_table`
/// 同一类既有限制，见该字段文档）。
pub fn tree_at(world: &WorldState, pos: TorusPos, forest: TerrainKind) -> Option<Tree> {
    let species = match world.trees.get(pos) {
        // 偏差覆盖派生——**这一行的次序是本函数的全部要点**。
        Some(deviation) => {
            let species = deviation.species?;
            return Some(Tree {
                species,
                fruit_ready: fruit_ready(deviation.harvested_at, world.clock),
            });
        }
        None => derived_tree_at(
            world.seed,
            pos,
            world.terrain_at(pos)?,
            forest,
            world.size.height(),
            world.terrain_shape.climate_band_width,
        )?,
    };
    Some(Tree {
        species,
        // 从没被动过的树恒有果——「果子」在派生层不存在（它是时间的函数，
        // 而时间不是派生层的输入）。
        fruit_ready: true,
    })
}

/// 果子长好了没有。
///
/// `None`（从未采过）恒为真；采过的要等 [`FRUIT_REGROW_TICKS`]。
/// 用 `saturating_add` 而不是 `+`：`Tick` 是 `i64`，一个被构造出来的极端
/// `harvested_at` 不该让世界查询 panic。
fn fruit_ready(harvested_at: Option<Tick>, now: Tick) -> bool {
    match harvested_at {
        None => true,
        Some(at) => now.0 >= at.0.saturating_add(FRUIT_REGROW_TICKS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 每条气候带的权重和都等于折算基数() {
        // 这条守的是 `derived_species_at` 末尾那条回落分支：权重和一旦不等于
        // 基数，加权抽取就会漏到回落上，全图退化成 `TreeSpecies::ALL[0]`
        // ——而那是一个**看起来正常**的画面（满山橡树），没有任何东西会报错。
        for band in [ClimateBand::Hot, ClimateBand::Temperate, ClimateBand::Polar] {
            let total: u32 = species_weights(band).iter().sum();
            assert_eq!(
                total, BAND_WEIGHT_TOTAL,
                "{band:?} 的权重和是 {total}，不等于折算基数 {BAND_WEIGHT_TOTAL}"
            );
        }
    }

    #[test]
    fn 权重表每一行的长度都等于树种数() {
        // 加第四种树时这条当场红：权重表是定长数组，加变体不会自动加权重。
        for band in [ClimateBand::Hot, ClimateBand::Temperate, ClimateBand::Polar] {
            assert_eq!(
                species_weights(band).len(),
                TreeSpecies::ALL.len(),
                "{band:?} 的权重条数与 TreeSpecies::ALL 对不上"
            );
        }
    }

    #[test]
    fn 果子采过之后要等满一个周期才重新长好() {
        let 采于 = Tick(1000);
        assert!(!fruit_ready(Some(采于), Tick(1000)));
        assert!(!fruit_ready(
            Some(采于),
            Tick(1000 + FRUIT_REGROW_TICKS - 1)
        ));
        assert!(fruit_ready(Some(采于), Tick(1000 + FRUIT_REGROW_TICKS)));
        assert!(fruit_ready(None, Tick(0)), "从未采过的树恒有果");
    }

    #[test]
    fn 偏差表的哈希对树种与采摘时刻都敏感() {
        // ADR 0022：判据漏了字段，测试就是在空跑。这条逐字段确认
        // `write_hash` 真的看得见 `TreeDeviation` 的两个字段。
        let pos = ll_core::torus::TorusSize::new(64, 64).unwrap().wrap(3, 4);
        let digest = |dev: TreeDeviation| {
            let mut table = TreeDeviations::new();
            table.set(pos, dev);
            let mut hasher = StateHasher::new();
            table.write_hash(&mut hasher);
            hasher.finish()
        };
        let 橡树 = digest(TreeDeviation {
            species: Some(TreeSpecies::Oak),
            harvested_at: None,
        });
        let 松树 = digest(TreeDeviation {
            species: Some(TreeSpecies::Pine),
            harvested_at: None,
        });
        let 砍掉 = digest(TreeDeviation::felled());
        let 采过 = digest(TreeDeviation {
            species: Some(TreeSpecies::Oak),
            harvested_at: Some(Tick(7)),
        });
        assert_ne!(橡树, 松树, "换一个树种，摘要必须变");
        assert_ne!(橡树, 砍掉, "砍掉与站着，摘要必须变");
        assert_ne!(橡树, 采过, "采过与没采过，摘要必须变");
        assert_ne!(空表(), 橡树, "空表与有一条记录，摘要必须变");
    }

    fn 空表() -> u64 {
        let mut hasher = StateHasher::new();
        TreeDeviations::new().write_hash(&mut hasher);
        hasher.finish()
    }
}
