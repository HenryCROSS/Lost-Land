//! 与 GPU 无关的纯计算：地形 → 图集条目名映射、光照 → 视野半径换算。
//!
//! 拆成独立文件、脱离窗口/GPU 也能被 `cargo test --workspace` 覆盖的
//! 理由，与 `ll-sim` 的 `p5_coordinate_acceptance::layout` 一致，见其
//! 模块文档。本文件的 [`terrain_entry_name`]/[`effective_sight_radius`]/
//! [`effective_tint`] 是同一套换算的独立实现（不是同一份代码的引用
//! ——`p5_coordinate_acceptance` 是 `ll-sim` 的一个 `examples/` 目录，
//! 不是可供下游 crate 依赖的库 API，见 Cargo 对 `examples/` 的可见性
//! 规则），保持逻辑一致但物理上各自独立。
//!
//! 「逻辑一致」曾经短暂地不成立：据点建筑地形补图那一批只改了本文件的
//! [`terrain_entry_name`]，p5 那份仍按更早的借用关系画石地板/石墙，
//! 两张表就此分叉。项目所有者的裁定是统一（原话「第三条的话先统一了
//! 吧，避免以后有什么问题」），p5 那张表已经改回与本文件同一张贴图。
//! 恢复的方向是**把 p5 对齐到本文件**而不是反过来：本文件是生产渲染
//! 路径，p5 是验收 demo。守住这条一致性的是 p5 自己的
//! `十种地形两两不共用同一个图集条目`——任何一支改回借用它立刻变红。

use ll_core::hashing::StateHasher;
use ll_core::time::Tick;
use ll_core::torus::TorusPos;
use ll_mod::registry::Registry;
use ll_world::light::sight_radius_under_weather;
use ll_world::space_profile::{SpaceProfile, effective_ambient_light, effective_weather};
use ll_world::terrain::{BaseTerrainIds, TerrainKind};
use ll_world::weather::Weather;

/// 地表视野基准半径（格），随光照缩放。
pub const BASE_SIGHT_RADIUS: u32 = 12;

/// 把地形种类映射到图集条目名——覆盖本体 `define_base` 注册的~~**全部
/// 17 种**~~**全部 19 种**地形，一种不漏。
///
/// 〔2026-08-31 批次 28 原地更正（纪律第 9 条）：数字是 19 不是 17。
/// 气候条带批次给 `BaseTerrainIds` 加了 `desert`/`tundra`，这张表当时
/// **跟着加了两支**、句子里的数字没跟上。原文划掉保留只为追溯，表本身
/// 一直是对的。更正方：
/// `docs/superpowers/plans/2026-08-31-batch28-terrain-art.md` 五节第 3 条。〕
///
/// 返回值带 `lostland:` 前缀：图集条目名统一用完整命名空间字符串（见
/// `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 文档），这张表本身
/// 是硬编码字面量，不经过 [`Registry`]，因此前缀直接写死在表里，而不是
/// 运行期拼接。
///
/// 这张表的本地部分与地形在 [`Registry`] 里注册的内容 ID 本地部分并不
/// 相同（例如 `ids.grass` 对应的注册 ID 是 `lostland:grass`，这里查出
/// 的图集条目名却是 `lostland:terrain_grass`）——图集条目名描述的是
/// 「贴图长什么样」，注册 ID 描述的是「这是哪种地形」，两者是两套独立
/// 的字符串空间，只是恰好共享同一个本体命名空间前缀。
///
/// # 「一种不漏」这条为什么要单独写出来
///
/// 这张表此前只覆盖 10 种：8 种自然地形，加上 `floor_stone`/`wall_stone`
/// **借用** `terrain_dirt`/`terrain_mountain` 两张自然地形图。剩下 7 种
/// 建筑地形（`floor_wood`/`wall_wood`/`door_closed`/`door_open`/
/// `window`/`stairs_up`/`stairs_down`）在这里返回 `None`，落到
/// [`terrain_atlas_key`] 的 [`Registry`] 回退路径上，拿注册 ID
/// （`lostland:wall_wood`）当图集键去查——那条回退路径本来是给 mod 地形
/// 用的，本体地形的注册 ID 与图集条目名根本不是同一个字符串空间（见上
/// 一段），必然查不到。后果是玩家一走进据点，每帧每格刷一条「图集条目
/// 缺失，跳过本次绘制」的 ERROR，据点/建筑/室内一格都画不出来。
///
/// 守住「一种不漏」的是本文件的
/// `全部十九种本体地形都能查到图集条目` 与
/// `crates/ll-game/tests/atlas_coverage.rs`：前者钉这张表返回 `Some`，
/// 后者钉那个字符串在真实图集里查得到、且对应矩形里真的有像素。此前
/// 只有一条覆盖 8 种自然地形的测试，缺口正落在它的盲区里。
///
/// # 两处「借用」已经解除
///
/// `floor_stone`/`wall_stone` 现在各有专属贴图
/// （`terrain_floor_stone`/`terrain_wall_stone`），不再借用泥土/山体。
/// 理由是木质建筑地形一并有了图之后，暖褐的木地板会和同样暖褐的
/// `terrain_dirt` 糊在一起——所有者的验收方式是「走进据点看一眼」，
/// 木/石地板必须一眼可分。这是本批次的判断，不是所有者原话。
///
/// 当年 `p5_coordinate_acceptance` 里的同一处借用也一并解除过，理由见
/// 本模块文档开头「逻辑一致曾经短暂地不成立」一段（那个 demo 已于
/// 2026-08-29 随所有者裁定删除，见 ADR 0030）。
///
/// `terrain_dirt` 本身**没有**因此变成孤儿图，但**它的消费者已经从两处
/// 降到一处**：`crates/ll-game/src/content.rs` 的 mod 资产覆盖验收拿它
/// 当被覆盖的目标。另一处（`crates/ll-render/examples/p1_acceptance`
/// 拿它铺棋盘格）随上述裁定一起没了。剩下那一处**不是**借用——那里的
/// `terrain_dirt` 就是泥土本身，与「拿泥土冒充石地板」是两回事。
pub fn terrain_entry_name(kind: TerrainKind, ids: &BaseTerrainIds) -> Option<&'static str> {
    if kind == ids.deep_water {
        Some("lostland:terrain_deep_water")
    } else if kind == ids.shallow_water {
        Some("lostland:terrain_shallow_water")
    } else if kind == ids.sand {
        Some("lostland:terrain_sand")
    } else if kind == ids.grass {
        Some("lostland:terrain_grass")
    } else if kind == ids.forest {
        Some("lostland:terrain_forest")
    } else if kind == ids.hill {
        Some("lostland:terrain_hill")
    } else if kind == ids.mountain {
        Some("lostland:terrain_mountain")
    } else if kind == ids.snow {
        Some("lostland:terrain_snow")
    } else if kind == ids.desert {
        Some("lostland:terrain_desert")
    } else if kind == ids.tundra {
        Some("lostland:terrain_tundra")
    } else if kind == ids.floor_wood {
        Some("lostland:terrain_floor_wood")
    } else if kind == ids.floor_stone {
        Some("lostland:terrain_floor_stone")
    } else if kind == ids.wall_wood {
        Some("lostland:terrain_wall_wood")
    } else if kind == ids.wall_stone {
        Some("lostland:terrain_wall_stone")
    } else if kind == ids.door_closed {
        Some("lostland:terrain_door_closed")
    } else if kind == ids.door_open {
        Some("lostland:terrain_door_open")
    } else if kind == ids.window {
        Some("lostland:terrain_window")
    } else if kind == ids.stairs_up {
        Some("lostland:terrain_stairs_up")
    } else if kind == ids.stairs_down {
        Some("lostland:terrain_stairs_down")
    } else {
        None
    }
}

/// 把地形种类映射到图集条目名，覆盖本体注册的自然地形**与** mod 注册
/// 的自定义地形（例如 `mods/example_mod` 的 `examplemod:lava_floor`）。
///
/// # 为什么需要这个回退，而不是只用 [`terrain_entry_name`]
///
/// [`terrain_entry_name`] 是一张写死的静态映射表，只认识本体注册的
/// 那几种基础地形——mod 通过 `register-terrain` 注册的新地形种类，
/// 这张表天然查不到（它压根不知道这些地形的存在），此前 mod 自定义
/// 地形因此永远画不出来，只能靠 [`tile_tint`] 之外没有任何降级路径，
/// 直接在 [`terrain_entry_name`] 返回 `None` 时被跳过——这正是「mod
/// 能注册一把剑，却给不了它一张图」这条真实瓶颈在地形渲染上的具体
/// 体现。
///
/// 回退路径反查 [`Registry::resolve`] 拿到这个地形种类的完整命名空间
/// ID（例如 `"examplemod:lava_floor"`），直接把这个字符串当图集查找
/// 键——`ll_mod::asset_vfs::ResolvedSprite::atlas_name` 对任意命名空间
/// 的精灵，图集条目名恒定就是这个完整 ID 字符串（本体与 mod 统一，见
/// 其文档），两边约定完全对齐，不需要额外的映射表。
///
/// 这条回退路径只对 mod 注册的地形成立——本体注册的自然地形已经被
/// [`terrain_entry_name`] 挡在前面提前返回，走不到这里；[`Registry`]
/// 里本体地形的注册 ID（本地部分是 `grass`/`mountain` 这类简称）与图集
/// 条目名（本地部分是 `terrain_grass`/`terrain_mountain`）本就不是同一
/// 个字符串，`registry.resolve` 直接查也查不出正确的图集键——这正是
/// [`terrain_entry_name`] 这张表不能被这条回退路径整个取代的原因。
///
/// 与 GPU 无关的纯函数：[`Registry`] 是普通数据，不需要真实图集就能
/// 单测覆盖「查到了哪个字符串」这层逻辑；「这个字符串在图集里查不查
/// 得到条目」是下一步 `GpuResources::resolve_key` 的职责，不在本函数范围。
///
/// # `pos` 是干什么的（批次 28 加的第四个入参）
///
/// 同一种地形按**位置**取多张贴图——所有者报的现象是地表「看起来太
/// 单调」，一整片草原铺同一张 16×16 图。哪一格取第几张由
/// [`terrain_variant_at`] 现算，**是纯函数、不进世界状态、不消耗任何
/// 随机流**，理由见那个函数的文档。
///
/// 每种地形有几张由 [`terrain_variant_count`] 声明；今天只有草地/森林/
/// 沙地各有多张，其余 16 种恒 1 张——对它们而言本函数与加 `pos` 之前
/// **逐字符相同**（变体 0 的条目名恒等于 [`terrain_entry_name`] 的返回值）。
pub fn terrain_atlas_key(
    kind: TerrainKind,
    ids: &BaseTerrainIds,
    registry: &Registry,
    pos: TorusPos,
) -> Option<String> {
    terrain_atlas_key_for_variant(kind, ids, registry, terrain_variant_at(kind, ids, pos))
}

/// 变体贴图的条目名后缀，接在基准条目名与**从 1 开始**的变体号之间
/// （`lostland:terrain_grass` → `lostland:terrain_grass_alt1`）。
///
/// 变体 0 **不带后缀**，条目名恒等于 [`terrain_entry_name`] 的返回值。
/// 这不是省事，是两条实在的收益：既有那批地形 PNG 一个字节都不用重画；
/// 把 [`terrain_variant_count`] 全改回 `1`，行为就**精确**回到多变体
/// 落地之前（批次 28 计划三节「最可反转」一条）。
const TERRAIN_VARIANT_SUFFIX: &str = "_alt";

/// 某种地形有几张贴图可供按位置挑选。恒 `>= 1`。
///
/// # 为什么这张表在引擎侧，不在内容侧也不在资产侧
///
/// 三条路的完整代价对照见
/// `docs/superpowers/plans/2026-08-31-batch28-terrain-art.md` 第三节，
/// 这里只留结论与那两条被否掉的路各自的**致命伤**：
///
/// - **内容侧声明**（地形定义里加一个 `variants` 字段）：mod 作者能自己
///   声明变体数是真收益，但今天没有任何 mod 需要它；代价是内容 schema
///   变了 ⇒ `ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION` 要升
///   ⇒ 三条黄金基准里的「有人世界」那条跟着红。**留给「mod 真的要自带
///   多变体地形」那天再走**，那时这段文档就是落点。
/// - **资产侧按文件名发现**（扫 `assets/sprites/` 数有几张 `_alt*.png`）：
///   本函数是每帧上千次的纯函数，手上没有图集也没有资产 VFS；更要命的是
///   它**把「漏了一张图」从错误变成了正常**——少一张 alt 就自动少一个
///   变体，全绿。那正是 ADR 0022 点名的「覆盖退化」形状。
///
/// 因此变体数写在这里，与 [`terrain_entry_name`] 那张硬编码静态表并排、
/// 同一个形状（一串 `if`，不经任何哈希容器，符合约束 C5）。
///
/// **声明侧与资产侧是两份清单，会漂。** 漂了当场红，两个方向都有锁：
/// `crates/ll-game/tests/atlas_coverage.rs` 的
/// `每一种本体地形的每一张变体在真实图集里都查得到条目`（声明多了、图没画）
/// 与 `图集里不许有声明侧数不出来的变体贴图`（图画了、声明没加）。
///
/// # 为什么只有三种地形有变体
///
/// 所有者报的现象是地表「看起来太单调」，而铺得最满的就是草地与森林，
/// 海岸沙地紧随其后。建筑地形（墙/门/窗/楼梯）刻意**不做**：它们靠
/// 结构图案（门板、窗棂、砖缝）表达自己是什么，变体很容易跌到「看起来
/// 是另一种地形」，需要单独论证，不在本批范围。
///
/// 三种地形的变体数**刻意不一样**（3/3/2），把「每种地形变体数可以不同」
/// 这条真的走一遍，而不是留成一条没人验过的纸面能力。
///
/// mod 注册的地形恒返回 `1`——它们走 [`terrain_atlas_key`] 的
/// [`Registry`] 回退路径，那条路径上「图集条目名 = 完整命名空间 ID」
/// 是一对一的约定，没有变体这个概念。
pub fn terrain_variant_count(kind: TerrainKind, ids: &BaseTerrainIds) -> u32 {
    // 草地与森林写成**一支**只是因为 clippy 的 `if_same_then_else` 不接受
    // 两支返回同一个数（本批实测撞到过）——它们的张数是**各自**定的，不是
    // 「必须相同」。哪天草地要第 4 张，把这一支拆回两支即可。
    if kind == ids.grass || kind == ids.forest {
        3
    } else if kind == ids.sand {
        2
    } else {
        1
    }
}

/// 这一格该用第几张变体贴图——**渲染期现算的纯函数**，输入只有
/// 「哪种地形」与「哪一格」。
///
/// # 不进世界状态（这是本函数最要紧的一条性质）
///
/// 返回值**不写回任何结构体**：不进 `ll_world::state::WorldState`、不进
/// `WorldState::hash()`、不进存档。它每帧被重新算出来、用完就扔。因此
/// 加变体这件事结构上不可能动到三条黄金基准（世界摘要 / 回放摘要 /
/// 有人世界摘要）——批次 28 实测确认过这一点，见其计划文档十节。
///
/// # 为什么不用 `DetRng`
///
/// `ll_core::rng::DetRng` 是**世界状态**的随机源（约束 C3：一切随机来自
/// `hash(种子, 实体 id, 事件计数)`）。在渲染层向它取数有两处硬伤：
///
/// 1. 地形瓦片每帧铺满整屏，取数次数取决于**这一帧画了几格**——摄像机
///    缩放、窗口大小、视野半径都会改变它。让这种量参与随机流，等于把
///    确定性重放交给显示器分辨率。
/// 2. 语义上也不对：变体号不是「世界里发生的事」，是「这一格长什么样」，
///    它压根不该出现在事件流里。
///
/// 因此这里走**位置哈希**：`ll_core::hashing::StateHasher`（FNV-1a）。
/// 选它而不是自己再写一个，是因为它的模块文档已经把这里需要的性质写死了
/// ——「完全由整数运算构成、由规范唯一确定，因而跨平台跨版本恒定」。
///
/// # 三个输入各自为什么在里面
///
/// - **`pos.x()` / `pos.y()`**：位置本身。
/// - **条目名**：不混它的话，`grass` 与 `forest` 在同一格会算出同一个
///   变体号，两种地形的图案在交界处对齐，读起来又是一种规则感——正是
///   本批要消除的那种单调。用 `write_len_prefixed_bytes` 而不是裸字节，
///   理由是该方法自己文档里那条碰撞论证。
///
/// # 混入次序为什么是位置在前、名字在后
///
/// **这条是实测出来的，不是设计出来的。** FNV-1a 逐字节 `异或 + 乘质数`，
/// 最后混进去的那几个字节没有足够的轮数被摊开；而 `write_i64` 写的是
/// 小端 8 字节，世界坐标那种小整数**高 7 字节全是 0**——也就是说相邻两格
/// 只差最低那一个字节，之后只剩 7 轮「异或 0 再乘」。
///
/// 后果是**最后混进去的那个维度，相邻格子会明显倾向取到同一张图**。
/// 128×128 格实测（草地，3 张变体，理想值 33.3%）：
///
/// | 混入次序 | 横向相邻同变体 | 纵向相邻同变体 | 斜向相邻同变体 |
/// |---|---|---|---|
/// | 名字, x, y（y 在最后） | 30.2% | **47.3%** | 31.4% |
/// | 名字, y, x（x 在最后） | **47.3%** | 30.2% | 31.4% |
/// | **x, y, 名字（本函数）** | **33.0%** | **32.4%** | **30.5%** |
///
/// 47% 意味着纵向平均每两格才换一次图——画面上就是一条条竖纹，
/// 把「太单调」换成了另一种规则感。条目名有 20 个以上字节，放在最后
/// 恰好给位置那 16 个字节补足了混合轮数。
///
/// 守住这条的是本文件的 `相邻格子取到同一变体的比例接近相互独立`：
/// 把这三行的次序换回去，它当场红。
///
/// # 环面：一行取模都没有
///
/// 入参类型是 [`TorusPos`]，它的字段私有、只能经
/// `ll_core::torus::TorusSize::wrap` 构造，不变式是「坐标恒被规范化到
/// `[0, width) × [0, height)`」。也就是说**绕回这件事在类型边界上就已经
/// 做完了**：环面上同一个物理格子，无论从哪个方向走到它、坐标算出来
/// 是 `-1` 还是 `width - 1`，`TorusPos` 都是同一个值，因此变体号必然相同。
/// 本函数因此不需要、也不允许自己写 `%` 或 `rem_euclid`（与仓库那条
/// 「禁止手写欧氏距离」的门禁同一条精神）。
///
/// # 折算为什么取高位而不是取余
///
/// FNV-1a 的**低位雪崩弱**：相邻格子的摘要低几位相关性明显，直接
/// `% 3` 会在规则网格上留下肉眼可见的条纹——那是把一种单调换成另一种。
/// 这里用乘法取高位（Lemire 折算），与 `ll_core::rng::DetRng::gen_range`
/// 逐字同一条写法，没有理由在这里另发明一个。
pub fn terrain_variant_at(kind: TerrainKind, ids: &BaseTerrainIds, pos: TorusPos) -> u32 {
    let count = terrain_variant_count(kind, ids);
    if count <= 1 {
        return 0;
    }
    let Some(bare) = terrain_entry_name(kind, ids) else {
        // mod 地形走 Registry 回退路径，那条路径上没有变体这个概念。
        // 理论上到不了这里（`terrain_variant_count` 只对本体地形返回
        // 大于 1 的值），写成显式的 0 而不是 `expect`：渲染层宁可退回
        // 单张贴图，也不该因为一个美术分支把整局游戏打断。
        return 0;
    };
    let mut hasher = StateHasher::new();
    // **次序不是随手写的**：位置在前、条目名在后。理由见本函数文档
    // 「混入次序为什么是位置在前、名字在后」一节——反过来写会在画面上
    // 留下肉眼可见的纵向条纹，实测数字在那一节里。
    hasher.write_i64(i64::from(pos.x()));
    hasher.write_i64(i64::from(pos.y()));
    hasher.write_len_prefixed_bytes(bare.as_bytes());
    let digest = hasher.finish();
    // Lemire 乘法折算：取高 64 位，见本函数文档「折算为什么取高位」。
    ((u128::from(digest) * u128::from(count)) >> 64) as u32
}

/// 指定变体号时的图集条目名——**门禁按变体逐张枚举时用的那一条**，
/// 不在渲染热路径上。
///
/// 渲染路径走 [`terrain_atlas_key`]（它自己按位置算变体号），一格只
/// 分配一个 `String`。本函数刻意**不**返回 `Vec<String>`：地形瓦片是
/// 全仓库最热的一处循环，给它一个每格都要分配一个 `Vec` 的 API，
/// 迟早有人在渲染路径上用它。
///
/// `variant` 超出 [`terrain_variant_count`] 时返回 `None`——静默截断成
/// 合法值会让「门禁多数了一张」变成「门禁重复验了同一张」，正是 ADR 0022
/// 那种「测试全绿但保护不存在」的形状。
pub fn terrain_atlas_key_for_variant(
    kind: TerrainKind,
    ids: &BaseTerrainIds,
    registry: &Registry,
    variant: u32,
) -> Option<String> {
    if variant >= terrain_variant_count(kind, ids) {
        return None;
    }
    if let Some(bare) = terrain_entry_name(kind, ids) {
        return Some(if variant == 0 {
            bare.to_string()
        } else {
            format!("{bare}{TERRAIN_VARIANT_SUFFIX}{variant}")
        });
    }
    registry.resolve(kind.index()).map(|id| id.to_string())
}

/// 一个图集条目名是不是某种地形的**变体**（带 [`TERRAIN_VARIANT_SUFFIX`]
/// 后缀 + 十进制变体号）。
///
/// 供 `crates/ll-game/tests/atlas_coverage.rs` 的反向锁使用：扫真实图集里
/// 全部满足本判据的条目，逐个要求它出现在声明侧那份清单里。判据放在生产
/// 代码这一侧而不是测试里另抄一份正则，理由与该文件模块文档反复写的
/// 那条一样——**凡是把真相源之外的副本当判据，迟早分叉，而分叉时没有
/// 任何东西会报错**。
pub fn is_terrain_variant_entry(name: &str) -> bool {
    let Some((_, tail)) = name.rsplit_once(TERRAIN_VARIANT_SUFFIX) else {
        return false;
    };
    !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit())
}

/// 给定空间在某一世界时刻、某种天气下的环境光换算出的视野半径。
///
/// 不叠加任何种族暗视（夜间下限恒取未声明时的默认值）——本函数留给不知道「谁在看」的调用方（例如本
/// 文件与 `ll-sim` p5 验收 demo 里只关心「这个空间本身多亮」的测试）。
/// 真正的玩家渲染路径需要暗视时用 [`effective_sight_radius_for_race`]。
///
/// # 天气在这里进来两次，不是重复
///
/// 天气有两个独立的乘数（见 `ll_world::weather::WeatherDef::sight_scale`）：
/// `light_scale` 经 [`effective_ambient_light`] 折进光照，`sight_scale`
/// 由 [`sight_radius_under_weather`] 在光照换算**之后**单独再乘一次。
/// 两次都必须先过 [`effective_weather`]——那是「洞窟不受天气影响」这条
/// 判断的唯一真相源，`effective_ambient_light` 内部也走它，两个消费者
/// 因此不可能对同一个空间给出相反的结论。
pub fn effective_sight_radius(profile: &SpaceProfile, clock: Tick, weather: Weather) -> u32 {
    let light = effective_ambient_light(profile, clock, weather);
    sight_radius_under_weather(
        BASE_SIGHT_RADIUS,
        light,
        effective_weather(profile, weather),
        NO_DARKVISION,
    )
}

/// 「这个调用方不知道谁在看」时传给暗视参数的取值。
///
/// `0` 在 [`ll_world::light::sight_radius_at`] 里被解读成**未声明**
/// 暗视，落回 [`ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS`]——与
/// [`effective_sight_radius`] 长出这个参数之前的行为逐格相同。写成
/// 具名常量而不是散落的字面量 `0`，是为了让「这里传 0 是因为没有
/// 观察者」与「某个种族真的声明了 0」在读代码时不会混淆（后者不可能
/// 出现——0 恒被解读成未声明）。
const NO_DARKVISION: u32 = 0;

/// 给定空间在某一世界时刻的有效光照，叠加某个种族声明的**夜间视野
/// 格数下限**后换算出的视野半径——`race-system.md`「五、暗视」一节的
/// 渲染侧接线点。
///
/// # 暗视只改「看多远」，不改「看多清」
///
/// 项目所有者裁定暗视只买视野格数：本函数的返回值只喂给 FOV，画面
/// 亮度那一路（[`effective_tint`]）读的是环境光本身，与暗视无关——
/// 夜视好的种族在黑暗里看得**更远**，不是让整个世界对它变亮。这也是
/// 为什么本函数不再先算一个「有效光照」再交给半径换算：暗视根本不
/// 经过光照这个量。
///
/// # 为什么接在这一步，不是更早或更晚
///
/// 现有链路是 `season_light_scale → ambient_light →
/// effective_ambient_light → effective_sight_radius`——`ambient_light`/
/// `effective_ambient_light` 只描述「这个世界时刻、这个空间本身多亮」，
/// 与「谁在看」完全无关（同一个地下城任何种族站进去，`ambient_light_floor`
/// 都一样），暗视是**观察者的属性**，不该往上游这两步塞：往
/// `ambient_light` 塞会让同一个空间对所有种族都变亮（错——暗视应该是
/// 「这个种族看得见，其余种族看不见」，不是「这个地方变亮了」），
/// 往 `effective_ambient_light` 塞同理，且两者都定义在 `ll-world`，
/// 而 `darkvision_cells` 在下游 `ll-mod::race`（依赖方向不允许
/// `ll-world` 认识它）。唯一合适的落点是 `effective_ambient_light` 算
/// 出「这个空间这一刻本身多亮」**之后**、喂给视野半径换算
/// **之前**——[`ll_sim::vision::sight_radius_for_race`] 正是卡在这两步
/// 中间，见其模块文档「为什么定义在 `ll-sim`」一节。
///
/// # 依赖方向：`RaceDarkvisionSource` 由调用方传入，不是本函数去查
///
/// `ll-game` 依赖 `ll-mod`/`ll-sim`（见 `Cargo.toml`），可以直接认识
/// `ll_mod::race::RaceTable`，本可以在这里直接要一个 `&RaceTable`——
/// 但 [`ll_sim::vision::sight_radius_for_race`] 的签名是
/// `&dyn RaceDarkvisionSource`（依赖倒置接口，定义在 `ll-sim`），这里
/// 沿用同一个接口类型而不是收窄成具体的 `RaceTable`，理由与
/// `ll_game::world::build_player_agent` 调用
/// `ll_sim::character::bake_race_stat_modifiers` 时把
/// `&content.race_table` 当 `&dyn RaceStatModifierSource` 传入完全
/// 一致：调用方是唯一同时持有真实 `RaceTable` 与真实空间/时钟的地方，
/// 但真正做换算的函数不需要认识 `RaceTable` 这个具体类型，只需要认识
/// 接口。
pub fn effective_sight_radius_for_race(
    profile: &SpaceProfile,
    clock: Tick,
    weather: Weather,
    race: ll_core::ident::ContentIndex,
    darkvision: &dyn ll_sim::vision::RaceDarkvisionSource,
) -> u32 {
    let light = effective_ambient_light(profile, clock, weather);
    // 暗视是**夜间视野格数的下限**，天气的 sight_scale 是一个乘数——
    // `sight_radius_for_race` 内部把下限应用在乘数**之前和之后**各一
    // 次，因此雾雪削得掉光照换算出来的那部分视野，削不掉暗视这条底线
    // （`ll_world::light::sight_radius_under_weather` 文档「夜间下限在
    // 这里第二次应用」一节）。这一步不能拆成「先算半径、再乘天气」两
    // 句写在本函数里——那正是暗视会被恶劣天气吃掉的写法。
    ll_sim::vision::sight_radius_for_race(
        BASE_SIGHT_RADIUS,
        light,
        effective_weather(profile, weather),
        race,
        darkvision,
    )
}

/// 画面整体亮度的下限——再暗的夜晚也不会低于这个值。
///
/// 项目所有者的要求是「黑夜要有一个还算能看的亮度」。原先午夜的调制
/// 系数是 0.1，连当前视野内的格子都被压得几乎看不出地形。
///
/// 这条是**纯表现层**决策（ADR 0020 甲区：结果只变成像素），与视野半径
/// 的下限 [`ll_world::light::DEFAULT_NIGHT_SIGHT_RADIUS`] 分属两件事——一个管
/// 「看得清不清」，一个管「看得到多远」。前者可以自由用浮点、不进
/// `WorldState`、不参与 `hash()`；后者会经 FOV 影响探索记忆，是世界状态。
///
/// 取 0.4 而不是更高：夜晚仍要明显暗于白天（正午为 1.0），否则昼夜就
/// 只剩计时意义。已探索但当前无视野的格子还会再乘
/// [`EXPLORED_MEMORY_DIM_FACTOR`]，因此夜里的记忆层约为 0.14——能看出
/// 轮廓，但一眼能和当前视野区分开。
pub const MIN_VISIBLE_TINT: f32 = 0.4;

/// 画面整体亮度调制（灰阶），下限为 [`MIN_VISIBLE_TINT`]。
///
/// 天气只经 `light_scale` 影响这里——`sight_scale`（雾）**不**参与画面
/// 亮度：雾让人看不远，不让人看不清脚下这一格，把它折进色调会让雾变成
/// 「又暗又看不远」的第二种阴天，见 `ll_world::light::sight_radius_under_weather`
/// 文档「为什么是第二个乘数」一节。
pub fn effective_tint(profile: &SpaceProfile, clock: Tick, weather: Weather) -> [f32; 4] {
    let light = effective_ambient_light(profile, clock, weather)
        .0
        .clamp(0, 1000) as f32
        / 1000.0;
    let light = light.max(MIN_VISIBLE_TINT);
    [light, light, light, 1.0]
}

/// 已探索但当前无视野的格子（战争迷雾「记忆」层）在 [`effective_tint`]
/// 基础上再压暗的系数。
///
/// 只影响像素颜色，不进 [`ll_world::state::WorldState`]——世界状态禁止
/// 浮点（约束见 `ll_world::exploration` 模块文档「只存位图」一节：
/// `ExplorationMemory` 只记「看没看过」这一个 bit，暗化多少是纯表现层
/// 决策，不该反过来污染世界状态）。取值小于 1 让记忆层比当前视野暗、
/// 大于零让它比「从未探索」（完全不画、留黑）更亮——三层可见性
/// （项目所有者原话：「没有视野的地方就暗下来一些……没去过的地方就
/// 黑着」）因此不是三个离散色阶,而是「不画」与「按此系数压暗」两种
/// 处理叠加在同一套 `effective_tint` 光照调制之上。
const EXPLORED_MEMORY_DIM_FACTOR: f32 = 0.35;

/// 把当前光照色调换算成「已探索但当前无视野」格子应使用的记忆色调。
///
/// 见 [`EXPLORED_MEMORY_DIM_FACTOR`] 文档：只压暗 RGB，不动 alpha——
/// 记忆层格子仍需完全不透明地画出来，只是比当前视野内的格子暗。
pub fn memory_tint(tint: [f32; 4]) -> [f32; 4] {
    [
        tint[0] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[1] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[2] * EXPLORED_MEMORY_DIM_FACTOR,
        tint[3],
    ]
}

/// 三层可见性判定：给定一格「当前是否在玩家视野内」与「是否已被探索
/// 过」，返回这一帧该不该画这一格、画的话用哪种色调。
///
/// 从 [`crate::app`] 的 `render_surface` 抽成与 GPU 无关的纯函数——三层
/// 可见性本身只是一张判定表（项目所有者原话：「没有视野的地方就暗
/// 下来一些，有视野的地方就没问题。而没去过的地方就黑着」），不需要
/// 靠跑起整条渲染管线才能验证：
///
/// - 当前有视野 → 画，用 `tint`（当前光照色调）。
/// - 当前无视野但已探索过 → 画，用 [`memory_tint`]（记忆层，比当前
///   光照暗）。
/// - 既无视野也没探索过 → 不画（`None`），调用方应当跳过这一格，让
///   `ll-render` 的黑色清屏背景顶替「从未探索」的黑。
pub fn tile_tint(currently_visible: bool, explored: bool, tint: [f32; 4]) -> Option<[f32; 4]> {
    if currently_visible {
        Some(tint)
    } else if explored {
        Some(memory_tint(tint))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个仅在测试里现造的露天地表 profile——本文件多条断言都需要
    /// 「露天、没有地板光、其余字段无所谓」这同一个形状，抽成函数避免
    /// 每条各拼一遍（既有的几条测试各自内联构造，改动它们不属于本批次
    /// 范围，新增的几条用这个帮手）。
    /// 本体矮人在 `mods/lostland/races.json5` 里声明的暗视格数。
    ///
    /// 本文件的断言只需要「一个高于默认值的声明」这条性质，取本体真实
    /// 数值而不是另编一个，是为了让这里失败时能直接对上内容里的那一行
    /// ——端到端那一侧（`ll-mod/tests/base_mod_darkvision.rs`）钉的是
    /// 同一个数字经真实 `mods/` 装载之后的结果。
    const DWARF_DARKVISION_CELLS: u32 = 7;

    fn surface_profile() -> SpaceProfile {
        SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        }
    }

    #[test]
    fn 本体地形直接查到带命名空间前缀的图集条目而不需要回退到registry() {
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let registry = Registry::new();

        // Act：取变体号 0 那一格。变体 0 的条目名恒等于
        // `terrain_entry_name` 的返回值，见 `TERRAIN_VARIANT_SUFFIX`。
        let pos = first_pos_with_variant(ids.grass, &ids, 0);
        let key = terrain_atlas_key(ids.grass, &ids, &registry, pos);

        // Assert
        assert_eq!(key.as_deref(), Some("lostland:terrain_grass"));
    }

    /// 测试用的环面尺寸。取值只要够大到能扫出全部变体即可，与真实世界
    /// 尺寸无关——变体号只看 `TorusPos` 的两个坐标，不看世界多大。
    fn test_world() -> ll_core::torus::TorusSize {
        ll_core::torus::TorusSize::new(64, 64).expect("非零尺寸恒合法")
    }

    /// 扫出第一个取到指定变体号的格子。找不到就 panic——「某个声明出来
    /// 的变体在 64×64 格里一次都没被取到」本身就是缺陷（等于那张图白画）。
    fn first_pos_with_variant(kind: TerrainKind, ids: &BaseTerrainIds, variant: u32) -> TorusPos {
        let world = test_world();
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                let pos = world.wrap(x, y);
                if terrain_variant_at(kind, ids, pos) == variant {
                    return pos;
                }
            }
        }
        panic!("64×64 格里一次都没取到变体 {variant}");
    }

    #[test]
    fn 同一格的地形变体号跑两次恒相同() {
        // 「确定性」这条性质的最小可执行形式：变体选取是渲染期每帧重算
        // 的，同一格在相邻两帧算出不同的图会直接闪烁。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let world = test_world();

        // Act & Assert
        for y in 0..world.height() as i32 {
            for x in 0..world.width() as i32 {
                let pos = world.wrap(x, y);
                let first = terrain_variant_at(ids.grass, &ids, pos);
                let second = terrain_variant_at(ids.grass, &ids, pos);
                assert_eq!(first, second, "({x}, {y}) 两次取到不同的变体号");
            }
        }
    }

    #[test]
    fn 环面上绕回同一格的坐标取到同一个变体号() {
        // 环面：`TorusPos` 的不变式保证 `(-1, -1)` 与 `(63, 63)` 是同一个
        // 值，因此变体号必然相同。这条钉的是「有人把入参从 `TorusPos`
        // 放宽成裸 `(i32, i32)`」那种失效方式——那一刻绕回就断了，而
        // 画面上只会表现成世界接缝处的一条纹路，没人看得出是这里的问题。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let world = test_world();

        // Act & Assert
        for (x, y) in [(0, 0), (7, 3), (63, 63), (12, 40)] {
            let direct = world.wrap(x, y);
            let wrapped = world.wrap(x - world.width() as i32, y + world.height() as i32);
            assert_eq!(direct, wrapped, "绕回后应当是同一个 TorusPos");
            assert_eq!(
                terrain_variant_at(ids.grass, &ids, direct),
                terrain_variant_at(ids.grass, &ids, wrapped),
            );
        }
    }

    #[test]
    fn 位置不同真的会取到不同的地形变体() {
        // 反例验证点名的第三条：「位置不同真的会取到不同变体（否则等于
        // 没做）」。判据不止「至少出现过两个值」——那条太松，一张变体
        // 只在角落里出现一次也算过。这里要求**每一个声明出来的变体号在
        // 64×64 格里都至少占到 1/6**，即分布没有塌到某一张上。
        //
        // 1/6 这个下界怎么来的：本批最多 3 个变体，均匀分布下每个应当
        // 占 1/3；取一半当门槛，既容得下哈希的自然抖动，又拦得住
        // 「某张图几乎永远选不到」。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let world = test_world();
        let total = (world.width() * world.height()) as usize;

        for kind in all_base_kinds(&ids) {
            let count = terrain_variant_count(kind, &ids);
            if count <= 1 {
                continue;
            }
            // Act
            let mut tally = vec![0usize; count as usize];
            for y in 0..world.height() as i32 {
                for x in 0..world.width() as i32 {
                    let variant = terrain_variant_at(kind, &ids, world.wrap(x, y));
                    assert!(variant < count, "变体号 {variant} 越界（共 {count} 张）");
                    tally[variant as usize] += 1;
                }
            }

            // Assert
            for (variant, hits) in tally.iter().enumerate() {
                assert!(
                    *hits * 6 >= total,
                    "地形 {:?} 的变体 {variant} 在 {total} 格里只被取到 {hits} 次，\
                     不足 1/6——这张图等于白画",
                    kind.index()
                );
            }
        }
    }

    #[test]
    fn 相邻格子取到同一变体的比例接近相互独立() {
        // **这条是本批唯一一条抓到真问题的断言，不是补充说明。**
        // 「每个变体各占三分之一」这条太弱：把 `terrain_variant_at` 里
        // 三行混入的次序换一下，分布仍然是漂亮的 33/33/33，而画面上会
        // 长出一条条竖纹——纵向相邻同变体的比例从 32% 跳到 47%，也就是
        // 平均每两格才换一次图。那是把「太单调」换成另一种规则感。
        //
        // 门槛是**相互独立时的比例再加 8 个百分点**——不是一个固定数字：
        // 相互独立时相邻两格同变体的概率就是 `1/变体数`，草地（3 张）
        // 是 33%，沙地（2 张）是 50%，拿同一个绝对数字卡两者必然冤枉
        // 其中一个。
        //
        // 128×128 实测（本函数 vs 把三行次序换回去）：
        //
        // | 混入次序 | 草/林 n=3（理想 33%） | 沙 n=2（理想 50%） |
        // |---|---|---|
        // | x, y, 名字（本函数） | 横 33 / 纵 32 / 斜 30 | 横 48 / 纵 50 / 斜 49 |
        // | 名字, x, y | 横 30 / **纵 47** / 斜 31 | 横 48 / **纵 62** / 斜 49 |
        // | 名字, y, x | **横 47** / 纵 30 / 斜 31 | **横 62** / 纵 48 / 斜 49 |
        //
        // 本函数最大偏离 0 个百分点，两种错写法偏离 12~14 个。8 卡在
        // 中间，两种错写法在草地与沙地上**都**会红。
        const ADJACENT_SAME_SLACK_PERCENT: usize = 8;

        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let world = ll_core::torus::TorusSize::new(128, 128).expect("非零尺寸恒合法");
        let (w, h) = (world.width() as i32, world.height() as i32);

        for kind in all_base_kinds(&ids) {
            if terrain_variant_count(kind, &ids) <= 1 {
                continue;
            }
            // Act：横、纵、斜三个方向各数一遍。
            let mut same = [0usize; 3];
            let total = (w * h) as usize;
            for y in 0..h {
                for x in 0..w {
                    let here = terrain_variant_at(kind, &ids, world.wrap(x, y));
                    for (slot, (dx, dy)) in [(1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                        if terrain_variant_at(kind, &ids, world.wrap(x + dx, y + dy)) == here {
                            same[slot] += 1;
                        }
                    }
                }
            }

            // Assert
            for (slot, label) in ["横向", "纵向", "斜向"].into_iter().enumerate() {
                let percent = same[slot] * 100 / total;
                let independent = 100 / terrain_variant_count(kind, &ids) as usize;
                let threshold = independent + ADJACENT_SAME_SLACK_PERCENT;
                assert!(
                    percent <= threshold,
                    "地形 {:?} 的{label}相邻格有 {percent}% 取到同一张变体\
                     （相互独立时应为 {independent}%，门槛 {threshold}%）\
                     ——画面上会读成一条条纹路",
                    kind.index()
                );
            }
        }
    }

    #[test]
    fn 变体号越界时查不到条目名() {
        // `terrain_atlas_key_for_variant` 刻意不截断越界的变体号：截断会
        // 让「门禁多数了一张」退化成「门禁重复验了同一张」，正是 ADR 0022
        // 那种「测试全绿但保护不存在」的形状。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let registry = Registry::new();
        let count = terrain_variant_count(ids.grass, &ids);

        // Act & Assert
        assert!(terrain_atlas_key_for_variant(ids.grass, &ids, &registry, count - 1).is_some());
        assert_eq!(
            terrain_atlas_key_for_variant(ids.grass, &ids, &registry, count),
            None
        );
    }

    #[test]
    fn 变体条目名带后缀且能被反向锁的判据认出来() {
        // 反向锁（`atlas_coverage.rs` 的「图集里不许有声明侧数不出来的
        // 变体贴图」）用 `is_terrain_variant_entry` 判定，判据必须与
        // `terrain_atlas_key_for_variant` 产出的名字对得上——两边分叉的
        // 话反向锁会漏掉真正的孤儿图而自己毫不知情。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();
        let registry = Registry::new();

        // Act & Assert
        for variant in 0..terrain_variant_count(ids.grass, &ids) {
            let key = terrain_atlas_key_for_variant(ids.grass, &ids, &registry, variant)
                .expect("变体号在范围内");
            assert_eq!(
                is_terrain_variant_entry(&key),
                variant > 0,
                "{key} 的变体判定不对"
            );
        }
        assert!(is_terrain_variant_entry("lostland:terrain_grass_alt1"));
        assert!(!is_terrain_variant_entry("lostland:terrain_grass"));
        // 后缀后面不是十进制数字的不算变体——否则真有一种地形叫
        // `xxx_altar`，反向锁会把它当孤儿变体图报红。
        assert!(!is_terrain_variant_entry("lostland:terrain_altar"));
        assert!(!is_terrain_variant_entry("lostland:terrain_grass_alt"));
    }

    #[test]
    fn mod注册的地形回退到registry查出完整命名空间字符串() {
        // 这条测试直接对应「mod 能注册一把剑，却给不了它一张图」这条
        // 瓶颈在地形渲染上的修复：examplemod 注册的 lava_floor 不在
        // BaseTerrainIds 这张静态表里，terrain_atlas_key 必须回退到
        // Registry 反查出完整命名空间字符串，而不是直接判定「查不到」。
        // Arrange：地形索引与 mod 地形索引必须来自同一个 Registry——
        // 与真实装载流程一致（本体先注册、mod 后 intern，见
        // `ll_mod::pipeline` 模块文档「本体内容不经过这条管线」一节）。
        // 若各用一个独立 `Registry::new()`，两边的索引计数器各自从零
        // 开始，数值可能巧合重叠，`terrain_entry_name` 会在真正测试
        // 回退逻辑之前就已经因为索引数值碰巧相等而误判命中。
        let mut registry = Registry::new();
        let (ids, _table) = ll_mod::base_terrain::register_base_terrain(&mut registry)
            .expect("本体地形声明表内部一致，注册恒不失败");
        let mod_id = ll_core::ident::NamespacedId::parse("examplemod:lava_floor")
            .expect("测试用命名空间恒合法");
        let index = registry.intern(mod_id);
        let mod_terrain = ll_world::terrain::TerrainKind::from_index(index);

        // Act：mod 地形恒 1 张，任何位置都取变体 0，走 Registry 回退。
        let key = terrain_atlas_key(mod_terrain, &ids, &registry, test_world().wrap(3, 7));

        // Assert
        assert_eq!(key.as_deref(), Some("examplemod:lava_floor"));
        assert_eq!(terrain_variant_count(mod_terrain, &ids), 1);
    }

    /// ~~`define_base` 注册的全部 17 种本体地形~~，与
    /// `ll_world::terrain` 里那张注册表逐条对应。
    ///
    /// 写成一张具名表而不是就地展开，是因为下面几条测试都要遍历它。
    ///
    /// # 〔2026-08-31 批次 28 原地更正（纪律第 9 条）：这张表当时漏了两种〕
    ///
    /// 原文说「全部 17 种」——**在气候条带批次给 [`BaseTerrainIds`] 加上
    /// `desert`/`tundra` 之后，这句话就不成立了**：表还是 17 行，注册表
    /// 已经是 19 种，沙漠与冻原整个落在本文件三条断言的盲区里，
    /// 与 `crates/ll-game/tests/atlas_coverage.rs` 模块文档记着的那次
    /// 「手写地形清单漏掉新地形」是**同一个形状、同一份债**。
    ///
    /// 更正方：`docs/superpowers/plans/2026-08-31-batch28-terrain-art.md`
    /// 五节第 3 条（变体覆盖的分母就是这张表，分母漏一行，逐张覆盖照样
    /// 退化）。
    ///
    /// **改法不是补两行，是让它没法再漏**：下面对 [`BaseTerrainIds`] 做
    /// **穷尽解构**——没有 `..`，加一个字段这里编译不过。这正是
    /// `atlas_coverage.rs` 那份文档给出的、当时没人还的那条建议。
    fn all_base_kinds(ids: &BaseTerrainIds) -> [TerrainKind; BASE_TERRAIN_COUNT] {
        // 穷尽解构：这里**不许**写 `..`。加一种本体地形，本行编译不过，
        // 而不是安静地少验一种。
        let BaseTerrainIds {
            deep_water,
            shallow_water,
            sand,
            grass,
            forest,
            hill,
            mountain,
            snow,
            desert,
            tundra,
            floor_wood,
            floor_stone,
            wall_wood,
            wall_stone,
            door_closed,
            door_open,
            window,
            stairs_up,
            stairs_down,
        } = *ids;
        [
            deep_water,
            shallow_water,
            sand,
            grass,
            forest,
            hill,
            mountain,
            snow,
            desert,
            tundra,
            floor_wood,
            floor_stone,
            wall_wood,
            wall_stone,
            door_closed,
            door_open,
            window,
            stairs_up,
            stairs_down,
        ]
    }

    /// [`all_base_kinds`] 的长度。数组长度写成具名常量，是为了让
    /// 「加了一种地形却忘了往数组字面量里补一行」在**长度不匹配**这一步
    /// 就编译不过，而不是靠人去数。
    const BASE_TERRAIN_COUNT: usize = 19;

    #[test]
    fn 全部十九种本体地形都能查到图集条目() {
        // 此前这条只覆盖 8 种自然地形，7 种建筑地形整个落在盲区里——
        // 见 `terrain_entry_name` 文档「一种不漏」一节。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();

        // Act & Assert
        for kind in all_base_kinds(&ids) {
            assert!(
                terrain_entry_name(kind, &ids).is_some(),
                "地形索引 {:?} 查不到图集条目名",
                kind.index()
            );
        }
    }

    #[test]
    fn 十九种本体地形的图集条目名两两不同() {
        // 「都查得到」不等于「查到的不是同一张图」：此前 `wall_stone`
        // 与 `mountain` 就共用 `terrain_mountain`，两条都是 Some，屏幕
        // 上却分不出哪格是山、哪格是石墙。这条钉的是那种失效方式。
        // Arrange
        let (ids, _table) = ll_world::terrain::base_terrain_fixture();

        // Act
        let names: Vec<&str> = all_base_kinds(&ids)
            .into_iter()
            .map(|kind| terrain_entry_name(kind, &ids).expect("上一条测试已保证恒为 Some"))
            .collect();

        // Assert：BTreeSet 而非 HashSet——约束 C5 禁止逻辑依赖哈希
        // 容器迭代顺序，这里虽然只数个数，仍统一用有序容器。
        let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "{BASE_TERRAIN_COUNT} 种地形只查出 {} 个不同的图集条目名：{names:?}",
            unique.len()
        );
    }

    #[test]
    fn 光照全灭时视野半径缩小到基准值以下() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_dark").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: false,
            base_temperature: 0,
            diggable: false,
            buildable: false,
            reverb_tag: None,
        };

        // Act
        let radius = effective_sight_radius(&profile, Tick(0), Weather::CLEAR);

        // Assert
        assert!(radius < BASE_SIGHT_RADIUS);
    }

    /// 测试用的最小 `RaceDarkvisionSource`——固定返回同一个暗视格数，
    /// 不依赖 `ll_mod::race::RaceTable`，只用来隔离验证
    /// [`effective_sight_radius_for_race`] 这一步换算本身的行为，见其
    /// 文档「依赖方向」一节。
    ///
    /// 取值直接用本体矮人声明的 7 格：暗视改成「夜间视野格数下限」
    /// 之后，测试用的数字与 `mods/lostland/races.json5` 里的数字终于是
    /// 同一个量纲。旧形态（暗视是光照千分比下限）下这个夹具必须写成
    /// `FixedDarkvision(DWARF_DARKVISION_CELLS)`——把本体矮人实际声明的 4 放大 150 倍才能
    /// 让功能表现出可观测差异，那本身就是「机制对、数值错」的自白，
    /// 见 `ll_sim::vision` 模块文档「缺口是什么」一节。
    struct FixedDarkvision(u32);

    impl ll_sim::vision::RaceDarkvisionSource for FixedDarkvision {
        fn darkvision_cells(&self, _race: ll_core::ident::ContentIndex) -> u32 {
            self.0
        }
    }

    #[test]
    fn 暗视种族夜间视野大于无暗视种族() {
        // 同一时刻、同一地点，唯一变量是种族声明的暗视格数——直接
        // 对应 `effective_sight_radius_for_race` 文档「为什么接在这一
        // 步」一节要接线的效果。手工验证：把
        // `ll_sim::vision::sight_radius_for_race` 改成恒传 0（不查种族
        // 声明），这条测试会失败——两者都落回
        // `DEFAULT_NIGHT_SIGHT_RADIUS`，断言 `>` 不再成立。
        //
        // **旧公式下这条断言是假的**：暗视还是「光照千分比下限」时，
        // 本体矮人的 4 连午夜环境光 100 都抬不动，矮人与人类的夜间视野
        // 完全相同（都撞在 4 格下限上），只有把夹具放大到 600 才测得
        // 出差异。现在夹具用的就是矮人真实声明的 7 格。
        // Arrange：地表深夜（`Tick(0)`，午夜光照按昼夜曲线不为零但很低）。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };
        let midnight = Tick(0);
        let race = ll_core::ident::ContentIndex::default();
        let darkvision = FixedDarkvision(DWARF_DARKVISION_CELLS);
        let no_darkvision = FixedDarkvision(0);

        // Act
        let with_darkvision =
            effective_sight_radius_for_race(&profile, midnight, Weather::CLEAR, race, &darkvision);
        let without_darkvision = effective_sight_radius_for_race(
            &profile,
            midnight,
            Weather::CLEAR,
            race,
            &no_darkvision,
        );

        // Assert
        assert!(with_darkvision > without_darkvision);
    }

    #[test]
    fn 白天暗视种族与无暗视种族视野相同() {
        // 正午满光照（1000）下基准半径 12 格本就远高于任何种族声明的
        // 暗视格数——夜间下限在这种输入下根本不参与取值，证明暗视只在
        // 暗处起作用，不是无脑加成。
        // Arrange：地表正午（与 `ll_world::light` 「正午光照最强」测试
        // 同一个采样点：夏季第 30 天正午，季节缩放不折损）。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };
        let noon = Tick(30 * ll_core::time::TICKS_PER_DAY + 12 * ll_core::time::TICKS_PER_HOUR);
        let race = ll_core::ident::ContentIndex::default();
        let darkvision = FixedDarkvision(DWARF_DARKVISION_CELLS);
        let no_darkvision = FixedDarkvision(0);

        // Act
        let with_darkvision =
            effective_sight_radius_for_race(&profile, noon, Weather::CLEAR, race, &darkvision);
        let without_darkvision =
            effective_sight_radius_for_race(&profile, noon, Weather::CLEAR, race, &no_darkvision);

        // Assert
        assert_eq!(with_darkvision, without_darkvision);
    }

    /// 开局那一刻玩家到底看得见什么——这条是**组合断言**。
    ///
    /// 午夜环境光（千分之一百）、`sight_radius_at` 的缩放、`effective_tint`
    /// 的整体调制、以及三层可见性里「从未探索就不画」，四条规则各自都
    /// 正确、各自都有测试守着，叠在一起却让 `Tick(0)` 开局变成纯黑屏加
    /// 正中央五个格子——项目所有者实测报告了这个现象。缺的从来不是某
    /// 一块的测试，而是「这些块凑在一起时开局长什么样」这一条。
    #[test]
    fn 新游戏起始时刻的地表视野远大于最小半径() {
        // Arrange：露天地表，没有额外的环境光下限加成。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let radius =
            effective_sight_radius(&profile, crate::world::NEW_GAME_START_TICK, Weather::CLEAR);

        // Assert：至少要有基准半径的一半，否则开局仍然近乎瞎。
        assert!(radius >= BASE_SIGHT_RADIUS / 2);
    }

    /// 与上一条配套：起始时刻的画面整体亮度不能低到把可见格子也压黑。
    #[test]
    fn 新游戏起始时刻的画面亮度过半() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let tint = effective_tint(&profile, crate::world::NEW_GAME_START_TICK, Weather::CLEAR);

        // Assert
        assert!(tint[0] > 0.5);
    }

    /// 项目所有者的要求：「让黑夜有个最低视野范围以及一个还算能看的
    /// 亮度」。这条锁住亮度那一半，视野那一半由
    /// `ll_world::light` 的 `午夜视野不低于最小半径` 锁住。
    #[test]
    fn 午夜的画面亮度不低于可见下限() {
        // Arrange：露天地表，午夜，没有任何额外光源。
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let tint = effective_tint(&profile, Tick(0), Weather::CLEAR);

        // Assert
        assert!(tint[0] >= MIN_VISIBLE_TINT);
    }

    /// 但夜晚仍必须明显暗于正午，否则昼夜只剩计时意义。
    #[test]
    fn 午夜画面明显暗于正午() {
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_surface").expect("字面量恒合法"),
            ambient_light_floor: 0,
            exposed_to_sky: true,
            base_temperature: 0,
            diggable: true,
            buildable: true,
            reverb_tag: None,
        };

        // Act
        let midnight = effective_tint(&profile, Tick(0), Weather::CLEAR);
        let noon = effective_tint(
            &profile,
            Tick(12 * ll_core::time::TICKS_PER_HOUR),
            Weather::CLEAR,
        );

        // Assert
        assert!(midnight[0] < noon[0]);
    }

    /// 生产渲染路径上的天气组合断言——`ll_world::light` 那一侧已经钉住
    /// 了「换算本身不会把视野压到不可玩」，这里钉的是**接线**：
    /// `effective_sight_radius`/`effective_tint` 这两个每帧被
    /// `crate::app::render_surface` 调用的函数真的会因为天气不同而给出
    /// 不同答案。链路断在 `ll-game` 这一节的话，上游断言全绿、玩家却
    /// 看不出任何区别，正是本项目反复吃亏的那类缺口。
    #[test]
    fn 露天空间的视野半径与画面亮度都随天气变化() {
        // Arrange：夏季正午，露天地表——排除昼夜与季节的干扰。
        let profile = surface_profile();
        let noon = Tick(30 * ll_core::time::TICKS_PER_DAY + 12 * ll_core::time::TICKS_PER_HOUR);
        let (ids, table) = ll_world::weather::base_weather_fixture();
        let foggy = Weather {
            kind: Some(ids.fog),
            light_scale: table.light_scale(ids.fog),
            sight_scale: table.sight_scale(ids.fog),
            temperature_offset: 0,
        };

        // Act
        let clear_radius = effective_sight_radius(&profile, noon, Weather::CLEAR);
        let foggy_radius = effective_sight_radius(&profile, noon, foggy);
        let clear_tint = effective_tint(&profile, noon, Weather::CLEAR);
        let foggy_tint = effective_tint(&profile, noon, foggy);

        // Assert
        assert!(foggy_radius < clear_radius, "雾必须真的缩短实机视野半径");
        assert!(foggy_tint[0] < clear_tint[0], "雾必须真的压暗画面");
    }

    #[test]
    fn 非露天空间的视野半径与画面亮度都不随天气变化() {
        // 「洞窟不受天气影响」这条语义在生产渲染路径上的落点——两个
        // 乘数都必须被 effective_weather 中和掉，不能只中和一个。
        // Arrange
        let profile = SpaceProfile {
            id: ll_core::ident::NamespacedId::parse("lostland:test_cave").expect("字面量恒合法"),
            ambient_light_floor: 200,
            exposed_to_sky: false,
            base_temperature: 0,
            diggable: true,
            buildable: false,
            reverb_tag: None,
        };
        let noon = Tick(30 * ll_core::time::TICKS_PER_DAY + 12 * ll_core::time::TICKS_PER_HOUR);
        let storm = Weather {
            kind: None,
            light_scale: 100,
            sight_scale: 100,
            temperature_offset: 0,
        };

        // Act & Assert
        assert_eq!(
            effective_sight_radius(&profile, noon, Weather::CLEAR),
            effective_sight_radius(&profile, noon, storm)
        );
        assert_eq!(
            effective_tint(&profile, noon, Weather::CLEAR),
            effective_tint(&profile, noon, storm)
        );
    }

    #[test]
    fn 开局那一刻可能出现的任何天气下视野都不低于基准半径的一半() {
        // 既有断言「开局视野至少要有基准半径的一半，否则开局仍然近乎
        // 瞎」在加进天气之后仍须成立——这是天气最容易破坏的一条既有
        // 保证（两个乘数相乘很容易把开局压穿；本批次实测雾的视野乘数
        // 取 650 时开局会掉到 5，因此本体表把它改成了 700，见
        // `ll_world::weather::materialize_base_weathers` 文档第 3 条）。
        //
        // 只遍历**开局那一季真的可能出现**的天气：雪在春季权重为 0，
        // 新游戏（春季早八点）永远不会开在雪天，把它算进来是在给一个
        // 不可能发生的组合立断言，会逼着未来的人为了让测试变绿去改一个
        // 与开局无关的数值。
        // Arrange
        let profile = surface_profile();
        let start = crate::world::NEW_GAME_START_TICK;
        let (_ids, table) = ll_world::weather::base_weather_fixture();
        let slot = ll_world::weather::season_slot(start.season());

        // Act & Assert
        let mut checked = 0;
        for index in table.registered() {
            if table.season_weights(*index)[slot] == 0 {
                continue;
            }
            checked += 1;
            let weather = Weather {
                kind: Some(*index),
                light_scale: table.light_scale(*index),
                sight_scale: table.sight_scale(*index),
                temperature_offset: 0,
            };
            let radius = effective_sight_radius(&profile, start, weather);
            assert!(
                radius >= BASE_SIGHT_RADIUS / 2,
                "开局视野半径 {radius} 低于基准半径的一半"
            );
        }
        assert!(
            checked >= 2,
            "开局那一季只有 {checked} 种可能的天气，这条断言几乎没检查到东西"
        );
    }

    #[test]
    fn 记忆色调比原始光照色调暗() {
        // Arrange
        let tint = [1.0, 1.0, 1.0, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert!(dimmed[0] < tint[0]);
    }

    #[test]
    fn 记忆色调不改变透明度() {
        // Arrange
        let tint = [0.6, 0.6, 0.6, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert_eq!(dimmed[3], tint[3]);
    }

    #[test]
    fn 全黑光照下记忆色调仍是全黑() {
        // Arrange：夜间/无光照场景，压暗系数不该把零变成非零。
        let tint = [0.0, 0.0, 0.0, 1.0];

        // Act
        let dimmed = memory_tint(tint);

        // Assert
        assert_eq!(dimmed, [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn 当前有视野的格子用原始色调绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(true, false, tint);

        // Assert
        assert_eq!(result, Some(tint));
    }

    #[test]
    fn 探索过但当前无视野的格子用记忆色调绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(false, true, tint);

        // Assert
        assert_eq!(result, Some(memory_tint(tint)));
    }

    #[test]
    fn 从未探索且当前无视野的格子不绘制() {
        // Arrange
        let tint = [0.8, 0.8, 0.8, 1.0];

        // Act
        let result = tile_tint(false, false, tint);

        // Assert
        assert_eq!(result, None);
    }
}
