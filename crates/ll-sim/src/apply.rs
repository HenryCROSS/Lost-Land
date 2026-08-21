//! `apply`：把一个 [`Effect`] 落到 [`WorldState`] 上的唯一入口。

use ll_world::entity::EntityId;
use ll_world::fov::compute_fov;
use ll_world::script_state::ScriptStateTarget;
use ll_world::space::Space;
use ll_world::state::WorldState;
use ll_world::surface_store::SurfaceWindow;

use crate::effect::Effect;
use crate::xp_curve::{FlatXpCurve, XpCurveCatalog, eval_xp_curve};

/// 把一个 [`Effect`] 应用到世界状态，这是全局唯一允许改动
/// [`WorldState`] 的函数。
///
/// # 三条纪律（见规格 Task 5 简报，逐条落实为下面的实现）
///
/// 1. **它是全局唯一能改世界的函数。** 别处出现 `&mut WorldState` 都是
///    设计错误——`resolve`（批次 C）只读世界、产出 `Effect`，不持有
///    `&mut WorldState`；时间轴、Intent 层同样不改世界。仓库里理应
///    只有这一处代码同时具备「拿到 `&mut WorldState`」与「据此给字段
///    赋值」两件事。
/// 2. **它不含任何游戏逻辑。** 下面每个分支要么是直接赋值，要么是
///    `if let Some(..)` 这类边界防御（实体是否还存在），没有一处在
///    判断「这算不算命中」「伤害该扣多少」之类的规则——那些判断已经
///    在 `resolve` 里做完，产出这个 `Effect` 时就已经带着最终数字。
/// 3. **它必须极短。** 全部分支各自不超过两行，任何看起来需要更多
///    行数才能表达的分支，多半是规则判断偷偷混了进来，应该退回
///    `resolve`。
///
/// # 签名如何拦住「不经 `Effect` 就改世界」
///
/// [`WorldState`] 的字段全部公开（存档格式即结构体本身，见其文档），
/// Rust 的可见性系统无法把「写 `WorldState`」这件事在编译期锁死到
/// 只有本函数能做——那需要把字段私有化、只留访问器，是比本批次大得
/// 多的封装改造。本函数的签名 `apply(world: &mut WorldState, effect:
/// &Effect)` 做到的是把「要改世界，必须先有一个 `Effect` 值」焊进
/// 类型签名：拿不出一个 `Effect`，就没有把状态改动传给这个函数的
/// 办法。守住「别处不会真的绕过去」靠的是约定而非编译器——评审时若
/// 在本文件之外看到 `&mut WorldState` 紧跟着字段赋值，就是这条纪律
/// 被打破的信号，这正是简报原文给出的兜底标准（「在类型上做不到，
/// 或至少显然错误」的后半句）。
///
/// # 目标实体不存在时：忽略，不报错
///
/// `Damage`/`Kill`（以及 `MoveTo`/`ScheduleNext`/`AdjustWallet`）的
/// `target`/`actor` 都可能指向一个已经不存在的实体——时间轴队列或
/// 一批 `resolve` 产出的 `Effect` 里，可能残留着指向刚被别的 `Effect`
/// 销毁的实体的条目（例如同一轮结算里先 `Kill` 后又对同一目标
/// `Damage`）。这里统一选择**静默忽略**而不是返回 `Err`：
///
/// 1. 这与既有五个分支（`MoveTo`/`ScheduleNext`/`AdjustWallet`
///    用 `if let Some(..)`、`Arena::despawn` 对不存在的实体自己返回
///    `false`）是同一套行为，`Damage`/`Kill` 不该是例外，否则调用方
///    得区分「这个 `Effect` 需要处理返回值，那个不需要」。
/// 2. `apply` 的签名是 `fn apply(..)`，不返回 `Result`——真要让某个
///    分支报错，全部分支就都要改签名，这是比本次任务大得多的改动。
/// 3. 目标不存在本身不是异常状况（见规则 2 的场景），是结算并发/时序
///    下的正常可能性，不需要中断整批 `Effect` 的应用。
///
/// [`apply`] 的既有签名不接收任何内容注册表——这对绝大多数效果没有
/// 影响（各分支的赋值都只需要 `Effect` 自身携带的朴素数据），但
/// [`Effect::GrantExperience`] 的升级循环必须知道「这个实体该用哪条
/// 经验曲线」才能重算 [`ll_world::entity::Agent::xp_to_next_level`]
/// ——曲线注册表定义在下游的 `ll-mod`（依赖方向不允许本 crate 反过来
/// 依赖它）。本函数是真正接住这个输入的入口，`apply` 本身则是保留
/// 既有签名、传入保底曲线（[`FlatXpCurve::DEFAULT`]）的薄封装——与
/// `resolve`/`resolve_with_skills`/`resolve_with_skills_and_quests` 的
/// 分层入口同一个理由：不强迫尚未装载任何 mod、或明确不需要经验结算
/// 的既有调用点都多传一份目录。
///
/// # 仍然是「唯一写入口」
///
/// `apply` 现在只是本函数套一层默认曲线的薄封装，不是第二个独立的
/// 写入口——真正持有 `&mut WorldState` 并对字段赋值的代码只存在于本
/// 函数体内，`apply` 自己不再重复一份匹配逻辑，模块文档「三条纪律」
/// 描述的「全局唯一函数」这条不变式没有被打破，只是这个函数现在多了
/// 一个可选输入。
pub fn apply_with_xp_curves(world: &mut WorldState, effect: &Effect, curves: &dyn XpCurveCatalog) {
    // 不再 `match *effect`（`Effect` 因 `SetScriptState` 携带 `Vec` 而
    // 不再是 `Copy`，见其文档）——改为按引用匹配，Copy 子字段用 `*`
    // 显式取值，与既有全部分支的赋值写法保持一致；`SetScriptState`
    // 携带的 `Vec`/`String`/`ScriptValue` 本身不是 `Copy`，逐条 `clone`
    // 写入，见该分支注释。
    match effect {
        Effect::MoveTo { actor, pos } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.pos = *pos;
            }
        }
        Effect::Damage { target, amount } => {
            if let Some(agent) = world.actors.get_mut(*target) {
                agent.health -= amount;
            }
        }
        Effect::Kill { target, .. } => {
            // killer/cause 只服务 `resolve` 侧的历史事件判定
            // （`append_kill_history` 在此之前已经读过它们，见
            // `Effect::RecordHistoricalEvent` 文档「为什么必须排在
            // 对应的 Effect::Kill 之前」）——apply 本身销毁实体不需要
            // 这两个字段，忽略它们不是遗漏，是「apply 不含游戏逻辑」
            // 纪律的直接体现：这两个字段的意义已经在 resolve 阶段被
            // 消费完毕。
            world.actors.despawn(*target);
        }
        Effect::RecordHistoricalEvent {
            at,
            location,
            victim,
            killer,
            cause,
            damage,
            remaining_health,
        } => {
            world.record_kill(ll_world::history::KillReport {
                at: *at,
                location: *location,
                victim: *victim,
                killer: *killer,
                cause: *cause,
                damage: *damage,
                remaining_health: *remaining_health,
            });
        }
        Effect::IncrementKillCount { kind } => {
            // 决策二（数全部击杀，取代决策一原有的无名单位限定，见
            // Effect::IncrementKillCount 文档）——把「按 kind 归并」这个
            // 已经在 resolve 阶段算好的判断原样落到 WorldState.kill_counts，
            // apply 本身不重新判断该按什么归并、也不重新判断这场击杀
            // 该不该计数，符合「apply 不含任何游戏逻辑」的纪律。
            world.record_kill_count(*kind);
        }
        Effect::ScheduleNext { actor, at } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.next_action_at = *at;
            }
        }
        Effect::SetTerrain { pos, kind } => {
            world.terrain.set_terrain(*pos, *kind);
        }
        Effect::AdjustWallet { actor, delta } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.wallet += delta;
            }
        }
        Effect::ChangeSpace { actor, space } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.current_space = *space;
            }
            // 与常驻预算的钉住状态同步（裁定 CS-3）——这两行不是
            // 「规则判断」，是把同一个决定（目标空间是什么）落到
            // WorldState 已有的两处状态上，见 Effect::ChangeSpace 文档。
            match space {
                Space::Interior { id, .. } => {
                    world.enter_interior(*id);
                }
                Space::Surface { .. } => {
                    world.exit_interior();
                }
            }
        }
        Effect::AdjustResource {
            actor,
            resource,
            delta,
        } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                match resource {
                    crate::skill::ResourceKind::Mana => agent.mana += delta,
                    crate::skill::ResourceKind::Stamina => agent.stamina += delta,
                }
            }
        }
        Effect::SetSkillCooldown {
            actor,
            skill,
            until,
        } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.skill_cooldowns.insert(*skill, *until);
            }
        }
        Effect::ApplyStatModifier {
            target,
            attribute,
            delta,
            expires_at,
            source,
        } => {
            if let Some(agent) = world.actors.get_mut(*target) {
                let incoming = ll_world::entity::ActiveStatModifier {
                    delta: *delta,
                    expires_at: *expires_at,
                };
                // 同源合并：(attribute, source) 相同即视为同一效果再次
                // 施加，走 merge_same_source（强度取较强、到期取较晚，
                // 两个维度独立比较）；不同来源各自独立占一个内层键，
                // 互不覆盖——这就是「不同效果能叠加，同效果只刷新时间」
                // 在这里唯一要做的判断，见 Effect::ApplyStatModifier 文档
                // 「source：施加者身份」一节。
                agent
                    .active_stat_modifiers
                    .entry(*attribute)
                    .or_default()
                    .entry(*source)
                    .and_modify(|existing| *existing = existing.merge_same_source(incoming))
                    .or_insert(incoming);
            }
        }
        Effect::SetScriptState { writes } => {
            // 逐条写入，各自落到全局或对应实体的每实体存储——实体已
            // 不存在时静默跳过，与本函数其余分支「目标实体不存在时忽略
            // 不报错」的既有纪律一致（见本函数文档）。这里不做任何
            // 判断（配额、命名空间隔离全部已经在 `ll-script` 侧的
            // `state-set!`/`entity-state-set!` 完成，进了这批 `writes`
            // 就是已经通过校验、只等落盘的数据），符合「apply 不含任何
            // 游戏逻辑」的纪律。
            for write in writes {
                match write.target {
                    ScriptStateTarget::Global => {
                        world.global_script_state.insert(
                            (write.mod_namespace.clone(), write.key.clone()),
                            write.value.clone(),
                        );
                    }
                    ScriptStateTarget::Entity(entity) => {
                        if let Some(agent) = world.actors.get_mut(entity) {
                            agent.script_state.insert(
                                (write.mod_namespace.clone(), write.key.clone()),
                                write.value.clone(),
                            );
                        }
                    }
                }
            }
        }
        Effect::MarkExplored { origin, radius } => {
            // 与渲染路径同一套调用（SurfaceWindow + compute_fov）——
            // 全过程唯一一处真正跑 FOV，见 Effect::MarkExplored 文档
            // 「为什么『apply 算出的集合与 resolve 看到的完全一致』
            // 自动成立」一节。`layout` 取自 `world.terrain`，与
            // `ExplorationMemory::mark_explored` 要求的「同一个
            // ZoneLayout」天然一致（同一个 WorldState 只有一份地形，
            // 不存在换布局的可能）。
            let layout = *world.terrain.layout();
            let visible = compute_fov(
                &SurfaceWindow::new(&world.terrain),
                &world.terrain_table,
                *origin,
                *radius,
            );
            for pos in visible.iter() {
                world.exploration.mark_explored(&layout, pos);
            }
        }
        Effect::GrantExperience { target, amount } => {
            grant_experience_and_level_up(world, *target, *amount, curves);
        }
        Effect::AdjustResourcePool { actor, pool, delta } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                let current = agent.resource_pools.entry(*pool).or_insert(0);
                *current += delta;
            }
        }
        Effect::SpendBloodCost { target, amount } => {
            // 无条件扣血,不查防御/抗性——见 Effect::SpendBloodCost 文档
            // 「为什么不是 Effect::Damage」一节,与 Effect::Damage 分支
            // 唯一的差异就是"绕开减伤"这件事本身已经在 resolve 侧完成
            // （血代价的数字从不经过 damage_after_defense),apply 这里
            // 与 Damage 分支写法一样简单,只是不共用同一个变体。
            if let Some(agent) = world.actors.get_mut(*target) {
                agent.health -= amount;
            }
        }
        Effect::AdjustResourceSlot {
            actor,
            pool,
            tier,
            delta,
        } => {
            // 已消耗数不能是负的——`i64` 运算后钳位到非负再落回 `u32`,
            // 与 `Effect::AdjustResourcePool` 允许当前值降到负数不同
            // （标量池的"当前值"语义上可以为负，钳位留给读取时的
            // `resource_pool_usable`），已消耗数是一个纯粹的计数,负数
            // 没有意义。
            if let Some(agent) = world.actors.get_mut(*actor) {
                let current = agent.spent_slots.entry((*pool, *tier)).or_insert(0);
                *current = (i64::from(*current) + i64::from(*delta)).max(0) as u32;
            }
        }
        Effect::BeginRest {
            actor,
            target_ticks,
        } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.resting = Some(ll_world::entity::RestState {
                    started_at: world.clock,
                    target_ticks: *target_ticks,
                });
            }
        }
        Effect::ClearResting { actor } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.resting = None;
            }
        }
        Effect::RemoveGroundItem { pos, def } => {
            // 按 (pos, def) 定位并移除第一条匹配——resolve 已经确认过
            // 这堆存在（见 Effect::RemoveGroundItem 文档「为什么按
            // (pos, def) 定位」一节），这里只做机械的查找+移除，不再
            // 判断该不该移除。
            if let Some(index) = world
                .ground_items
                .iter()
                .position(|item| item.pos == *pos && item.stack.def == *def)
            {
                world.ground_items.remove(index);
            }
        }
        Effect::AddGroundItem {
            pos,
            stack,
            dropped_at,
            contents,
        } => {
            world.ground_items.push(ll_world::item::GroundItemStack {
                pos: *pos,
                stack: *stack,
                dropped_at: *dropped_at,
                contents: contents.clone(),
            });
        }
        Effect::MergeIntoInventory {
            actor,
            replaced,
            resulting,
        } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                if let Some((def, durability)) = replaced
                    && let Some(index) = agent
                        .inventory
                        .iter()
                        .position(|stack| stack.def == *def && stack.durability == *durability)
                {
                    agent.inventory.remove(index);
                }
                agent.inventory.extend(resulting.iter().copied());
            }
        }
        Effect::RemoveFromInventory {
            actor,
            def,
            durability,
        } => {
            if let Some(agent) = world.actors.get_mut(*actor)
                && let Some(index) = agent
                    .inventory
                    .iter()
                    .position(|stack| stack.def == *def && stack.durability == *durability)
            {
                agent.inventory.remove(index);
            }
        }
        Effect::Equip { actor, slot, stack } => {
            // 无条件覆盖写入——resolve_equip 已经保证同一批效果里冲突
            // 槽位的 Effect::Unequip 排在本效果之前,见 Effect::Equip
            // 文档「为什么 apply 不检查槽位是否已被占用」一节。
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.equipment.insert(*slot, *stack);
            }
        }
        Effect::Unequip { actor, slot } => {
            if let Some(agent) = world.actors.get_mut(*actor) {
                agent.equipment.remove(slot);
            }
        }
        Effect::ConsumeInventoryItem {
            actor,
            def,
            durability,
        } => {
            // 按 (def, durability) 定位——resolve 已经确认过这堆存在,
            // 见 Effect::ConsumeInventoryItem 文档「为什么按 (def,
            // durability) 定位」一节。数量减一,减到零时整条堆移除,不
            // 留一个 count == 0 的死堆（ItemStack.count 文档「恒 ≥ 1」
            // 一节的既有不变式）。
            if let Some(agent) = world.actors.get_mut(*actor)
                && let Some(index) = agent
                    .inventory
                    .iter()
                    .position(|stack| stack.def == *def && stack.durability == *durability)
            {
                if agent.inventory[index].count > 1 {
                    agent.inventory[index].count -= 1;
                } else {
                    agent.inventory.remove(index);
                }
            }
        }
        Effect::AdjustEquipmentDurability { actor, slot, delta } => {
            // 钳位到非负——见 Effect::AdjustEquipmentDurability 文档
            // 「为什么钳位到非负在 apply 做」一节。没有耐久概念的物品
            // （`durability == None`）保持 `None`,不会被凭空赋予一个
            // 耐久值。
            if let Some(agent) = world.actors.get_mut(*actor)
                && let Some(stack) = agent.equipment.get_mut(slot)
                && let Some(durability) = stack.durability
            {
                stack.durability = Some((durability + delta).max(0));
            }
        }
    }
}

/// [`apply`] 的既有调用点使用的薄封装：套一层
/// [`FlatXpCurve::DEFAULT`] 保底曲线，行为对不产出 `Effect::GrantExperience`
/// 的调用点完全透明，见 [`apply_with_xp_curves`] 文档。
pub fn apply(world: &mut WorldState, effect: &Effect) {
    apply_with_xp_curves(world, effect, &FlatXpCurve::DEFAULT);
}

/// [`Effect::GrantExperience`] 的完整落地逻辑：加经验、循环判定升级、
/// 每次升级增量重算 `xp_to_next_level`——设计文档六节裁定「升级判定
/// 整段放进 apply 一次算完」，本函数就是那一整段。
///
/// # 为什么是循环，不是一次 `if`
///
/// 一次性授予的经验量可能足够连续跨越好几级（例如一次性给了一大笔
/// 任务奖励经验）——`while` 循环让每一级各自消耗掉对应的门槛、各自
/// 重算下一级门槛，直到剩余经验不够再升一级为止，与设计文档「可能
/// 连续触发好几次」一致。
///
/// # 经验语义：当前等级内的进度条，不是终身累计总量
///
/// 每次升级都从 `agent.experience` 里扣掉刚消耗的门槛（见
/// [`ll_world::entity::Agent::experience`] 文档）——升级后的经验值是
/// 「这一级已经攒了多少」，不是「一辈子攒了多少」，与
/// [`ll_world::entity::Agent::xp_to_next_level`] 存的是「delta 门槛」
/// 而不是「累积总门槛」这一点是同一套语义,两者必须按同一种口径才能
/// 直接比较大小。
///
/// # 防御性下限：`xp_to_next_level <= 0` 时不循环
///
/// 正常曲线不会算出零或负的门槛，但装载期无法排除 mod 作者写出一条
/// 退化曲线（例如恒返回 0）——`xp_to_next_level <= 0` 时经验永远
/// `>=` 门槛，若不加这道防线会死循环。这里选择直接停止升级（不再
/// 消耗经验、不再递增等级），是防御性兜底，不是设计允许的正常路径,
/// 与 `XpCurveOp::Div` 除以零时返回 0 同一条纪律。
fn grant_experience_and_level_up(
    world: &mut WorldState,
    target: EntityId,
    amount: i64,
    curves: &dyn XpCurveCatalog,
) {
    let Some(agent) = world.actors.get_mut(target) else {
        return;
    };
    agent.experience = agent.experience.saturating_add(amount);
    while agent.xp_to_next_level > 0 && agent.experience >= agent.xp_to_next_level {
        let consumed = agent.xp_to_next_level;
        agent.experience -= consumed;
        agent.level += 1;
        let curve = curves.curve_for(agent.profession, agent.race);
        agent.xp_to_next_level = eval_xp_curve(&curve, agent.level, consumed);
    }
}

#[cfg(test)]
mod tests {
    use ll_core::time::Tick;
    use ll_core::torus::TorusSize;
    use ll_world::entity::{Agent, BaseStats};
    use ll_world::generate::GenParams;
    use ll_world::item::{EquipSlot, ItemStack};
    use ll_world::terrain::base_terrain_fixture;
    use ll_world::zone::ZoneLayout;

    use super::*;

    /// 测试用区块布局：边长 64，单个区块——与 `ll-world` 既有测试同一
    /// 常量，满足 `WorldState::new` 的前置条件，整个测试世界落在这一
    /// 个区块内。
    fn test_layout() -> ZoneLayout {
        let zone_count = TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束")
    }

    fn test_world() -> WorldState {
        let layout = test_layout();
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn blank_agent(world: &WorldState) -> Agent {
        // `ContentIndex` 只能经 `Interner::intern` 取得（见其文档：
        // 索引依赖登记顺序，没有可以凭空构造的公开常量）——测试只是
        // 需要一个占位职业，登记哪个标识符不重要。
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(0, 0);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            luck: 0,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: ll_world::space::Space::surface(
                zone,
                ll_core::ident::ContentIndex::default(),
            ),
            script_state: std::collections::BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: ll_world::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: ll_world::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
        }
    }

    #[test]
    fn 移动效果改变实体位置() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        let target_pos = world.size.wrap(5, 7);

        // Act
        apply(
            &mut world,
            &Effect::MoveTo {
                actor,
                pos: target_pos,
            },
        );

        // Assert
        assert_eq!(
            world.actors.get(actor).expect("刚生成的实体必然存在").pos,
            target_pos
        );
    }

    #[test]
    fn incrementkillcount效果按kind累加计数() {
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let goblin = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));

        // Act：同一个 kind 累加两次。
        apply(&mut world, &Effect::IncrementKillCount { kind: goblin });
        apply(&mut world, &Effect::IncrementKillCount { kind: goblin });

        // Assert
        assert_eq!(world.kill_counts.get(&goblin), Some(&2));
    }

    #[test]
    fn incrementkillcount效果对不同kind分别计数() {
        // Arrange
        let mut world = test_world();
        let mut interner = ll_core::ident::Interner::new();
        let goblin = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:goblin").expect("合法标识符"));
        let wolf = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:wolf").expect("合法标识符"));

        // Act
        apply(&mut world, &Effect::IncrementKillCount { kind: goblin });
        apply(&mut world, &Effect::IncrementKillCount { kind: wolf });
        apply(&mut world, &Effect::IncrementKillCount { kind: wolf });

        // Assert
        assert_eq!(world.kill_counts.get(&goblin), Some(&1));
        assert_eq!(world.kill_counts.get(&wolf), Some(&2));
    }

    #[test]
    fn 伤害效果扣减生命() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);

        // Act
        apply(&mut world, &Effect::Damage { target, amount: 30 });

        // Assert
        assert_eq!(
            world
                .actors
                .get(target)
                .expect("刚生成的实体必然存在")
                .health,
            Agent::STARTING_HEALTH - 30
        );
    }

    #[test]
    fn 对已销毁实体施加效果不会崩溃() {
        // 时间轴队列可能残留死者条目，effect 仍可能对着一个已销毁的
        // 实体到来——apply 必须安全地忽略，而不是 panic。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        world.actors.despawn(actor);
        let pos = world.size.wrap(1, 1);

        // Act & Assert：不应崩溃。
        apply(&mut world, &Effect::MoveTo { actor, pos });
        apply(
            &mut world,
            &Effect::Damage {
                target: actor,
                amount: 10,
            },
        );
        apply(
            &mut world,
            &Effect::Kill {
                target: actor,
                killer: None,
                cause: ll_world::history::KillCause::Fall,
            },
        );
        apply(&mut world, &Effect::ScheduleNext { actor, at: Tick(5) });
        apply(&mut world, &Effect::AdjustWallet { actor, delta: 100 });
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        apply(
            &mut world,
            &Effect::ChangeSpace {
                actor,
                space: ll_world::space::Space::surface(
                    zone,
                    ll_core::ident::ContentIndex::default(),
                ),
            },
        );
    }

    #[test]
    fn 切换空间效果改变实体的当前空间() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        let mut counter = 0u32;
        let mut interner = ll_core::ident::Interner::new();
        let profile = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
        let anchor = world.size.wrap(0, 0);
        let interior_id = ll_core::ident::WorldId::next(&mut counter);
        let mut interior = ll_world::interior::Interior::new(interior_id, anchor, profile);
        let size = ll_core::bounded::BoundedSize::new(4, 4).expect("4x4 是合法尺寸");
        let (ids, _table) = base_terrain_fixture();
        interior.set_floor(
            0,
            ll_world::bounded_grid::BoundedGrid::new(size, ids.floor_stone),
        );
        world.insert_interior(interior);
        let target_space = ll_world::space::Space::Interior {
            id: interior_id,
            floor: 0,
            anchor,
            profile,
        };

        // Act
        apply(
            &mut world,
            &Effect::ChangeSpace {
                actor,
                space: target_space,
            },
        );

        // Assert
        assert_eq!(
            world
                .actors
                .get(actor)
                .expect("刚生成的实体必然存在")
                .current_space,
            target_space
        );
    }

    #[test]
    fn 切换到interior空间会钉住其锚点区块() {
        // apply 响应 ChangeSpace 时必须同步调用 WorldState::enter_interior
        // ——不能只改 Agent 字段，否则常驻预算的钉住状态（裁定 CS-3）
        // 会与玩家实际所在空间脱节。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        let mut counter = 0u32;
        let mut interner = ll_core::ident::Interner::new();
        let profile = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:dungeon").expect("字面量恒合法"));
        let anchor = world.size.wrap(0, 0);
        let interior_id = ll_core::ident::WorldId::next(&mut counter);
        world.insert_interior(ll_world::interior::Interior::new(
            interior_id,
            anchor,
            profile,
        ));

        // Act
        apply(
            &mut world,
            &Effect::ChangeSpace {
                actor,
                space: ll_world::space::Space::Interior {
                    id: interior_id,
                    floor: 0,
                    anchor,
                    profile,
                },
            },
        );

        // Assert
        assert_eq!(world.current_interior, Some(interior_id));
    }

    #[test]
    fn 效果的应用顺序不影响最终世界哈希() {
        // 用两个互不重叠位置的地形效果验证 apply 本身不引入隐藏的顺序
        // 依赖——SetTerrain 直接落在 WorldState::hash 已经覆盖的地形
        // 网格上，不需要为这条测试额外扩大 hash 的覆盖范围。
        // Arrange
        let (terrain_ids, _table) = base_terrain_fixture();
        let pos_a = TorusSize::new(64, 64).expect("64x64 合法").wrap(2, 2);
        let pos_b = TorusSize::new(64, 64).expect("64x64 合法").wrap(40, 12);
        let effect_a = Effect::SetTerrain {
            pos: pos_a,
            kind: terrain_ids.floor_stone,
        };
        let effect_b = Effect::SetTerrain {
            pos: pos_b,
            kind: terrain_ids.wall_stone,
        };

        let mut forward = test_world();
        apply(&mut forward, &effect_a);
        apply(&mut forward, &effect_b);

        let mut backward = test_world();
        apply(&mut backward, &effect_b);
        apply(&mut backward, &effect_a);

        // Act & Assert
        assert_eq!(forward.hash(), backward.hash());
    }

    #[test]
    fn setscriptstate效果写入全局存储() {
        // 裁定 P5-1 的直接验收：脚本状态写入经由 Effect::SetScriptState
        // 走 apply 这唯一写入口落进 WorldState.global_script_state。
        // Arrange
        let mut world = test_world();
        let effect = Effect::SetScriptState {
            writes: vec![ll_world::script_state::ScriptStateWrite {
                target: ll_world::script_state::ScriptStateTarget::Global,
                mod_namespace: "lostland".to_string(),
                key: "reputation".to_string(),
                value: ll_world::script_state::ScriptValue::Int(100),
            }],
        };

        // Act
        apply(&mut world, &effect);

        // Assert
        assert_eq!(
            world
                .global_script_state
                .get(&("lostland".to_string(), "reputation".to_string())),
            Some(&ll_world::script_state::ScriptValue::Int(100))
        );
    }

    #[test]
    fn setscriptstate效果写入指定实体的每实体存储() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        let effect = Effect::SetScriptState {
            writes: vec![ll_world::script_state::ScriptStateWrite {
                target: ll_world::script_state::ScriptStateTarget::Entity(actor),
                mod_namespace: "lostland".to_string(),
                key: "cooldown".to_string(),
                value: ll_world::script_state::ScriptValue::Int(5),
            }],
        };

        // Act
        apply(&mut world, &effect);

        // Assert
        let stored = world
            .actors
            .get(actor)
            .expect("刚生成的实体必然存在")
            .script_state
            .get(&("lostland".to_string(), "cooldown".to_string()));
        assert_eq!(stored, Some(&ll_world::script_state::ScriptValue::Int(5)));
    }

    #[test]
    fn setscriptstate效果对已销毁实体的写入静默忽略而不崩溃() {
        // 与本文件其余分支「目标实体不存在时忽略不报错」的既有纪律
        // 一致——见本文件 apply 函数文档。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        world.actors.despawn(actor);
        let effect = Effect::SetScriptState {
            writes: vec![ll_world::script_state::ScriptStateWrite {
                target: ll_world::script_state::ScriptStateTarget::Entity(actor),
                mod_namespace: "lostland".to_string(),
                key: "cooldown".to_string(),
                value: ll_world::script_state::ScriptValue::Int(5),
            }],
        };

        // Act & Assert：不应崩溃。
        apply(&mut world, &effect);
    }

    #[test]
    fn 一条setscriptstate效果可以携带多组键值() {
        // 裁定 P5-1 的性能解法：一次决策期间的多次写入收集成一条
        // Effect 携带多组键值一次性发出——这里验证 apply 会把批内每一
        // 条都落地，不只处理第一条。
        // Arrange
        let mut world = test_world();
        let effect = Effect::SetScriptState {
            writes: vec![
                ll_world::script_state::ScriptStateWrite {
                    target: ll_world::script_state::ScriptStateTarget::Global,
                    mod_namespace: "lostland".to_string(),
                    key: "a".to_string(),
                    value: ll_world::script_state::ScriptValue::Int(1),
                },
                ll_world::script_state::ScriptStateWrite {
                    target: ll_world::script_state::ScriptStateTarget::Global,
                    mod_namespace: "lostland".to_string(),
                    key: "b".to_string(),
                    value: ll_world::script_state::ScriptValue::Int(2),
                },
            ],
        };

        // Act
        apply(&mut world, &effect);

        // Assert
        assert_eq!(world.global_script_state.len(), 2);
    }

    #[test]
    fn adjustresource效果改变实体的法力值() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);

        // Act
        apply(
            &mut world,
            &Effect::AdjustResource {
                actor,
                resource: crate::skill::ResourceKind::Mana,
                delta: -10,
            },
        );

        // Assert
        assert_eq!(
            world.actors.get(actor).expect("刚生成的实体必然存在").mana,
            Agent::STARTING_MANA - 10
        );
    }

    #[test]
    fn setskillcooldown效果写入技能冷却表() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let actor = world.actors.spawn(agent);
        let mut interner = ll_core::ident::Interner::new();
        let skill = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:strike").expect("合法标识符"));

        // Act
        apply(
            &mut world,
            &Effect::SetSkillCooldown {
                actor,
                skill,
                until: Tick(50),
            },
        );

        // Assert
        assert_eq!(
            world
                .actors
                .get(actor)
                .expect("刚生成的实体必然存在")
                .skill_cooldowns
                .get(&skill),
            Some(&Tick(50))
        );
    }

    /// 造一个仅用于「来源」的 `ContentIndex`——每次调用内部新开一个
    /// `Interner`，只保证「这一个值是一个合法的 `ContentIndex`」，**不**
    /// 保证与另一次独立调用得到的值不同（`Interner` 从零开始计数，两次
    /// 独立调用哪怕传入不同字符串也可能撞到同一个索引）。只需要一个
    /// 来源的测试用它；需要两个明确不同来源的测试必须用
    /// [`test_two_sources`]，不能连续调用本函数两次。
    fn test_source(id: &str) -> ll_core::ident::ContentIndex {
        let mut interner = ll_core::ident::Interner::new();
        interner.intern(ll_core::ident::NamespacedId::parse(id).expect("合法标识符"))
    }

    /// 造两个保证互不相同的「来源」`ContentIndex`——同一个 `Interner`
    /// 里连续 `intern` 两个不同的命名空间字符串，`Interner` 对不同字符串
    /// 分配不同索引这一点在同一实例内成立（见其模块文档），跨实例才不
    /// 成立，这正是 [`test_source`] 文档警告的陷阱。
    fn test_two_sources(
        a: &str,
        b: &str,
    ) -> (ll_core::ident::ContentIndex, ll_core::ident::ContentIndex) {
        let mut interner = ll_core::ident::Interner::new();
        let source_a = interner.intern(ll_core::ident::NamespacedId::parse(a).expect("合法标识符"));
        let source_b = interner.intern(ll_core::ident::NamespacedId::parse(b).expect("合法标识符"));
        (source_a, source_b)
    }

    #[test]
    fn applystatmodifier效果写入活跃属性修正表() {
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);
        let source = test_source("lostland:brace");

        // Act
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Constitution,
                delta: 3,
                expires_at: Tick(80),
                source,
            },
        );

        // Assert
        let stored = world
            .actors
            .get(target)
            .expect("刚生成的实体必然存在")
            .active_stat_modifiers
            .get(&ll_world::entity::AttributeKind::Constitution)
            .and_then(|per_source| per_source.get(&source));
        assert_eq!(
            stored,
            Some(&ll_world::entity::ActiveStatModifier {
                delta: 3,
                expires_at: Tick(80),
            })
        );
    }

    #[test]
    fn applystatmodifier效果对同一来源再次施加时合并而非各自独立存在() {
        // 验收 buffs-and-triggers.md 六节②③「同效果只刷新时间」：同一
        // (attribute, source) 再次被施加修正时，走 merge_same_source
        // （强度取较强、到期取较晚），不是两条修正共存。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);
        let source = test_source("lostland:brace");
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 2,
                expires_at: Tick(30),
                source,
            },
        );

        // Act：同一来源再次施加，强度更强（2 -> 5）、到期更晚（30 -> 90）。
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 5,
                expires_at: Tick(90),
                source,
            },
        );

        // Assert：这一项属性上只有一条记录（同源合并，不是两条并存），
        // 且强度、到期都取了较晚/较强的那一次。
        let per_source = world
            .actors
            .get(target)
            .expect("刚生成的实体必然存在")
            .active_stat_modifiers
            .get(&ll_world::entity::AttributeKind::Strength)
            .expect("已施加过修正");
        assert_eq!(per_source.len(), 1);
        assert_eq!(
            per_source.get(&source),
            Some(&ll_world::entity::ActiveStatModifier {
                delta: 5,
                expires_at: Tick(90),
            })
        );
    }

    #[test]
    fn applystatmodifier效果对不同来源的同一属性各自叠加而非覆盖() {
        // 验收六节①「不同效果能叠加」：两个不同来源（source_a、
        // source_b）各自给同一属性施加修正，必须各自保留一条独立记录，
        // 后写入的不能覆盖先写入的（这正是本节要推翻的旧行为）。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);
        let (source_a, source_b) = test_two_sources("lostland:brace", "lostland:blessing");

        // Act
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 2,
                expires_at: Tick(30),
                source: source_a,
            },
        );
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 3,
                expires_at: Tick(50),
                source: source_b,
            },
        );

        // Assert：两条记录都在，互不覆盖。
        let per_source = &world
            .actors
            .get(target)
            .expect("刚生成的实体必然存在")
            .active_stat_modifiers[&ll_world::entity::AttributeKind::Strength];
        assert_eq!(per_source.len(), 2);
        assert_eq!(
            per_source.get(&source_a),
            Some(&ll_world::entity::ActiveStatModifier {
                delta: 2,
                expires_at: Tick(30),
            })
        );
        assert_eq!(
            per_source.get(&source_b),
            Some(&ll_world::entity::ActiveStatModifier {
                delta: 3,
                expires_at: Tick(50),
            })
        );
    }

    #[test]
    fn applystatmodifier效果对同源更弱的再次施加保持较强强度但仍刷新到期时刻() {
        // 验收六节③「同一来源不同强度」这条最容易写错的规则：较弱的
        // 一次重复施放不应冲淡已经生效的强化版本（强度保持不变），但
        // 依然应该把到期时刻续到自己本该持续到的那一刻（到期时刻仍然
        // 更新）——两个维度独立比较，不是「较弱的施放完全不产生任何
        // 效果」。
        // Arrange：先施加一条较强的修正（|delta| = 5）。
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);
        let source = test_source("lostland:brace");
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 5,
                expires_at: Tick(10),
                source,
            },
        );

        // Act：同一来源再次施加，这一次更弱（|delta| = 2），但到期时刻
        // 更晚（50）。
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 2,
                expires_at: Tick(50),
                source,
            },
        );

        // Assert：强度仍是较强的 5（较弱的重复施放没能冲淡它），但到期
        // 时刻更新为两者中较晚的 50（弱化版本依然续了到期时刻）。
        let stored = world
            .actors
            .get(target)
            .expect("刚生成的实体必然存在")
            .active_stat_modifiers[&ll_world::entity::AttributeKind::Strength][&source];
        assert_eq!(stored.delta, 5);
        assert_eq!(stored.expires_at, Tick(50));
    }

    #[test]
    fn applystatmodifier效果中一个来源过期后另一个来源仍然独立生效() {
        // 验收「各条修正各自到期」——两个不同来源各自持有自己的
        // expires_at，一条过期不影响另一条是否仍然生效（这里直接检查
        // 存储层面两条记录都还在、各自的 expires_at 互不牵连；是否
        // 「生效」的现算判断由 derive_stats 负责，resolve.rs
        // 的 `一条来源过期后另一条来源的修正仍然独立生效` 覆盖那一层）。
        // Arrange
        let mut world = test_world();
        let agent = blank_agent(&world);
        let target = world.actors.spawn(agent);
        let (source_a, source_b) = test_two_sources("lostland:brace", "lostland:blessing");
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 4,
                expires_at: Tick(5),
                source: source_a,
            },
        );
        apply(
            &mut world,
            &Effect::ApplyStatModifier {
                target,
                attribute: ll_world::entity::AttributeKind::Strength,
                delta: 6,
                expires_at: Tick(200),
                source: source_b,
            },
        );

        // Act：不需要额外动作——两条记录的 expires_at 在写入时已经各自
        // 独立固定，惰性判定不要求任何清理动作就能观察到「各自到期」
        // 这件事本身（是否已过期由读取侧现比对，这里只断言两条记录各自
        // 保留了自己写入时的 expires_at，互不覆盖）。

        // Assert
        let per_source = &world
            .actors
            .get(target)
            .expect("刚生成的实体必然存在")
            .active_stat_modifiers[&ll_world::entity::AttributeKind::Strength];
        assert_eq!(per_source[&source_a].expires_at, Tick(5));
        assert_eq!(per_source[&source_b].expires_at, Tick(200));
    }

    #[test]
    fn markexplored效果把视野内的格子写入探索记忆() {
        // 探索记忆写入路径的最小验收：apply 落地 Effect::MarkExplored
        // 之后，原点自身（compute_fov 恒把原点纳入可见集合，见
        // ll_world::fov::compute_fov 文档）必须能在探索记忆里查到。
        // Arrange
        let mut world = test_world();
        let origin = world.size.wrap(10, 10);
        let layout = *world.terrain.layout();

        // Act
        apply(&mut world, &Effect::MarkExplored { origin, radius: 3 });

        // Assert
        assert!(world.exploration.is_explored(&layout, origin));
    }

    #[test]
    fn 已探索的墙格在玩家走远后仍然保留在探索记忆里() {
        // ADR 0007：对称阴影投射刻意接受「某些墙格的四角参与遮挡计算、
        // 自己却因中心恰好落在扇区外而不被标记可见」这个代价（见
        // ll_world::fov 模块文档「为什么墙本身可见（但不是每一面墙）」
        // 一节），靠探索记忆的「只增不减」兜底——玩家上次见过这面墙时
        // 已经记下来了，之后哪怕站在一个当下看不见它的位置，它也不该
        // 从地图上凭空消失。这条测试不复现那个具体的边界几何反例（那
        // 由 fov_blackbox.rs 的属性测试守护），而是直接锁住更根本的
        // 前提：探索记忆没有「取消标记」这个操作，一次不包含某格的
        // MarkExplored 不会把它从「已探索」改回「未探索」。
        //
        // Arrange：贴着出发点正东两格摆一面墙——与
        // ll_world::fov::tests::正对原点的墙可见 完全同一种布局，这类
        // 布局下墙必然进入可见集合。出发点与墙之间的地形显式改写成
        // 草地，不依赖噪声生成算法在这一带恰好给出可通行地形；玩家
        // 随后走到一个半径完全覆盖不到这面墙的远处。
        let mut world = test_world();
        let (terrain_ids, _table) = base_terrain_fixture();
        let near = world.size.wrap(10, 10);
        let between = world.size.wrap(11, 10);
        let wall = world.size.wrap(12, 10);
        let far = world.size.wrap(40, 40);
        world.terrain.set_terrain(near, terrain_ids.grass);
        world.terrain.set_terrain(between, terrain_ids.grass);
        world.terrain.set_terrain(wall, terrain_ids.wall_stone);
        let layout = *world.terrain.layout();

        // Act：先在墙跟前标记一次（墙进入可见集合、被记进探索记忆），
        // 再在远处标记一次（这次的可见集合完全不覆盖墙，但也不应该
        // 把上一次的标记抹掉）。
        apply(
            &mut world,
            &Effect::MarkExplored {
                origin: near,
                radius: 6,
            },
        );
        apply(
            &mut world,
            &Effect::MarkExplored {
                origin: far,
                radius: 3,
            },
        );

        // Assert
        assert!(world.exploration.is_explored(&layout, wall));
    }

    #[test]
    fn consumeinventoryitem效果对数量大于一的堆只减一() {
        // 耐久与 Intent::Use 落地批次（P6 第五批）。
        // Arrange
        let mut world = test_world();
        let mut agent = blank_agent(&world);
        let mut interner = ll_core::ident::Interner::new();
        let potion =
            interner.intern(ll_core::ident::NamespacedId::parse("lostland:potion").unwrap());
        agent.inventory.push(ItemStack::new(potion, 3));
        let actor = world.actors.spawn(agent);

        // Act
        apply(
            &mut world,
            &Effect::ConsumeInventoryItem {
                actor,
                def: potion,
                durability: None,
            },
        );

        // Assert
        let stack = world
            .actors
            .get(actor)
            .expect("刚生成的实体必然存在")
            .inventory
            .iter()
            .find(|s| s.def == potion)
            .expect("数量减到二,堆本身仍应留在背包里");
        assert_eq!(stack.count, 2);
    }

    #[test]
    fn consumeinventoryitem效果对数量恰为一的堆整条移除() {
        // 反例：与上一条测试成对——数量恰好是一时,消耗后不该留下一个
        // count == 0 的死堆（ItemStack.count 文档「恒 ≥ 1」的既有
        // 不变式），整条从背包移除,证明"只减一"不是无条件生效的分支。
        // Arrange
        let mut world = test_world();
        let mut agent = blank_agent(&world);
        let mut interner = ll_core::ident::Interner::new();
        let potion =
            interner.intern(ll_core::ident::NamespacedId::parse("lostland:potion").unwrap());
        agent.inventory.push(ItemStack::new(potion, 1));
        let actor = world.actors.spawn(agent);

        // Act
        apply(
            &mut world,
            &Effect::ConsumeInventoryItem {
                actor,
                def: potion,
                durability: None,
            },
        );

        // Assert
        assert!(
            !world
                .actors
                .get(actor)
                .expect("刚生成的实体必然存在")
                .inventory
                .iter()
                .any(|s| s.def == potion)
        );
    }

    #[test]
    fn adjustequipmentdurability效果扣减指定槽位的耐久() {
        // Arrange
        let mut world = test_world();
        let mut agent = blank_agent(&world);
        let mut interner = ll_core::ident::Interner::new();
        let armor = interner.intern(ll_core::ident::NamespacedId::parse("lostland:armor").unwrap());
        agent
            .equipment
            .insert(EquipSlot::BODY, ItemStack::with_durability(armor, 1, 10));
        let actor = world.actors.spawn(agent);

        // Act
        apply(
            &mut world,
            &Effect::AdjustEquipmentDurability {
                actor,
                slot: EquipSlot::BODY,
                delta: -3,
            },
        );

        // Assert
        let stack = world
            .actors
            .get(actor)
            .expect("刚生成的实体必然存在")
            .equipment
            .get(&EquipSlot::BODY)
            .expect("装备仍在槽位里");
        assert_eq!(stack.durability, Some(7));
    }

    #[test]
    fn adjustequipmentdurability效果钳位到零而不是负数() {
        // 反例：扣减量超过当前耐久时,不该产出负的耐久值——见
        // Effect::AdjustEquipmentDurability 文档「为什么钳位到非负」
        // 一节,证明钳位逻辑真的在起作用,不是恰好没被触发到。
        // Arrange
        let mut world = test_world();
        let mut agent = blank_agent(&world);
        let mut interner = ll_core::ident::Interner::new();
        let armor = interner.intern(ll_core::ident::NamespacedId::parse("lostland:armor").unwrap());
        agent
            .equipment
            .insert(EquipSlot::BODY, ItemStack::with_durability(armor, 1, 2));
        let actor = world.actors.spawn(agent);

        // Act
        apply(
            &mut world,
            &Effect::AdjustEquipmentDurability {
                actor,
                slot: EquipSlot::BODY,
                delta: -5,
            },
        );

        // Assert
        let stack = world
            .actors
            .get(actor)
            .expect("刚生成的实体必然存在")
            .equipment
            .get(&EquipSlot::BODY)
            .expect("装备仍在槽位里");
        assert_eq!(stack.durability, Some(0));
    }
}
