//! 地表世界内容 → 绘制指令的**纯计算**：地面物品堆、放置家具、NPC 三
//! 类内容各该用哪个图集键、落在 [`Layer`] 的哪一层、同层内部按什么顺序
//! 排。与 GPU 无关，拆出来的理由与 [`crate::layout`] 一致。
//!
//! # 这个模块补的是哪个洞
//!
//! `crate::app::render_surface` 此前只 push 两样东西：地形瓦片与玩家
//! 标记。地面物品（[`ll_world::state::WorldState::ground_items`]）、
//! 放置家具（同一份数据里 `placed == true` 的那些）、NPC
//! （[`ll_world::state::WorldState::actors`] 里除玩家之外的全部）在引擎
//! 里都存在、能交互、有测试，**但从来没有任何一条渲染路径读过它们**
//! ——玩家看不见脚下有东西。本模块与
//! `crate::app::render_surface` 里对它的调用，就是把这三类内容接上屏幕
//! 的那一步。
//!
//! # 项目所有者的裁定：地面物品统一用一个「团」
//!
//! > 当物品丢在地上，无论是一个还是N个，交互的时候都统一以列表显示，
//! > 并且统一用一个团表示哪一个地方有东西
//!
//! [`ground_pile_draws`] 因此**按坐标去重**：一格上躺着一件还是二十件
//! 东西，产出的都是恰好一条指令、恒定指向同一个图集键
//! [`GROUND_PILE_SPRITE`]。件数、种类、是不是尸体，全都不影响这一条
//! 指令——那些是交互列表的事。
//!
//! # 引擎不认识任何一件具体内容（ADR 0021）
//!
//! 「已放置的家具画成什么样」「这个 NPC 画成什么样」**不在 Rust 里按
//! 内容 id 分支**。规矩是两级：
//!
//! 1. 先拿这条内容在 [`Registry`] 里的**完整命名空间 ID** 当图集键去查
//!    （家具查物品 ID，NPC 的身子查种族 ID、挂件查职业 ID）。这不是本
//!    模块发明的新约定——
//!    `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 规定「任何精灵的
//!    图集条目名恒等于它的完整命名空间 ID」，`crate::layout::terrain_atlas_key`
//!    的 mod 地形回退路径用的就是同一条约定。**内容因此已经有办法声明
//!    自己的精灵键了：往自己的 `assets/sprites/` 里放一张与本地名同名的
//!    图即可**，不需要在 `items.json5`/`races.json5`/`classes.json5` 里
//!    新增字段，也就不需要动 `CONTENT_HASH_ALGORITHM_VERSION`。
//! 2. 查不到就退化到一张通用记号（[`PLACED_FURNITURE_SPRITE`]/
//!    [`NPC_SPRITE`]），或者——当这一层本来就是可选的叠加层时——
//!    **什么都不画**（[`SurfaceDraw::fallback_key`] 为 `None`）。
//!
//! # NPC 为什么是两条指令而不是一张「种族×职业」的图
//!
//! **这一节记的是分层合成为什么存在，它现在是回退链的最后一段而不是
//! 唯一的画法**——所有者后来裁定「每个种族的每个职业画上风格不同的
//! 图片」，那 117 张合成图排在它前面（见下一节）。分层这一层没有退休：
//! mod 新加的种族或职业没有合成图时，落到的就是这里。
//!
//! 所有者最初要求「npc 根据职业种族做出区别」。本体现有 9 个种族 × 13
//! 个职业 = 52 种组合——逐个组合备一张图，加第 10 个种族要补 13 张，
//! 加第 14 个职业要补 9 张，是乘法级的负担。
//!
//! 本模块因此把一个 NPC 拆成**两条绘制指令、两张图**：
//!
//! - **身子**：查种族 ID（`lostland:dwarf`），查不到退回 [`NPC_SPRITE`]。
//!   决定体型、肤色、耳朵、胡子这些「他是什么」的东西。
//! - **挂件**：查职业 ID（`lostland:blacksmith`），**查不到就不画**。
//!   一张四周透明、只在胸口有图案的同尺寸贴图，叠在身子上，决定「他
//!   干什么」。
//!
//! 于是资产量从 `种族数 × 职业数` 降到 `种族数 + 职业数`：加第 10 个
//! 种族只要多一张身子图，加第 14 个职业只要多一张挂件图，**另一侧一张
//! 都不用补，引擎一行都不用改**。
//!
//! 这条抽象的正当理由仍然是 ADR 0021 说的「有算法要共用」而不是「看起来
//! 该对称」：身子与挂件共用同一条「优先键 → 兜底」的查图次序、同一套
//! 锚点/缩放换算、同一个消费点（`render_surface` 里的 `push_surface_draw`）。
//! 它同时也是那条纪律的**另一侧**——把同一张人形抄 52 遍同样没有正当
//! 理由，因为那 52 份之间没有任何算法差异，只有一个查表键的差异。
//!
//! # 合成图的回退链（角色创建批次，所有者裁定「留个位置，不要复制粘贴」）
//!
//! 上面那条「身子 + 挂件」的分层合成**没有被推翻**，它成了三段回退链的
//! 最后一段：
//!
//! ```text
//! <种族 ID>_<压平的职业 ID>_<性别>   ← 今天一张都没有
//!       ↓ 查不到就退
//! <种族 ID>_<压平的职业 ID>          ← 美术批次的 117 张（9 族 × 13 职业）
//!       ↓ 查不到就退
//! <种族 ID> + <职业 ID>               ← 分层合成（种族没有合成图时的兜底）
//! ```
//!
//! 键的确切拼法（以及「为什么冒号那一位必须留给种族」）见
//! [`composite_keys`] 文档「键的形状」一节——那一段记着一个真实踩过的
//! 坑：拼错一个字符，117 张图会**静默**全部失效。
//!
//! 所有者的原话是「以后可能会加入不同性别的贴图，不过目前先留着个位置
//! 默认用其中一个好了」。**「留着个位置」不等于把同一张图复制两份**
//! ——两份同样的图迟早会漂（本仓库有过先例）。性别那一段今天仍然零张
//! ⇒ 每个人都落到第二段（合成图）⇒ 男女暂时同图，但槽位是真实存在的：
//! 往 `assets/sprites/` 里放一张 `human_lostland_blacksmith_female.png`
//! 并在精灵清单里登记，它就生效，引擎一个字都不用改。
//!
//! ## 命中合成图时，职业挂件那一层必须让位
//!
//! 合成图里职业已经画在身子上了，挂件再叠一层等于同一件事画两遍。
//! 这就是 [`SurfaceDraw::superseded_by`] 存在的全部理由：挂件那条指令
//! 声明「这几个键里任何一个查得到，我就不画」。合成图批次落地之后
//! 本体九族全部命中，**挂件层因此对本体 NPC 恒不画**；没有合成图的
//! mod 内容（示例 mod 的半精灵/死灵法师）仍然走分层合成那条老路。
//!
//! ## 这条链就是 `Agent::gender` 那条门禁豁免的理由
//!
//! `scripts/ci/check_field_consumers.py` 里 `Agent.gender` 那条豁免写的
//! 是「渲染层今天就在读它」。指的就是 [`npc_draws`] 里那句
//! `agent.gender.sprite_tag()`。豁免理由要成立，这条链就必须是真接上的，
//! 不是写在文档里的计划。
//!
//! 「身子按种族、挂件按职业」这条分工是本批次的判断，不是所有者原话；
//! 所有者只说了「根据职业种族做出区别」。真要改成「职业也换体型」或者
//! 「装备也画上去」，是往这条规则上再加层，不是推翻它。
//!
//! 抽象在这里的正当理由是**有算法要共用**（ADR 0021）：三类内容共用
//! 同一条「优先键 → 兜底键」的查图次序与同一条确定性排序规则，写成
//! [`SurfaceDraw`] 这一种指令 + 一个消费点（`render_surface` 里的
//! `push_surface_draw`），才不至于把同一段查图逻辑抄三遍。反过来，本
//! 模块**不**把「地面物品堆」也做成可被内容覆盖的键——那不是共用算法，
//! 那是所有者明确裁定过的「统一一个团」。
//!
//! # 确定性（约束 C5）
//!
//! 同一格上可能同时有多样东西，绘制顺序必须逐帧、逐进程恒定：
//!
//! - 地面物品堆按 `(y, x)` 收进 [`std::collections::BTreeSet`]（**不是** `HashSet`）去重，
//!   产出顺序是行主序。
//! - 放置家具按 `ground_items` 这个 [`Vec`] 的下标排。
//! - NPC 按 `ll_world::entity::Arena` 的槽位下标排（`iter_with_id`
//!   本身就是 `Vec` 顺序）。
//!
//! 三者的绘制序号（[`SurfaceDraw::entity`]，[`ll_render::sprite::DrawOrder`] 的最后一级
//! 比较键）落在互不重叠的号段里，见本模块的四个 `*_ENTITY_BASE` 常量。

use std::collections::BTreeSet;

use ll_core::torus::TorusPos;
use ll_mod::registry::Registry;
use ll_render::sprite::Layer;
use ll_world::entity::{EntityId, Gender};
use ll_world::state::WorldState;
use ll_world::terrain::TerrainKind;
use ll_world::tree::{TreeSpecies, tree_at};

/// 玩家标记在绘制顺序里固定的实体号（[`Layer::ENTITY`] 层）。
///
/// 与 [`NPC_ENTITY_BASE`] 同住 [`Layer::ENTITY`]，两者必须互不相撞——
/// 这正是这个常量从 `crate::app` 搬到这里的理由：玩家号与 NPC 号段是
/// **同一个决定的两面**，分居两个文件时，改动其中一个的人看不见另一个。
pub const PLAYER_ENTITY: u64 = 0;

/// 地形瓦片绘制顺序号的起始偏移（[`Layer::TERRAIN`] 层）。
///
/// 与其余号段不同层，因此与它们不可能相撞（[`ll_render::sprite::DrawOrder`] 先比层），
/// 放在这里只是为了让「绘制序号一共分了哪几段」有唯一一处清单。
pub const TERRAIN_ENTITY_BASE: u64 = 1;

/// 地面物品堆绘制顺序号的起始偏移（[`Layer::DECOR`] 层）。
///
/// 号段是 `[0, 世界格数)`——每格至多一堆（按坐标去重），序号取
/// `y * 世界宽 + x`，与地形用的是同一套行主序编号。
pub const GROUND_PILE_ENTITY_BASE: u64 = 0;

/// 放置家具绘制顺序号的起始偏移（[`Layer::DECOR`] 层）。
///
/// 取 `1 << 63` 而不是「世界格数」这类随世界大小变化的值：号段起点必须
/// 是编译期常量，否则「两个号段有没有可能重叠」这个问题的答案会依赖
/// 运行期的世界尺寸，没法在这里一眼断言。`ll_world::core::TorusSize::MAX_EXTENT`
/// 是 `i32::MAX / 2`，最大世界的格数上限约 `2^60`，仍远低于 `2^63`。
pub const PLACED_FURNITURE_ENTITY_BASE: u64 = 1 << 63;

/// 树木绘制顺序号的起始偏移（[`Layer::TERRAIN`] 层）。
///
/// # 为什么树在 `TERRAIN` 层而不是 `DECOR` 层
///
/// 树是**长在地上的**，掉在树下的东西、摆在树旁的家具、站在树前的人
/// 都该盖在它前面。放进 `DECOR` 会让树与地面物品堆争同一层，而
/// [`ll_render::sprite::DrawOrder`] 在同层同脚底纵坐标时比的是序号
/// ——那就要在两个号段之间做一次没有语义的大小约定。放 `TERRAIN` 层
/// 直接由「层」表达这件事：树恒在地形之上、恒在其余一切之下。
///
/// 取 `1 << 62` 而不是「世界格数」这类随世界大小变化的值，理由与
/// [`PLACED_FURNITURE_ENTITY_BASE`] 逐字相同（号段起点必须是编译期
/// 常量）。地形自己的号段是 `[1, 世界格数]`，最大世界的格数上限约
/// `2^60`（见那一段推导），因此本号段与它**不可能相撞**。
pub const TREE_ENTITY_BASE: u64 = 1 << 62;

/// NPC 绘制顺序号的起始偏移（[`Layer::ENTITY`] 层）。
///
/// 从 1 起，把 0 让给 [`PLAYER_ENTITY`]：NPC 的序号取
/// `NPC_ENTITY_BASE + 槽位下标`，槽位下标从 0 开始，因此 NPC 永远不会
/// 拿到玩家那个号。
pub const NPC_ENTITY_BASE: u64 = 1;

/// 地面物品堆那一个「团」的图集键。**恒定这一个**，见模块文档所有者
/// 裁定一节。
pub const GROUND_PILE_SPRITE: &str = "lostland:ground_pile";

/// 放置家具查不到内容自带贴图时的通用记号。
pub const PLACED_FURNITURE_SPRITE: &str = "lostland:furniture_placed";

/// NPC 查不到种族自带贴图时的通用记号。
pub const NPC_SPRITE: &str = "lostland:npc_idle_0";

/// 树木贴图键的公共前缀（命名空间 + `tree_`）。
///
/// **唯一真相源**：[`tree_sprite_key`] 拼它，
/// `crates/ll-game/tests/atlas_coverage.rs` 的反向锁
/// （`图集里不许有声明侧数不出来的树贴图`）也扫它——判据放在生产代码
/// 这一侧而不是在测试里另抄一份字符串，理由与该文件模块文档反复写的
/// 那条一样：**凡是把真相源之外的副本当判据，迟早分叉，而分叉时没有
/// 任何东西会报错**。
pub const TREE_SPRITE_PREFIX: &str = "lostland:tree_";

/// 一种树的图集键。
///
/// 拼的是命名空间前缀 + [`TreeSpecies::sprite_stem`]——后者与
/// `tools/ll-artgen` 那三条配方的名字、与 `draw_entry` 的三支派发逐字
/// 一致。运行期真正被查的键是**带前缀的**（`ll_mod::asset_vfs` 把清单
/// 条目名与所属命名空间拼起来），上一批五张 HUD 贴图正是栽在「查裸
/// 名字、图集里存的是带前缀的」这一步上，五张全部静默失效、不打任何
/// 日志。
///
/// 与 [`GROUND_PILE_SPRITE`]/[`NPC_SPRITE`] 同一档：命名空间写死成
/// `lostland:`。树不是内容表里的一条（一百万棵以上，它们是派生出来
/// 的，见 `ll_world::tree` 模块文档），因此没有 `registry.resolve` 那
/// 条路可走。
pub fn tree_sprite_key(species: TreeSpecies) -> String {
    format!(
        "{TREE_SPRITE_PREFIX}{}",
        species.sprite_stem().trim_start_matches("tree_")
    )
}

/// NPC 职业挂件绘制顺序号的起始偏移（[`Layer::ENTITY`] 层）。
///
/// 取 `1 << 63` 的理由与 [`PLACED_FURNITURE_ENTITY_BASE`] 逐字相同
/// （号段起点必须是编译期常量），另外还多担一件事：**挂件必须画在身子
/// 之上**。同层同脚底纵坐标时 [`ll_render::sprite::DrawOrder`] 比的正是
/// 这个号，号大的后画、后画的盖在上面；身子的号是
/// `NPC_ENTITY_BASE + 槽位下标`，槽位下标上限远低于 `2^60`（见
/// [`PLACED_FURNITURE_ENTITY_BASE`] 的同一段推导），因此任何挂件的号都
/// 严格大于任何身子的号。
pub const NPC_BADGE_ENTITY_BASE: u64 = 1 << 63;

/// 一条地表内容的绘制指令。
///
/// 刻意**不含屏幕坐标、不含 tint、不含 zoom**：那些要么依赖相机、要么
/// 依赖当前光照，都属于 `render_surface` 那一侧的事。本类型只回答三个
/// 与 GPU 无关的问题——画在世界的哪一格、用哪个图集键（含兜底）、在
/// 绘制顺序里排第几。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDraw {
    /// 画在世界的哪一格。
    pub pos: TorusPos,
    /// 落在哪一层。
    pub layer: Layer,
    /// [`ll_render::sprite::DrawOrder`] 的最后一级比较键，号段见模块
    /// 文档「确定性」一节。
    pub entity: u64,
    /// 内容自己声明的图集键，**按优先级从高到低排列**——通常是内容的
    /// 完整命名空间 ID。空列表表示这类内容**不允许**被内容覆盖（目前
    /// 只有地面物品堆，见模块文档）。
    ///
    /// # 为什么是一串而不是一个
    ///
    /// NPC 的身子层有三个候选（`<种族>_<职业>_<性别>` →
    /// `<种族>_<职业>` → `<种族>`，见模块文档「合成图的回退链」一节）。
    /// 其余两类内容各只有零个或一个候选，语义因此**一个字没变**：
    /// [`SurfaceDraw::keys`] 依旧是「按次序取第一个查得到的」。
    pub preferred_keys: Vec<String>,
    /// 这一层被**哪些键压制**：其中任何一个在图集里查得到，本条指令就
    /// 整个不画。空列表表示没有任何东西能压制它。
    ///
    /// # 它防的是什么
    ///
    /// 职业挂件层。身子层一旦落到合成图（`<种族>_<职业>[_<性别>]`），
    /// 职业信息就已经画在身子上了，挂件再叠一层等于同一件事画两遍。
    /// 美术批次落地之后本体九族 × 十三职业全部命中合成图，这个字段对
    /// 本体 NPC **恒生效**；没有合成图的 mod 内容仍然走分层合成，那时
    /// 它恒不命中。见模块文档「合成图的回退链」一节。
    ///
    /// # 为什么只声明键，不在这里查图
    ///
    /// 本模块是**纯计算**（模块文档第一句），拿不到图集。「查不查得到」
    /// 由唯一的消费点 `crate::app::push_surface_draw` 回答，与
    /// [`Self::keys`] 那条查图次序落在同一处，不散成两份。
    pub superseded_by: Vec<String>,
    /// 优先键查不到时的通用记号。`None` 表示这一层**本来就是可选的**
    /// ——查不到就整条指令不画，而不是退到某张兜底图。目前只有 NPC 的
    /// 职业挂件是这一种：没有为某个职业准备挂件贴图是正常状态（mod 新
    /// 注册一个职业时的默认状态就是这样），画一张「通用职业记号」反而
    /// 会让所有没画过的职业看起来是同一个职业。
    pub fallback_key: Option<&'static str>,
}

impl SurfaceDraw {
    /// 按优先级列出该依次尝试的图集键。
    ///
    /// 消费方（`render_surface`）应当取**第一个在图集里查得到**的那个。
    /// 把「次序」收在这里而不是让每个消费方自己写 `match`，是模块文档
    /// 「不许把同一段查图逻辑抄三遍」那条的落点。
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.preferred_keys
            .iter()
            .map(String::as_str)
            .chain(self.fallback_key)
    }
}

/// 本帧地表上除地形与玩家之外该画的全部东西，已按 `(层, 号段)` 排好。
///
/// 返回的次序就是**建议的 push 次序**：地面物品堆 → 放置家具 → NPC。
/// 真正的遮挡排序由 [`ll_render::sprite::DrawOrder`] 在批次里做（先比
/// 层、再比脚底屏幕纵坐标、最后比 [`SurfaceDraw::entity`]），本函数的
/// 次序只保证「同样的世界状态产出同样的序列」。
///
/// `player` 是玩家实体——它由 `render_surface` 单独画（要走当前动画帧，
/// 见 `crate::animation`），在这里必须跳过，否则玩家会被画两次：一次
/// 是动画帧，一次是通用 NPC 记号。
pub fn surface_draws(
    world: &WorldState,
    registry: &Registry,
    player: EntityId,
) -> Vec<SurfaceDraw> {
    let mut draws = ground_pile_draws(world);
    draws.extend(placed_furniture_draws(world, registry));
    draws.extend(npc_draws(world, registry, player));
    draws
}

/// 给定的这批格子里，每一格**现在真的长着树**的产出一条指令。
///
/// # 为什么入参是一串格子，而不是像其余三类那样扫世界状态
///
/// 因为树**不在世界状态里**。一百万棵以上的树由地形 + 确定性噪声现算
/// （`ll_world::tree::derived_tree_at`），世界状态里只有被玩家动过的那
/// 一小撮偏差记录——「扫一遍 `world` 把树列出来」这件事从定义上就做不
/// 到，也不该做（那等于把一百万棵树物化一遍）。
///
/// 调用方给出「这一帧要画哪些格」，本函数逐格问一次
/// [`ll_world::tree::tree_at`]。**这是全仓库唯一一处把树变成绘制指令的
/// 地方**：生产渲染路径（`crate::app::surface`）与冻结像素基准
/// （`crates/ll-game/tests/visual_baselines.rs`）调的是同一个函数，
/// 不各写一遍（ADR 0021）。
///
/// # 确定性（约束 C5）
///
/// 产出次序 = 入参次序。调用方给的都是行主序的确定迭代（相机的
/// `visible_tiles_zoomed` 与基准测试那两重 `for`），全程不碰任何哈希
/// 容器。`entity` 取 `TREE_ENTITY_BASE + y * 宽 + x`，与地形瓦片同一套
/// 行主序编号，因此**同一格恒得同一个号**，与这一帧画了几格无关。
pub fn tree_draws(
    world: &WorldState,
    forest: TerrainKind,
    tiles: impl Iterator<Item = TorusPos>,
) -> Vec<SurfaceDraw> {
    let width = world.size.width() as u64;
    tiles
        .filter_map(|pos| {
            let tree = tree_at(world, pos, forest)?;
            Some(SurfaceDraw {
                pos,
                layer: Layer::TERRAIN,
                entity: TREE_ENTITY_BASE + pos.y() as u64 * width + pos.x() as u64,
                preferred_keys: vec![tree_sprite_key(tree.species)],
                superseded_by: Vec::new(),
                // `None`：查不到就**不画这一格的树**，地形底图照常。
                // 与 NPC 职业挂件同一档——退到一张「通用树记号」会让三
                // 种树看起来是同一种，那比不画更糟。
                fallback_key: None,
            })
        })
        .collect()
}

/// 每一格「躺着东西」的地方产出恰好一条指令。
///
/// 「躺着」= [`ll_world::item::GroundItemStack::placed`] 为假。立着的
/// 那些是家具，走 [`placed_furniture_draws`]。
pub fn ground_pile_draws(world: &WorldState) -> Vec<SurfaceDraw> {
    let width = world.size.width() as u64;
    // BTreeSet 而非 HashSet：约束 C5 禁止逻辑（这里是绘制顺序）依赖
    // 哈希容器的迭代顺序。键写成 `(y, x)` 让迭代顺序恰好是行主序，与
    // 地形瓦片的编号方式一致。
    let occupied: BTreeSet<(i32, i32)> = world
        .ground_items
        .iter()
        .filter(|ground| !ground.placed)
        .map(|ground| (ground.pos.y(), ground.pos.x()))
        .collect();

    occupied
        .into_iter()
        .map(|(y, x)| SurfaceDraw {
            pos: world.size.wrap(x, y),
            layer: Layer::DECOR,
            entity: GROUND_PILE_ENTITY_BASE + y as u64 * width + x as u64,
            // 恒定空：所有者裁定「统一用一个团」，内容不得为
            // 「地上躺着的东西」声明自己的样子。
            preferred_keys: Vec::new(),
            superseded_by: Vec::new(),
            fallback_key: Some(GROUND_PILE_SPRITE),
        })
        .collect()
}

/// 每一件立着的家具产出一条指令，优先用这件物品自己的贴图。
pub fn placed_furniture_draws(world: &WorldState, registry: &Registry) -> Vec<SurfaceDraw> {
    world
        .ground_items
        .iter()
        .enumerate()
        .filter(|(_, ground)| ground.placed)
        .map(|(index, ground)| SurfaceDraw {
            pos: ground.pos,
            layer: Layer::DECOR,
            entity: PLACED_FURNITURE_ENTITY_BASE + index as u64,
            preferred_keys: registry
                .resolve(ground.stack.def)
                .map(|id| id.to_string())
                .into_iter()
                .collect(),
            superseded_by: Vec::new(),
            fallback_key: Some(PLACED_FURNITURE_SPRITE),
        })
        .collect()
}

/// 除玩家之外的每个存活角色产出**两条**指令：先身子（查种族 ID，兜底
/// [`NPC_SPRITE`]），再职业挂件（查职业 ID，查不到就不画）。
///
/// 为什么是两条而不是一张「种族×职业」的合成图，见模块文档
/// 「NPC 为什么是两条指令」一节。
///
/// # 两个字段各自的真相源
///
/// 身子查的是 `ll_world::entity::Agent::race`，挂件查的是
/// `Agent::profession`——后者是所有者裁定的职业唯一真相源。两者都已经
/// 在 `Agent` 上，本函数不需要任何新的数据通路：`WorldState::actors`
/// 里的 `Agent` 本来就是整只结构体，渲染层拿得到 `race` 就同样拿得到
/// `profession`。
///
/// # 引擎里没有任何一处按种族/职业 id 分支
///
/// 两条指令的键都只是「把 [`ContentIndex`] 翻回它注册时的完整命名空间
/// 字符串」（[`Registry::resolve`]），没有 `match "lostland:dwarf"` 这
/// 种东西。新增一个种族或职业，本文件一个字都不用改——内容声明它，
/// 往自己的 `assets/sprites/` 里放一张同名图，就画出来了。
///
/// [`ContentIndex`]: ll_core::ident::ContentIndex
/// [`Registry::resolve`]: ll_mod::registry::Registry::resolve
pub fn npc_draws(world: &WorldState, registry: &Registry, player: EntityId) -> Vec<SurfaceDraw> {
    world
        .actors
        .iter_with_id()
        .filter(|(id, _)| *id != player)
        .flat_map(|(id, agent)| {
            let slot = id.index() as u64;
            let race_key = registry.resolve(agent.race).map(|id| id.to_string());
            let class_key = registry.resolve(agent.profession).map(|id| id.to_string());
            let composites =
                composite_keys(race_key.as_deref(), class_key.as_deref(), agent.gender);
            // 身子层：合成图优先，退到分层的种族身子，最后退到通用记号。
            let mut body_keys = composites.clone();
            body_keys.extend(race_key);
            [
                SurfaceDraw {
                    pos: agent.pos,
                    layer: Layer::ENTITY,
                    entity: NPC_ENTITY_BASE + slot,
                    preferred_keys: body_keys,
                    superseded_by: Vec::new(),
                    fallback_key: Some(NPC_SPRITE),
                },
                SurfaceDraw {
                    pos: agent.pos,
                    layer: Layer::ENTITY,
                    entity: NPC_BADGE_ENTITY_BASE + slot,
                    preferred_keys: class_key.into_iter().collect(),
                    // 身子层一旦落到合成图，职业已经画在身子上了，这一层
                    // 必须让位——见模块文档「命中合成图时，职业挂件那一
                    // 层必须让位」一节。今天零张合成图，因此恒不生效。
                    superseded_by: composites,
                    // 没有挂件贴图就不画这一层，见 `fallback_key` 字段
                    // 文档。
                    fallback_key: None,
                },
            ]
        })
        .collect()
}

/// 「种族 × 职业」合成图的候选键，按优先级从高到低。
///
/// 两段：`<种族>_<职业>_<性别>`（今天零张，槽位留给以后）与
/// `<种族>_<职业>`（美术批次的 117 张，本体 9 族 × 13 职业；mod 内容
/// 没有就自动落到分层合成）。种族或职业任一解析不出完整命名空间 ID 时返回
/// 空列表——**没有半个键这种东西**：拿 `#103` 之类的裸索引去拼键，
/// 拼出来的是一个永远查不到、还会误导后来人的字符串。
///
/// # 键的形状：种族 ID 原样保留，职业 ID 压平接在后面
///
/// `lostland:human` + `lostland:blacksmith` →
/// `lostland:human_lostland_blacksmith`。
///
/// 这个形状不是审美选择，是**图集条目名的形状逼出来的**。
/// `ll_mod::asset_vfs::ResolvedSprite::atlas_name` 恒等于
/// `"{命名空间}:{清单条目名}"`（见那份字段文档「本体也加前缀」一节），
/// 因此任何查得到的键**必然恰好含一个冒号，且冒号左边是一个真实存在
/// 的命名空间**。本函数在合成图批次开工前返回的是
/// `lostland_human_lostland_blacksmith`——**一个冒号都没有，图集里
/// 永远查不到**。而回退链查不到只会静默退回分层合成、**不打任何日志**
/// （`GpuResources::lookup_first` 只对最后一个候选打 error），于是 52
/// 张合成图会一张都用不上、屏幕上毫无异常、无人察觉。这与 `skin.rs`
/// 查裸名字导致五张 HUD 贴图全军覆没是逐字同型的失效方式。
///
/// 于是：**冒号那一位留给种族**（合成图归种族所属的命名空间——加一个
/// 种族的 mod 才是那一族 13 张图的作者），职业那一段按
/// `atlas_name` 既有的「命名空间与本地名之间用下划线」约定压平接在
/// 后面。带性别时再接一段：
/// `lostland:human_lostland_blacksmith_female`。
///
/// 端到端防线在 `crates/ll-game/tests/npc_appearance.rs` 的
/// `本体每一个种族与职业的组合在真实图集里都查得到合成图`：它用真实
/// `assets/` + `mods/` 打出真实图集，逐个组合断言本函数拼出的键真的
/// 查得到。名字差一个字符它当场红。
///
/// # 为什么性别用 [`Gender::sprite_tag`] 而不是展示名
///
/// 展示名随语言变（「女性」/`Female`），资产文件名不能随语言变——同
/// `ll_platform::config::NewGameConfig::terrain_preset`「存标识而不是
/// 译名」那条既有理由。
fn composite_keys(race: Option<&str>, class: Option<&str>, gender: Gender) -> Vec<String> {
    let (Some(race), Some(class)) = (race, class) else {
        return Vec::new();
    };
    let base = format!("{race}_{}", sprite_segment(class));
    vec![format!("{base}_{}", gender.sprite_tag()), base]
}

/// 把一个完整命名空间 ID 变成可以出现在文件名里的一段——冒号换下划线。
///
/// 与 `ll_mod::asset_vfs` 那侧的约定一致，见 [`composite_keys`] 文档
/// 「键的形状」一节。**只用在职业那一段**：种族那一段要原样保留自己的
/// 冒号，否则拼出来的键在图集里恒查不到。
fn sprite_segment(id: &str) -> String {
    id.replace(':', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll_core::ident::{ContentIndex, NamespacedId};
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_sim::item::ItemStack;
    use ll_world::generate::GenParams;
    use ll_world::item::GroundItemStack;
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    /// 一个只够本模块用的最小世界：构造方式逐字取自
    /// `ll_world::state` 的 `test_world`，本模块只往里塞 `ground_items`
    /// 与 `actors`，其余字段一概不碰。
    fn empty_world() -> WorldState {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(5, 5);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn at(world: &WorldState, x: i32, y: i32) -> TorusPos {
        world.size.wrap(x, y)
    }

    /// 一个最朴素的测试用 `Agent`——种族/职业/性别由调用方按需改写。
    fn agent_at(pos: TorusPos, zone: ll_world::space::ZoneCoord) -> ll_world::entity::Agent {
        use ll_world::entity::{Agent, BaseStats};
        Agent {
            gender: Gender::default(),
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession: ContentIndex::default(),
            goals: Vec::new(),
            race: ContentIndex::default(),
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
            current_space: ll_world::space::Space::surface(zone, ContentIndex::default()),
            mod_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: Tick(0),
            remembered_id: None,
            level: Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
            home: None,
        }
    }

    /// 注册一条内容并拿到它的索引——`ContentIndex` 没有公开构造函数
    /// （索引只能来自 [`Registry`] 的 intern，见其类型文档），因此测试
    /// 也走同一条路。
    fn intern(registry: &mut Registry, id: &str) -> ContentIndex {
        registry.intern(NamespacedId::parse(id).expect("字面量合法"))
    }

    fn ground(pos: TorusPos, def: ContentIndex, placed: bool) -> GroundItemStack {
        GroundItemStack {
            pos,
            stack: ItemStack::new(def, 1),
            dropped_at: Tick(0),
            contents: Vec::new(),
            placed,
        }
    }

    #[test]
    fn 同一格躺着多件东西只画一个团() {
        // Arrange：一格上堆三件不同的东西。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let pos = at(&world, 3, 4);
        for name in ["testmod:a", "testmod:b", "testmod:c"] {
            let def = intern(&mut registry, name);
            world.ground_items.push(ground(pos, def, false));
        }

        // Act
        let draws = ground_pile_draws(&world);

        // Assert：所有者裁定「无论是一个还是N个都统一用一个团」。
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].pos, pos);
        assert_eq!(
            draws[0].keys().collect::<Vec<_>>(),
            vec![GROUND_PILE_SPRITE]
        );
    }

    #[test]
    fn 立着的家具不算进地面物品堆() {
        // Arrange：同一格上一件立着的家具，别无他物。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let def = intern(&mut registry, "testmod:anvil");
        let pos = at(&world, 5, 6);
        world.ground_items.push(ground(pos, def, true));

        // Act
        let piles = ground_pile_draws(&world);

        // Assert：立着的东西是家具，不该同时又画一个「地上躺着东西」的团。
        assert!(piles.is_empty(), "立着的家具不该产出地面物品堆指令");
    }

    #[test]
    fn 地面物品堆按行主序排且与推入顺序无关() {
        // Arrange：刻意按「后面的行先塞」的顺序推进去。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let far = at(&world, 1, 9);
        let near = at(&world, 7, 2);
        let a = intern(&mut registry, "testmod:a");
        let b = intern(&mut registry, "testmod:b");
        world.ground_items.push(ground(far, a, false));
        world.ground_items.push(ground(near, b, false));

        // Act
        let draws = ground_pile_draws(&world);

        // Assert：行主序（y 小的在前），与推入顺序无关——约束 C5。
        let positions: Vec<TorusPos> = draws.iter().map(|draw| draw.pos).collect();
        assert_eq!(positions, vec![near, far]);
        assert!(draws[0].entity < draws[1].entity);
    }

    #[test]
    fn 家具优先用物品自己的完整id当图集键兜底才是通用记号() {
        // Arrange：注册一件物品，摆一件立着的。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let def = intern(&mut registry, "testmod:anvil");
        let pos = at(&world, 2, 2);
        world.ground_items.push(ground(pos, def, true));

        // Act
        let draws = placed_furniture_draws(&world, &registry);

        // Assert：先试内容自己的 ID，查不到才退到通用记号——引擎里
        // 没有任何一处按 id 分支。
        assert_eq!(draws.len(), 1);
        assert_eq!(
            draws[0].keys().collect::<Vec<_>>(),
            vec!["testmod:anvil", PLACED_FURNITURE_SPRITE]
        );
    }

    #[test]
    fn 同一件物品躺着时不给内容留覆盖余地() {
        // Arrange：与上一条同一件物品，但这次是躺着的。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let def = intern(&mut registry, "testmod:anvil");
        let pos = at(&world, 2, 2);
        world.ground_items.push(ground(pos, def, false));

        // Act
        let draws = ground_pile_draws(&world);

        // Assert：与上一条形成对照——同一件物品，立着时用自己的图，
        // 躺着时只能是那个团。这正是所有者裁定的两侧。
        assert!(draws[0].preferred_keys.is_empty());
        assert_eq!(
            draws[0].keys().collect::<Vec<_>>(),
            vec![GROUND_PILE_SPRITE]
        );
    }

    #[test]
    fn 地面物品堆与家具的绘制序号号段不重叠() {
        // Arrange：一格躺着东西、一格立着东西，两者同在 DECOR 层。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let a = intern(&mut registry, "testmod:a");
        let b = intern(&mut registry, "testmod:b");
        let far = at(&world, 63, 63);
        let near = at(&world, 0, 0);
        world.ground_items.push(ground(far, a, false));
        world.ground_items.push(ground(near, b, true));

        // Act
        let piles = ground_pile_draws(&world);
        let furniture = placed_furniture_draws(&world, &registry);

        // Assert：同层内部序号必须互不相撞，否则同一格上「团」与「家具」
        // 的前后顺序会变成未定义的。
        assert_eq!(piles[0].layer, furniture[0].layer);
        assert!(piles[0].entity < furniture[0].entity);
    }

    #[test]
    fn 身子层的候选键恰好是三段回退链且次序正确() {
        // 所有者裁定的回退链（见模块文档「合成图的回退链」一节）：
        // <种族>_<职业>_<性别> → <种族>_<职业> → <种族> → 通用记号。
        // Arrange
        let mut world = empty_world();
        let mut registry = Registry::new();
        let race = intern(&mut registry, "lostland:human");
        let class = intern(&mut registry, "lostland:blacksmith");
        let pos = at(&world, 3, 3);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let player = world.actors.spawn(agent_at(pos, zone));
        let mut npc = agent_at(pos, zone);
        npc.race = race;
        npc.profession = class;
        npc.gender = Gender::Female;
        world.actors.spawn(npc);

        // Act
        let draws = npc_draws(&world, &registry, player);
        let body = draws
            .iter()
            .find(|draw| draw.entity < NPC_BADGE_ENTITY_BASE)
            .expect("身子层必然产出一条");

        // Assert
        assert_eq!(
            body.keys().collect::<Vec<_>>(),
            vec![
                "lostland:human_lostland_blacksmith_female",
                "lostland:human_lostland_blacksmith",
                "lostland:human",
                NPC_SPRITE,
            ]
        );
    }

    #[test]
    fn 性别不同时身子层的第一个候选键跟着不同() {
        // 这一条是 `Agent::gender` 那条门禁豁免（「渲染层今天就在读它」）
        // 的直接证据：改一个实体的性别，渲染层算出来的候选键真的变了。
        // Arrange
        let mut world = empty_world();
        let mut registry = Registry::new();
        let race = intern(&mut registry, "lostland:human");
        let class = intern(&mut registry, "lostland:blacksmith");
        let pos = at(&world, 3, 3);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let _ = &mut world;

        let first_key_of = |gender: Gender| {
            let mut world = empty_world();
            let player = world.actors.spawn(agent_at(pos, zone));
            let mut npc = agent_at(pos, zone);
            npc.race = race;
            npc.profession = class;
            npc.gender = gender;
            world.actors.spawn(npc);
            let draws = npc_draws(&world, &registry, player);
            draws
                .iter()
                .find(|draw| draw.entity < NPC_BADGE_ENTITY_BASE)
                .and_then(|draw| draw.keys().next().map(str::to_string))
                .expect("身子层必然有第一个候选")
        };

        // Act / Assert
        assert_ne!(
            first_key_of(Gender::Male),
            first_key_of(Gender::Female),
            "两个性别算出了同一个精灵键——性别没有真的参与查图"
        );
    }

    #[test]
    fn 挂件层被两段合成图压制而身子层不被压制() {
        // 合成图里职业已经画在身子上，挂件必须让位，否则同一件事画两遍。
        // Arrange
        let mut world = empty_world();
        let mut registry = Registry::new();
        let race = intern(&mut registry, "lostland:human");
        let class = intern(&mut registry, "lostland:blacksmith");
        let pos = at(&world, 3, 3);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        let player = world.actors.spawn(agent_at(pos, zone));
        let mut npc = agent_at(pos, zone);
        npc.race = race;
        npc.profession = class;
        world.actors.spawn(npc);

        // Act
        let draws = npc_draws(&world, &registry, player);
        let body = draws
            .iter()
            .find(|draw| draw.entity < NPC_BADGE_ENTITY_BASE)
            .expect("身子层必然产出一条");
        let badge = draws
            .iter()
            .find(|draw| draw.entity >= NPC_BADGE_ENTITY_BASE)
            .expect("挂件层必然产出一条");

        // Assert
        assert!(
            body.superseded_by.is_empty(),
            "身子层不该被任何东西压制——它自己就是那条回退链"
        );
        assert_eq!(
            badge.superseded_by,
            vec![
                "lostland:human_lostland_blacksmith_male".to_string(),
                "lostland:human_lostland_blacksmith".to_string(),
            ],
            "挂件层该被两段合成图压制"
        );
    }

    #[test]
    fn 种族或职业解析不出id时没有半个合成键() {
        // 拿 `#103` 之类的裸索引拼键，拼出来的是一个永远查不到、还会
        // 误导后来人的字符串。宁可整段不产出。
        // Arrange
        let mut registry = Registry::new();
        let race = intern(&mut registry, "lostland:human");
        let race_id = registry.resolve(race).map(|id| id.to_string());

        // Act / Assert
        assert!(composite_keys(race_id.as_deref(), None, Gender::Male).is_empty());
        assert!(composite_keys(None, race_id.as_deref(), Gender::Male).is_empty());
        assert_eq!(
            composite_keys(race_id.as_deref(), race_id.as_deref(), Gender::Male).len(),
            2
        );
    }

    #[test]
    fn 每个合成键都恰好含一个冒号且冒号左边是种族的命名空间() {
        // 这条守的是本文件历史上最贵的一个字符：图集条目名恒是
        // `"{命名空间}:{条目名}"`，因此**不含冒号的候选键在图集里永远
        // 查不到**，而回退链查不到只会静默退回分层合成、不打任何日志。
        // 合成图批次开工时本函数返回的正是不含冒号的串，117 张图会一张
        // 都用不上且无人察觉。判据写成「形状」而不是「等于某个字面量」，
        // 是为了让它对任意 mod 的种族/职业都成立。
        //
        // 反例（本次开发实跑）：把 `composite_keys` 里的 `{race}` 改回
        // `sprite_segment(race)`，本条报「候选键 … 一个冒号都没有」。
        // Arrange
        let mut registry = Registry::new();
        let race = intern(&mut registry, "othermod:half_elf");
        let class = intern(&mut registry, "lostland:blacksmith");
        let race_id = registry.resolve(race).map(|id| id.to_string());
        let class_id = registry.resolve(class).map(|id| id.to_string());

        // Act
        let keys = composite_keys(race_id.as_deref(), class_id.as_deref(), Gender::Female);

        // Assert
        assert_eq!(keys.len(), 2);
        for key in &keys {
            assert_eq!(
                key.matches(':').count(),
                1,
                "候选键 {key} 的冒号数量不是 1——图集条目名恒是「命名空间:条目名」，\
                 冒号数对不上的键永远查不到，而且查不到不打任何日志"
            );
            assert!(
                key.starts_with("othermod:half_elf_"),
                "候选键 {key} 的命名空间不是种族自己的——合成图归种族所属的命名空间"
            );
        }
        assert_eq!(keys[0], "othermod:half_elf_lostland_blacksmith_female");
        assert_eq!(keys[1], "othermod:half_elf_lostland_blacksmith");
    }

    #[test]
    fn 三类指令各自落在预期的图层() {
        // Arrange：地上躺一堆、立一件家具。
        let mut world = empty_world();
        let mut registry = Registry::new();
        let a = intern(&mut registry, "testmod:a");
        let b = intern(&mut registry, "testmod:b");
        world.ground_items.push(ground(at(&world, 1, 1), a, false));
        world.ground_items.push(ground(at(&world, 2, 1), b, true));

        // Act
        let piles = ground_pile_draws(&world);
        let furniture = placed_furniture_draws(&world, &registry);

        // Assert：地面物品与家具都在地形之上、角色之下。
        assert_eq!(piles[0].layer, Layer::DECOR);
        assert_eq!(furniture[0].layer, Layer::DECOR);
        assert!(Layer::TERRAIN < Layer::DECOR);
        assert!(Layer::DECOR < Layer::ENTITY);
    }
}
