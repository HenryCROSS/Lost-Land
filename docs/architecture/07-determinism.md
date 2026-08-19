# 确定性与可重放

**冻结时间**：2026-08-17，核对提交 `7a126f5`。**2026-08-18 补充**：规格新增约束 C5 后，本文档
补上了与 [`03-invariants.md`](03-invariants.md) C5 一节的交叉引用（HashMap 迭代顺序这条机制
本身此前已经记录在本文档，只是未与规格编号对应），其余内容仍以 `7a126f5` 为准。

规格称这是"整个游戏逻辑一次性解决的问题"之一，`knowledge/decisions/0002-integer-only-world-state.md`
称其为"要求同一个种子加同一串操作，在任何机器上都必须产出逐位相同的世界状态"。本文档汇总
代码里实际支撑这条性质的机制，以及一份"哪些常见写法会悄悄破坏它"的清单——后者比前者更重要，
因为破坏确定性的代码通常**编译通过、测试全绿、看起来完全正常**，只有跑跨平台黄金基准测试时
才会暴露，而那时往往已经不知道是哪次改动引入的。

## 支撑确定性的机制清单

| 机制 | 代码位置 | 解决什么问题 |
|---|---|---|
| 世界状态全整数，禁浮点 | 见 [`05-integer-discipline.md`](05-integer-discipline.md) | 消除跨平台浮点运算的最低位差异 |
| 确定性 RNG，按实体派生 | `ll_core::rng::DetRng::for_entity`（约束 C3，见 [`03-invariants.md`](03-invariants.md#c3--一切随机来自-hash种子实体-id事件计数)） | 消除全局流对调用顺序/线程调度的依赖 |
| 环面距离的稳定打平局规则 | `TorusSize::shortest_offset`（见 [`04-torus-topology.md`](04-torus-topology.md)） | 消除"两个方向等长时结果不确定"的边界情形 |
| 时间轴按值排序，不依赖插入历史 | `Timeline`（`crates/ll-sim/src/timeline.rs`，见下） | 消除堆内部数组顺序对存档字节的影响 |
| `apply` 唯一写入口 + `Effect` 应用顺序无关 | `crates/ll-sim/src/apply.rs`（见下） | 消除"谁先谁后改世界"这类隐藏的顺序依赖 |
| `BTreeMap` 而非 `HashMap` 存放需要确定性遍历的数据 | `EntityId` 派生 `Ord` 正是为此预留（见下） | 消除哈希表遍历顺序的不确定性（约束 C5，见 [`03-invariants.md`](03-invariants.md) "C5" 一节） |
| `Interner` 禁止遍历内部哈希表 | `ll_core::ident::Interner`（见下） | 同上，专门针对内容 ID 池 |
| FNV-1a 而非标准库 `DefaultHasher` | `ll_core::hashing::StateHasher`（见下） | 消除哈希算法本身跨版本/跨平台不稳定的风险 |
| 跨平台黄金基准测试 | `crates/ll-core/tests/determinism.rs`、`crates/ll-world/tests/determinism.rs` | 把"确定性是否被破坏"变成一行可自动运行的断言 |

## 逐项展开

### `Timeline` 的序列化：显式排序而非直接存内部堆结构

`crates/ll-sim/src/timeline.rs:115-125`：

```rust
impl Serialize for Timeline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<TimelineEntry> =
            self.heap.iter().map(|Reverse(entry)| *entry).collect();
        entries.sort();
        entries.serialize(serializer)
    }
}
```

模块文档解释了为什么不能直接序列化 `BinaryHeap` 的内部数组：

> `BinaryHeap` 的内部数组顺序是堆结构的实现细节，不是弹出顺序本身，若直接把内部数组写进
> 存档，存档的字节内容会依赖插入历史（同一批条目以不同顺序插入，内部数组可能不同），即使
> 弹出顺序其实一致。

这条机制同时也说明了确定性的一个微妙之处：**弹出顺序确定**（`TimelineEntry` 的 `Ord` 只由
`(at, actor)` 两个字段的值决定，与插入历史无关）**不等于存档字节确定**——后者还要求序列化
本身不泄露实现细节。反序列化时逐条 `schedule` 重新入堆（`timeline.rs:131-143`），因为弹出
顺序既然只由值决定，重建出的堆无论如何入堆都会产出与序列化前一致的弹出顺序。

### `Effect` 应用顺序无关性：由测试直接验证

`crates/ll-sim/src/apply.rs:191-218` 有一条测试专门验证"同一批 `Effect` 无论以何种顺序应用，
最终世界哈希一致"（对应规格 §14.2 表格里"Effect 应用"这一行的不变式）：

```rust
#[test]
fn 效果的应用顺序不影响最终世界哈希() {
    // ... 两个互不重叠位置的 SetTerrain
    let mut forward = test_world();
    apply(&mut forward, &effect_a);
    apply(&mut forward, &effect_b);

    let mut backward = test_world();
    apply(&mut backward, &effect_b);
    apply(&mut backward, &effect_a);

    assert_eq!(forward.hash(), backward.hash());
}
```

这条测试目前只覆盖了两个互不重叠的 `SetTerrain`——它验证的是"`apply` 本身不引入隐藏的顺序
依赖"，不是"任意一批 `Effect` 无论顺序都产出相同结果"（例如两个 `Effect::Damage` 打在同一个
目标上，顺序当然不影响总扣血量，但若未来出现"扣到负数就死亡"这类规则，`Kill` 与 `Damage` 的
相对顺序就可能真正影响结果——这类顺序依赖需要在 `resolve` 决定 `Effect` 顺序时显式处理，
不是 `apply` 层能兜底的）。

### `EntityId` 派生 `Ord`：为 `BTreeMap` 场景预留，且有真实的历史案例

`EntityId` 专门派生了 `Ord`（`crates/ll-world/src/entity/id.rs:33-39` 的派生列表），比较
顺序是 `(index, generation)` 字典序——模块文档说明这是"一个任意但稳定的全序，不必也不需要与
`as_u64` 的打包顺序一致"，只要求它是稳定的，不要求它有业务含义。这条派生存在的直接目的就是
让 `BTreeMap<EntityId, _>` 这类需要确定性遍历顺序的容器可用——`HashMap` 的迭代顺序不得参与
任何逻辑判断（约束 C5），遍历全体实体这类操作若需要确定性，必须走 `BTreeMap` 的键序而不是
哈希桶序。

这条纪律曾经有一个真实的用例：`WorldState` 一度用 `BTreeMap<EntityId, i32>` 存放各实体的
生命值（而不是 `HashMap`，理由正是上一段）。但这个字段后来被完全移除，改成 `Agent::health`
——不是因为 `BTreeMap` 选错了，而是因为这张独立旁挂表撞上了另一类风险（与哈希表遍历顺序无关）：
它不受 `Arena` 的世代号管辖，实体被 `despawn` 后旁挂表里的条目不会跟着清除，会积累出指向
不存在实体的孤儿记录。完整的因果链见 [`06-entity-storage.md`](06-entity-storage.md#世代索引之外的教训旁挂表与孤儿记录)。

这个案例值得放进"哪些写法会破坏确定性"的反面教材列表旁边，但要说清楚它教的是**另一条**纪律：
**"用了正确的容器类型"不代表"这份数据的生命周期管理就是安全的"**——两者是独立的正确性维度，
一个管"遍历顺序是否确定"，一个管"数据会不会变成脱离实体生死的僵尸记录"，前者做对了不能替代
后者。

### `Interner` 明确禁止遍历内部哈希表

`crates/ll-core/src/ident.rs:99-103`（`Interner` 结构体文档）：

> **不变式：内部的哈希表永远不得被遍历。** 索引只能来自 `to_id` 的插入顺序，而哈希表的遍历
> 顺序不保证跨运行稳定——一旦有任何逻辑依赖它，确定性存档与跨平台一致性会同时失效。若将来
> 需要枚举全部标识符，请遍历 `to_id`。

这是全仓库里把"HashMap 遍历顺序不可信"这条原则（约束 C5）表达得最直白的一处注释，也解释了
为什么 `Interner` 内部同时维护 `to_index: HashMap<..>` 与 `to_id: Vec<..>` 两份数据——`HashMap`
只用于 O(1) 查找（"这个字符串 ID 有没有登记过"），永远不用于遍历；需要顺序遍历的场景一律
走 `to_id`（一个按插入顺序排列的 `Vec`）。

### 为什么用 FNV-1a 而不是标准库的 `DefaultHasher`

`crates/ll-core/src/hashing.rs:1-16` 模块文档：

> `std::collections::hash_map::DefaultHasher` 的算法**不保证跨版本稳定**，标准库文档明确
> 说明它可能在任何 Rust 版本变更。用它做黄金基准，会在某次工具链升级后集体失效，而那时无法
> 区分是升级导致的还是真的引入了缺陷。

`StateHasher` 因此手写了 FNV-1a——算法由规范唯一确定，且全部由整数运算构成。实现里还有一处
容易被忽略的细节（`hashing.rs:44-53`）：`write_u64` **显式按小端序**逐字节混入，而不是依赖
`to_ne_bytes()`（本机字节序）：

> 必须显式指定字节序——依赖本机字节序会让大端平台产出不同的哈希，正好破坏本模块存在的意义。

## 跨平台黄金基准测试：这条防线怎么用

`crates/ll-core/tests/determinism.rs` 与 `crates/ll-world/tests/determinism.rs` 用固定的
黄金基准哈希值断言"某段确定性相关的计算，其摘要值恒等于某个具体的 64 位常量"。测试文件顶部
的规矩（`determinism.rs:1-14`）是本项目对这类测试**唯一**允许的处理方式：

> 若某次改动让这里的摘要变了，只有两种可能：1. 有意修改了算法或常量——那么更新期望值，并在
> 提交信息里说明为什么。2. **无意引入了平台相关行为**（最常见的是浮点运算，或依赖了哈希表的
> 遍历顺序）。这是必须立刻修复的缺陷。**绝不允许"测试挂了就把期望值改成实际值"**——那等于
> 删掉这道防线。

文件里确实发生过一次"有意修改"的真实案例（`EXPECTED_TIME_DIGEST` 的重冻，`determinism.rs:27-38`）：
`Tick::is_daylight()` 从固定的 `6..18` 小时边界改为由光照曲线阈值派生后，18 点这一采样点的
判定结果从 `false` 变为 `true`，测试作者在注释里写清楚了"这是算法本身的有意变更，不是平台
相关行为泄漏，期望值随之更新"——这正是规矩要求的"说明为什么"，而不是静默改数字。

## 哪些常见写法会悄悄破坏确定性（清单）

这份清单是本文档最实用的部分——每一条都对应真实的历史教训或代码里已经写明的红线，不是泛泛
而谈：

1. **在 `resolve`（或任何声称"只读"的路径）里使用全局/线程局部 RNG**，而不是
   `DetRng::for_entity(seed, entity_id, event_counter)`。见约束 C3
   （[`03-invariants.md`](03-invariants.md#c3--一切随机来自-hash种子实体-id事件计数)）。
2. **让渲染层算出的浮点值（子格插值、动画补间）写回 `WorldState` 的任何字段**。判断标准：
   这个值会不会被 `serde` 写进存档？会，就是错的。见
   [`05-integer-discipline.md`](05-integer-discipline.md)。
3. **用 `HashMap`/`HashSet` 存放需要确定性遍历顺序的世界数据，并遍历它**。只用于 O(1) 查找、
   从不遍历是可以的（如 `Interner::to_index`）；需要遍历就要用 `BTreeMap`/`Vec`（如
   `Interner::to_id`；`EntityId` 派生 `Ord` 正是为 `BTreeMap<EntityId, _>` 这类场景预留，
   见本文档上一节）。见约束 C5（[`03-invariants.md`](03-invariants.md) "C5" 一节）——这条此前
   只以约定俗成的形式活在代码注释里，2026-08-18 才正式编号进规格，历史上五处引用因此分裂标成
   了 C3/C4，详见 C5 一节。
4. **手写欧氏距离或直接比较原始世界坐标的大小/远近**，而不经过 `TorusSize::delta` 或
   `Camera::world_to_screen`。环面上原始坐标数值的大小关系在接缝处会反转（`DrawOrder` 的
   历史教训，见 [`04-torus-topology.md`](04-torus-topology.md)）。规格 §7.1 要求此项由 CI
   静态检查禁止手写欧氏距离——但据 `knowledge/handoff/p2-to-p3.md` 记录，这项检查截至 P2
   收尾时**尚未在 `.github/workflows/ci.yml` 里落地**（见 [`discrepancies.md`](discrepancies.md)），
   意味着这条红线目前只能靠评审拦，没有自动化兜底。
5. **让时间轴（或任何优先队列）的序列化直接暴露内部数据结构顺序**，而不是先转换成按值排序
   的形式。见本文档 `Timeline` 一节。
6. **在闭包或 trait 对象里携带需要跨帧/跨存档保留的状态**，而不是把状态显式放进
   `WorldState` 并让调度队列只存朴素数据。见约束 C2（[`03-invariants.md`](03-invariants.md#c2--时间轴队列只装朴素数据)）。
7. **"能跑多少跑多少"式的后台推进**，让离屏世界的推进量依赖墙钟时间而非确定的 tick 目标。
   见约束 C4——这条目前是尚未实现部分的红线，见 [`03-invariants.md`](03-invariants.md#c4--后台推进必须推到确定的-tick)。
8. **测试挂了就把黄金基准的期望值改成实际值**。这本身不是"破坏确定性"，而是"关掉能发现
   确定性被破坏的唯一自动化手段"——效果同样致命，且更隐蔽，因为改完之后测试会重新变绿。
