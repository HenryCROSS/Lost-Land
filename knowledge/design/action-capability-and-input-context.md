# 行动能力与输入上下文：角色为什么动不了

**冻结于** 2026-08-20。**落地状态**：纯设计，`crates/` 中无任何对应类型——`crates/ll-platform/src/keybind.rs` 的 [`InputContext`](../../crates/ll-platform/src/keybind.rs) 目前只有 `Gameplay` 一个变体，`ll_world::entity::Agent` 没有 `active_buffs` 或任何「行动能力」字段，`crates/ll-sim/src/effect.rs` 六个变体不含增益/触发器相关内容（与 [buffs-and-triggers.md](buffs-and-triggers.md) 现状核实一致）。**冻结时对应 git 提交**：`7149122`（本文档写作时的仓库 HEAD，`main` 分支）。

**已核实的现状**（供复核）：

- `crates/ll-platform/src/keybind.rs`：`InputContext` 枚举、`KeyBindings::resolve`/`try_bind`、`(键, 修饰键, 上下文)` 三元组判重，均已落地并有测试覆盖。
- `crates/ll-platform/src/input.rs`：`InputState`（`held`/`just_pressed`/`repeat_next_at`/`repeated` 四个定长数组，按 `GameKey` 索引）与 `InputState::clear()`（窗口失焦时清空全部按键状态，已有完整文档说明与测试）。
- `crates/ll-sim/src/intent.rs`：`Intent` 枚举现有七个变体——`Wait`/`Move`/`Attack`/`OpenDoor`/`EnterSpace`/`ExitSpace`/`UseSkill`。
- `crates/ll-sim/src/resolve.rs`：`resolve_move`（`Intent::Move` 撞墙/撞门/`Interior` 内部漫游三种「无意义/静默作废」处理）、`resolve_attack`、`schedule_after`（`Tick(world.clock.0 + i64::from(cost))`）均已核实现状；撞墙目前**不产生任何效果**（不消耗 tick）——与本文档要设计的、项目所有者新裁定的「撞墙也消耗 tick」规则不一致，是一处尚待落地的差异，见五、2 节说明。
- `knowledge/design/buffs-and-triggers.md`：`ActiveEffect`（惰性到期判定，只存 `expires_at`）、`TriggerDef`/`TriggerResponse` 通用触发器，均纯设计。
- `knowledge/design/mod-lifecycle-and-event-api.md`（2026-08-20 同日冻结）：事件监听 API、装载期一次性 API，均纯设计。
- [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md)：确定性哈希必须覆盖 `WorldState` 全部影响玩法的字段，否则守护形同虚设。

---

## 零、起因：两件事，不是一类

项目所有者的原话把「眩晕/定身时按方向键」与「背包打开时按方向键」并列成一句话，但核实之后这是**两个完全不同层面的问题**，混在一起设计会两头不讨好：

| | 背包打开时按 W | 眩晕/定身时按 W |
|---|---|---|
| 问题出在哪 | 这个按键**不该产生游戏动作**——它现在的职责是「菜单上移」 | 这个按键**产生了游戏动作**，但角色的身体动不了 |
| 谁能观察到 | 只有正在看着这个 UI 的那个人（玩家） | 世界里的任何观察者——NPC 打 NPC 一样会晕 |
| 归属层 | 输入层：物理键 → 抽象动作这一步的映射本身就该变 | 模拟层：抽象动作已经变成了「移动」这个意图，只是这个意图在结算时失败了 |
| 要不要进 `WorldState` | 不要——UI 模式是本机会话状态 | 要——哪个实体现在不能干什么，是世界的事实，必须能被存档、被重放、被脚本查询 |
| 对 NPC 生效吗 | 不适用（NPC 没有键盘） | **必须生效**，否则「怪物打怪物、怪物打玩家」的眩晕效果就是假的 |

**核实结论：项目所有者的初步框架站得住，是正确的切入点**——见一节。下面按框架逐层设计，四节问题按顺序给结论。

---

## 一、三层框架复核

```
物理按键 →[输入上下文]→ 抽象动作 →[行动能力]→ 模拟动作 → 动画状态
 KeyCode    InputContext    GameKey   ActionCapability  Intent      Clip/Playback
（已落地）   （已落地，仅    （已落地）  （本文档新设计）  （已落地）   （部分落地，见
             一个变体）                                             animation-and-
                                                                    vfx-boundary.md）
```

这张对照表把所有者的框架精确对齐到既有类型名——五个箭头里已经有三个（`KeyBindings::resolve`、`GameKey`→`Intent` 的既有转译、`Intent`→动画）是已落地或已有独立设计的机制，本文档只需要**新增第二个箭头**（`GameKey`→`Intent` 之前的「这个上下文还要不要把这个键当回事」判断，属于既有 `InputContext` 分内的事，见二节）和**填上第三个箭头**（`Intent` 进 `resolve` 之后，「这个实体现在能不能做这类事」的判断，本文档称之为「行动能力」，见三节）。

**框架成立的核心原因**：这条链路上每一环回答的问题层次都不同，且**层次严格单调**——上游只回答「这个输入现在该不该被当成一个游戏动作」，下游只回答「这个已经成立的游戏动作，世界准不准它发生」。背包打开截断在第一环（`GameKey` 从未变成 `Intent::Move`，`resolve` 从未被调用）；眩晕截断在第二环（`Intent::Move` 已经产生，`resolve_move` 内部判定这个实体现在不能移动）。**两个触发点分别精确对应「背包」与「眩晕」两个所有者原始问题，不是巧合，是框架切分正确的证据**。

**唯一需要修正所有者原话的一处**：「行动能力」不是一个独立的架构层（不新增一个 `ll-xxx` crate 或一个新的调度阶段），它是 `resolve()` 内部、在检查地形/目标/空间类型之前多做的**一次读取**——与 `resolve_move` 已有的撞墙检查、`Interior` 检查处在同一个函数、同一个层级。这一点在五节的「一致性判据」会看到直接好处：撞墙判定与能力判定从此是同一段代码的两个分支，不是两套并行机制。

**动画层不需要为这两个场景开特例，核实成立**：无论是「背包吞掉了按键，`Intent::Move` 根本没产生」还是「`Intent::Move` 产生了但 `resolve_move` 因为能力不足返回空效果」，落到 `Effect` 流上的结果都是「这一步没有 `Effect::MoveTo`」——[animation-and-vfx-boundary.md](animation-and-vfx-boundary.md) 描述的动画层只消费 `Effect` 流，没有 `MoveTo` 就播 idle，天然覆盖这两种情况，不需要新增「眩晕动画特判」或「菜单打开时强制切 idle」这类专用分支。

---

## 二、输入上下文

### 2.1 模式栈还是单一模式：栈，但栈不属于 `InputContext` 本身

**结论：需要栈，但栈是 UI 层（`ll-ui`，P7 才建立）自己维护的一个 `Vec<UiMode>`，不是给 `InputContext` 本身加状态。**

理由：`InputContext` 现在的定位（模块文档已经写明）是 `KeyBindings` 冲突检测的判重维度——一个**无状态的分类标签**，`KeyBindings::resolve(key, modifiers, context)` 是纯函数，不关心「之前发生过什么」。若把嵌套语义（游戏中 → 背包 → 物品详情 → 确认框，Esc 逐层弹出）直接塞进 `InputContext` 本身，`KeyBindings` 就要开始关心"上一次的上下文是什么"，这会把一个纯查表结构变成一个有状态的导航栈，污染它本该只回答"这个键在这个标签下绑的是什么"的单一职责。

正确的分工：

```
UiMode 栈（ll-ui，P7，本文档不设计具体形状，只给出接缝）：
    栈空                → 当前 InputContext 隐式为 Gameplay（不需要真的压一条 Gameplay 进栈底）
    栈非空 → 栈顶决定：
        (a) 这个物理键该解析到哪个 InputContext 下查表（见 2.2，目前只有一种取值：Menu）
        (b) 解析出的 GameKey 该怎么被"读"——同一个 GameKey::Up，
            背包首页读成"选中上一件物品"，物品详情页读成"切换到上一个操作按钮"，
            确认框读成"切到 Yes"——这一层差异完全在 UiMode 内部，
            InputContext/KeyBindings 从不需要知道
```

**这就是为什么 2.2 节只需要新增一个 `Menu` 变体，而不是给背包首页、物品详情、确认框各开一个 `InputContext`**：物理键到 `GameKey` 的映射（`ArrowUp`/`KeyW` → `GameKey::Up`）在全部菜单类场景下是同一份表，会变的只是"栈顶的 `UiMode` 怎么解读这个 `GameKey`"，那是纯 UI 层的路由问题，不该反过来让 `InputContext` 为每一层嵌套都长出一个新变体——嵌套深度是运行时可变的（确认框可能是任意菜单页面弹出的），若「一层嵌套一个 `InputContext` 变体」，`InputContext` 会变成一个必须与 UI 树结构同构的东西，任何 UI 改版都要跟着改这个本该稳定的枚举，这正是模块文档已经警告过的 speculative generality 的反面——不是"设计不足"，是"设计过头"。

**被否决的方案：`InputContext` 自己变成一个栈（`Vec<InputContext>`）**。否决理由：`KeyBindings::resolve` 的签名会从"给定一个上下文查表"变成"给定一份历史查表"，`try_bind`/冲突检测的语义也要跟着变复杂（"在栈的哪个深度冲突才算冲突？"）——这是拿输入解析层的确定性去换 UI 导航层的方便,而 UI 导航层自己维护一个 `Vec<UiMode>` 完全够用,不需要下沉到 `ll-platform`。

### 2.2 现在需要哪些上下文：两个，`Gameplay` 与 `Menu`

**结论：新增 `InputContext::Menu` 一个变体，覆盖背包首页、物品详情、确认框等**全部**尚未建成的模态 UI 场景**——它们共享同一份"方向键=导航、Confirm=确认、Cancel=返回上一层"物理键映射，差异只在 2.1 节说的"栈顶 `UiMode` 怎么解读 `GameKey`"这一层，不需要各自的 `InputContext`。

```rust
pub enum InputContext {
    /// 既有变体，不变。
    Gameplay,
    /// 新增：任意模态 UI 覆盖游戏画面时的输入上下文——背包、物品详情、
    /// 确认框、未来的设置界面/暂停菜单，全部共用这一个变体。哪一层
    /// 具体在响应，由 UI 层自己的模式栈（见 2.1）决定，不是
    /// `InputContext` 的职责。
    Menu,
}
```

**为什么不现在就分「背包」「设置」「暂停菜单」三个变体**：这三者现在都不存在（P6/P7），提前分裂只会在真正建 UI 时发现"分错了"——例如设置界面可能需要文本输入（改名字/改端口号），那才是一种真正需要不同物理键映射的场景（字母键要打字，不是触发 `GameKey`），但那也是 P7 才需要面对的问题，现在没有实现,分不出正确的边界,不如先留一个 `Menu` 覆盖当前唯一已知的真实需求（背包类导航型 UI），把"文本输入是不是需要独立 `InputContext`"标记为**开放问题，留给 P7 设计设置界面时决定**。

### 2.3 上下文切换时按住的键：复用既有的 `InputState::clear()`，不新造机制

**这是本节风险最高的一处，给出可直接落地的结论，不留开放问题。**

`crates/ll-platform/src/input.rs` 的 `InputState` 已经暴露过完全同构的一个 bug 并修好了它——**窗口失焦**：玩家按住方向键时切到别的窗口，操作系统只把按键事件送给有焦点的窗口，对应的松开事件永远送不到，若不清空，`held` 永久为真，切回来后角色停不下来。修复是 `InputState::clear()`：一次性清空 `held`/`just_pressed`/`repeat_next_at`/`repeated` 四个数组，见其文档「窗口失去焦点时必须调用」。

**`InputContext` 切换是同一类 bug 的另一个实例，成因略有不同但后果完全相同**：

```
成因（失焦）：OS 层面松开事件送不到本窗口
成因（上下文切换）：松开事件送得到，但 held/repeat_next_at 是按 GameKey（resolve 之后的抽象动作）
                    索引的（见 InputState 字段文档），不是按 (KeyCode, InputContext) 索引——
                    W 在 Gameplay 与 Menu 两个上下文下多半会解析到同一个 GameKey::Up
                    （见 2.2 节：菜单复用同一份方向键映射），于是 held[GameKey::Up]
                    在上下文切换前后是同一个数组槽位，不会自动归零
```

**具体后果，两个方向都要处理**：

- **打开背包时 W 正按着（移动中）**：`held[GameKey::Up] == true`、`repeat_next_at` 已经跑到某个未来时刻。若不清空，背包一打开就立刻读到「`GameKey::Up` 已按住」，用一个**为移动场景设置的重复计时基准**触发菜单光标的自动重复——玩家没有再碰这个键，光标却自己开始往上滚，且滚动节奏是移动的节奏（`initial_delay`/`interval`），不是这一刻才「刚按下」该有的节奏。
- **关闭背包时 W 仍按着（玩家在菜单里按着方向键选东西，还没松手就关了菜单）**：对称的问题——回到 `Gameplay` 上下文后 `held[GameKey::Up]` 仍是 `true`，角色立刻开始移动,即使玩家从未在 `Gameplay` 上下文下按过这个键。

**结论：`InputContext` 每一次切换（栈的每一次 push 与 pop，2.1 节的 `UiMode` 栈变化）都必须调用一次 `InputState::clear()`，与失焦时完全同一个函数、同一套语义——不新增方法，不新增一套"上下文专用"的清空逻辑。**

这个结论等价于把「`InputContext` 变化」定义成第三种「隐式全键松开」边界（另两种是失焦、`InputState::new()` 初始状态）——玩家物理上仍然按着 W，游戏逻辑却把它当成"这一刻起 W 是松开的",直到玩家真正松开再重新按下才会被新上下文看见。这看起来"浪费"了一次已经按下的输入，但**任何试图保留这次按下的方案都会引入不对称**：要么打开菜单瞬间光标跳一格（继承了移动场景的计时基准），要么关闭菜单瞬间角色窜一格（继承了菜单场景的计时基准）——两者都是真实会发生的可感知 bug，而「清空、要求重新按下」没有任何不对称的方向，代价只是一次几乎不可感知的输入丢失（玩家的手指仍按着，游戏只是要求"再确认一次"，多数玩家不会察觉,因为他们本来就没打算在按下 W 的同一时刻打开背包——这两个动作通常是不同的按键触发的）。

**被否决的方案：给 `InputState` 加一份按 `(GameKey, InputContext)` 组合索引的独立 `held` 表，让每个上下文维护自己的按键状态**。否决理由：这能保留"背包里按住的方向键回到游戏时继续生效"这种字面意义上更精确的语义，但代价是 `InputState` 从"按固定数量的动作键"（`KEY_COUNT` 一个维度）变成"动作键 × 上下文"两个维度，且这份多出来的状态本身没有任何真实需求驱动——没有任何游戏设计文档要求"背包里按着 W 不松手,关闭背包后应该无缝继续移动"这种行为,这是纯粹的实现复杂度,换不来任何被要求过的手感。

### 2.4 要不要进配置：不要，YAGNI

**结论：`UiMode` 栈（2.1 节）与 `InputContext` 当前值都不进配置文件，也不进存档。**

- 不进存档：与既有 `KeyBindings` 模块文档「持久化：进配置，不进存档」一节同一条纪律的直接延伸——UI 导航状态不是世界状态,`resolve`/`apply` 从不读它,不该出现在 `WorldState::hash()` 的输入里（呼应五节的一致性判据表：背包打开这件事对世界而言"什么都没发生过"）。
- 不进配置：「上次打开的是哪个页签」这类需求现在没有任何设计文档提出过,`ll-mod`/`ll-ui` 都还没建成,提前决定"要不要记住上次页签"是在给一个不存在的 UI 猜它未来的交互细节——违反 `coding-style.md` 的 YAGNI 一条,与所有者的初步倾向一致,本文档不推翻,只补一句理由：如果未来真的需要（例如设置界面页签很多,记住上次选择确实提升体验）,那也是**这份配置系统自己的一个字段**,不需要现在为它预留任何接口——`InputContext`/`UiMode` 本身不需要为"未来可能被存进配置"这件事改变形状。

---

## 三、行动能力

### 3.1 形状：位标志集（bitflags），不是枚举集合

```rust
/// 一个实体当前能不能执行某一类动作——纯派生值，见 3.3 节，绝不直接
/// 存进 `Agent`。
///
/// 位标志而非 `HashSet<ActionKind>` 或 `Vec<ActionKind>`：
/// - 判定"能不能做 X"是 `capability.contains(ActionCapability::MOVE)`，
///   一次位运算,不是集合查找。这个判定在 `resolve()` 的热路径上——
///   每一次 `Intent` 结算都要问一次,量级与「AI 目标—需求—任务—悬赏
///   循环」（`agent-goals-and-economy.md`）同一数量级,` HashSet` 的哈希
///   开销在这个频率下不是免费的。
/// - 与既有的 `SlotMask`（`equipment-slots.md`，22 槽位位掩码）是完全
///   同一个模式在另一个场景的复用——本仓库已经确立"离散、数量少、
///   频繁做交并补运算的分类"用位标志,不是第一次引入这个惯用法。
/// - 整数运算,天然满足「不得引入浮点」（ADR 0002 整数世界状态）——不
///   需要为此专门论证,位运算本来就不涉及浮点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionCapability(u8);

impl ActionCapability {
    pub const MOVE: ActionCapability   = ActionCapability(0b0001);
    pub const ATTACK: ActionCapability = ActionCapability(0b0010);
    pub const CAST: ActionCapability   = ActionCapability(0b0100);
    pub const ITEM: ActionCapability   = ActionCapability(0b1000);
    /// 默认：不受任何限制。
    pub const ALL: ActionCapability = ActionCapability(0b1111);
}
```

**被否决的方案：一个 `enum ActionState { Normal, Stunned, Rooted, Silenced }` 封闭枚举**。否决理由：眩晕≠"定身+沉默+禁攻击"这三者的简单并集在枚举里天然成立,但下一个需求一来（例如"缠绕：禁移动+禁施法,但能普通攻击"）就要么塞进枚举炸出组合爆炸（4 类动作理论上 16 种组合,枚举方式每加一种新组合都要加一个变体）,要么退化成一堆 `bool` 字段各自独立——位标志集从一开始就是"任意子集"的正确表达,不需要为组合预先枚举。

### 3.2 粒度：四类，与既有 `Intent` 变体一一对应

| `ActionCapability` 位 | 覆盖的 `Intent` 变体（`crates/ll-sim/src/intent.rs`，均已落地） | 典型限制它的效果 |
|---|---|---|
| `MOVE` | `Move`、`OpenDoor`、`EnterSpace`、`ExitSpace`——凡是"这个身体想挪到别的地方/别的空间"的意图 | 定身（禁）、缠绕（禁）、眩晕（禁,与其余三类一起） |
| `ATTACK` | `Attack` | 缴械（禁,未来装备系统落地后可能细分"禁近战但不禁施法"，当前先粗粒度） |
| `CAST` | `UseSkill` | 沉默（禁） |
| `ITEM` | 目前**没有对应的 `Intent` 变体**——P6「物品与装备」落地后 `Intent::UseItem`/`Intent::Equip` 等新增时接上，现在只占位声明这一位，不接线 | （P6 之后才有意义） |

`Intent::Wait` **刻意不受任何 `ActionCapability` 约束**——"什么都不做"永远可以选择,不存在"连等待都不被允许"这种状态；若某个设计确实需要"完全无法行动、连回合都跳过"的效果（多数游戏里眩晕的真实观感其实是这个）,那不是靠 `ActionCapability` 挡住 `Intent::Wait`,而是靠调度层（`TurnEngine`/AI 决策层）**根本不为这个实体生成任何 `Intent`**——这与二节区分"输入层拦截"和"结算层拦截"是同一层意义上的判断：AI 决定"这一回合不产生任何意图"是决策层的事,不是 `resolve()` 该管的,`resolve()` 只回答"如果确实收到了一个 `Intent`,准不准"。

**四类是否够用，如实标注**：所有者举的三个例子（眩晕=禁全部、定身=只禁移动、沉默=只禁施法）全部落在 `MOVE`/`CAST` 两类的组合里,`ATTACK`/`ITEM` 是按对称性（"既然移动和施法都能单独禁,攻击和用物品也该能"）与已有 `Intent` 变体清单补全的,不是凭空猜的第五、第六类——**这四类与当前七个 `Intent` 变体是满射**（每个变体都能归到其中一类,`Wait` 除外,已说明原因）,没有遗漏,也没有为不存在的动作预留分类。

### 3.3 谁来改它：没有人直接改，纯派生，绝不存储

**这是「默认派生，只存偏差」（[ADR 0009](../decisions/0009-derive-by-default-store-only-deviation.md)）在本文档的第十二个实例，与 `DerivedStats`（衍生属性）同一形状。**

```rust
/// 现算,不存进 Agent、不进 hash()——真正需要入 hash() 的是它读取的
/// active_buffs（一旦 buffs-and-triggers.md 落地）,见 3.4 节。
fn current_capability(agent: &Agent, tick: Tick) -> ActionCapability {
    agent
        .active_buffs
        .iter()
        .filter(|effect| tick < effect.expires_at)          // 惰性判定，buffs-and-triggers.md 一节
        .filter_map(|effect| buff_registry.get(effect.def))  // 查注册表拿这条增益的定义
        .fold(ActionCapability::ALL, |caps, def| caps.difference(def.restricts))
}
```

**现在（buff 系统未落地前）的最小可用形状**：`Agent` 还没有 `active_buffs` 字段,`current_capability` 现在只能恒返回 `ActionCapability::ALL`——这与 `buffs-and-triggers.md` 本身"纯设计,尚无代码"的状态完全一致,本文档不提前造一个字段。**`resolve()` 现在就可以接上这个函数的调用点**（三节开头"检查点位置"）,哪怕它现在恒真——这正是本文档要交付的东西：不是"现在能不能屏蔽玩家移动"（现在还不能,也不该能,因为没有任何游戏内容会产生这类效果）,而是"未来某个 `BuffDef` 一旦声明了限制,`resolve()` 该从哪个函数、以什么形状读到这个限制"这条接缝先定好。

**将来 buff 落地时怎么接上**：`buffs-and-triggers.md` 现有的 `TriggerDef`/增益注册表只缺一个字段——给增益的定义（暂命名 `BuffDef`,该文档尚未给出具体类型名,只给了 `ActiveEffect` 这个"实例"侧的形状）加一个 `restricts: ActionCapability` 字段,默认 `ActionCapability::ALL`（即"不限制任何行动，只改属性的增益,例如力量加成,`restricts` 恒为 `ALL`"）。**这不需要新的机制**——`buffs-and-triggers.md` 二节已经确立"`derive_stats` 的入参新增'生效增益'一项,现算最终 `DerivedStats`",`current_capability` 是完全同构的第二个"现算函数",读同一个 `active_buffs`,只是折叠出的目标类型从 `DerivedStats` 换成 `ActionCapability`——不需要新的存储、不需要新的失效判定逻辑,`buffs-and-triggers.md` 一节的惰性到期判定天然覆盖它。

### 3.4 必须进 `hash()`：进的是 `active_buffs`，不是 `ActionCapability` 本身

**直接呼应 [ADR 0022](../decisions/0022-guard-coverage-gap-defeats-the-guard.md)「不完整的确定性哈希等于没有哈希」**：`ActionCapability` 本身是纯函数的返回值（3.3 节),不存储,因此**不需要、也不应该**单独出现在 `WorldState::hash()` 的输入里——把一个现算值也塞进哈希是重复存储,违反 ADR 0009 同一条纪律（「默认派生只存偏差」的反面就是"不要把派生值也当成需要哈希覆盖的独立事实"）。**真正需要覆盖的是它读取的 `active_buffs: Vec<ActiveEffect>` 字段**——这是「真正的偏差」（`buffs-and-triggers.md` 原文用语),`ActiveEffect` 的四个字段（`def`/`expires_at`/`stacks`/`applied_at`/`source`）全部是整数/`ContentIndex`/`EntityId`,天然不含浮点,与 ADR 0022 的要求完全兼容——**这条要求本文档不是新提出的,是把 ADR 0022 的通则应用到 `active_buffs` 这一个具体字段上,提醒未来落地 buff 系统的人：`active_buffs` 一旦从 `Agent` 里长出来,必须在同一次改动里进 `WorldState::hash()`,不能像 `player_entity` 那次一样分两次提交（见 ADR 0022 实例一)。**

### 3.5 必须对 NPC 同样生效：检查点写在函数入口，不写在任何"是否是玩家"的分支里

**结论：`current_capability`/能力检查必须插在 `resolve_move`/`resolve_attack`/`resolve_use_skill` 各自函数体的最顶端,在读取 `agent`（`world.actors.get(actor)`）之后、做任何地形/目标判断之前——这个位置对任何 `actor: EntityId` 一视同仁,不存在"如果是玩家就检查、如果是 NPC 就跳过"这种分支的容身之处。**

```rust
fn resolve_move(world: &WorldState, actor: EntityId, dir: Direction) -> Vec<Effect> {
    let Some(agent) = world.actors.get(actor) else {
        return Vec::new();
    };
    // 新增：能力检查，任何 actor（玩家或 NPC）一视同仁——见 3.5 节。
    if !current_capability(agent, world.clock).contains(ActionCapability::MOVE) {
        return failed_attempt(world, agent, actor);  // 见五节，返回值与撞墙同构
    }
    // ……原有的 Interior 检查、撞墙检查、地形检查，不变
}
```

**为什么这个位置结构性保证了 NPC 同等生效，而不是靠约定**：`resolve_move` 现有代码里**唯一**一处按"是不是玩家"分支的地方是尾部的 `MarkExplored` 追加（`world.player_entity == Some(actor)`,见其文档"为什么只有玩家移动才追加"一节)——那是探索记忆的天然玩家专属语义,理由清楚且不可复用。能力检查插在函数最顶端、在这处玩家分支**之前**,意味着代码读起来就能确认"这一条判断先于任何'谁是玩家'的知识",不需要额外测试去证明"NPC 也会被挡住"这件事——它在结构上就不可能不生效,因为函数压根还不知道 `actor` 是谁的时候就已经做完了这个检查。`resolve_attack`/`resolve_use_skill` 同理,检查点插在各自函数体最顶端。

---

## 四、一条一致性判据：撞墙、眩晕、背包是不是同一条规则

### 4.1 判据本身：核实成立

**判据**：「`resolve()` 有没有被以某个 `actor` 的某个 `Intent` 调用」决定消不消耗 tick——调用了（无论结算成功还是失败）就消耗,没调用（`Intent` 从未产生）就不消耗。这比所有者原话「游戏世界有没有把这当成一次动作尝试」更精确一层：**"动作尝试"的判定点不是模糊的直觉,是具体的一次函数调用**。

| 情形 | `resolve()` 是否被调用 | 消耗 tick？ | 判据落点 |
|---|---|---|---|
| 撞墙 | 是（`resolve_move` 被调用,`blocks_move` 分支判定失败） | **消耗**（所有者已定,当前代码尚未实现,见头部「已核实的现状」） | 结算层内部失败 |
| 被眩晕时按 W | 是（`GameKey::Up` 在 `Gameplay` 上下文下正常产生 `Intent::Move`,`resolve_move` 被调用,3.5 节的能力检查判定失败） | **应该消耗，见 4.2 节完整论证** | 结算层内部失败,与撞墙**同一段代码、同一种"静默作废"结果** |
| 背包打开时按 W | **否**（二节：`Menu` 上下文下 `GameKey::Up` 根本不会被翻译成 `Intent::Move`,`resolve()` 从未被调用） | 不消耗 | 输入层,`resolve()` 尚未被触及 |

**三行结论与所有者的判据完全吻合，且给出了"为什么"的机制解释,不只是复述结论**：背包这一行"不是动作尝试"不是因为"背包打开时角色不想动",是因为**这次按键根本没有走到会产生 `Intent` 的那条路径上**——`resolve()` 的调用点在时间线上从未发生。

### 4.2 眩晕行是不是反直觉：不是，但有一条必须钉死的前提

**这是本节真正的风险点，所有者点名要核实的地方，给出完整论证。**

`crates/ll-sim/src/resolve.rs` 的 `schedule_after(world, cost) = Tick(world.clock.0 + i64::from(cost))`——**每个实体的下一次行动时刻,是"当前世界时钟 + 这一步的行动耗时",不是"上一次行动时刻 + 耗时"**。这意味着"消耗 tick"字面上做的事是"把这个实体自己的下一次行动往后推 `cost` 格",不是直接推动全局时钟。真正推动全局时钟前进的是调度器（`TurnEngine`,时间轴优先队列,规格 §8）挑选"下一个该行动的实体",把世界时钟推进到那个实体的 `next_action_at`。

**反直觉的风险场景是什么，精确定位**：如果一次"眩晕时失败的移动尝试"收费**低于**一次正常动作（例如若实现者图省事,给失败尝试一个很小的固定耗时,类似"撞一下不算数,几乎不耗时间"）,那么被眩晕的玩家会发现——只要疯狂按方向键（每次都失败,但每次都只扣一点点耗时）,自己的 `next_action_at` 会比"老老实实按 `Wait`"推进得**更慢**,于是自己能在同样一段游戏时间窗口内"多按几次键、多等几个回合",而由于眩晕的 `expires_at` 是一个**绝对世界时钟刻度**（`buffs-and-triggers.md` 一节),不是"再让你等 N 个回合"这种相对计数器,疯狂按键实际上不会让眩晕**提前解除**——但会让玩家在眩晕解除之前**多操作几次**,体感上像是"用按键頻率换来了额外的行动机会",这确实是不该出现的行为。

**结论与必须钉死的前提**：**"被眩晕/定身挡下的失败尝试"，`cost` 不能低于一次 `Intent::Wait` 的耗时**（即 `action_cost(BASE_ACTION_COST, speed)`,与 `resolve_wait` 用的公式完全一样)——这条约束必须在实现时显式写成一条测试断言,不能只是注释里的一句话。理由：`Intent::Wait` 是"什么都不做也要花的最低时间成本"这条基线,任何"尝试了但失败"的行动不该比"压根没打算做任何事"更便宜——否则会激励玩家在任何不确定能不能行动的场合都倾向于"随便按点什么试试",而不是老老实实等待,这是一个明确的反模式,必须靠 `cost` 下限结构性堵死,不能指望玩家"自觉不去卡bug"。**撞墙同理**——四节的判据表把"撞墙"和"被眩晕挡下"归成同一类,这条 `cost` 下限约束对两者同样适用,不是眩晕专属的特殊规则。

**满足这条前提之后，为什么不反直觉**：一旦失败尝试的 `cost` ≥ `Wait` 的 `cost`,疯狂按键与老老实实按 `Wait` 在"消耗多少世界时间换来一次操作机会"这件事上**完全等价**——玩家能操作的次数由"眩晕还剩多少世界时间"除以"每次操作最低耗时"决定,与具体按了哪个键无关。按键频率不再是一个可以被"优化"的变量,眩晕挡下的移动尝试退化成"一次更贵的、什么都没发生的 `Wait`",体感上与"卡在原地干等"完全一致,这正是眩晕应有的观感。

### 4.3 会不会与「眩晕本身跳过若干回合」重复计时：不会，且原因是既有设计的正确选择

**核实结论：不重复,且不重复的原因不是巧合,是 `buffs-and-triggers.md` 一节"惰性到期判定,不排进时间轴"这条既有设计本身正确避开了这个陷阱。**

`ActiveEffect.expires_at` 是一个**绝对**世界时钟刻度,在增益被施加的那一刻算出来（`tick_at_apply + duration`）,此后**永不因为任何后续事件而改变**——它不是一个"还剩 N 个回合"的相对计数器,更不会因为角色本身的行动次数而递减。这意味着：

```
若眩晕是"绝对到期时刻"（buffs-and-triggers.md 既有设计）：
    角色这段时间里做了几次失败尝试、每次尝试花了多少 tick，
    都只影响"角色自己的 next_action_at 往后挪多少"，
    不影响 expires_at 这个早已钉死的绝对值——两套时间记账完全独立，
    互不干扰，天然不会重复计时。

若眩晕是"跳过接下来 N 个回合"（相对计数器，本文档未采纳的假想实现）：
    "回合"本身如果被定义成"这个角色的下一次 resolve() 调用"，
    那么"失败尝试也算一次回合"与"眩晕要跳过 N 个回合"就会产生
    真实的重复计时问题——疯狂按键每次都触发一次"回合"，
    N 个回合很快就被"用掉"，眩晕反而会被提前经过的"回合数"用尽，
    这才是真正会出现"卡bug"的实现方式。
```

**结论**：`buffs-and-triggers.md` 选择「惰性判定 + 绝对到期时刻」而不是「排进时间轴的到期事件」（该文档一节的原始论证是为了让约束 C4 的后台跳跃推进正确），**这个选择顺带也是躲开"按键消耗 tick 与眩晕跳过回合重复计时"这个陷阱的正确姿势**——本文档不需要为此再发明任何新机制,只需要在 3.3 节把 `ActionCapability` 接到同一个 `active_buffs`/`expires_at` 上,这条"不会重复计时"的性质就自动继承过来。**这是本文档确认既有设计选对了的一个额外证据,不是本文档新增的保证。**

---

## 五、与既有设计的接口

### 5.1 `buffs-and-triggers.md`：行动能力长在哪

**结论：`ActionCapability` 不是 `buffs-and-triggers.md` 需要重新设计的新系统,是给它现有的"增益定义"（该文档尚未命名的、`ActiveEffect.def` 指向的那张注册表条目类型）加一个新字段 `restricts: ActionCapability`,默认 `ALL`（3.3 节已给出完整方案）。惰性到期判定、多重增益的确定性合并顺序（该文档二节"按 `def` 升序,同 `def` 再按 `applied_at` 升序"）、堆叠策略 `StackPolicy`——这些既有设计原样复用,`ActionCapability` 的折叠（3.3 节 `fold`）用的正是同一个已排序的 `active_buffs` 遍历,不需要为"能力限制"单独维护一套排序或堆叠规则。**

**唯一需要 `buffs-and-triggers.md` 未来落地时补一句的地方**：多个增益同时限制同一类行动（例如眩晕禁 `ALL`,同时又中了一个只禁 `MOVE` 的定身）——`ActionCapability` 用位运算 `difference`（交集取反）天然满足"多重限制取并集"（越多限制,能做的事越少,`ALL.difference(A).difference(B)` 恒等于 `ALL.difference(A ∪ B)`),不需要额外的合并顺序规则,这与 `DerivedStats` 需要"钉死结算顺序"的原因不同——数值加成的顺序会影响结果（加法 vs 乘法),但"能不能做某类事"是布尔交集,天然满足交换律,不存在"先禁后禁结果不同"的问题,这点值得在 `buffs-and-triggers.md` 真正接线时明确记一句,避免有人误以为需要复用它的排序规则。

### 5.2 `mod-lifecycle-and-event-api.md`：mod 能不能定义新东西

**结论分两半，不对称，理由不同。**

**mod 能不能定义新的行动能力类别：能，但走的是"预留位号"，不是"注册表条目"，与 `equipment-slots.md` 的 `SlotMask` 剩余位留给 mod 完全同一个先例。**

`ActionCapability` 现在只用了 `u8` 的低四位（`MOVE`/`ATTACK`/`CAST`/`ITEM`),本体保留低半字节,高半字节（四位）留给 mod 扩展新类别（例如某个 mod 加一种"骑乘"动作,需要"下马时不能骑乘技能"这类新限制)——**这不需要走内容注册表（`ContentIndex`）那一套"字符串 ID→索引,装载期物化"的机制**,理由是 `ActionCapability` 从头到尾只是一组**运行期位运算**,不是"内容"（不像 `TerrainDef`/`RaceDef` 那样有数值表、有本地化名字、需要被查询展示给玩家)——mod 若要新增一类行动,只需要在自己的 Rust 侧（如果 mod 未来允许原生扩展,当前 mod 全部走 Steel 脚本,这里如实标注**这是一个开放问题**：Steel 脚本本身没有"定义一个新的 `enum`/位标志"这种能力,mod 若要用高四位,需要宿主先在 Rust 侧预留一个"mod 自定义能力位"的注册接口,形状类似 `register-custom-capability! "mymod:mounted" 5`（第 5 位),这个接口本身**不在本文档设计范围内**,与 `equipment-slots.md`"装备槽位剩余 10 位留给 mod,位号由本体注册表分配"是完全同一个模式,可以直接照抄那份设计的分配机制,不需要另起一套。

**mod 能不能定义新的输入上下文：不能，这是一条明确的边界，理由与位标志相反。**

`InputContext` 是 `KeyBindings`（`ll-platform`,不依赖 `ll-world`/`ll-sim`,更不依赖 `ll-mod`）冲突检测的判重维度,是一个**编译期确定的封闭枚举**,不是内容注册表条目——若允许 mod 动态新增 `InputContext` 变体,`KeyBindings::try_bind` 的冲突检测（"这个键在这个上下文下有没有被占用"）就要在运行期处理一个开放集合,且**这样的场景现在不存在**：mod 目前完全没有渲染/UI 层的接入点（`ll-mod` 依赖顺序在 `ll-render`/`ll-ui` 之下,规格 §5 crate 分层),mod 想弹出一个"只有 mod 自己知道存在"的输入上下文,意味着 mod 要能画自己的 UI 屏幕并让宿主的输入系统认识这个屏幕——**这本身是一个此前从未被任何设计文档正面处理过的更大问题（"mod 能不能定义新的 UI 屏幕"）,不是本文档能顺带回答的**,如实标注为待后续设计文档处理的开放问题,本文档只给出否定结论：现在不能,且不该在没有"mod 定义 UI"这条更大设计之前假装能。

### 5.3 §15 阶段归属：现在定形状，将来分批落地

| 部分 | 阶段 | 理由 |
|---|---|---|
| `InputContext::Menu` 新变体、`UiMode` 栈、`InputState::clear()` 在上下文切换时的调用 | **P7**（UI 层） | 背包/菜单 UI 本身是 P6（物品与装备的数据模型）之后、P7（`ll-ui` 完整控件库、菜单/设置）才真正建成的东西——见规格 §15 P6/P7 两行。`InputContext` 枚举本身现在就可以加这个变体（改动极小,不依赖任何未落地的系统),但真正驱动它切换的 `UiMode` 栈需要背包 UI 本身存在才有意义。 |
| `ActionCapability` 类型定义、`resolve()` 各函数顶部的检查点（3.5 节） | **可以现在定形状,检查点可以现在就接（因为 `current_capability` 现在恒返回 `ALL`,不改变任何现有行为）,真正生效要等 buff 系统** | 与 `buffs-and-triggers.md` 自己的阶段归属一致——该文档"落地状态"一行写明"新 P6『物品与装备』需要装备的属性加成先落地,本文档描述的增益系统在此基础上属于战斗结算的后续批次";`ActionCapability` 是它的一个子集,归属完全跟随。**但检查点插入 `resolve()` 本身不需要等 buff 系统**——3.5 节的四行代码现在就可以落地,因为 `current_capability` 暂时恒真,不改变任何现有测试的结果,这是"先把接缝焊好,再等内容"的低成本先手棋。 |
| 撞墙消耗 tick（4.2 节,所有者已裁定但代码未实现） | **不属于本文档的实现范围,但本文档的一致性判据依赖它,已如实标注现状差异** | 这是 `resolve_move` 现有行为的一处修改（`blocks_move` 分支从"返回空 `Vec`"改成"返回一条 `ScheduleNext`"),影响面比本文档广（任何调用 `resolve_move` 的既有测试都要核对),不应该被本文档顺带捎带落地——留给下一次真正接触 `resolve.rs` 的实现批次,本文档只负责把"撞墙"与"眩晕"这两类失败尝试的 `cost` 规则钉死成同一条（4.2 节),保证两者将来落地时行为一致,不会先后两次实现出两套不同的耗时规则。 |
| `active_buffs` 字段本身、进 `hash()`、`BuffDef.restricts` 字段 | **随 buff 系统整体落地**（`buffs-and-triggers.md` 阶段归属) | 3.3/3.4 节已给出接线点,不需要现在提前造字段。 |

---

## 相关文档

- [增益与通用触发器](buffs-and-triggers.md) — `ActiveEffect`/惰性到期判定/`StackPolicy`，本文档 `ActionCapability` 的折叠对象与阶段归属均直接依赖它
- [装备栏位与占位掩码](equipment-slots.md) — `SlotMask` 位标志先例，本文档 `ActionCapability` 形状与 mod 扩展位分配方式均照抄其模式
- [扩充给 mod 的 Steel 脚本 API](mod-lifecycle-and-event-api.md) — 「mod 能不能定义新东西」判据总纲（ADR 0018 三步法 + ADR 0016 三档）的直接来源，本文档 5.2 节沿用同一判据
- [动画与视觉特效的边界](animation-and-vfx-boundary.md) — 本文档一节论证「动画层不需要为眩晕/背包开特例」的依据：两者最终都表现为「`Effect` 流里没有 `MoveTo`」
- [ADR 0009 — 默认派生，只存偏差](../decisions/0009-derive-by-default-store-only-deviation.md) — `ActionCapability` 纯派生、绝不存储的原始纪律
- [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../decisions/0022-guard-coverage-gap-defeats-the-guard.md) — 3.4 节 `active_buffs` 必须进 `hash()` 的直接依据
- [ADR 0016 — mod 性能分档按声明方式，不按作者身份](../decisions/0016-mod-performance-tiers-by-declaration.md) / [ADR 0018 — 引擎层与玩法层脚本边界](../decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md) — 5.2 节判据总纲
- `crates/ll-platform/src/keybind.rs`（`InputContext`/`KeyBindings`，已落地）
- `crates/ll-platform/src/input.rs`（`InputState`/`InputState::clear()`，已落地）
- `crates/ll-sim/src/resolve.rs`（`resolve_move`/`resolve_attack`/`schedule_after`，已落地）
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) §8（时间轴调度器）、§15（阶段划分，P6/P7 两行）
