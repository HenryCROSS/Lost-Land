# 剧本系统：作者化叙事结构与生成世界的接口

**冻结于** 2026-08-20。核对提交 `5769bae007d336adedad2e589da931e40ce99688`（`main` 分支，967 测试全绿）。

> **【2026-08-30 复核：下面「落地状态」的四条依赖前提与正文三处「必须等 `format-text`
> 落地」全部已过期。正文原样保留，逐条更正见文末「⚠ 落地状态复核更正（2026-08-30）」。】**
> **最要紧的两条**：① `OrgInstance` 已经落地（`crates/ll-world/src/entity/org.rs:36`），
> `WorldState::factions`（`crates/ll-world/src/state.rs:502`）随存档一起走——本文档二节
> 「角色绑定解析要等世界生成」那条阻塞**已经解除**，不要再照它推迟；② 对话变量插值
> **今天就能做**，`Catalog::resolve_with_args`（`crates/ll-i18n/src/lib.rs:167`）早已落地
> 并有六个真实调用方。详见 [2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md)。

**落地状态**：纯设计，`crates/` 中无任何对应类型。已核实的依赖前提：

- [任务系统](class-skill-quest-system.md)的 `QuestNodeDef`/`QuestCondition`（`crates/ll-mod/src/quest.rs`）已落地，DAG 无环校验复用 `crate::prereq_graph`，任务进度走脚本状态存储的每实体存储（`mark_quest_completed`/`is_quest_completed`，已下沉到 `ll_sim::quest`）。本文档大量复用这套既有机制，不重新发明。
- [脚本状态存储](script-state-storage.md)已落地（`crates/ll-world/src/script_state.rs`、`crates/ll-script/src/api/state.rs`），`ScriptValue` 当前只有 `Int`/`Bool`/`Str`/`Ref`/`Entity`/`List`/`Map` 七个变体，**没有 `WorldId` 变体**——该文档「十、开放问题」第 1 条明确留白"留给后续任务视真实 mod 需求决定"，本文档四节给出这个真实需求。
- [身份与 ID 空间](identity-and-ids.md)的 `WorldId`/`OrgInstance` 与[世界历史生成](world-history.md)均**纯设计，无代码**——本文档二节的结论直接依赖这两份文档已经定下的"查询式而非引用式"原则，但本文档描述的角色绑定解析本身要等世界生成（P7）落地才能真正跑起来。
- [击杀与死亡记录](kill-and-death-events.md)已经把 `HistoricalEvent` 信封首次定型，本文档五节讨论剧本事件是否该并入这套信封时直接复用其结论方法。

---

## 一、剧本与任务的边界

**任务（`QuestNodeDef`）是系统性的、可重复的、由条件驱动的工作单元**——"击杀 3 个哥布林"这类任务不关心具体是哪个玩家、在哪个世界种子下，只关心一个可判定的完成条件（[任务系统](class-skill-quest-system.md)已经把它设计成 mod 定义的**内容**，走 `ContentIndex` 注册表，与 `ClassDef`/`SkillDef` 同一套模式）。

**剧本是作者写好的故事结构**——一段有顺序的事件、对话，可能跨越多个任务节点的战役线。剧本关心的是**顺序**（先做 A 才有 B，B 完成后 C 才有意义）与**在地性**（"主角要去找那位老国王"，这是一个具体的谁,不是一个抽象的完成条件）。

区分这两者的核心判据：**任务回答"什么时候算完成"，剧本回答"讲了个什么故事,按什么顺序展开,牵涉哪些具体的人和地"**。

| | 任务（`QuestNodeDef`） | 剧本 |
|---|---|---|
| 结构 | DAG，前置解锁网状图（多对多）,"解锁了什么"是派生视图 | 有作者意图的顺序（beat 序列，可分支）,通常比任务图更强调"这一步之后才是那一步"的叙事因果 |
| 完成判据 | 一档声明式数据 / 三档脚本回调（[任务系统](class-skill-quest-system.md)已裁定不做二档） | 见三节——直接复用任务系统的同一套一/三档分级，不重新发明 |
| 是否需要具体实体 | 通常不需要——"击杀 N 个某类型敌人"与具体是哪个敌人无关 | 天然需要——"找到那位老国王"必须指向一个具体的谁,这是二节要解决的核心矛盾 |
| 进度存储 | 脚本状态存储,每实体（已落地模式） | 同上，见四节——本文档不新增存储机制,只扩展值类型 |

**结论：剧本不是任务系统的替代品，也不是重新发明一套 DAG——剧本是任务系统之上的一层叙事编排**。一份剧本的某个 beat 完成条件，完全可以直接是"某个既有 `QuestNodeDef` 已完成"（见三节 `BeatCompletion::QuestLinked`），剧本本身只负责任务系统不管的那部分：**beat 之间的叙事顺序、对话文本、以及"这一步该找谁/去哪"这类需要具体世界锚点的引用**。`QuestNodeDef.condition` 只回答"什么时候算完成"，从不回答"讲了个什么故事"——这正是两者不重叠的边界线。

**被否决的方案：把剧本做成"任务图的一种特殊情况"（例如给 `QuestNodeDef` 加一个 `narrative_text_key` 字段了事）**。否决理由：任务系统的核心不变式是"网状、前置列表是单一真相源、解锁视图是派生"——这套不变式服务于"系统性、可重复"的场景。剧本天然需要携带任务系统从未考虑过的东西（角色绑定、多个 beat 共享同一个已解析实例、对话文本的具名参数插值），把这些字段硬塞进 `QuestNodeDef` 会让这个已经落地、被 `ClassTable`/`SkillTable` 同构复用的类型背上一堆与"任务"无关的职责，且任务系统的多对多解锁语义（一个前置任务可以同时解锁多个后续任务）本身就不适合表达"这是一条有作者意图的、单向推进的故事线"——线性/树状的叙事推进硬塞进网状图里，读者需要额外心智负担才能看出"这几个节点其实是一条剧本"。

---

## 二、核心矛盾：剧本要具体的人和地，而世界是生成的

[身份与 ID 空间](identity-and-ids.md)「脚本 API 必须是查询式，不是引用式」已经定死：名字是随机生成的，mod 脚本无法按名字引用某个具体的生成物——"卡拉克之战"换一个种子根本不存在。但剧本天然想说"主角要去找那位老国王"，这与查询式原则字面冲突,必须正面处理,不能假装矛盾不存在。

### 三条路的评估

**路一：剧本自带实体**——像 mod 定义的具体势力那样（[身份与 ID 空间](identity-and-ids.md)四节已支持），剧本直接定义一个具体的 NPC/地点，播种进世界，领取一个 `WorldId`。

**否决**。理由：这条路的本质是让"老国王"成为一个**不是从历史生成里长出来的**、悬浮在生成世界之外的固定角色——他不是这一局 500 年历史模拟的产物,只是被作者塞进去的一个道具。这与整个项目"世界由种子完全决定、可复现"的设计前提（[世界历史生成](world-history.md)「诚实警告」一节）在精神上冲突：一个"程序生成的世界"里混入若干"作者手工放置、与生成历史毫无因果关系"的固定角色,越用越多就会让世界观分裂成两半——"生成的部分"与"硬编的部分"互不知道对方存在。更实际的问题是**不可扩展**：剧本想表达"距离最近的、历史悠久的王国的统治者",这本身就是一个依赖具体这一局世界生成结果的角色,不可能用"剧本自带一个固定实体"来表达——固定实体天然回答不了"这个种子生成的王国恰好在哪、统治者是谁"这类问题,只能回答"我预先造了一个,不管这局种子实际长什么样"。这不是"路一在某些场景不适用",是路一从根本上无法覆盖"剧本想要的是某种关系角色而非某个固定个体"这个最常见的诉求。

**路三：剧本声明需求，世界生成满足它**（例如"需要一个沿海城市和一个山中遗迹"，世界生成过程本身按约束求解）。

**否决,但不是永久否决,标注为将来扩展**。这条路能给出最强的保证（剧本要的地理特征一定存在）,但代价是让世界生成本身变成一个约束求解问题——多个已安装的叙事 mod 同时声明需求时（一个要沙漠、一个要雨林）,需要一整套优先级/协商机制才能不互相冲突,这正是[命名、改名与本地化](naming-and-localization.md)六节已经预见并明确搁置的"生成器钩子体系"（"此事不急……可以等到真正实现命名钩子这个具体需求时再展开"）——那份文档已经把这类"mod 在生成阶段介入"的需求列为独立的前瞻方向,不属于当前批次。为剧本系统单独抢先实现这套机制,是本文档要极力避免的"设计过头"：**当前没有任何已知剧本内容因为"路二不够用"而非要不可地需要路三**,路三留给未来真的出现这类需求时,作为「生成器钩子体系」的一个具体应用去做,不在本文档展开。

**路二：按角色绑定——剧本声明"最近的王国的统治者"，世界生成时（或玩家落地/剧本首次触发时）解析成具体实例。**

**结论：采纳。**

理由：

1. **零新增世界生成机制**——这条路完全建立在已经设计好的东西上：[身份与 ID 空间](identity-and-ids.md)的查询式 API、[世界历史生成](world-history.md)「白送的三件事」第一条（"玩家可以是某王朝后裔"——选点时从当地家族池派生血统）已经示范了完全同一种模式："查询一个角色,绑定到具体实例,此后当作确定的东西使用"。剧本要的"老国王"不需要发明新的世界生成能力,只需要复用已经存在的"距离最近的、历史悠久的王国的统治者"这类查询,在剧本触发的那一刻解析一次。
2. **在任何种子下都成立**——这正是[身份与 ID 空间](identity-and-ids.md)论证"查询式优于引用式"的原始理由,剧本按角色写就自动继承这个性质,不需要剧本作者自己操心"万一这个种子没有沿海城市怎么办"这类问题（见下方"解析失败怎么办"）。
3. **真正缺的只是"持久化一次解析的结果"**——这不是一个需要新设计的问题,是[脚本状态存储](script-state-storage.md)已经解决过的问题的一个新消费场景,只差给 `ScriptValue` 补一个变体（见四节）。

### 解析失败怎么办：图鉴式降级，不是约束满足

路二仍然可能撞上"这个种子里查不到符合条件的实例"（例如全大陆无海,没有"沿海城市"这种东西）。**这不需要路三那种"重新做一次约束满足生成"来解决**——更便宜、更诚实的处理方式是：**剧本在声明角色需求时,同时声明"解析失败时,这个剧本干脆不可用"**,与任务系统前置条件不满足时"这个任务节点还不能开始"是同一种语义,不是错误,是"这局世界里这个故事讲不出来"这一诚实结论。剧本作为一份内容,本来就应该允许"在某些世界里不存在"——就像某个 `SkillDef` 要求某个前置技能,前置不满足就是学不到,不需要因此让技能树生成过程本身重新设计一遍。

---

## 三、剧本的存储形式

[任务系统](class-skill-quest-system.md)已经指出 ADR 0016 三档分级同样适用于任务系统："本批次不做二档……当前已知需求下,任务完成条件要么是简单计数（一档够用）要么是复杂逻辑（需要三档），中间的"公式"这一档没有明确用例"。**剧本系统直接复用同一个结论,理由相同,不重复论证。**

剧本的内容天然分三层，各自遵循已有的分级纪律，**不是三种互不相干的格式，是同一份 `.scm` 文件里，不同字段各自复用既有约定**：

```rust
/// 一份剧本，mod 定义的内容,走内容注册表——与 `ClassDef`/`SkillDef`/
/// `QuestNodeDef` 同一套「私有字段 + define 注册期校验 +
/// materialize_base_* + *_fixture 测试夹具」模式,物化为按
/// `ContentIndex` 索引的平铺列（ADR 0016/0017）。
pub struct NarrativeDef {
    pub id: NamespacedId,
    pub display_name_key: NamespacedId,
    /// beat 序列，DAG——前置列表是单一真相源,"解锁了哪些后续 beat"
    /// 是派生视图,与 QuestNodeDef 第四节同一条纪律,复用同一套无环
    /// 校验（crate::prereq_graph）,不重新写一份 DFS。
    pub beats: Vec<ContentIndex>,   // 指向 NarrativeBeatDef
    /// 本剧本用到的角色绑定声明,见二节。
    pub roles: Vec<ContentIndex>,   // 指向 NarrativeRoleDef
}

pub struct NarrativeBeatDef {
    pub id: NamespacedId,
    /// 前置 beat，DAG 的边，空列表表示"起点" beat。
    pub prerequisites: Vec<ContentIndex>,
    /// 对话/描述文本——Fluent 本地化键,绝不是字面字符串（见六节）。
    pub text_key: NamespacedId,
    /// 完成判据——只有两档,同 QuestCondition,不做二档,理由相同。
    pub completion: BeatCompletion,
}

/// 一档：直接绑定一个既有 QuestNodeDef——那个任务完成即这个 beat
/// 完成，不重复发明"完成"这件事怎么判定。
/// 三档：脚本回调——处理"某个对话选择""复杂多因素判定"这类无法
/// 穷举成数据的条件，与 QuestCondition::Script 同一个形状。
pub enum BeatCompletion {
    QuestLinked(ContentIndex),
    Script(NamespacedId),
}

/// 角色绑定声明——本 beat 序列引用的具体世界锚点，见二节。
pub struct NarrativeRoleDef {
    pub id: NamespacedId,
    pub query: RoleQuery,
}

/// 一档：引擎提供的固定查询集合（依赖 P7 世界生成的 OrgInstance/
/// Agent 查询接口，本文档不展开具体查询实现，那是世界历史生成/
/// 身份与 ID 空间两份文档的职责，本文档只确认"角色绑定按 0016
/// 分级表达"这条形状）。
/// 三档：mod 自定义解析逻辑（脚本回调，用批量查询 API 自己选出
/// 一个实例）——覆盖内置查询集合之外的自定义角色概念。
pub enum RoleQuery {
    Builtin(NamespacedId),   // 引用引擎预注册的查询名，如 "nearest-kingdom-ruler"
    Script(NamespacedId),
}
```

**为什么不做二档（受限公式）**：与任务系统同一个理由——当前没有任何已知的剧本需求落在"比声明式数据复杂、但比任意脚本简单"这个中间地带。角色查询要么是引擎已经写好的固定几种（"最近的王国统治者""最近的沿海聚落"），要么复杂到必须用脚本自己组合批量查询原语，没有中间层的用例。

**文件格式**：与 `ClassDef`/`SkillDef`/`QuestNodeDef` 完全一致——`.scm` + `register-narrative`/`register-narrative-beat`/`register-narrative-role` 系列注册函数,遵循规格 §11.1「游戏数据表用 Steel .scm，数据与效果逻辑同文件」，**不引入 TOML 或任何其他数据表格式**——剧本的"数据"（beat 结构、前置关系、本地化键引用）与"逻辑"（三档脚本回调）本来就应该同文件维护，这正是 §11.1 选择 `.scm` 而非 JSON/TOML 的原始理由，剧本没有任何特殊性需要打破这条既有分工。

**被否决：为剧本单独设计一种"剧本脚本语言"或专属数据格式**——否决理由：规格 §10.5「行为树」一节已经证明"S 表达式本身即树结构，无需发明第三种格式"这条思路的价值，剧本的 beat 序列同样是一种树/图结构，没有理由不复用同一条思路；专属格式还需要专属解析器、专属校验工具，纯属重复造轮子。

---

## 四、进度持久化

**结论：与任务进度完全同一套机制——脚本状态存储的每实体存储，写入经 `apply`（[ADR 0023](../decisions/0023-script-state-writes-go-through-apply.md)）。** 这不是"是否照办"的选择题——[ADR 0023](../decisions/0023-script-state-writes-go-through-apply.md)的结论本身就是"任何东西一旦被放进 `WorldState`，就自动被约束 C1 覆盖，不存在因为存储位置特殊所以可以绕开管线的例外"，剧本进度没有任何理由构成这条纪律的又一个例外。

具体落地遵循与 `mark_quest_completed`/`is_quest_completed`（`ll_sim::quest`）完全相同的形状，建议同样落在 `ll-sim`（例如 `ll_sim::narrative::{mark_beat_reached, is_beat_reached}`），理由与 `quest.rs` 模块文档「任务进度基础操作搬到了 ll-sim」一节相同——若剧本 beat 的完成判定需要在 `resolve` 阶段直接产出（例如 beat 绑定的任务完成时顺带标记 beat 也完成），操作函数必须放在 `ll-sim` 才能被 `resolve` 直接调用，不能反向依赖 `ll-mod`。

### 剧本比任务进度多出的一件真事：角色绑定需要新的值类型

任务进度只需要"哪些节点完成了"（`Set`/`List` 足够表达）。剧本额外需要持久化**已解析的角色绑定**——"老国王"一旦在二节的角色查询里解析成一个具体的 `Agent`/`OrgInstance`，此后全部 beat 都要读**同一个**绑定，不能每次都重新查询（重新查询可能因为世界状态变化而解析出不同的实例，破坏叙事连续性）。

[脚本状态存储](script-state-storage.md) 3.3 节已经允许存储 `Entity(EntityId)`（限厚层 `Agent`），这直接覆盖"老国王是一个具体 NPC"这类角色绑定。但若角色绑定的对象是一个**势力/王国**（`OrgInstance`，用 `WorldId` 标识，不是 `Agent`/`EntityId`）——`ScriptValue` 当前**没有**能装 `WorldId` 的变体。[脚本状态存储](script-state-storage.md)「十、开放问题」第 1 条已经如实标注这个缺口，写着"没有出现明确需要脚本'记住某个 `WorldId`'的场景……留给后续任务视真实 mod 需求决定"——**本文档就是那个真实需求**：剧本要绑定的角色经常是"某个王国""某个宗教团体"这类组织实例，而不是某个具体 NPC。

**建议补充**：

```rust
pub enum ScriptValue {
    Int(i64),
    Bool(bool),
    Str(Box<str>),
    Ref(ContentRef),
    Entity(EntityId),
    World(WorldId),          // 本文档新增
    List(Vec<ScriptValue>),
    Map(std::collections::BTreeMap<Box<str>, ScriptValue>),
}
```

**这个新增比 `Entity` 变体更简单**：[身份与 ID 空间](identity-and-ids.md)已经定死 `WorldId` "永不复用、不需要代际号"——它本身就是一个稳定的整数，序列化即整数本身，不需要 `Entity` 变体那一套"世代号失效检测、读到已死亡实体返回哨兵值"的机制（`WorldId` 指向的势力可以灭亡，但"灭亡的势力仍然被引用"本来就是[身份与 ID 空间](identity-and-ids.md)三节"①永不复用"论证过的**期望行为**，不是需要检测的失效状态）。

**这是对已落地类型的一次追加变更，需要在真正对外发布前完成**——`ScriptValue` 已经落地并参与序列化（P5 批次 D），新增枚举变体本身若走可扩展的 serde 表示（未知变体不 panic、按可扩展模式处理），属于[击杀与死亡记录](kill-and-death-events.md)「schema 迁移问题」一节论证过的"内容层面扩充，不是容器结构变化"，不需要一次真正的存档迁移；但仍然建议尽早补上，理由与该文档相同——晚加不是不能加，只是越晚加、越可能已经有真实存档需要兼容。

---

## 五、与历史事件的关系

**结论：剧本进度本身不是历史事件；剧本产生的"世界性后果"如果有，走既有的 `HistoricalEvent` 变体，不新增 `HistoricalEventKind::Narrative`。**

[击杀与死亡记录](kill-and-death-events.md)论证"不做独立战斗日志"的判据是：击杀这件事**天然是**一个多方都要查询的、客观存在的世界事实（血仇、任务计数、传说浏览都要读它）,分开存会导致"同一件事被独立定义两次"的既有教训重演（ADR 0010）。

**这个判据不能照搬到"剧本 beat 完成"上，因为它不满足同一个前提**：一个 beat 完成（"玩家找到了老国王并说服了他加入联盟"）本身通常**不是**一个其他系统需要查询的客观世界事实——它是"这一局游戏里,这个玩家的这段剧情走到了哪一步",是高度个体化的进度信息,与"哪个具体历史人物几时死于何种死因"这类任何存档、任何查询式 API 都可能想问的问题性质不同。把每一次 beat 完成都记成一条 `HistoricalEvent`，会把[世界历史生成](world-history.md)已经反复强调、只记"约一万条可被引用的事件"的日志预算，稀释成记录大量"没有人会去传说浏览里查阅"的个人进度条目——这正是[击杀与死亡记录](kill-and-death-events.md)三节"被否决的方案：朴素全记"论证过的同一类问题,只是换了个触发源。

**但区分开来的另一半是真的**：如果某个 beat 的完成**确实**改变了客观可查询的世界状态——例如"玩家说服老国王退位"这类后果理应影响 `OrgInstance.standing`、触发一次改名（王国改朝换代）——**那份后果本身应该走既有机制记录**（[命名、改名与本地化](naming-and-localization.md)四节的改名 `Effect` + `HistoricalEvent`，或未来 `HistoricalEventKind::DynastyChange`），与"这是不是剧本触发的"无关。这与击杀记录的现状是同一个模式：`Effect::Kill` 不关心这次击杀是不是"剧情安排好的",一场剧本要求的关键战斗产生的死亡,走的仍然是普通的 `Effect::Kill` → `resolve_attack` 管线,不需要因为"这是剧本"而单独开一条通路。**剧本只是世界事件的一个可能触发源，不是世界事件的一种新分类维度**——与"玩家杀的"、"NPC 杀的"、"历史生成期间产生的"这几种触发源在 `Effect::Kill`/`HistoricalEvent` 面前完全等价是同一个道理。

### 被否决的方案

**给 `HistoricalEventKind` 新增 `Narrative` 变体，记录每个剧本的每次 beat 完成**——否决，理由已在上文展开：beat 完成不满足"客观世界事实,多方需要查询"这个前提,把它塞进历史事件信封只会稀释"可被引用"这个筛选标准本身的意义（[世界历史生成](world-history.md)六节原话："值得记录本身就是筛选依据，不是懒得记"）。若未来某个具体的、真实存在的剧本内容证明确实需要"这个剧情节点本身要能被传说浏览查询到"，那时候应该走的是"这个节点产生的后果本身记一条已有种类的 `HistoricalEvent`"，而不是回头给"剧本"这个抽象概念本身开一个新变体。

---

## 六、本地化

**剧本的一切用户可见文本（beat 标题、对话行、描述）必须是 Fluent 本地化键，不得硬编码字面字符串**——这不是本文档新立的规则，是规格 §11.3「代码中不得出现任何硬编码的用户可见字符串」既有 CI 门禁的直接延伸，`NarrativeBeatDef.text_key` 因此必须是 `NamespacedId`，本地化文件的组织方式复用[mod 包结构与资产 VFS](mod-package-structure.md)一节"本地化文件"约定（键不重复编码命名空间，因为 `locales/` 目录本身已经按 mod 隔离）。

**对话变量插值依赖 ADR 0019 B-2（`format-text` 具名参数格式化），当前状态是"待办"**——剧本对话经常需要插入变量（"{ $king_name } 点了点头"这类需要嵌入已解析角色姓名的场景），[ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) B-2 已经论证过"手工字符串拼接假设了目标语言语序相同，换一种语序不同的语言会整句错位"，因此**剧本对话不得使用字符串拼接表达变量插值**，必须等 `format-text` 落地才能支持带变量的对话文本；在此之前，剧本对话只能使用不含变量的静态文案。**本文档不是在报告一个新缺口——`format-text` 已经是已知待办项，剧本系统只是这项工作的又一个真实消费者**，与[身份与 ID 空间](identity-and-ids.md) B-1"整数几何工具"一节"这条不补，'浮点默认更优路径已经存在'这句话就是空话"是同一种论证结构：`format-text` 不补，"剧本能写有变量的对话"这件事就无法兑现。

**已解析角色的姓名本身不需要任何新设计**——二节角色绑定解析出的具体 `Agent`，其姓名走[命名、改名与本地化](naming-and-localization.md)既有的 `given_name`/`surname`/`full_name` 纯函数与多语言音素表索引对齐机制，剧本对话引用"老国王的名字"时，切换语言会自动得到该语言版本的音素表渲染结果——这与其余任何"引用一个生成物姓名"的场景没有区别，本文档不重复设计。

---

## 七、阶段归属与必须提前留的接口

**剧本系统本身分成两半，阶段归属不同——这与[世界历史生成](world-history.md)自己"体量上是独立阶段，但三件事必须提前做"是完全相同的处理方式，本文档直接复用这个框架。**

| 部分 | 阶段 | 理由 |
|---|---|---|
| `NarrativeDef`/`NarrativeBeatDef` 声明骨架（beat DAG、本地化键、`BeatCompletion` 一/三档） | **P5-B**，紧跟任务系统任务 6（`QuestNodeDef`）之后，同批交付 | 不依赖世界生成任何能力——与 `ClassDef`/`SkillDef`/`QuestNodeDef` 同构，可以直接复用 `crate::prereq_graph` 无环校验、`register-*` 模式，是纯粹的内容注册表工作 |
| 角色绑定解析（`RoleQuery::Builtin` 真正落到具体 `OrgInstance`/`Agent`） | **P7**，与世界历史生成同批 | 这是本系统唯一有意义的世界生成依赖来源——`OrgInstance`/`WorldId` 本身都是纯设计（`identity-and-ids.md`「落地状态」已核实），P7 之前角色绑定查询无东西可查，提前也没有用 |
| `ScriptValue::World(WorldId)` 变体 | **现在**（下一次触及脚本状态存储的批次） | 已落地类型的追加变更，越早补代价越低，不依赖任何未落地的东西——`WorldId` 类型本身已经落地（`crates/ll-core/src/ident.rs`），缺的只是 `ScriptValue` 那一侧的一个新枚举分支 |
| `WorldState.history`/`WorldId` 分配器 | **P5 存档格式冻结之前** | [击杀与死亡记录](kill-and-death-events.md)已经论证过这条紧迫性（P3 战斗系统已在日常产出 `Effect::Kill`，容器不提前留，死亡统计/血仇/传说浏览全断档），本文档不重复论证，只指出**剧本是这个容器的第三个消费者**（世界历史生成、击杀记录之后）——若剧本产生的世界性后果（五节）需要记一条 `HistoricalEvent`，同样依赖这个容器已经存在 |
| `format-text` 具名参数格式化（ADR 0019 B-2） | 无强绑定阶段，待办 | 剧本对话的变量插值依赖它，但骨架本身（不含变量的静态对话）不受阻塞——若 P7 之前仍未落地，剧本对话在那之前只能用静态文案 |

**一条容易被忽略但必须写清楚的衔接**：`NarrativeDef` 骨架在 P5-B 就能声明"这个剧本有哪些 beat、需要哪些角色",但角色绑定解析要到 P7 才有实际能力可用——这意味着 P5-B 到 P7 之间，剧本内容可以被作者**写出来**（结构、文本、任务绑定都能声明），但**运行不起来**（任何依赖角色绑定的 beat 会因为查询不到而处于二节"解析失败,剧本不可用"的降级状态）。这不是缺陷,是诚实的阶段划分——与[任务系统](class-skill-quest-system.md)`QuestCondition::KillCount.target_kind` 早期"注册期无法校验、待未来敌人类型注册表落地后再补"的先例是同一类"结构先行、语义后补"的分期方式。

---

## 八、最小可用形状

**能支撑第一个真实剧本的最小闭环**：

1. 一个 `NarrativeDef`，携带若干 `NarrativeBeatDef`（DAG，通常是一条近似线性的链）。
2. 每个 beat 的完成判据用 `BeatCompletion::QuestLinked`，直接绑定一个已有的 `QuestNodeDef`——**第一个真实剧本不需要 `Script` 三档完成判据**，能表达"完成某个任务即推进剧情"就足够跑通"顺序 + 具体人地"这两个核心诉求。
3. 一个 `NarrativeRoleDef`，`RoleQuery::Builtin`，引用一个引擎预置的查询（例如"最近的王国统治者"）——角色绑定解析一次，用四节的新值类型（`ScriptValue::Entity`/`ScriptValue::World`，视角色是具体 NPC 还是组织实例而定）存进玩家的每实体脚本状态，此后全部 beat 复用同一个绑定。
4. 对话文本全部走 Fluent 键，且**第一个真实剧本不需要变量插值**——只在 `format-text` 落地之后再引入需要嵌入姓名/数字的对话行。
5. 进度持久化直接复用 `mark_quest_completed`/`is_quest_completed` 同款函数（`ll_sim::narrative::mark_beat_reached`/`is_beat_reached`），不新增持久化机制。

**明确标为将来扩展、本批次不做的**：

- 多剧本并行调度器 / 剧本管理 UI（"我的日志"这类玩家可见的剧情列表展示，属于 `ll-ui`/规格 §12 的 UI 层职责，本文档只管数据形状不管界面）。
- 剧本编辑器工具（`tools/`，规格 §16 提到的知识库/工具链范畴，不在本文档设计范围）。
- 世界生成约束满足（二节路三），标注为"将来若真有需求，作为生成器钩子体系的一个具体应用去做"，不是本批次的一部分。
- `HistoricalEventKind::Narrative` 新变体（五节已论证否决，除非未来出现真实反例）。
- 完成条件的二档"受限公式"（与任务系统同理由，YAGNI）。
- mod 之间剧本相互引用/依赖（一个 mod 的剧本引用另一个 mod 定义的角色/任务）——当前设计的 `NarrativeDef`/`NarrativeBeatDef` 字段本身没有阻止这种跨 mod 引用（`ContentIndex`/`NamespacedId` 天然支持跨命名空间引用，与 `SkillDef.owning_class` 允许指向别的命名空间的职业是同一个道理），但"剧本生态互操作"这类更复杂的场景（例如剧本 mod 之间的依赖版本约束、剧本本身的加载顺序敏感性）留给真实出现多剧本 mod 生态之后再设计。

---

## 相关文档

- [职业 / 技能树 / 副职 / 任务系统](class-skill-quest-system.md) —— `QuestNodeDef`/`QuestCondition` 一/三档分级、DAG 单一真相源纪律，本文档一、三节大量复用
- [身份与 ID 空间](identity-and-ids.md) —— `WorldId`/`OrgInstance`、查询式而非引用式原则，本文档二节结论的直接依据
- [世界历史生成](world-history.md) —— "白送的三件事"第一条（玩家可以是某王朝后裔）与本文档二节角色绑定解析同一个模式；事件日志"只记可被引用"的预算，本文档五节否决新增历史事件变体的依据
- [脚本状态存储](script-state-storage.md) —— `ScriptValue` 值类型系统、每实体存储、[ADR 0023](../decisions/0023-script-state-writes-go-through-apply.md) 写入必经 `apply`，本文档四节直接复用并提出新增 `World(WorldId)` 变体
- [击杀与死亡记录](kill-and-death-events.md) —— `HistoricalEvent` 信封定型、"不做独立日志"的判据方法论，本文档五节据此判断剧本进度不该并入历史事件
- [命名、改名与本地化](naming-and-localization.md) —— 生成物姓名的多语言音素表对齐、改名 `Effect`、"生成器钩子体系不急"的既有搁置结论（本文档二节否决路三时复用）
- [mod 包结构与资产 VFS](mod-package-structure.md) —— 剧本脚本走该文档「入口点分类」的 `scripts.content` 分类；本地化文件组织约定
- [0016 — mod 性能分档按声明方式](../decisions/0016-mod-performance-tiers-by-declaration.md) / [0019 — 被禁能力必须有替代品或理由](../decisions/0019-denied-capability-needs-substitute-or-justification.md) —— 三档分级方法论、`format-text` 待办项
- [0023 — 脚本状态写入必须经 apply](../decisions/0023-script-state-writes-go-through-apply.md) —— 本文档四节持久化机制的强制约束
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) —— §11.1 数据格式分工、§11.3 本地化 CI 门禁、§15 阶段表

---

## ⚠ 落地状态复核更正（2026-08-30）

来源：[2026-08-29 文档—代码一致性审计](../audit/2026-08-29-doc-code-audit.md)。
**正文一个字未改**，下面逐条说明哪一句在今天不成立、什么时候因为什么被推翻。

### 1. 开头「落地状态」四条依赖前提，四条全部过期

| 原文怎么说 | 今天的实际情况 | 什么时候因为什么变的 |
|---|---|---|
| 脚本状态存储「已落地（`crates/ll-world/src/script_state.rs`、`crates/ll-script/src/api/state.rs`）」 | **两条路径都不存在了。** `crates/ll-script/` 整个 crate 已删除；`script_state.rs` 改名为 `crates/ll-world/src/mod_state.rs` | 2026-08-23 Steel 脚本系统整体拆除（[ADR 0028](../decisions/0028-steel-engine-construction-memory-corruption.md)） |
| `ScriptValue` 七个变体、「没有 `WorldId` 变体」 | 类型已改名为 `ModStateValue`（`crates/ll-sim/tests/replay.rs:691` 记着这次改名）。**「缺 `WorldId` 变体」这个缺口本身仍然成立**，只是它现在挂在 `ModStateValue` 上 | 同上 |
| `OrgInstance` 与 `WorldId`「均**纯设计，无代码**」 | **`OrgInstance` 已落地**：`crates/ll-world/src/entity/org.rs:36`；`WorldState::factions: FactionTable` 进存档主体（`crates/ll-world/src/state.rs:502`），`CURRENT_SCHEMA_VERSION` 因此升到 4（`crates/ll-content/src/save_file.rs:139`）。`WorldId` 更早就落地了（`crates/ll-core/src/ident.rs`） | 2026-08-29 势力播种批次，计划见 `docs/superpowers/plans/2026-08-29-batch14-faction-seeding.md`；裁定来由见 [2026-08-28 会话交接](../handoff/2026-08-28-session-handoff.md) 第〇之二节第 3 条 |
| 世界历史生成「纯设计，无代码」 | **已落地并在跑**：`crates/ll-world/src/chronicle.rs`（12 纪元推演，含建城/战争/占领）、`crates/ll-world/src/history.rs`、`crates/ll-world/src/settlement.rs` | 2026-08-26 前后陆续落地，见 [2026-08-26 三份文档落地状态复核](../audit/2026-08-26-society-race-conflicts-reverification.md) 零节第 2 条 |

**这四条合起来的后果，是本次更正里最要紧的一条**：本文档二节把「角色绑定解析
（`RoleQuery::Builtin` 真正落到具体 `OrgInstance`/`Agent`）」列为本系统**唯一有意义的
世界生成依赖**，七节据此把整个剧本系统的可用形状推迟到世界生成之后。**那个阻塞今天
已经解除了。** 下一批要做剧本/对话的角色绑定时，不要再照 198 行那张表推迟——直接查
`WorldState::factions` 与 `crates/ll-world/src/faction.rs`。

### 2. 三处「对话变量插值必须等 `format-text` 落地」：今天不成立

涉及正文 **185 行**（三节末段）、**201 行**（那张前置依赖表里 `format-text` 那一行）、
**214 行**（最小可用形状第 4 条）。三处说的是同一件事：剧本对话在 `format-text` 落地
之前只能用不含变量的静态文案。

**今天不成立。** [ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md)
B-2 的 `format-text` 是**脚本侧**的 API，它随脚本系统一起作废；而它要解决的那个问题
（Fluent 具名参数、不许字符串拼接）**由引擎侧的 `ll-i18n` 独立解决了**：

- `Catalog::resolve_with_args`（`crates/ll-i18n/src/lib.rs:167`）与实参类型
  `FluentArgs`（`crates/ll-i18n/src/lib.rs:49`）都是 `pub`，**从一开始就是**。
- 生产调用方今天有六处：`crates/ll-game/src/menu_screen.rs:360`、
  `crates/ll-game/src/save_list.rs:119`、`crates/ll-game/src/settings_view.rs:78`、
  `crates/ll-ui/src/hud/character_panel.rs:321` 与 `:331`、`crates/ll-ui/src/hud/mod.rs:199`。
- 尸体名字走的正是「一条带 `$species` 参数的通用 Fluent 消息」，见
  [2026-08-28 会话交接](../handoff/2026-08-28-session-handoff.md) 第二节「其余零碎」。

**这一条不是本次新发现**：[对话系统](dialogue-system.md) 3.3 节（`dialogue-system.md:309`）
早已写下这条更正。本次更正只是把它**搬回被更正的这一份**——上一次只改了更正方、没有
在被更正方留任何标记，于是打开本文档的人拿不到这条信息。

### 3. 仍然成立、不需要改的

- 一节「剧本与任务的边界」、二节「按角色绑定」那条结论与另外两条被否决路线的论证，
  没有任何证据反对它们。
- `ModStateValue` 缺 `WorldId`（原文写 `ScriptValue::World(WorldId)`）这个缺口是真的，
  只是承载类型改了名。

