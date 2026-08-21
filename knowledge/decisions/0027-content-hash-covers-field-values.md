# 0027 — 内容哈希从「id 集合摘要」升级为「字段值摘要」

**日期**：2026-08-20
**状态**：已生效的项目纪律
**关键提交**：（本次会话，值哈希升级批次）
**影响范围**：`crates/ll-mod/src/registry.rs`（`Registry::content_hash`/`intern`/`fold_content_digest`）、`crates/ll-mod/src/content_hash.rs`（新增模块）、`crates/ll-core/src/hashing.rs`（`StateHasher::write_namespaced_id`/`write_len_prefixed_bytes`）、`crates/ll-content/src/header.rs`（`SaveHeader::content_hash_algorithm_version`）、`crates/ll-content/src/load_error.rs`（`check_content_hash_algorithm`/`LoadError::ContentHashAlgorithmUpgraded`）、`crates/ll-content/src/save_file.rs`、`crates/ll-game/src/content.rs`、`crates/ll-game/src/save.rs`

## 背景

`Registry::content_hash_of`（`ll_mod::registry`）此前只在 `Registry::intern` 内部累积——每次一个新的 `NamespacedId` 被注册，就把它的字符串本身（命名空间 + 路径）折进按命名空间统计的异或摘要。这份摘要因此只回答一个问题：**这个命名空间贡献了哪些内容 id**。它完全不知道、也无法知道每条内容具体的字段值是什么——原因不是疏忽，是时序：`intern` 发生在任何 `*Table::define`（`ClassTable`/`SkillTable`/`SubclassTable`/`QuestTable`/`RaceTable`/`ll_world::terrain::TerrainTable` 六张玩法内容表各自的注册期入口）**之前**，那一刻具体的字段值压根还没有被写入任何地方。

后果：一个 mod 版本号不变、但把某把武器技能的 `SkillEffect::DealDamage.base` 从 50 改成 500，`content_hash_of` 对这次修改完全无感——id 集合一个字符都没变。存档硬门禁（`ll_content::load_error::check_mod_content`）与内容哈希本身都会判定为「兼容」，但两次装载出来的其实是行为不同的世界。项目所有者裁定：升级。

## 决定

**内容哈希升级为覆盖字段值**，具体设计：

1. **叠加而非替换**：`Registry::intern` 保留原有的 id 摘要折叠行为不变（不破坏既有测试与既有语义）。新增 `Registry::fold_content_digest(namespace, digest)`，供全部内容装载完毕后的一次性收尾函数 `ll_mod::content_hash::apply_value_hashes` 调用——遍历 `registry.snapshot()` 里的每一条内容，对六张表里能找到定义的条目求一个包含全部字段值的摘要，对找不到定义的纯 id 引用（例如占位种族、任务系统里指向"敌人类型"的占位标识符）退化为只哈希 id 本身。两次折叠都用同一种异或手法，可交换、不依赖调用顺序。

2. **字段覆盖判据：完整性优先，不做取舍**——六张表 `*Attrs`/`*Def` 声明的全部字段都参与哈希，包括 `display_name_key` 这类指向 Fluent 本地化键（不是渲染文本本身）的字段。理由：本次升级要修的问题正是「旧哈希漏掉了真实变化」，若又主观排除一部分字段，是在换一种方式重新制造同一类盲区——`display_name_key` 是内容作者显式登记的技术标识符，把它改名是一次真实的内容变更,理应被这条哈希看见。

3. **`ContentIndex` 字段解析成字符串再哈希**：`SkillDef.owning_class`/`prerequisites`、`QuestNodeDef.prerequisites`、`QuestCondition::KillCount.target_kind`、`TerrainDef.opens_into` 这类字段的运行期类型是 `ContentIndex`，但其数值依赖注册（加载）顺序。直接哈希裸索引会让「同样的内容、不同的装载顺序」产出不同摘要。因此任何 `ContentIndex` 字段一律先用 `Registry::resolve` 换回 `NamespacedId` 字符串再混入哈希（`StateHasher::write_namespaced_id`，新增在 `ll-core`，与既有的长度前缀写法统一）。

4. **浮点**：核实过六张内容表当前无 `f32`/`f64` 字段，因此本次不新增任何浮点哈希路径——若未来某张表新增浮点字段，不能直接 `to_bits()` 混入（`NaN` 位模式不唯一、`+0.0`/`-0.0` 位不同但数值相等），需要先规范化，这条备忘留给那次改动。

5. **算法版本可诊断**：`SaveHeader` 新增 `content_hash_algorithm_version: u32`（`#[serde(default)]`，老存档 JSON 缺该键时补 `0`，`0` 是「早于本字段存在」的专用哨兵，不代表任何真实算法）。`ll_mod::content_hash::CONTENT_HASH_ALGORITHM_VERSION` 当前为 `1`。新增 `LoadError::ContentHashAlgorithmUpgraded` 与 `check_content_hash_algorithm`，读档时排在 `check_mod_content` 之前调用——算法版本不一致时提前拒绝，避免把「量尺换了」误判成「mod 内容真的变了」（`ModContentMismatch`）。

## 后果：老存档会被硬门禁全部拒绝

**明确承认**：升级后，任何在本次改动之前写出的存档，一旦其 `generation_mods` 携带非空的 `content_hash`，几乎必然在 `check_content_hash_algorithm` 这一步被拒绝（`content_hash_algorithm_version` 字段缺失 → 反序列化补 `0` → 与当前版本 `1` 不等）。项目尚未发布，全部存档都是开发用途，项目所有者已确认这个后果可以接受，**不做迁移**：不保留旧算法的哈希实现（那意味着永久维护两套哈希逻辑），不尝试用旧存档的字段数据反推旧算法哈希再比对。

`content_hash_algorithm_version` 字段的存在本身就是这个后果的诊断锚点——`ContentHashAlgorithmUpgraded` 与 `ModContentMismatch` 是两个不同的 `LoadError` 变体，玩家/mod 作者从错误类型上就能分清「这份存档太老」与「你的 mod 真的改坏了」，不会被引导去错误的方向排查。

## 被否决的选项

**在 `Registry::intern` 内部直接算值哈希，不新增收尾步骤**——否决：`intern` 发生在字段值写入之前，物理上拿不到需要哈希的数据。若为了凑时序把 `intern` 签名改成同时接收字段值，六张表的注册模式（先 `intern` 拿索引、再用索引 `define` 属性,`SkillDef.prerequisites`/`owning_class` 这类字段本就需要引用其他已 `intern` 过的索引）会被推翻,改动面远大于「装载完毕后跑一次收尾」。

**只哈希"重要"字段，排除 `display_name_key` 一类展示字段**——否决：见「决定」第 2 点,排除判据一旦掺入主观判断,就没有一条不会被下一个字段打破的分界线,而这正是本次升级要修的那类盲区的另一种形式。

**为老存档做兼容迁移（重放旧算法哈希、或跳过老存档的哈希校验）**——否决（当前批次）：项目尚未发布,全部存档都是开发用途,维护一份不再使用的旧哈希算法只为兼容几份内部测试存档,不划算。若未来在正式发布后再次发生类似的哈希算法升级,需要重新评估是否值得做迁移——那时的存档是真实玩家数据,权衡会不同。

## 后果（技术债与后续）

- `Registry::rebuild_from`（P5 读档重放路径预留）目前只重放 id 摘要，不重放值摘要——它只接收 `&[NamespacedId]` 快照，没有六张表的字段数据可用。这是既有的、文档已经承认的范围限制（"P4 只需要保证 produce/consume 这份快照的能力就绪"），值哈希升级没有让它变得更差,也没有修它,如实记录。
- 未来任何新增的第七张内容表，都需要同步在 `ll_mod::content_hash` 里补一个 `write_*_fields` 分支与一个 `TABLE_*` 判别常量——这条检验目前没有编译期强制手段，与 [0022](0022-guard-coverage-gap-defeats-the-guard.md) 记录的同一类「哈希覆盖需要持续被核实」的局限同构。
