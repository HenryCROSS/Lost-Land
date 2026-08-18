# 物品系统

**冻结于** 2026-08-17。**实现阶段** P5，但**类型布局必须在 P2 建 `WorldState` 前定稿**。

**落地状态**：纯设计。`crates/` 中尚未找到 `ItemDef`、`ItemStack`、`Owner`、`ItemLocation`、`Quality`、`StatBonus` 等类型（已核实：全代码库检索无匹配）。本文档全部内容仍待 P2/P5 实现验证。

所有数值一律整数（见 [0002 世界状态一律用整数](../decisions/0002-integer-only-world-state.md)）。需要小数的量用 `Milli`（千分之一为单位）。

## 一、定义与实例分离

这是整个系统最关键的一刀。

| `ItemDef`（静态定义，注册表持有） | `ItemStack`（运行时实例，世界状态持有） |
|---|---|
| 命名空间 ID `lostland:iron_sword` | `def: ContentIndex` 指向定义 |
| 名称键 / 描述键（走 i18n，不存字面量） | `count: u32` 堆叠数量 |
| `stack_limit: u32` 堆叠上限 | `durability: Option<i32>` 当前耐久 |
| `base_weight: Milli` | `owner: Owner` 归属 |
| `base_price: Milli` | `quality: Quality` 实例品质 |
| `max_durability: Option<i32>` | `modifiers: Vec<ContentIndex>` 附魔词条 |
| `equip_mask: SlotMask` 装备占位，见[装备栏位与占位掩码](equipment-slots.md) | |
| `stat_bonuses: Vec<StatBonus>`，参与[属性系统](attribute-system.md)的派生 | |
| `use_effect: Option<ContentIndex>` 使用效果脚本 | |
| `tags: Vec<ContentIndex>` 标签（武器 / 消耗品 / 任务物品…） | |

**为什么必须分离**：一千支箭共享一份定义，运行时只需要一个 `count: u32`。若每支箭都是完整对象，背包一满存档就爆炸。这是「支撑大量实体」在物品层的对应解法。

**品质在实例上而非定义上**：同一把「铁剑」可以是粗糙的也可以是传说的。若品质写死在定义里，就要为每个品质档各注册一份定义，注册表直接膨胀六倍。

## 二、堆叠规则

**两个 stack 可合并，当且仅当 `def` 相同且全部实例状态相同。**

```rust
fn can_merge(a: &ItemStack, b: &ItemStack) -> bool {
    a.def == b.def
        && a.durability == b.durability
        && a.owner == b.owner
        && a.quality == b.quality
        && a.modifiers == b.modifiers
}
```

耐久 50/100 的剑不能和全新的剑堆在一起——它们实例状态不同。

这条规则真正的价值在于**新增任何实例字段都自动被覆盖**：以后给 `ItemStack` 加了「绑定角色」字段，只要补进这个比较，堆叠逻辑就自动正确，不会漏。

`stack_limit == 1` 的物品（武器、装备）永不合并，无需特判——它们的实例状态几乎必然不同。

## 三、归属

```rust
pub enum Owner {
    Unowned,                 // 野外掉落、无主
    Player,
    Npc(EntityId),           // 具名 NPC 的私产
    Faction(ContentIndex),   // 阵营公有：城镇仓库、卫兵装备
    Shop(EntityId),          // 商店库存
}
```

一个字段同时驱动三件事：

- **偷窃判定**：拿起 `owner` 非 `Unowned` 且非 `Player` 的物品即构成盗窃；被目击则触发治安反应。`Owner::Faction` 的归属语义由[社会系统](society-and-affiliation.md)的 `Affiliation` 定义。
- **随从装备归属**：给随从的装备仍可标记为 `Player`，随从叛离时按归属决定带不带走。
- **商店库存**：商店物品天然带 `Shop` 归属，不必另做一套库存系统。

## 四、位置

```rust
pub enum ItemLocation {
    Inventory { holder: EntityId, slot: u16 },
    Equipped  { holder: EntityId, slot: EquipSlot },
    Ground    { pos: TorusPos, dropped_at: Tick },
    Container { container: EntityId, slot: u16 },   // 箱子、尸体
}
```

拿起、丢下、交易、装备、存入箱子——**全部走同一个 `Effect::MoveItem { stack, from, to }`**。一个 Effect 覆盖所有物品流动，是「意图—结算—效果」架构在物品层的直接收益。

### 地面物品与老化清理

`dropped_at` 记录丢弃时刻。地面物品在丢弃满 **30 游戏日**且玩家不在附近时清除。

**这条清理正好搭在惰性追赶机制上**：远景区域本就在玩家靠近时才做一次性跳算，顺带扫掉过期地面物品，不额外花 CPU。

高价值物品（品质 ≥ 稀有、任务物品）标记为**永不清理**——玩家把传说武器摆在家门口当装饰是合理玩法，不该被系统吞掉。

## 五、品质

六档，`u8` 索引。倍率表由注册表提供，全部是**千分比整数**：

| 品质 | 属性倍率 | 价格倍率 | 耐久倍率 |
|---|---|---|---|
| 粗糙 | 800‰ | 500‰ | 700‰ |
| 普通 | 1000‰ | 1000‰ | 1000‰ |
| 精良 | 1200‰ | 2000‰ | 1200‰ |
| 稀有 | 1500‰ | 5000‰ | 1500‰ |
| 史诗 | 2000‰ | 15000‰ | 2000‰ |
| 传说 | 3000‰ | 50000‰ | 3000‰ |

倍率表可被 mod 覆盖——本体自己也是通过注册表提供这张表的（「本体即 Mod」）。

价格倍率作用后的结果如何进入[行会定价](agent-goals-and-economy.md)的「基础价」因子，两份文档均未写明换算关系，见总索引冲突清单。

## 六、耐久

- 当前 / 上限，均为 `i32`。
- **归零 = 损坏不可用，但不消失**，可修复。
- 无耐久概念的物品（材料、消耗品）用 `None`。

物品凭空消失是最招玩家恨的机制之一，而且与重量管理的策略性直接冲突——东西没了反而变轻，惩罚变成了奖励。

## 七、重量与负重

重量用 `Milli`。负重后果**分档而非线性**：

| 负重 | 后果 |
|---|---|
| ≤ 100% | 无 |
| ≤ 150% | 敏捷 −20%，行动耗时 +25% |
| ≤ 200% | 敏捷 −50%，行动耗时 +100%，无法奔跑 |
| > 200% | 无法移动 |

分档是刻意的：线性惩罚会逼玩家每拿一件东西都做算术；分档只需知道自己在哪一档，决策成本低得多。

这组百分比与[属性系统](attribute-system.md)「与时间轴调度的接口」一节的 `行动耗时 = 基础代价 × 1000 / 有效敏捷` 公式吻合：敏捷打 8 折，耗时自然是 1/0.8 = 1.25 倍；打 5 折，耗时自然是 2 倍。表中数字不是另拍的，是那条公式的直接结果。

## 八、物品作用

`use_effect` 指向一个 Steel 脚本 ID。脚本**只能返回 `Effect` 列表，不能直接改世界**——这是脚本沙箱纪律在物品层的落点。

```scheme
;; scripts/items/healing_potion.scm
(define (on-use actor item world)
  (list (effect-heal actor 50)
        (effect-consume item 1)))
```

## 相关文档

- [装备栏位与占位掩码](equipment-slots.md) — `SlotMask` 与多部位占位
- [属性系统](attribute-system.md) — `StatBonus` 如何参与派生
- [世界状态一律用整数](../decisions/0002-integer-only-world-state.md)
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md)
