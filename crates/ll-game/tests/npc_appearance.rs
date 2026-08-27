//! 端到端验收：**不同种族、不同职业的 NPC 在屏幕上互相看得出区别**。
//!
//! # 这条验收在验什么（ADR 0018）
//!
//! 项目所有者的裁定原话是「npc 根据职业种族做出区别，多画点」，验收方式
//! 是「跑起游戏走进据点，不同种族、不同职业的 NPC 在屏幕上互相可分」。
//! 本文件是那句话的可执行版本，尺子与
//! [`atlas_coverage`](atlas_coverage.rs) 里「十七种本体地形两两之间至少
//! 四分之一像素不同」那条**是同一把**：不比「画得好不好看」，只比「两张
//! 图有没有被写成几乎一样」。
//!
//! 证据链仍然是两段，缺一不可：
//!
//! 1. **接线段**：每一个 `(种族, 职业)` 组合真的构造一个 `Agent`，走生产
//!    路径上的 [`ll_game::surface_draw::npc_draws`] 拿到它**实际会去查**
//!    的那两个图集键——不是在测试里重抄一份映射表。
//! 2. **有图段**：用同一份真实 `mods/` + `assets/`，走生产路径上的
//!    [`load_sprite_sources`] + [`pack_atlas`] 打出真实图集，把两个键
//!    对应的矩形按渲染层的层序**叠**成一张最终会出现在屏幕上的图，再逐
//!    像素两两比较。
//!
//! # 种族与职业的清单从注册表现查，不在这里抄
//!
//! [`registered_races`]/[`registered_professions`] 遍历
//! [`Registry::snapshot`]，按 `race_table`/`class_table` 的
//! `is_defined` 过滤。因此**加第 10 个种族的那一刻，这些断言自动开始
//! 管它**——不需要有人记得回来往某个数组里补一行。这正是「加种族只加
//! 数据」那条要求在测试侧的对称落点。
//!
//! # 每条断言的反例是什么（本次开发中真的逐条改坏跑过）
//!
//! | 改坏什么 | 哪条变红 |
//! | --- | --- |
//! | 删掉 `assets/sprites/dwarf.png` | `本体每一个种族在真实图集里都有自带身子贴图` |
//! | 删掉 `assets/sprites/mason.png` | `本体每一个职业在真实图集里都有自带挂件贴图` |
//! | 把 `goblin` 的肤色/发色/衣色/身高/肩宽/腿长六个参数改成与 `human` 相同 | `本体的种族与职业组合两两之间至少三十六个像素不同` 与 `同一个职业下不同种族至少差四分之一张图`（两条都报「只有 8 个像素不同」） |
//! | 把 `npc_draws` 里挂件那条的 `preferred_key` 改成 `None`，或改成查 `agent.race` | 本文件四条同时红（挂件那一层要么消失、要么与身子同图） |
//! | 删掉 `mods/example_mod/assets/sprites/half_elf.png` | `mod自己声明的种族与职业不改一行引擎代码就能有自己的样子` |
//! | 给 `mods/example_mod/assets/sprites/` 补一张 `dragonborn.png` | `没有自带贴图的种族退回通用记号而没有自带贴图的职业不画挂件` |
//!
//! 反例的实跑记录见提交信息。

use std::path::PathBuf;

use ll_core::ident::{ContentIndex, NamespacedId};
use ll_game::app::load_sprite_sources;
use ll_game::content::{LoadedContent, load_content};
use ll_game::surface_draw::{NPC_SPRITE, npc_draws};
use ll_game::world::{GameWorld, build_new_world};
use ll_render::atlas_pack::{PackedAtlas, pack_atlas};
use ll_world::entity::EntityId;

/// 仓库根——与 `atlas_coverage.rs` 的 `repo_root` 同一条推导。
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 装真实内容、建真实世界、用生产路径打出真实图集。
fn real_setup() -> (LoadedContent, GameWorld, PackedAtlas) {
    let root = repo_root();
    let content =
        load_content(&root.join("mods"), &root.join("assets")).expect("真实 mods/ 应当装得起来");
    let world = build_new_world(
        &content,
        ll_world::generate::GenParams {
            seed: 20260827,
            ..ll_world::generate::GenParams::default()
        },
    )
    .expect("默认参数应当建得出世界");
    let sources = load_sprite_sources(&content.asset_vfs);
    assert!(!sources.is_empty(), "真实资产目录里应当至少读得到一张精灵");
    let atlas = pack_atlas(&sources);
    (content, world, atlas)
}

/// 本体（基础 mod）的命名空间。**从本体自己的内容身上现取**，不写字面
/// 量：本体命名空间万一改名，这里跟着变，不会留下一处永远查不到东西的
/// 硬编码字符串。
fn base_namespace(content: &LoadedContent) -> String {
    content
        .registry
        .resolve(content.race_ids.human)
        .expect("本体人类必然已注册")
        .namespace()
        .to_string()
}

/// 注册表里全部**已定义属性**的种族，按注册顺序（[`Registry::snapshot`]
/// 是 `Vec`，不经任何哈希容器——约束 C5）。
fn registered_races(content: &LoadedContent) -> Vec<(NamespacedId, ContentIndex)> {
    content
        .registry
        .snapshot()
        .into_iter()
        .filter_map(|id| {
            let index = content.registry.get(&id)?;
            content.race_table.is_defined(index).then_some((id, index))
        })
        .collect()
}

/// 注册表里全部已定义属性的职业，理由同 [`registered_races`]。
fn registered_professions(content: &LoadedContent) -> Vec<(NamespacedId, ContentIndex)> {
    content
        .registry
        .snapshot()
        .into_iter()
        .filter_map(|id| {
            let index = content.registry.get(&id)?;
            content.class_table.is_defined(index).then_some((id, index))
        })
        .collect()
}

/// 一张最终会出现在屏幕上的 NPC 图：种族身子 + 职业挂件叠好之后的像素。
type Composite = Vec<[u8; 4]>;

/// 图集里这个条目对应矩形的像素，按行主序。
fn entry_pixels(atlas: &PackedAtlas, name: &str) -> (u16, u16, Vec<[u8; 4]>) {
    let entry = atlas
        .metadata
        .lookup(name)
        .unwrap_or_else(|| panic!("图集里查不到条目 {name}"));
    let rect = entry.rect;
    let mut pixels = Vec::with_capacity(usize::from(rect.width) * usize::from(rect.height));
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            pixels.push(atlas.canvas.get_pixel(u32::from(x), u32::from(y)).0);
        }
    }
    (rect.width, rect.height, pixels)
}

/// 把一个 NPC 的若干条绘制指令按**给定顺序**叠成一张图。
///
/// 叠法是最朴素的 source-over：上一层不透明就盖住下一层。本工具的贴图
/// alpha 只有 0 与 255 两种取值（见 `ll-artgen`），因此不需要做真正的
/// 混合运算——这一点由 `挂件除胸口徽记外全部透明` 在生成侧钉住。
///
/// 顺序**不是**本函数自己排的：调用方按 [`npc_draws`] 产出的
/// `SurfaceDraw::entity` 升序传进来，那正是 `ll_render::sprite::DrawOrder`
/// 在同层同脚底纵坐标时用的比较键。
fn composite(atlas: &PackedAtlas, keys: &[String]) -> Composite {
    let mut layers = keys.iter().map(|key| entry_pixels(atlas, key));
    let (width, height, mut base) = layers.next().expect("至少有身子那一层");
    for (w, h, pixels) in layers {
        assert_eq!(
            (w, h),
            (width, height),
            "叠加层与身子尺寸不一致——两者必须同尺寸同 pivot 才谈得上像素级对齐"
        );
        for (slot, pixel) in base.iter_mut().zip(pixels) {
            if pixel[3] > 0 {
                *slot = pixel;
            }
        }
    }
    base
}

/// 造一个种族/职业指定的 NPC，塞进世界，返回它的槽位。
///
/// 从玩家身上克隆一份再改两个字段：`Agent` 字段很多，逐个填一遍会让这个
/// 测试跟着 `Agent` 的演化一起腐烂，而本文件关心的只有 `race` 与
/// `profession` 两个字段。
fn spawn_npc(
    world: &mut GameWorld,
    race: ContentIndex,
    profession: ContentIndex,
    offset: i32,
) -> EntityId {
    let mut agent = world
        .world
        .actors
        .get(world.player)
        .expect("玩家必然存在")
        .clone();
    let pos = agent.pos;
    agent.pos = world.world.size.wrap(pos.x() + offset, pos.y());
    agent.race = race;
    agent.profession = profession;
    world.world.actors.spawn(agent)
}

/// 这个 NPC 会画出来的那张图——走生产路径的 [`npc_draws`] 取键，按绘制
/// 顺序号升序叠。
///
/// 「查不到就跳过」与生产路径上的 `push_surface_draw` 逐字一致：那里
/// `lookup_first` 返回 `None` 时直接 `return`，屏幕上这一层就是不画。
fn npc_composite(
    content: &LoadedContent,
    world: &GameWorld,
    atlas: &PackedAtlas,
    npc: EntityId,
) -> Composite {
    let mut mine: Vec<_> = npc_draws(&world.world, &content.registry, world.player)
        .into_iter()
        .filter(|draw| {
            draw.entity == ll_game::surface_draw::NPC_ENTITY_BASE + u64::from(npc.index())
                || draw.entity
                    == ll_game::surface_draw::NPC_BADGE_ENTITY_BASE + u64::from(npc.index())
        })
        .collect();
    mine.sort_by_key(|draw| draw.entity);
    let keys: Vec<String> = mine
        .iter()
        .filter_map(|draw| {
            draw.keys()
                .find(|name| atlas.metadata.lookup(name).is_some())
                .map(str::to_string)
        })
        .collect();
    assert!(!keys.is_empty(), "至少身子那一层必须查得到（兜底记号保证）");
    composite(atlas, &keys)
}

/// 两张图有多少个像素不同。
fn differing(a: &Composite, b: &Composite) -> usize {
    assert_eq!(a.len(), b.len(), "两张 NPC 图尺寸不一致，无法逐像素比较");
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

/// 「同一个种族、不同职业」这一对之间**至少**该差多少个像素：整块胸口
/// 徽记。
///
/// 6×6 = 36。为什么不能取更大的数：同种族不同职业时，两张图的差异**只
/// 可能**来自那块徽记（身子完全一样），因此 36 是这一对能达到的上界，
/// 也正好是设计上的下界——底板色与笔画色两两都不同，见
/// `tools/ll-artgen/src/npc.rs` 的 `十三个职业挂件两两之间每一个像素都不同`。
const BADGE_PIXELS: usize = 36;

/// 「同一个职业、不同种族」这一对之间至少该差多少：整张图的四分之一。
/// 16×24 = 384，四分之一是 96——与 `atlas_coverage.rs` 那条地形断言同
/// 一把尺子。
const BODY_PIXELS: usize = 96;

#[test]
fn 本体每一个种族在真实图集里都有自带身子贴图() {
    // 「有兜底记号」不等于「画出来了」：本体种族全部退回同一张
    // `npc_idle_0` 正是所有者报的原始现象——「所有 NPC 长得一模一样」。
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let base = base_namespace(&content);

    // Act & Assert
    for (index, (id, race)) in registered_races(&content).into_iter().enumerate() {
        if id.namespace() != base {
            continue;
        }
        let npc = spawn_npc(
            &mut world,
            race,
            content.class_ids.warrior,
            index as i32 + 1,
        );
        let draws = npc_draws(&world.world, &content.registry, world.player);
        let body = draws
            .iter()
            .find(|draw| {
                draw.entity == ll_game::surface_draw::NPC_ENTITY_BASE + u64::from(npc.index())
            })
            .expect("刚 spawn 的 NPC 必然有身子那一条指令");
        let chosen = body
            .keys()
            .find(|name| atlas.metadata.lookup(name).is_some())
            .expect("至少兜底记号查得到");
        assert_eq!(
            chosen,
            id.to_string(),
            "本体种族 {id} 没有自带身子贴图，退回了通用记号 {NPC_SPRITE}——\
             屏幕上它会和别的没图的种族长得一模一样"
        );
    }
}

#[test]
fn 本体每一个职业在真实图集里都有自带挂件贴图() {
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let base = base_namespace(&content);

    // Act & Assert
    for (index, (id, profession)) in registered_professions(&content).into_iter().enumerate() {
        if id.namespace() != base {
            continue;
        }
        let npc = spawn_npc(
            &mut world,
            content.race_ids.human,
            profession,
            index as i32 + 1,
        );
        let draws = npc_draws(&world.world, &content.registry, world.player);
        let badge = draws
            .iter()
            .find(|draw| {
                draw.entity == ll_game::surface_draw::NPC_BADGE_ENTITY_BASE + u64::from(npc.index())
            })
            .expect("刚 spawn 的 NPC 必然有挂件那一条指令");
        let chosen = badge
            .keys()
            .find(|name| atlas.metadata.lookup(name).is_some());
        assert_eq!(
            chosen,
            Some(id.to_string().as_str()),
            "本体职业 {id} 没有自带挂件贴图——这个职业的 NPC 会和同种族的\
             其他没图职业长得一模一样"
        );
    }
}

#[test]
fn 本体的种族与职业组合两两之间至少三十六个像素不同() {
    // 这是所有者那句「不同种族、不同职业的 NPC 在屏幕上互相可分」最直接
    // 的可执行版本：本体 4 × 13 = 52 种组合，两两比一遍（1326 对）。
    //
    // 门槛取 [`BADGE_PIXELS`] 而不是 [`BODY_PIXELS`]：同种族不同职业那
    // 些对之间的差异**只可能**来自胸口那块徽记，取更大的数等于要求
    // 「换个职业连体型也变」，那不是本批次的设计（见
    // `ll_game::surface_draw` 模块文档）。跨种族那一侧另有更严的
    // `同一个职业下不同种族至少差四分之一张图` 盯着。
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let base = base_namespace(&content);
    let races: Vec<_> = registered_races(&content)
        .into_iter()
        .filter(|(id, _)| id.namespace() == base)
        .collect();
    let professions: Vec<_> = registered_professions(&content)
        .into_iter()
        .filter(|(id, _)| id.namespace() == base)
        .collect();
    assert!(
        races.len() >= 4 && professions.len() >= 13,
        "本体应当有 4 个种族与 13 个职业"
    );

    let mut rendered: Vec<(String, Composite)> = Vec::new();
    let mut offset = 1;
    for (race_id, race) in &races {
        for (profession_id, profession) in &professions {
            let npc = spawn_npc(&mut world, *race, *profession, offset);
            offset += 1;
            rendered.push((
                format!("{race_id} / {profession_id}"),
                npc_composite(&content, &world, &atlas, npc),
            ));
        }
    }

    // Act & Assert
    for (i, (label_a, a)) in rendered.iter().enumerate() {
        for (label_b, b) in &rendered[i + 1..] {
            let diff = differing(a, b);
            assert!(
                diff >= BADGE_PIXELS,
                "「{label_a}」与「{label_b}」画出来只有 {diff} 个像素不同\
                 （门槛 {BADGE_PIXELS}）——屏幕上分不出这两个 NPC"
            );
        }
    }
}

#[test]
fn 同一个种族下不同职业至少差整块徽记() {
    // 上一条是全体两两比。这一条把「职业这条轴真的起作用了」单独钉出来
    // ——上一条即使职业完全不起作用，只要种族之间差得够多也可能碰巧通过
    // （不会，但那是巧合不是保证）。
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let base = base_namespace(&content);
    let professions: Vec<_> = registered_professions(&content)
        .into_iter()
        .filter(|(id, _)| id.namespace() == base)
        .collect();

    let mut rendered: Vec<(String, Composite)> = Vec::new();
    for (index, (id, profession)) in professions.iter().enumerate() {
        let npc = spawn_npc(
            &mut world,
            content.race_ids.dwarf,
            *profession,
            index as i32 + 1,
        );
        rendered.push((id.to_string(), npc_composite(&content, &world, &atlas, npc)));
    }

    // Act & Assert
    for (i, (label_a, a)) in rendered.iter().enumerate() {
        for (label_b, b) in &rendered[i + 1..] {
            let diff = differing(a, b);
            assert_eq!(
                diff, BADGE_PIXELS,
                "同为矮人的 {label_a} 与 {label_b} 差了 {diff} 个像素——\
                 同种族之间差异**只**该来自那块 6×6 徽记，多了说明职业\
                 悄悄改了身子，少了说明两个职业的徽记撞了色"
            );
        }
    }
}

#[test]
fn 同一个职业下不同种族至少差四分之一张图() {
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let base = base_namespace(&content);
    let races: Vec<_> = registered_races(&content)
        .into_iter()
        .filter(|(id, _)| id.namespace() == base)
        .collect();

    let mut rendered: Vec<(String, Composite)> = Vec::new();
    for (index, (id, race)) in races.iter().enumerate() {
        let npc = spawn_npc(
            &mut world,
            *race,
            content.class_ids.warrior,
            index as i32 + 1,
        );
        rendered.push((id.to_string(), npc_composite(&content, &world, &atlas, npc)));
    }

    // Act & Assert
    for (i, (label_a, a)) in rendered.iter().enumerate() {
        for (label_b, b) in &rendered[i + 1..] {
            let diff = differing(a, b);
            assert!(
                diff >= BODY_PIXELS,
                "同为战士的 {label_a} 与 {label_b} 只有 {diff} 个像素不同\
                 （门槛 {BODY_PIXELS}）——种族这条轴没起作用"
            );
        }
    }
}

#[test]
fn mod自己声明的种族与职业不改一行引擎代码就能有自己的样子() {
    // 「加第 10 个种族只加数据、不改 Rust」这条要求的可执行版本。
    // `examplemod:half_elf` 与 `examplemod:necromancer` 是示例 mod 在
    // 自己的 `races.json5`/`classes.json5` 里声明、在自己的
    // `assets/sprites/` 里配图的内容——`crates/` 下没有任何一处提到过
    // 这两个 id。
    //
    // 反例（本次开发实跑）：删掉
    // `mods/example_mod/assets/sprites/half_elf.png`，本条报身子退回了
    // 通用记号。
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let race = content
        .registry
        .get(&NamespacedId::parse("examplemod:half_elf").expect("字面量合法"))
        .expect("示例 mod 声明了半精灵");
    let profession = content
        .registry
        .get(&NamespacedId::parse("examplemod:necromancer").expect("字面量合法"))
        .expect("示例 mod 声明了死灵法师");
    let npc = spawn_npc(&mut world, race, profession, 1);

    // Act
    let mut mine: Vec<_> = npc_draws(&world.world, &content.registry, world.player)
        .into_iter()
        .filter(|draw| {
            draw.entity == ll_game::surface_draw::NPC_ENTITY_BASE + u64::from(npc.index())
                || draw.entity
                    == ll_game::surface_draw::NPC_BADGE_ENTITY_BASE + u64::from(npc.index())
        })
        .collect();
    mine.sort_by_key(|draw| draw.entity);
    let chosen: Vec<String> = mine
        .iter()
        .filter_map(|draw| {
            draw.keys()
                .find(|name| atlas.metadata.lookup(name).is_some())
                .map(str::to_string)
        })
        .collect();

    // Assert
    assert_eq!(
        chosen,
        vec![
            "examplemod:half_elf".to_string(),
            "examplemod:necromancer".to_string()
        ],
        "mod 自带的种族身子/职业挂件没被选中"
    );

    // 再往前一步：它画出来的样子与本体任何一个组合都不一样。
    let mod_look = npc_composite(&content, &world, &atlas, npc);
    let base = base_namespace(&content);
    for (race_id, base_race) in registered_races(&content) {
        if race_id.namespace() != base {
            continue;
        }
        let other = spawn_npc(&mut world, base_race, content.class_ids.warrior, 40);
        let other_look = npc_composite(&content, &world, &atlas, other);
        assert!(
            differing(&mod_look, &other_look) >= BADGE_PIXELS,
            "mod 的半精灵/死灵法师与本体的 {race_id}/战士 长得太像"
        );
    }
}

#[test]
fn 没有自带贴图的种族退回通用记号而没有自带贴图的职业不画挂件() {
    // 这两条降级路径必须**不一样**，理由见
    // `ll_game::surface_draw::SurfaceDraw::fallback_key` 字段文档：种族
    // 没图必须退回一个人形，否则那个 NPC 整个消失；职业没图必须什么都
    // 不画，否则所有没画过的职业会共用同一枚「通用职业徽记」，看起来
    // 像同一个职业。
    //
    // 挑的是示例 mod 里真的没配图的那两条内容，不是临时造的假数据。
    // Arrange
    let (content, mut world, atlas) = real_setup();
    let race = content
        .registry
        .get(&NamespacedId::parse("examplemod:dragonborn").expect("字面量合法"))
        .expect("示例 mod 声明了龙裔");
    let profession = content
        .registry
        .get(&NamespacedId::parse("examplemod:rogue").expect("字面量合法"))
        .expect("示例 mod 声明了盗贼");
    let npc = spawn_npc(&mut world, race, profession, 1);

    // Act
    let draws = npc_draws(&world.world, &content.registry, world.player);
    let body = draws
        .iter()
        .find(|draw| draw.entity == ll_game::surface_draw::NPC_ENTITY_BASE + u64::from(npc.index()))
        .expect("身子那一条");
    let badge = draws
        .iter()
        .find(|draw| {
            draw.entity == ll_game::surface_draw::NPC_BADGE_ENTITY_BASE + u64::from(npc.index())
        })
        .expect("挂件那一条");

    // Assert
    assert_eq!(
        body.keys()
            .find(|name| atlas.metadata.lookup(name).is_some()),
        Some(NPC_SPRITE),
        "没配图的种族应当退回通用人形记号"
    );
    assert_eq!(
        badge
            .keys()
            .find(|name| atlas.metadata.lookup(name).is_some()),
        None,
        "没配图的职业应当一层都不画，而不是退到某张通用职业记号"
    );
}
