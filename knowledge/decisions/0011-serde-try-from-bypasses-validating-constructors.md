# 0011 — serde 派生会绕过校验构造函数，必须用 try_from 中转

**日期**：2026-08-17
**状态**：已生效的项目级规矩
**关键提交**：124f7f8（TorusSize，裁定 P2-6）、54bd4cf（TerrainKind）、4e01249（WorldState）
**影响范围**：任何「私有字段 + 返回 `Option`/`Result` 的构造函数」且需要 `Deserialize` 的类型

## 背景

存档是外部不可信输入：玩家会手改、文件会损坏、旧版本存档可能带来意料之外的值。规格 §14.3 要求「任何输入都不得 panic，只允许返回 `Err`」。

问题在于：**Rust 的私有字段访问控制在 `#[derive(Deserialize)]` 面前完全无效**。`serde` 的派生宏在类型定义所在的模块内展开，能直接访问私有字段——如果一个类型靠「私有字段 + 构造函数校验」保证不变式（例如 `TorusSize::new` 返回 `Option`，拒绝零尺寸），直接派生 `Deserialize` 会绕开构造函数，把任意值直接怼进私有字段，不变式因此可能被外部输入打破。

这类缺陷在本项目 P2 阶段接连出现了三次：

## 三次出现

1. **`TorusSize`（124f7f8，裁定 P2-6）**：宽高非零由 `new()` 保证。派生 `Deserialize` 绕过后，零尺寸的 `TorusSize` 混入会让 `wrap()` 里的 `rem_euclid` 直接除零 panic。
2. **`TerrainKind`（54bd4cf）**：`TerrainKind(pub u16)` 此前直接派生 `Deserialize`，任意 `u16` 都能直通。篡改档混入的未知 ID 会一路传到 `blocks_sight`/`blocks_move` 内部的 `debug_assert!(is_known())`——debug 构建直接 panic，release 构建 `debug_assert!` 不生效，未知地形被静默当成「可通行且透明的地板」，两种表现都违反 §14.3。
3. **`WorldState`（4e01249）**：`size` 与 `terrain` 的反序列化各自独立校验、互不知道对方存在。存档若被篡改成 `size=512x320` 而 `terrain` 实际只有 `64x64` 格，两个字段各自看都合法，唯独合在一起不自洽——按 `size` 遍历坐标去查 `terrain` 会直接越界 panic（`world.hash()` 就会触发）。这与 `TorusSize` 是**同一类缺陷、同一个修法，只是漏了它的邻居**：修 `TorusSize` 时没有意识到 `WorldState` 也需要交叉校验。

## 决定：`#[serde(try_from = "...Repr")]` 中转类型

统一修法：反序列化先落地成一个**没有不变式**的中转表示（`XxxRepr`，字段公开、无校验），再经 `TryFrom` 委托给原本的校验构造函数：

```rust
// crates/ll-core/src/torus.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "TorusSizeRepr"))]
pub struct TorusSize { width: u32, height: u32 }

#[cfg(feature = "serde")]
#[derive(serde::Deserialize)]
struct TorusSizeRepr { width: u32, height: u32 }

#[cfg(feature = "serde")]
impl TryFrom<TorusSizeRepr> for TorusSize {
    type Error = &'static str;
    fn try_from(raw: TorusSizeRepr) -> Result<Self, Self::Error> {
        TorusSize::new(raw.width, raw.height).ok_or("世界尺寸的宽高必须为正，且不超过 MAX_EXTENT")
    }
}
```

校验因此对反序列化路径同样生效，不给绕过的余地。**`Serialize` 不受影响，仍是直接派生**——序列化只是把已经合法的值写出去，没有校验可绕，中转类型只需要单向（`Deserialize` 方向）存在。

## 验证方式：故意关闭校验，确认测试会变红

三次修复都做了同一件事来证明测试真的在测这件事，而不是碰巧通过：**临时去掉 `try_from` 改回直接派生（或让 `try_from` 永远放行），对应的失败分支测试必须立刻变红；恢复后必须全绿。**

- `TorusSize`：去掉 `try_from` 后，零宽/零高两条「无法反序列化」的测试如期变红，合法值往返那条不受影响仍为绿；恢复后三条全绿。
- `TerrainKind`：让 `try_from` 永远放行，新增的失败分支测试立刻转红；恢复后转绿。
- `WorldState`：临时把交叉校验短路成 `false`，新增的失败分支测试立刻转红；恢复后转绿。

这不是可选步骤——若不做这一步，无法排除「测试只是没跑到失败分支」这种假阳性。

## 被否决的选项

1. **在文档或约定层面提醒「不要手改存档」**——否决：存档本来就是不可信输入的定义就是「有人会手改、文件会损坏」，口头约定不能防止 panic，规格 §14.3 明确要求任何输入不得 panic。
2. **读档后另写一次性校验函数，调用方手动调用**——未采用：这正是 `WorldState` 案例暴露的问题本身——`TorusSize` 已经修了，「漏了它的邻居」，说明依赖使用者记得调用额外校验函数是不可靠的。`try_from` 中转把校验编译期绑定到反序列化路径本身，任何新增的反序列化调用点都无法绕过，不依赖使用者的记忆。

## 归纳为规矩

P2→P3 交接清单把这条明确写成了后续阶段的检查项：

> 凡是「私有字段 + 返回 `Option`/`Result` 的构造函数」的类型，加 serde 派生时都会被 serde 从背后绕过构造校验。P3 往 `WorldState` 里加实体、时间轴、技能冷却时，每加一个这样的类型都要问一次：反序列化能不能造出非法值？

`TerrainKind` 的裁定还额外记录了一处细节：P2 尚无 mod 内容注册表时，`try_from` 拒绝一切未知地形 ID（包括未来 mod 会合法注册的自定义地形）。这是**已知且接受**的暂时性代价——此刻拒绝未知 ID 比静默接受更安全，P4 接入注册表后校验标准应从「是否是本体常量」改为「是否已注册」，这个待办已写入 `TerrainKind` 文档而非代码 TODO（理由见 [0009](0009-derive-by-default-store-only-deviation.md) 相关的「写进规格会被已生效机制自动捕获」纪律）。

## 后果

- **每多一个需要这条保护的类型，就多一个私有 `Repr` 中转结构**——样板代码随类型数量线性增长，没有更省事的写法：Rust 的类型系统不提供「反序列化必须经过这个函数」的语言级机制，`try_from` 已经是最小的间接层。
- **这类缺陷不是一次排查能穷尽的**：三次修复的模式完全相同（改了当前这个类型，漏了旁边结构相似的类型），说明真正需要的是一条会被反复问起的规矩，而不是一份「已修复列表」——列表会过期，规矩需要在每次新增类型时被重新触发。
- 若未来出现「性能敏感路径不适合每次反序列化都跑一次完整校验」的场景，需要重新评估这条规矩的适用边界；目前项目里出现的三个类型（世界尺寸、地形枚举、世界整体状态）校验成本都很低（几次比较或一次遍历），尚未遇到这个矛盾。
