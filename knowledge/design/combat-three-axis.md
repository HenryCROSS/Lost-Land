# 战斗结算：三条正交轴，不按武器类型分类

**冻结于** 2026-08-18。**落地状态**：纯设计，尚无代码——`crates/ll-sim/src/resolve.rs` 的 `resolve_attack` 仍是占位实现（见「一、现状核实」）。**实现阶段**：主体属新 P6「物品与装备」（装备属性接线）与既有 P3「回合与战斗」的后续批次；`Intent::Attack` 的形状变更与 `TargetSpec` 落地建议随 P6 一并完成（技能树消费本文档定义的武器/系别/投送模型属 P5，但 P5 已先冻结，技能树若需要新增瞄准形状/伤害系别，走注册表扩展，不改本文档的轴本身）。**冻结时对应 git 提交**：`1abb1d3`（本文档写作时的仓库 HEAD，即插入「物品与装备」阶段那次提交）。

---

## 一、现状核实（写作本文档前已去代码核实）

`crates/ll-sim/src/resolve.rs:181` 的 `resolve_attack` 目前是纯粹的占位：

```rust
fn resolve_attack(world: &WorldState, actor: EntityId, target: EntityId) -> Vec<Effect> {
    let attack_power = attacker.stats.strength;
    let damage = damage_after_defense(attack_power, 0, Penetration::NONE);
    // ……
}
```

三处占位，缺一不可地说明了问题：**攻击力只读 `strength`**（意味着魔法/精神攻击无法表达）、**防御恒为 `0`**（意味着护甲从未真正生效）、**穿透恒为 `Penetration::NONE`**（意味着四种穿透定义了却从未被消费）。`crates/ll-sim/src/intent.rs` 的 `Intent::Attack { actor, target: EntityId }` 同样只能表达单体目标，连「打中一个人」之外的任何形状都表达不了。

伤害公式本身（`damage_after_defense`，`crates/ll-sim/src/combat.rs`）与三系攻防、四种穿透的定义（[属性系统](attribute-system.md) §二、§三、§四）**已经设计好且已实现（公式部分）**——缺的不是数值怎么算，是「谁在打谁、用哪个系别打、防御从哪来」这几件事怎么接进已有的管线。本文档正面回答这几件事。

---

## 二、核心设计：三条正交轴，不按武器类型分类

**最容易做错的地方是按"近战/远程/魔法"这类武器类型分类去实现战斗**——那样每加一种新武器类型都要重新写一遍从瞄准判定到伤害结算的整条流程，`resolve_attack`/`resolve_fireball`/`resolve_chain_lightning`……每个函数各自处理自己的判定逻辑，代码重复且容易在某一个分支里漏掉别处已经修好的边界情况（例如伤害下限、穿透顺序）。

真正独立的其实是三个正交的维度：

| 轴 | 取值 | 决定什么 |
|---|---|---|
| **瞄准形状**（`AimShape`） | 单体 / 直线 / 圆形范围 / 锥形 / 手选多目标 | `resolve` 如何把一个 `TargetSpec` 展开成实际命中的实体集合 |
| **伤害系别**（`DamageSchool`） | 物理 / 魔法 / 精神 | 用哪一组攻防数值（[属性系统](attribute-system.md) §二 三系攻防）、哪一种穿透（§三 四种穿透之一） |
| **投送方式**（`DeliveryMode`） | 近战（需相邻）/ 远程（需视线 + 射程）/ 法术（可能无视视线） | 这次攻击是否合法：距离/视线前置检查 |

三轴可以任意组合，一个具体的攻击内容**只需要声明一个组合**：

```
火球 = 圆形范围 × 魔法 × 远程
劈砍 = 单体     × 物理 × 近战
链电 = 手选多目标 × 魔法 × 远程
毒刃 = 单体     × 物理 × 近战    （附加 on_hit 触发器施加中毒，见 buffs-and-triggers.md）
```

这正好落在 [ADR 0017](../decisions/0017-tiered-declarations-materialize-columnar.md) 定义的**第一档：静态声明，物化成列式表**——武器/技能的内容定义只是在注册表里填三个枚举字段外加数值（伤害基数、射程、穿透），不需要为每种武器写一段专门的 Rust 代码或脚本回调。这与 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md)「按声明方式分档，不按武器类型分类」是同一个原则在战斗系统上的应用：mod 作者新增一种武器，只要能用这三个枚举表达，就和本体武器一样零开销。

### 为什么不做成继承/trait 分派

一个自然的替代方案是给 `Weapon` 定义一个 trait，`MeleeWeapon`/`RangedWeapon`/`SpellWeapon` 各自实现。这条路会立刻在两个维度上重复代码：一把"远程物理弓箭"与一把"远程魔法法杖"共享"远程"的距离/视线判定逻辑，却要在两个不同的 trait 实现里各写一遍；反过来"物理"的护甲穿透判定又要在近战与远程两个实现里各写一遍。三轴分解把这两份重复各自收敛成一处：投送方式的判定只写一次（供全部系别复用），伤害系别的判定只写一次（供全部投送方式复用）。

---

## 三、落到管线上

```rust
// crates/ll-sim/src/intent.rs（本文档只给形状，不改代码——落地属新 P6）
pub enum TargetSpec {
    /// 瞄准一个具体实体（单体）。
    Entity(EntityId),
    /// 瞄准一个地面坐标（直线/圆形/锥形的落点或方向）。
    Tile(TorusPos),
    /// 手选多目标——玩家/AI 已经明确选出了一组实体，不需要 resolve 再展开。
    Set(Vec<EntityId>),
}

pub enum Intent {
    // …… 既有变体不变
    Attack {
        actor: EntityId,
        target: TargetSpec,
        /// 指向注册表里的武器/技能定义，决定伤害系别、投送方式、
        /// 瞄准形状、射程、基础伤害、穿透——见四、接线点。
        weapon: ContentIndex,
    },
}
```

`Intent` 依然很小——它不携带任何展开后的结果，只携带"谁、朝哪、用什么"这三样最小信息，与 [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) §四已经给出的 `Intent::Attack` 解禁形状是同一条思路的自然延伸（那份文档只解决了"目标从哪来"，本文档补上"目标可以不止一个"）。

`resolve` 负责把 `TargetSpec` 按武器声明的 `AimShape` 展开成一组实际命中的实体，对每一个命中实体产出一条 `Effect::Damage`：

```
resolve(world, Intent::Attack { actor, target, weapon })
  → 查 WeaponDef（weapon: ContentIndex）取 aim_shape / damage_school / delivery
  → 投送前置检查（见五）：不合法则返回空 Vec（与 resolve_open_door 目的地非门时同构：静默作废，不报错）
  → 按 aim_shape 把 target 展开成命中实体集合（见五）
  → 对集合按 EntityId 升序排序（见六：确定性展开顺序，不是可选步骤）
  → 逐个实体：算真实防御与穿透（见四）→ damage_after_defense → 产出 Effect::Damage
  → 追加 Effect::Kill（生命 ≤ 0 者）与 Effect::ScheduleNext（同现有 resolve_attack 收尾）
  → 返回 Vec<Effect>
```

**`Intent` 保持很小、`Effect` 仍是逐实体的朴素数据（约束 C2 不破）、`apply` 不用改**——`apply` 已经在按 `Damage`/`Kill` 逐条处理（`crates/ll-sim/src/apply.rs`），且目标不存在时静默忽略（同一批 `Effect` 里先 `Kill` 后 `Damage` 同一目标的情形，`apply` 已有既定行为，见该文件「统一选择静默忽略」一节），批量攻击展开出的 N 条 `Damage` 天然复用这条既有纪律，不需要 `apply` 增加任何分支。

---

## 四、接线点：装备属性 → `DerivedStats`

**这是三轴设计里唯一还没有具体实现依据的部分，必须点名交代清楚。**

真实的防御值必须来自装备，而不是占位的 `0`。接线点在：

```
resolve 阶段，对每个命中的防御方：
  defender_derived = derive_stats(
      defender.stats,           // BaseStats，已落地
      defender.equipped_items,  // 见下——依赖新 P6 装备系统
      defender.active_buffs,    // 见 buffs-and-triggers.md
      defender.carried_weight,  // 已在物品系统设计中，见 item-system.md
  )
  真实防御 = match weapon.damage_school {
      Physical => defender_derived.armor,
      Magical  => defender_derived.magic_resist,
      Mental   => defender_derived.will_resist,
  }
  穿透 = weapon 声明的对应 Penetration（Physical→破甲、Magical→破魔、Mental→破意；
         破盾单独作用于护盾层，见 attribute-system.md §二「护盾」，本文档不重复设计）
```

`derive_stats(基础属性, 装备, 状态效果, 负重) -> DerivedStats` 这个签名**已经在** [属性系统](attribute-system.md) §七写下，但那份文档明确标注"`装备`如何把 `ItemDef.stat_bonuses` 转成属性加成，两份文档都未给出具体消费逻辑"——这正是[设计文档总索引](README.md)概念对照表里 `StatBonus`"未正式定义"那一行（缺口 5）。**本文档不重新定义 `StatBonus`**，只指出接线点在哪：`derive_stats` 的"装备"入参需要是"当前已装备的 `ItemStack` 列表"（由装备栏位系统的 22 槽位遍历得出，见[装备栏位与占位掩码](equipment-slots.md)），逐件累加其 `stat_bonuses` 到基础攻防上，产出 `DerivedStats.armor`/`magic_resist`/`will_resist`。**这项累加逻辑的具体实现是新 P6 阶段的工作**，与 `StatBonus` 类型本身的正式定义一起补齐——本文档只钉死"三轴战斗结算要从 `derive_stats` 的输出里按伤害系别选一个字段当防御用"这一条接口约定，避免 P6 实现者在没有这份文档的情况下自己发明第二套接线方式。

---

## 五、可直接复用的三样——不要重新发明

### 视线：`fov::compute_fov`

远程/法术投送方式判断"能不能打到"，直接调用 `crates/ll-world/src/fov.rs:120` 的 `compute_fov(grid, table, origin, radius)`，取其返回的 `VisibleSet` 判断目标坐标是否在内。这个函数**已对称**（[0007](../decisions/0007-symmetric-shadowcasting-fov.md)），已被属性测试守护——不要为战斗系统另写一套视线判定，那会引入第二套可能与既有 FOV 不一致的几何实现，且拿不到既有的对称性保证。

- `delivery = 近战`：不查视线，改查距离（见下）是否为 1（相邻，环面 Chebyshev 距离）。
- `delivery = 远程`：必须视线 + 射程都满足。
- `delivery = 法术`：按具体技能声明（多数法术仍查视线，但设计上允许声明"无视视线"的例外，例如"感知系"法术）。

### 距离：`TorusSize::{chebyshev, squared_euclidean}`

`crates/ll-core/src/torus.rs` 的 `chebyshev`/`squared_euclidean`/`manhattan`/`delta` 已经处理了环面绕接缝取最短路径。**不要手写距离计算**——规格 §7.1 明确写着"禁止在任何地方手写欧氏距离，此项由 CI 静态检查强制"，本文档的射程判定、圆形范围展开全部走这几个既有函数，不新增第二套距离算法。

- 近战相邻判定：`chebyshev(attacker.pos, target.pos) == 1`（八方向都算相邻，与既有寻路/移动代价的方向定义一致）。
- 圆形范围展开：候选实体与落点的 `squared_euclidean` ≤ 半径的平方——与[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) §5.2「按距离」筛选算子（`filter-within-distance`）用的是**同一个底层比较**，理由相同：避免开方引入的跨平台浮点不确定性。

### 范围展开：与脚本层批量查询原语共用同一个 Rust 实现

[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) §5.2 已经设计了"半径内实体"这一类批量查询原语（`filter-within-distance`），供 Steel 脚本一次跨界拿到一批实体句柄。**`resolve` 展开圆形/锥形/直线范围时，不应该为战斗系统单独再写一遍"遍历所有实体、算距离、过滤"的循环**——这既是重复实现，也制造了"脚本侧範围查询"与"战斗结算范围展开"两条本该完全一致却可能悄悄跑偏的逻辑（例如一个用平方距离比较、另一个不小心用了开方近似）。正确做法：把"给定中心与半径，返回候选实体集合"抽成 `ll-world`（或 `ll-sim`）里的一个纯 Rust 函数，脚本层的 `filter-within-distance` 与本文档的范围展开**共享同一份实现**，脚本 API 只是这个 Rust 函数外面裹的一层 `Custom` 句柄跨界包装。这样"半径内有哪些实体"永远只有一个答案来源，不会出现战斗算出来命中三个人、脚本查询同一个半径却只看到两个人这类分裂。

- 直线：候选实体投影到攻击方向的射线上，误差在半格容差内（具体容差数值属数值设计范畴，本文档不给定案）。
- 锥形：候选实体相对攻击方向的夹角落在锥角内，且距离不超过锥形长度。
- 手选多目标：`TargetSpec::Set` 已经是最终集合，不需要展开，直接进入下一步排序。

---

## 六、确定性展开顺序：按 `EntityId` 升序，与时间轴平局规则一致

**必须钉死，不是可选的代码风格偏好**：`resolve` 把 `TargetSpec` 展开成命中实体集合后，**在产出 `Effect::Damage` 之前必须先按 `EntityId` 升序排序**，逐个按这个顺序产出效果。

### 为什么

`apply` 逐条消费 `Vec<Effect>`，若同一批效果里某个 `Damage` 把一个实体的生命打到 ≤ 0、紧接着触发 `Effect::Kill`，而后续 `Effect`（例如 [buffs-and-triggers.md](buffs-and-triggers.md) 描述的 `on_death` 触发器产出的连锁效果）依赖"这个实体死亡时，其余目标当时的状态"——**若两次运行里范围展开的顺序不同**（例如一次是"张三先死、李四的连锁反击没有目标而落空"，另一次是"李四先死、连锁反击命中了张三"），确定性回归测试会在这类边界场景下分叉，且分叉只在"中途恰好有人死亡触发连锁"这种概率性条件下才会暴露，极难在评审时肉眼发现，只有真正跑到那个边界才会现形——与[属性系统](attribute-system.md) §四"公式顺序错误只在边界值才暴露"是同一类教训。

### 与既有约定的关系

这不是本文档发明的新规则，是复用[脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) §5.5"确定性"一节已经定下的平局规则（"排序必须是全序，平局按 `EntityId` 升序打破"），而那份文档本身又是复用 `crates/ll-sim/src/timeline.rs`"同刻打破平局"一节解决同一个问题（同一 `Tick` 多个实体行动时的弹出顺序）的既有约定。三处（时间轴弹出顺序、脚本批量查询排序、本文档的范围展开顺序）用的是同一条纪律，理由完全相同：**排序结果只能由排序键 + 一个固定的、与输入到达顺序无关的平局规则决定**，`EntityId` 升序满足这一点（`Arena` 内部按槽位下标 + 世代号的字典序是稳定的既有 `Ord` 实现，不需要新增比较逻辑）。

---

## 七、开放问题与不属于本文档范围的事（如实标注）

- **`StatBonus` 的正式定义、`derive_stats` 的具体实现**——本文档只钉死接线点在哪，不重新设计这两样，见四、接线点一节。留给新 P6 阶段实现时与物品/装备系统一并补齐。
- **锥形/直线的具体几何容差数值**——本文档给出判定方式（投影/夹角），不给定案数值，数值设计范畴。
- **护盾（第四层，独立于三系之外）如何与三轴组合**——[属性系统](attribute-system.md) §二已提到护盾"临时护盾值优先承伤，可被破盾额外削减"，本文档的伤害系别轴不改变这条既有设计，护盾结算发生在 `damage_after_defense` 产出的伤害应用到生命值之前，具体扣减顺序留给实现时对照属性系统文档处理，本文档不重复设计。
- **技能树如何消费三轴**（例如某个天赋"物理攻击附加破魔穿透"这类跨系别效果）——超出本文档范围，属 P5 技能树设计，若确有需要，应作为一种"武器/技能定义可以声明多个伤害系别分量"的扩展，而不是打破三轴正交这个基本假设。

---

## 相关文档

- [角色属性系统](attribute-system.md) — 三系攻防、四种穿透、`damage_after_defense` 公式、`derive_stats` 签名（已定义，未实现装备消费逻辑）
- [物品系统](item-system.md) — `ItemDef`/`ItemStack`、`StatBonus` 缺口（缺口 5）
- [装备栏位与占位掩码](equipment-slots.md) — 22 槽位、装备如何转化为攻防数值（同样标注为未定义，本文档给出接线点但不重新定义）
- [脚本层数据句柄与批量查询](script-entity-handles-and-batch-queries.md) — `Intent::Attack` 解禁的既有设计、批量查询原语、确定性排序平局规则
- [增益/减益与通用触发器](buffs-and-triggers.md) — `on_hit`/`on_death` 等触发器如何与本文档的伤害结算衔接
- [ADR 0016 — mod 性能分档按声明方式，不按作者身份](../decisions/0016-mod-performance-tiers-by-declaration.md)
- [ADR 0017 — 声明式分档物化为列式数据，注册期完整校验](../decisions/0017-tiered-declarations-materialize-columnar.md)
- [ADR 0007 — 对称阴影投射视野及其墙可见性取舍](../decisions/0007-symmetric-shadowcasting-fov.md)
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) §7.1（禁止手写欧氏距离）、§4（约束 C2）
