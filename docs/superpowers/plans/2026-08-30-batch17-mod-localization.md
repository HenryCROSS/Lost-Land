# 批次 17：mod 的 `.ftl` 装载（对话系统的批次 0）

> **【2026-08-31 编号更正（批次 25）】本文档正文里的「ADR 0018 反例验证」编号有误。**
> 讲反例验证／「覆盖不全的守护等于没有守护」的是
> [ADR 0022 — 覆盖不全的确定性哈希，等于没有确定性哈希](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)；
> [ADR 0018](../../../knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md)
> 讲的是引擎层／玩法层的脚本边界，正文与反例验证无关（`grep -c 反例 knowledge/decisions/0018-*.md` 在 0018 末尾追加订正节之前为 0）。**纪律本身完全成立、
> 一字不改，错的只是编号。** 错误源头是
> [2026-08-27 会话交接](../../../knowledge/handoff/2026-08-27-session-handoff.md)
> 第一节第 6 条（该条已原地更正）。本文档是历史档案，按纪律第 9 条**原文一字不改**，
> 只在此加标记。更正方：[批次 25 计划](2026-08-31-batch25-adr-citation-sweep.md)。

**工作树** `wt-modftl`，分支 `wt-modftl`，基线 `3d2649b`（`main` HEAD）。

**规格来源**：`knowledge/design/dialogue-system.md` 三节 3.2「致命缺口：mod 的 `.ftl`
装载零实现」与八节的分批表（本批是**批次 0**，对话系统全部后续批次的硬前置）。

**改前基线**（自己跑，不抄）：

- `bash scripts/ci/run_tests.sh` → `EXIT=0`，`test result: ok` 116 条，合计 **2798 passed**。
- `python3 scripts/ci/check_i18n_strings.py` → 996 处 warn，exit 0（warn 模式）。
- 两条黄金基准（grep 自取，不在本文档留副本）：
  `crates/ll-world/tests/determinism.rs` 的 `EXPECTED_WORLD_DIGEST`、
  `crates/ll-sim/tests/replay.rs` 的 `EXPECTED_REPLAY_DIGEST`。
  **本批不碰世界状态，预期两条都不变**；若变了，说明改到了不该改的地方，当场停下来查。
- `CONTENT_HASH_ALGORITHM_VERSION` = 27（grep 自取）。**本批不新增内容类型、不改哈希
  算法，这个值不动。**

---

## 一、缺口复核（行号自己 grep 过，不抄设计文档）

### 问题 ①：唯一的装载点只读本体一个目录

- `crates/ll-game/src/lib.rs:480`：`let catalog = Catalog::load_dir(&paths.locales_root);`
  —— 全仓库**唯一**的生产装载点。
- `crates/ll-game/src/lib.rs:108`：`locales_root: assets_root.join(LOCALES_DIR_NAME)`，
  `LOCALES_DIR_NAME = "locales"`（`:73`），`assets_root = base.join("assets")`（`:102`）。
- `grep -rn "Catalog::load_dir" crates/` 的其余七处全部是测试。
- `ls mods/*/` ⇒ `mods/lostland/` 与 `mods/example_mod/` 下**都没有 `locales/` 目录**。

**结论：成立。** 第三方 mod 的 `.ftl` 没有任何代码会去读它。

### 问题 ②：`to_fluent_id` 把命名空间整个剥掉，跨 mod 撞键

- `crates/ll-i18n/src/lib.rs:238-241`：
  ```rust
  fn to_fluent_id(key: &str) -> String {
      let path = key.split_once(':').map_or(key, |(_, path)| path);
      path.replace('.', "-")
  }
  ```
- `crates/ll-i18n/src/lib.rs:67-69`：`Catalog { bundles: HashMap<String, FluentBundle> }`
  —— 键是**语言标签**，没有命名空间这一维。

**结论：成立。** `mymod:race.elf.display_name` 与 `lostland:race.elf.display_name` 都
折成 Fluent id `race-elf-display_name`，落进同一个 bundle。`FluentBundle::add_resource`
对重复 id 会报错，而 `load_bundle` 把这类错误折成 `LoadError::Syntax` **整份文件跳过**
——也就是说撞键的表现是「后来者整份 `.ftl` 装不进来」或（跨文件时）「查到的是别人的
文案」，两种都没有任何东西会点名说「你和谁撞了」。

### 两个问题是相互独立的

修 ① 不修 ②：mod 的 `.ftl` 被读了，但只要有一条 id 与本体重名，行为就是静默取错或整份
文件被丢。修 ② 不修 ①：没有任何 mod 的 `.ftl` 被读，分流逻辑没有输入。**同批修完。**

---

## 二、修法

### 2.1 `Catalog` 的键改成 `(命名空间, 语言标签)`

```rust
pub struct LocaleSource { pub namespace: String, pub dir: PathBuf }

pub struct Catalog {
    base_namespace: String,
    bundles: HashMap<(String, String), FluentBundle<FluentResource>>,
}

impl Catalog {
    pub fn load(base_namespace: &str, sources: &[LocaleSource]) -> Catalog;
    pub fn load_one(namespace: &str, dir: &Path) -> Catalog;   // = load(ns, &[ns → dir])
    pub fn resolve(&self, language, key) -> String;            // 带语言回退，面向玩家
    pub fn resolve_with_args(&self, language, key, args) -> String;
    pub fn try_resolve(&self, language, key) -> Option<String>; // 精确，不回退，面向门禁
}
```

- **`Catalog::load_dir` 删掉。** 它没有命名空间入参，是「本体特权路径」的实现载体；
  留着它就等于留一条不用声明命名空间也能装载的口子。八处调用点全部改成 `load_one`。
- **本体不是特例**：`load` 的入参是一个**同构的 `LocaleSource` 切片**，本体那一条与任何
  mod 那一条在类型上不可区分，只是 `dir` 指向 `assets/locales/` 而不是 `mods/<id>/locales/`
  ——这与 `asset_vfs::build(mods_root, base_assets_dir, base_namespace)` 里本体资产根目录
  独立于 `mods_root` 是同一形状，且已被 `mod-package-structure.md:84` 明文认可
  （「规格 §5 `locales/` 目录本身就可以理解成本体这个虚拟 mod 自己的 `locales/`」）。
- `base_namespace` 只有一个用途：**裸键**（不含冒号，如 `window.title`、
  `hud-status-time-label`）归到哪个命名空间。这些键属于引擎/HUD 自身而不属于任何内容表，
  今天就没有前缀，保持现有行为。

### 2.2 查表按命名空间分流

`resolve` 先把 key 拆成 `(命名空间, 路径)`：有冒号取前缀，没有则用 `base_namespace`；
路径部分照旧 `replace('.', "-")` 折成 Fluent id（`mod-package-structure.md`「本地化文件：
为什么键不需要再编码命名空间」定死的两步查找规则，本批不动它）。然后查
`bundles[(命名空间, 语言)]`。**命名空间前缀不再被丢弃，而是用来选 bundle。**

### 2.3 装载端遍历 `mods/*/locales/`

新模块 `crates/ll-mod/src/locale_vfs.rs`（与 `asset_vfs.rs` 并列）：

```rust
pub const LOCALES_DIR: &str = "locales";
pub fn discover_locale_dirs(mods_root: &Path) -> Vec<(String, PathBuf)>;
```

- 复用 `discover::discover_mods` + `manifest::parse_manifest` + `topo::topo_sort`，
  与 `asset_vfs::build` 逐条同构：解析失败的候选跳过（它的 Failed 记录已经在
  `pipeline::load_all` 的报告里），`topo_sort` 失败整批返回空。
- **只返回真的存在 `locales/` 子目录的 mod**——否则每个没有本地化的 mod 都会在
  `Catalog::load` 里刷一条「目录不存在」的 warn，把真正有意义的告警淹掉。
- **为什么不走 `asset_vfs` 那条路**：`asset_vfs` 的主体是**覆盖解析**（`overrides/<目标
  命名空间>/`、同路径冲突、`ResolvedSprite` 的 id/路径双索引）。本地化按 `(命名空间, 语言)`
  分桶之后**结构上不存在覆盖这件事**——两个 mod 的同名 id 落在两个不同的桶里，没有谁盖谁。
  把它塞进 `asset_vfs` 就要在一个专为覆盖而生的数据结构里加一条「这一类不覆盖」的例外
  （ADR 0021：共享的是语法不是算法）。**共用的是 mod 发现/清单解析/拓扑排序这三件**，
  那三件本来就是 `ll-mod` 的公共设施，`asset_vfs` 自己也只是它们的消费者。
- **本批不提供本地化覆盖机制**（「mod A 改写 mod B 的某条译文」）。今天没有需求，
  YAGNI；要做也应当是独立一批，且要先回答「覆盖的粒度是文件还是条目」。

`crates/ll-game/src/lib.rs` 的装载点改成：

```rust
let mut sources = vec![LocaleSource::new(BASE_NAMESPACE, &paths.locales_root)];
sources.extend(locale_vfs::discover_locale_dirs(&paths.mods_root)
    .into_iter().map(|(ns, dir)| LocaleSource::new(ns, dir)));
let catalog = Catalog::load(BASE_NAMESPACE, &sources);
```

### 2.4 语言回退（规格没定，本批裁定）

**裁定：同一命名空间内的语言回退链 —— 请求语言 → `en` → 该命名空间其余语言（字典序）
→ 键名。** 每次落到回退都记一条 `warn`。

理由：

1. 任务书硬约束「**不许静默显示原始键名**」。今天的行为就是直接回退到键名，一个只提供
   zh-CN 的 mod 在英文玩家那里会整屏 `mymod:item.foo.display_name`。
2. 回退到**另一种语言的真实文案**是「看得懂但语言不对」，回退到键名是「看不懂」。
   前者玩家能继续玩，且一眼能看出是翻译缺失；后者两者都做不到。
3. `en` 优先而不是纯字典序：本项目首发中英双语，`en` 是 mod 作者最可能提供的那一份，
   也是最可能被最多人看懂的那一份。
4. **回退不跨命名空间**：`mymod:greet` 缺 en 时绝不去看 `lostland` 的 en——那正是本批
   要消灭的撞键行为的另一种形式。
5. 它**只在今天会显示键名的那些情形下**才改变行为，因此是可逆的：删掉回退链那一段，
   行为逐字回到今天。

**代价与对策**：语言回退会让「某个键漏了 zh-CN 译文」在玩家那里表现为一句英文，
不再表现为键名——`ll-i18n` 里那三条真实资产覆盖率测试（`真实资产目录覆盖全部本体键的
中英文翻译`）判据正是「解析结果 == 键名即视为缺译」，回退会让它们**变哑**。因此同批新增
`try_resolve`（精确、不回退），**把那三条门禁改成用它**。这不是顺手改，是本裁定的必要
配套：不配套就等于用一条玩家体验改善换掉一道已经在生效的门禁。

### 2.5 `languages()` / `loaded_language_count()` 的口径（规格没定，本批裁定）

**裁定：`languages()` 仍然只报**本体命名空间**的语言清单。**

理由：这份清单的唯一消费者是设置界面的语言切换（`menu_screen.rs:873` 的文档写明「顺序
本身就是逻辑」）。一个 mod 提供了 `ja.ftl` 并不意味着**游戏本体 UI** 有日文——让 `ja`
出现在设置里，玩家选中后看到的是一整屏走回退链的英文 UI 加一小撮日文 mod 文案。
「mod 能不能给游戏新增一种可选语言」是一个独立的产品问题（要连带回答「本体 UI 缺译时
显示什么」「语言列表的显示名从哪个命名空间取」），本批不替它做决定。
**这条保持今天的行为逐字不变**，是最容易反转的选择。

启动日志改用新增的 `loaded_bundle_count()`（全部 `(命名空间, 语言)` 桶数），这样日志里
能直接看出「有几个 mod 的本地化被装进来了」。

---

## 三、验收标的：`example_mod` 带上自己的 `.ftl`

`mods/example_mod/` **不许删、不许绕过**（77 个文件依赖它，它是「本体即 Mod」唯一的活
证据）。本批让它第一次真的带上自己的本地化。

### 3.1 键名改成与本体逐字同构的形状

> **【落地时被推翻，不删原文】** 本小节原计划把 `example_mod` 的 46 条
> 扁平键改成本体的 `<内容类型>.<id>.display_name` 形状。**所有者侧的
> 硬约束在开工后明确为「不要碰 `mods/example_mod/` 的既有内容结构——
> 本批是给它加 `locales/`，不是改它别的东西」，因此这次改名没有做。**
>
> 撞键那条验收断言改用另一条路，且**不需要动任何内容文件**：
> `mods/example_mod/locales/*.ftl` 末尾留两条**故意与本体同 id** 的
> 条目（`race-elf-display_name` 与**裸键** `hud-inventory-empty`），
> 在文件里逐条注明它们是活的回归夹具。先例是同一个 mod 目录下的
> `assets/overrides/lostland/sprites/terrain_dirt.png`——那张图同样只为了
> 让资产覆盖机制有一份真实证据而存在。
>
> **这条替代路比原计划更狠**：裸键那一条覆盖的是「第三方 mod 能不能
> 劫持**引擎自己**的 HUD 文案」，而 mod 恒在本体之后装载。反例实测
> （把 `Catalog` 还原成本批之前的一张扁平表）显示：示例 mod 真的会
> 把本体的「（空）」换成自己那句话，且没有任何东西会报错。
>
> 随之作废的还有本小节「代价」段：**内容哈希一个字节都没变**，
> 此前用 `example_mod` 生成的存档不受影响。

现状：`example_mod` 的 47 条 `display_name_key` 里，**46 条是扁平形状**
（`examplemod:iron_sword_display_name`），**1 条已经是分层形状**
（`examplemod:weather.ashfall.display_name`，天气批次写的）。本体全部 94 条无一例外是
`<内容类型>.<id>.display_name`（`grep` 统计：class 13 / culture 6 / damage_category 1 /
item 36 / race 4 / recipe 12 / recipe_category 5 / resource 7 / subclass 6 / trait 4）。

**把 46 条扁平键改成本体同一形状。** 三条理由：

1. `example_mod` 存在的全部意义是证明「本体的声明与第三方 mod 的声明除了 id 里的命名
   空间字符串之外没有任何结构性差异」。键名形状不同就是一处结构性差异。
2. 它自己内部已经不一致（天气那条是分层的），迟早要统一，现在改成本零。
3. **它是本批第二条验收断言的前提**：改完之后 `examplemod` 与 `lostland` **真的**共享
   若干条同路径的键——两边都定义了 `elf` 与 `goblin` 种族、都定义了 `cooking` 与
   `forging` 配方类别——于是 `race.elf.display_name`、`recipe_category.cooking.display_name`
   这几条在两个命名空间下同时存在。**这不是为测试造的场景，是两个 mod 各自独立定义同名
   内容的自然结果，也正是问题 ② 在真实世界里的样子。**

**代价，如实登记**：改内容 = `examplemod` 的内容哈希变了 ⇒ 此前用 `example_mod` 生成的
存档会被 `check_mod_content` 拒绝。仓库里**没有任何提交进版本库的存档**（`git ls-files`
核实），这是本地开发存档的一次性代价，与 `save-and-mod-version-policy.md` 既有的
「版本不对就打不开」同一条路径。**不递增 `CONTENT_HASH_ALGORITHM_VERSION`**——算法一个
字没改，改的是内容本身，那正是内容哈希该检测出来的东西。

### 3.2 `mods/example_mod/locales/{zh-CN,en}.ftl`

47 条键 × 两种语言，全部真译文，中英不同。

---

## 四、断言与反例验证（ADR 0018）

| # | 断言 | 位置 | 反例（改坏它必须变红） |
|---|---|---|---|
| 1 | 真实装载管线下，`examplemod:race.half_elf.display_name` 解析出 example_mod **自己**的中文文案 | `ll-game` 集成测试 | 把 `mods/*/locales/` 那段遍历去掉 ⇒ 回退到键名 |
| 2 | `race.elf.display_name` 在 `lostland` 与 `examplemod` 两个命名空间下解析出**两段不同的、各自正确的**文案 | 同上 | 把 `resolve` 里的命名空间分流去掉（恢复 `to_fluent_id` 剥前缀）⇒ 必红 |
| 3 | `recipe_category.cooking.display_name` 同上（第二条同形状的真实撞键） | 同上 | 同上 |
| 4 | 一个只提供 zh-CN 的 mod，在 `en` 下解析出它的 zh-CN 文案而**不是键名** | `ll-i18n` 单元测试 | 去掉回退链 ⇒ 返回键名，必红 |
| 5 | 回退**不跨命名空间**：mod 缺的键不会解析成本体同路径的文案 | `ll-i18n` 单元测试 | 让回退链跨命名空间 ⇒ 必红 |
| 6 | `try_resolve` 对缺译返回 `None`（门禁不被回退链弄哑） | `ll-i18n` 单元测试 | 让 `try_resolve` 走回退链 ⇒ 必红 |
| 7 | 装载顺序/发现顺序不影响结果（C5） | `ll-i18n` 单元测试 | —— 同一份 sources 打乱后逐条相同 |
| 8 | 两个 mod 各自的同名 id 都装得进来（不再是「后一份整个文件被丢」） | `ll-i18n` 单元测试 | 合并成一个 bundle ⇒ 必红 |

---

## 五、门禁与既有纪律

- **`check_i18n_strings.py`**：只扫 `crates/*/src/**/*.rs` 的 CJK 字面量，对
  `mods/**/*.json5` 与 `**/*.ftl` 完全看不见。本批**不扩大它的范围**（独立一批）。
  本批新增的 `.ftl` 与 json5 键名改动都在它视野之外；新增的 Rust 单元测试里会有中文
  fixture 字符串（与 `ll-i18n` 现有测试同一形状），会让 warn 计数上升——**warn 模式，
  不阻断**，改前 996 处，改后数字写进报告。
- **`check_no_examples.sh`**（ADR 0030）：本批不新增任何 example target。
- **`check_file_size_budget.py`**：`crates/ll-i18n/src/lib.rs` 改前 382 代码行 / 700 总行，
  **不在快照里**，因此改后必须仍 ≤ 800 代码行。`ll-mod/src/locale_vfs.rs` 是新文件，
  同一条上限。`crates/ll-game/src/lib.rs` 477 行、`ll-mod/src/asset_vfs.rs` 756 行也都
  不在快照里，同样不许越过 800。
- **`check_save_schema_version.py`**：本批不动存档主体形状，它不应该拦。拦了就是改到了
  不该改的地方。
- **`check_field_consumers.py`**：新增的公开字段（`LocaleSource` 的两个）当天就有生产
  消费者。
- **两条黄金基准**：本批不碰世界状态与结算，预期不变，实跑验证。

---

## 六、提交切分

1. `docs:` 本计划文档。
2. `refactor(ll-i18n):` `Catalog` 加命名空间维度 + 语言回退链 + `try_resolve`，更新
   自有测试。
3. `feat(ll-mod):` `locale_vfs` 模块（遍历 `mods/*/locales/`）。
4. `feat(ll-game):` 装载点接线（本体注册成 `lostland` 命名空间 + 全部 mod），更新调用点。
5. `feat(mods):` `example_mod` 键名与本体对齐 + 自带 `locales/{zh-CN,en}.ftl` + 端到端
   与撞键断言。
6. `docs:` 设计文档回填落地状态。

**不 push，不合并 main。**

---

## 七、边界（本批明确不做）

- 不做对话系统本身（批次 1 及以后）。
- 不做 `mods/**/*.json5` 的 CJK 字面量门禁、不做 `text_key` 多语言覆盖率门禁
  （设计文档三节建议的两条，各自独立一批）。
- 不做本地化覆盖机制（mod 改写别人的译文）。
- 不把本体的 `.ftl` 从 `assets/locales/` 搬进 `mods/lostland/locales/`——设计文档三节
  3.2 明文裁定「本体的 `assets/locales/` 注册成命名空间 `lostland` 的 locales」，
  搬目录是另一个决定，且会同时动部署布局（`assets/` 与 `mods/` 是两个并列的发行目录）。
  **残余的不对称如实登记在报告里。**

---

## 八、落地后回填

### 实测数字

| | 改前 | 改后 |
|---|---|---|
| `run_tests.sh` 合计通过 | **2798** | **2817** |
| `ll-i18n` 单元测试 | 11 | 19 |
| `ll-mod::locale_vfs` | 0（模块不存在） | 7 |
| `ll-game/tests/mod_locales.rs` | 0（文件不存在） | 6 |
| `check_i18n_strings.py` 疑似命中（warn 模式） | 996 | **1008** |

`check_i18n_strings.py` 多出来的十二条**全部在 `crates/ll-i18n/src/lib.rs`**：
`tracing::warn!` 的日志文案与测试夹具里的 `.ftl` 片段（`"race-elf-display_name = 精灵
"`
这一类）。它们与该文件里既有的同类命中同一形状，是开发者向诊断信息与测试数据，
不是玩家可见文本。**这道门禁的假设没有被本批改动破坏**：它扫的仍然是
`crates/*/src/**/*.rs` 的 CJK 字面量，对 `mods/**/*.json5` 与 `**/*.ftl` 一如既往地
看不见——本批新增的 `mods/example_mod/locales/*.ftl` 正落在它的盲区里，
这也正是设计文档三节 3.1 建议补第二道门禁的理由。补那道门禁属于独立一批。

### 两条黄金基准

**都没变**，实跑通过（`determinism.rs` 与 `replay.rs` 两个文件在本批里
`git diff` 为空）。本批不碰世界状态与结算，这是预期结果。
`CONTENT_HASH_ALGORITHM_VERSION` 同样不动（没有新增内容类型，也没有改内容）。

### 计划之外、但必须做的四件

1. **`ll-ui` 的 53 处调用点**。计划二节 2.1 只数了 `ll-game` 的八处，漏了 `ll-ui`
   的五十三处（全部在 `#[cfg(test)]` 里）。为它们在 `ll-ui/src/lib.rs` 立了一个
   `TEST_LOCALE_NAMESPACE` 常量，而不是把 `"lostland"` 抄五十三遍。
2. **`ll_game::test_support::empty_catalog`**。命名空间化之后构造式变长，就地写会被
   rustfmt 拆成四行，把 `crates/ll-game/src/app_tests.rs` 从 1053 顶到 1059 代码行，
   **文件行数棘轮门禁当场红**。抽成一个共用函数之后两个调用点各自回到一行，
   `app_tests.rs` 逐字回到原行数——**没有动 `--bless`**。
3. **`clippy::items-after-test-module`**。`empty_catalog` 一开始追加在文件末尾，
   落在 `mod tests` 之后；`-D warnings` 下这是硬错误。已移到测试模块之前。
4. **一条 rustdoc 内链**。`LoadError` 的文档指向被删掉的 `Catalog::load_dir`，
   `-D rustdoc::broken-intra-doc-links` 让 `check_doc_links.sh` 红。改指
   `Catalog::load`。

### 规格没裁定、本批临时选的做法（可反转，逐条列出）

1. **语言回退链**（二节 2.4）：请求语言 → `en` → 该命名空间其余语言（字典序）→ 键名。
2. **`languages()` 只报本体命名空间**（二节 2.5）：保持今天的行为不变，
   把「mod 能不能给游戏新增一种可选语言」留给独立一批。
3. **`example_mod` 的撞键夹具是两条与本体同 id 的 `.ftl` 条目**（三节 3.1 的更正段）：
   原计划改它的键名，被「不许动 example_mod 既有内容结构」这条约束否掉。
4. **不做本地化覆盖机制**（mod 改写别人的译文）：今天没有需求，且要做得先回答
   「覆盖粒度是文件还是条目」。
5. **同一个命名空间被给出两次时后来者生效并记 warn**：`topo_sort` 已经会先一步
   拒绝重复命名空间，这条只是兜底，选了「不丢数据、留痕迹」这一侧。
