//! 端到端调用链的非交互式回归。
//!
//! # 为什么需要这个文件（如实记录）
//!
//! 任务 15 的验收要求真实窗口 + `SendKeys` 注入按键（沿用 P3/P4 的
//! 方法论）。本批次实测发现：当前运行环境下合成键盘事件不能可靠地
//! 只送达目标窗口——`GetForegroundWindow()` 诊断确认前台窗口始终是
//! 宿主的 Claude 桌面应用本身，而不是刚启动的 demo 窗口，即便依次尝试
//! 了 `SetForegroundWindow`、鼠标点击抢焦点、`PostMessage` 直接投递到
//! 目标窗口句柄。这不是「图形环境不可用」（demo 真实启动、真实用
//! wgpu 渲染、窗口截图确认画面正确——minimap、FOV 圆形可见区域、地形
//! 配色都符合预期），而是**这台机器上的输入注入无法安全隔离到目标
//! 窗口**——继续尝试会把合成按键泄漏进宿主聊天窗口本身（已经实际发生
//! 并如实报告给了用户）。
//!
//! 按照本任务的既有纪律「若图形环境不可用，如实说明，改用自动化测试
//! 覆盖可测链路，不要谎报『已目视确认』」，这个文件就是那条「自动化
//! 测试覆盖可测链路」——它不模拟按键，直接驱动
//! `Intent → resolve → Effect → apply` 这条真实链路本身（与
//! `main.rs::Demo::advance`/`try_interact` 调用的是完全相同的公开
//! 函数，唯一的差别是意图从「键盘生成」换成「测试直接构造」），程序化
//! 走一遍验收 demo 要证明的四件事，得到比静态单元测试更强的连线证据，
//! 但仍然**不是**肉眼验收——报告里如实区分这两者，不混为一谈。

#![cfg(test)]

use ll_sim::apply::apply;
use ll_sim::effect::Effect;
use ll_sim::intent::{Direction, Intent};
use ll_sim::resolve::resolve;
use ll_world::space::Space;

use crate::layout::{
    EAST_CORRIDOR_LENGTH, EAST_WALK_INTO_UNWARMED_ZONE, STREAM_RADIUS_ZONES, effective_sight_radius,
};
use crate::world::{DemoWorld, build_demo_world};

/// 走一步：先维护流式邻域（与 `Demo::maintain_streaming` 相同的调用），
/// 再 `resolve`+`apply` 一次 `Intent::Move`，返回这一步是否真的移动了
/// （`resolve_move` 在撞墙/撞水/目标区块未常驻时都会产出空效果）。
fn step(demo: &mut DemoWorld, dir: Direction) -> bool {
    let actor = demo.player;
    let pos = demo.world.actors.get(actor).expect("玩家必然存在").pos;
    demo.world.terrain.stream_neighborhood(
        &demo.noise,
        &demo.params,
        &demo.terrain_ids,
        pos,
        STREAM_RADIUS_ZONES,
        demo.world.clock,
    );
    let intent = Intent::Move { actor, dir };
    let effects = resolve(&demo.world, &intent);
    let moved = effects
        .iter()
        .any(|effect| matches!(effect, Effect::MoveTo { .. }));
    for effect in &effects {
        apply(&mut demo.world, effect);
    }
    moved
}

#[test]
fn 沿东向走廊连续移动跨越多个区块边界全程无阻挡() {
    // 验收点①的程序化证据：连续 260 步 Move::East，每一步都必须真的
    // 移动（走廊全程强制可通行，见 EAST_CORRIDOR_LENGTH 文档）——若
    // 中途任意一步被判定为「撞墙」，要么是走廊没铺够远，要么是流式
    // 加载在某个区块边界处掉了链子（目标区块未常驻，resolve_move 保守
    // 地视为不可通行），两者都是需要立刻查的缺陷,不是「反正走不到那么
    // 远也无所谓」。
    // Arrange
    let mut demo = build_demo_world();
    let start_zone = demo
        .world
        .terrain
        .layout()
        .tile_to_zone(demo.world.actors.get(demo.player).expect("必然存在").pos)
        .0;

    // Act & Assert
    for i in 0..EAST_CORRIDOR_LENGTH {
        assert!(step(&mut demo, Direction::East), "第 {i} 步向东移动被阻挡");
    }

    // 落脚区块必须与出生区块不同——否则这条测试只是在原地打转,没有
    // 真的跨越任何边界。
    let end_pos = demo.world.actors.get(demo.player).expect("必然存在").pos;
    let end_zone = demo.world.terrain.layout().tile_to_zone(end_pos).0;
    assert_ne!(start_zone, end_zone);
}

#[test]
fn 出生点邻域预热本身覆盖不到第3列区块() {
    // 验收点①的第二层证据的前半段：先独立证明「出生点一次性预热」这
    // 件事本身覆盖不到第 3 列——不经过 build_demo_world（它为了铺
    // 走廊会提前用 terrain_at 把整条路径都流式加载进来，见其文档，
    // 那样就没法把「出生预热覆盖了哪里」与「走廊铺设覆盖了哪里」这
    // 两件事分开看），直接用同样的区块布局构造一个没有任何走廊/入口
    // 改写的 WorldState，单独检查 SPAWN_WARM_RADIUS 那一圈邻域本身的
    // 覆盖范围。
    // Arrange
    let layout = crate::world::build_zone_layout();
    let params = ll_world::generate::GenParams::default();
    let (terrain_ids, terrain_table) = ll_world::terrain::base_terrain_fixture();
    let spawn = layout
        .tile_size()
        .wrap(crate::layout::SPAWN_X, crate::layout::SPAWN_Y);

    // Act
    let world =
        ll_world::state::WorldState::new(layout, &params, &terrain_ids, terrain_table, spawn)
            .expect("demo 区块布局满足全部构造前置条件");
    let (third_column_zone, _) = layout.tile_to_zone(layout.tile_size().wrap(
        crate::layout::SPAWN_X + EAST_WALK_INTO_UNWARMED_ZONE,
        crate::layout::SPAWN_Y,
    ));

    // Assert
    assert!(
        !world.terrain.is_resident(third_column_zone),
        "出生点预热不该覆盖到第 3 列区块 {third_column_zone:?}——若这条断言变红，\
         说明 SPAWN_WARM_RADIUS 或世界区块数被改动过，EAST_WALK_INTO_UNWARMED_ZONE\
         的取值需要跟着重新核算"
    );
}

#[test]
fn 走到出生邻域预热覆盖不到的区块查询地形不panic证明真流式加载生效() {
    // 验收点①的第二层证据的后半段：真实玩过一遍——build_demo_world
    // 为了保证走廊可通行,构造时就已经用 terrain_at 把整条走廊（含第
    // 3 列）提前流式加载过一遍（这本身也是流式加载在正常发挥作用，
    // 不是绕过它，见 build_demo_world 文档「先用 terrain_at 触发按需
    // 生成」一节）。这条测试验证的是走到那里之后，resolve/apply 这条
    // 真实结算链路能不能正常查到地形、正常移动进去——不会撞见
    // resolve_move 因为「目标区块未常驻」而保守拒绝移动这种断链。
    // Arrange
    let mut demo = build_demo_world();

    // Act：走到第 3 列区块内部。
    for _ in 0..EAST_WALK_INTO_UNWARMED_ZONE {
        assert!(
            step(&mut demo, Direction::East),
            "走向第 3 列区块的一步被阻挡"
        );
    }
    let end_pos = demo.world.actors.get(demo.player).expect("必然存在").pos;
    let layout = *demo.world.terrain.layout();
    let (end_zone, _) = layout.tile_to_zone(end_pos);

    // Assert：落脚区块确实是预热覆盖不到的第 3 列，且此刻查询地形能
    // 正常拿到值（不是 None，说明真被流式加载进来了，不是巧合常驻）。
    assert_eq!(end_zone.x(), 3);
    assert!(demo.world.terrain_at(end_pos).is_some());
}

#[test]
fn 世界地图标记随玩家移动更新到新的区块坐标() {
    // 验收点②的程序化证据：continent_map 展示的是区块坐标，标记位置
    // 就是 tile_to_zone(agent.pos) ——这里直接断言这个换算本身随移动
    // 更新，真正的像素绘制由 main.rs::push_minimap 消费，不在这条
    // 测试范围内（那部分是纯粹的坐标到像素换算，layout.rs 已有独立
    // 测试覆盖）。
    // Arrange
    let mut demo = build_demo_world();
    let layout = *demo.world.terrain.layout();
    let start_zone = layout.tile_to_zone(demo.world.actors.get(demo.player).expect("必然存在").pos);

    // Act：走到跨越至少一个区块边界的距离。
    for _ in 0..100 {
        step(&mut demo, Direction::East);
    }
    let end_zone = layout.tile_to_zone(demo.world.actors.get(demo.player).expect("必然存在").pos);

    // Assert
    assert_ne!(start_zone, end_zone);
}

#[test]
fn 进出interior的完整调用链走通且层属性生效() {
    // 验收点③④的程序化证据：站在入口格触发 EnterSpace、只有
    // current_space 变化、退出后 pos 精确回到锚点、且地下层的视野半径
    // 明显小于地表——与 ll-sim/src/resolve.rs 的单元测试断言的是同一批
    // 事实，这里额外验证的是"用本 demo 实际搭建的世界（真实的
    // WorldState::new + insert_interior + spawn_player 组合）跑，而不是
    // resolve.rs 测试里简化的夹具世界"，是两套独立构造路径对同一组
    // 不变式的交叉验证。
    // Arrange
    let mut demo = build_demo_world();
    let player = demo.player;
    let spawn_pos = demo.world.actors.get(player).expect("必然存在").pos;
    let clock = demo.world.clock;
    let surface_profile = demo.profile_of(Space::surface(
        demo.world.terrain.layout().tile_to_zone(spawn_pos).0,
        demo.space_ids.surface,
    ));
    let surface_radius = effective_sight_radius(&surface_profile, clock);

    // Act 1：走到入口（正南 ENTRANCE_OFFSET_Y 格）。
    for _ in 0..crate::layout::ENTRANCE_OFFSET_Y {
        assert!(step(&mut demo, Direction::South), "走向入口的一步被阻挡");
    }
    let on_entrance = demo.world.actors.get(player).expect("必然存在").pos;
    assert_eq!(on_entrance, demo.interior_anchor);

    // Act 2：进入。
    let entries = demo.world.interiors.entries_at(on_entrance);
    assert_eq!(entries, vec![demo.interior_id]);
    let enter_effects = resolve(
        &demo.world,
        &Intent::EnterSpace {
            actor: player,
            target: demo.interior_id,
        },
    );
    assert!(!enter_effects.is_empty(), "站在入口格必须能产出进入效果");
    for effect in &enter_effects {
        apply(&mut demo.world, effect);
    }

    // Assert：pos 不变，只有 current_space 变化，且只渲染当前层——即
    // 地下 profile 的视野半径明显小于地表（层属性生效，验收点④）。
    let agent = demo.world.actors.get(player).expect("必然存在");
    assert_eq!(agent.pos, on_entrance);
    let interior_space = agent.current_space;
    assert!(matches!(interior_space, Space::Interior { .. }));
    let interior_profile = demo.profile_of(interior_space);
    let interior_radius = effective_sight_radius(&interior_profile, clock);
    assert!(
        interior_radius < surface_radius,
        "地下视野半径 {interior_radius} 应明显小于地表 {surface_radius}"
    );

    // Act 3：退出。
    let exit_effects = resolve(&demo.world, &Intent::ExitSpace { actor: player });
    assert!(!exit_effects.is_empty(), "在 Interior 内必须能产出退出效果");
    for effect in &exit_effects {
        apply(&mut demo.world, effect);
    }

    // Assert：退出后 pos 精确回到锚点，current_space 回到地表。
    let agent = demo.world.actors.get(player).expect("必然存在");
    assert_eq!(agent.pos, demo.interior_anchor);
    assert!(matches!(agent.current_space, Space::Surface { .. }));
}
