# mod 包结构与资产 VFS

**冻结于** 2026-08-20。核对提交 `5769bae007d336adedad2e589da931e40ce99688`（`main` 分支，967 测试全绿）。**实现阶段**：目录布局与清单字段扩展可在 P5 随任务系统同批推进（不依赖任何未落地的引擎能力）；资产 VFS 本身依赖 `ll-render`/`ll-text` 的图集与本地化管线已就绪的事实（均已在 P1/P4 落地），可以独立于 P7 世界生成随时开工。

**落地状态**：纯设计。已核实的现状：

- `ModManifest`（`crates/ll-mod/src/manifest.rs`）目前只有 `id`（从 `namespace` 解析）、`version: String`、`dependencies: Vec<String>`、`entry_points: Vec<PathBuf>` 四个字段，`topo_sort`（`crates/ll-mod/src/topo.rs`）只做命名空间去重/缺失依赖/成环三类校验，不涉及版本比较。
- `mods/example_mod/` 是唯一真实存在的完整 mod：`mod.json5` 旁边直接平铺 `terrain.scm`/`gameplay.scm`/`behavior.scm`。`behavior.scm` **刻意不在** `entry_points` 里——它面向 `ll_mod::script_behavior_source::ScriptBehaviorSource` 这一套独立的运行期引擎，装载管线（`ll_mod::pipeline::load_all`）用的是另一个只注册六个 `register-*` 函数的引擎，两者互不相通。
- 规格 §5 `ll-mod` 一行写明该 crate 负责「mod 发现、清单解析、依赖拓扑排序、**资产 VFS**、内容注册表」，但 `crates/ll-mod/src/` 目前没有 `vfs.rs` 或等价实现——[ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) J 组已核实同一结论（「规划存在，未实现」）。
- `GenerationModSet`/`CurrentModSet`（`crates/ll-mod/src/mod_set.rs`）与 `Registry::content_hash_of`（`crates/ll-mod/src/registry.rs:100`）已落地，是本文档五节讨论版本兼容策略时可以直接复用的既有机制。

本文档不实现代码，只定形状；对现有 `mods/example_mod/` 的迁移成本（把 `entry_points` 拆成分类字段）在各节随手标注，不单独另起一节。

**落地状态更新（2026-08-20，资产 VFS 批次）**：四节「资产 VFS」已实现——`crates/ll-mod/src/asset_vfs.rs`（路径校验、`assets/sprites/manifest.json5` 解析、`assets/overrides/<目标命名空间>/sprites/` 覆盖解析、冲突产出 `LoadStatus::Warning`）与 `crates/ll-render/src/atlas_pack.rs`（运行期图集打包，取代 `include_bytes!` 编译期烧死）。目录约定与本节描述基本一致，一处已知偏差：清单文件名固定为 `manifest.json5`（不是 `<相对路径>.atlas.json` 这个按文件命名的形式），且不含 `rect` 字段（摆放位置改由打包器运行期决定，而非声明方预先给出）——这是「运行期打包」这个后续决定（详见提交历史与 `crates/ll-render/src/atlas_pack.rs` 模块文档）反过来收窄的一处形状调整，本节其余部分（覆盖规则、路径安全、冲突处理）未受影响。一节「目录布局」、二/三/五节（清单字段、入口点分类、依赖版本约束）仍是纯设计，未在本批次实现。

**落地状态更新（2026-08-20，配置格式统一批次）**：项目所有者裁定「全用 json5 吧，还可以写注释方便日后维护」，本仓库全部手写配置格式（清单、资产清单、玩家配置）统一改成 JSON5，本地化文件继续用 Fluent `.ftl`（所有者另一条裁定「i18n 就用 FTL」，不受本次调整影响）。这是一次面向清单文件名的破坏性变更——`mod.toml` 改名 `mod.json5`，`manifest.json` 改名 `manifest.json5`——但当时仓库里没有任何第三方 mod，改名成本是免费的；本文档下文提到的 `mod.toml`/`manifest.json` 均已同步改成 `mod.json5`/`manifest.json5`，内容形状（字段名、语义）未变，只是语法从 TOML/紧凑 JSON 换成允许注释与尾逗号的 JSON5。生成端（`tools/ll-artgen`）产出的清单文件仍然是普通 JSON——JSON 是 JSON5 的严格子集，生成端不需要注释，消费端统一用 `json5::from_str` 读取。

---

## 一、目录布局

### 推荐结构

```
yourmod/
  mod.json5                       清单（JSON5，唯一固定文件名）
  *.scm                           脚本入口与内部实现，路径由清单显式列出，
                                  作者可以自由组织子目录（例如 scripts/terrain.scm）
  assets/
    sprites/
      <相对路径>.png               本 mod 自己新增的精灵/图集原图
      <相对路径>.atlas.json        图集清单（footprint/pivot/uv 矩形/动画帧，
                                  格式与本体 `assets/atlas/` 下的既有约定一致——
                                  见「与本体共用格式」）
    audio/
      <相对路径>.ogg
    overrides/
      <目标命名空间>/
        sprites/<相对路径>.png     覆盖目标命名空间下同路径的资产（见四节）
        audio/<相对路径>.ogg
  locales/
    <语言标签>.ftl                 如 `en.ftl`/`zh-CN.ftl`，见「本地化文件」
```

### 为什么这样分

**脚本不强制分文件夹，资产/本地化强制固定目录名。** 这是两条不同的设计考量，值得分开说：

1. **脚本的组织权已经在清单里**——`entry_points`（本文档三节会拆分成更细的分类字段）本来就是一份显式路径列表，作者想把脚本放进 `scripts/` 子目录、按功能拆成多个文件，清单天然支持，不需要额外规定"脚本必须放在哪"。`mods/example_mod/` 现在平铺三个 `.scm` 文件也完全合法，不构成需要迁移的历史包袱。
2. **资产与本地化需要固定目录名，理由是 VFS 与发现机制必须有一个不依赖清单声明的锚点**——与 `discover.rs` 用固定文件名 `mod.json5` 发现候选 mod（不需要清单反过来声明"我的清单在哪"，那是鸡生蛋）同一个道理：资产 VFS 要能在**不求值任何脚本、不解析清单里某个可选字段**的前提下，回答"这个 mod 有没有 `examplemod:some_sprite` 这张图"，最简单可靠的办法是把这类内容锚定在固定的目录名上，由 VFS 直接按约定路径查找，不经过一层可配置的"资产目录声明"字段。

**被否决：让清单里加一个 `asset_dirs: [...]` 字段声明资产根目录。** 否决理由：

- 目前没有真实需求要求资产能放在任意自定义位置——`mod.json5`/`entry_points` 之所以需要显式声明，是因为脚本文件数量、组织方式因 mod 而异，天然需要一份列表；而"资产放在 `assets/` 下"是一个可以直接规定死的强约定，没有必要为一个没人会用到的自由度多加一个字段（YAGNI）。
- 固定目录名让资产 VFS 的安全边界更容易讲清楚：VFS 的根天生就是 `<mod 目录>/assets/`，不存在"某个 mod 声明了一个指向别处的资产根"这类需要额外校验的输入。

### 本地化文件：为什么键不需要再编码命名空间

`display_name_key: NamespacedId` 这类字段（`ClassDef`/`SkillDef` 等已经在用，见 `crates/ll-mod/src/class.rs`）存的是形如 `"examplemod:necromancer_display_name"` 的完整命名空间字符串。**但 Fluent 的消息标识符语法本身不允许冒号**（合法字符集是字母数字/连字符/下划线），若要求 `.ftl` 文件里直接写 `examplemod:necromancer_display_name = ...` 这一整串当 key，会在 Fluent 解析阶段直接报语法错误——这是一个真实的、此刻就该定下来的兼容性问题，不是未来才会撞见的边角情况。

**解法：本地化文件本身已经按 mod 目录隔离，键里不需要再重复命名空间。** `locales/<语言标签>.ftl` 物理上就活在这个 mod 自己的目录里，"这个键属于哪个命名空间"这件事已经由**文件所在的目录**回答了，不需要再让键的字面文本自己携带一遍命名空间。查找规则因此是两步：

1. 从 `NamespacedId` 取出 `namespace`，定位到对应 mod（或本体）的 `locales/` 目录；
2. 取 `path` 部分（`necromancer_display_name`）作为 Fluent 消息 id，在该目录对应语言的 `.ftl` 文件里查找。

```ftl
# mods/examplemod/locales/zh-CN.ftl
necromancer_display_name = 死灵法师
```

**本体的本地化文件遵循完全相同的约定**——规格 §5 `locales/` 目录本身就可以理解成"本体这个虚拟 mod（命名空间 `lostland`）自己的 `locales/`"，与任何 mod 的 `locales/` 是同一套查找规则，不需要为本体另开一条特殊路径——这是「本体即 Mod」在本地化查找上的又一次直接体现，不是本文档新发明的例外。

### 与本体共用格式：图集清单不重新发明

`.atlas.json` 的具体字段（footprint、pivot、UV 矩形、动画帧序列）是资产管线（`knowledge/pipelines/`）的职责，不在本文档权限范围内展开定义——本文档只确定"文件放在 `assets/sprites/` 下、与图片同名、供 mod 引用"这一层组织约定。**mod 的图集清单必须与本体 `assets/atlas/` 下已有的格式完全一致**，不允许 mod 专属的第二套图集格式——这既是「本体即 Mod」的要求，也避免渲染层（`ll-render`）为了兼容两套格式而分叉逻辑。

---

## 二、清单字段

现有四字段（`namespace`/`version`/`dependencies`/`entry_points`）保留，`entry_points` 的形状变化见三节。以下是新增字段，**每个都给出加它的理由，以及为什么没有加某些看起来"应该有"的字段**。

```json5
{
  namespace: "examplemod",
  version: "0.1.0",
  display_name_key: "examplemod:mod_display_name",   // 可选
  description_key: "examplemod:mod_description",     // 可选
  author: "某人",                                      // 可选
  compatible_game_version: ">=0.5.0, <0.7.0",          // 可选

  dependencies: {
    othermod: ">=1.0.0",                               // 见五节
  },

  scripts: {
    content: ["terrain.scm", "gameplay.scm"],           // 见三节
    behaviors: ["behavior.scm"],
  },
}
```

### 新增字段逐项理由

| 字段 | 类型 | 必填？ | 理由 |
|---|---|---|---|
| `display_name_key` | `NamespacedId` | 否，缺省回退到裸 `namespace` 字符串 | 加载管理界面（规格 §10.6）要展示 mod 列表——直接显示 `namespace` 这类技术标识符对玩家不友好。**必须是本地化键，不是字面字符串**：与 `ClassDef.display_name_key` 同一条纪律（一处校验、多语言自动跟随），也是 CI「无硬编码用户可见字符串」门禁（规格 §11.3）本该覆盖到的范围——若清单允许直接填字面字符串，这道门禁就会漏掉 mod 清单这一个入口 |
| `description_key` | `NamespacedId` | 否，缺省为空 | 同上，多语言的 mod 简介文本，理由与 `display_name_key` 完全一致，不重复展开 |
| `author` | 字符串 | 否 | **刻意不是本地化键**——作者署名是一个专名（"这是谁写的"），与[命名、改名与本地化](naming-and-localization.md)「覆盖名原样透传，不翻译」是同一条道理：署名切换语言不应该被机器翻译或音译，本来就该原样显示。这与 `display_name_key`/`description_key`（内容文本，理应跟随语言）是两类不同性质的字符串，不能用同一套字段处理 |
| `compatible_game_version` | 字符串（版本约束） | 否，缺省不做检查 | 见五节——这是"mod 与游戏引擎本身"的兼容性，与"mod 与 mod"的依赖版本约束（`[dependencies]`）是两条独立的轴，理由与失败语义都不同，分开设计 |

### 被否决的字段

- **`icon`（mod 图标路径）**——否决：加载管理界面目前的设计（规格 §10.6）是"分组显示已加载/警告/失败列表"，没有任何已确认的图标展示需求；真要加图标，按固定文件名约定（例如 `assets/sprites/icon.png`）就能满足，不需要专门的清单字段声明它在哪（与一节否决 `asset_dirs` 是同一个理由）。
- **`homepage`/`license`/`repository` 这类元数据**——否决：这是发行分发层面的信息（类似 `Cargo.toml` 的 `[package]` 扩展字段），当前既没有 mod 市场/分发渠道的设计，也没有任何消费方会读取它们。加了只是为了"看起来完整"，YAGNI。若未来出现 mod 分发平台，那时候再补，不属于加载管线需要关心的字段。
- **`tags`（mod 分类标签）**——否决，理由同上；且容易与[命名、改名与本地化](naming-and-localization.md)六节"`OrgInstance.tag`——mod 可匹配的标签"混淆，那是**世界生成实例**的标签（供命名钩子按条件匹配），与"这个 mod 属于什么类别"是完全不同的概念，不应该共用字段名或在同一批设计中顺手引入。

---

## 三、入口点分类

### 现状里的真坑

`behavior.scm` 不在 `entry_points` 里，理由已经在 `mods/example_mod/mod.json5` 的注释与 `ScriptBehaviorSource` 模块文档里写清楚：`entry_points` 列的是**装载时**跑一次、注册进 `Registry`/六张内容表的脚本（对应 `ll_mod::pipeline::load_all`），而行为树脚本面向的是**运行时**按需构造的另一个引擎（`ScriptBehaviorSource::new`），构造时机、生命周期、注册的 API 集合完全不同——把它塞进 `entry_points`，装载管线的引擎会因为认不出 `nearby-enemy`/`skill-ready?` 这些运行期查询函数名而白名单拒绝，直接报错。

**这说明"入口点"从来就不是一个单一概念，而是"哪个消费者会在什么时机加载这份脚本"的分类。** 现有代码用"干脆不列在任何地方，靠调用方自己知道路径"来规避这个问题——这不是设计，是巧合地绕过了，且不可持续：下一个需要独立加载时机的脚本类别（例如剧本系统，见[剧本系统设计](narrative-system.md)）如果还是这么做，又会有一份"没有任何清单能查到，只能翻源码才知道在哪"的脚本。

### 设计：`[scripts]` 表，按用途分类，而不是给每个脚本打标签

```json5
scripts: {
  content:   ["terrain.scm", "gameplay.scm"],   // 装载管线消费，load_all 时求值
  behaviors: ["behavior.scm"],                    // ScriptBehaviorSource 消费，运行期按需求值
}
```

**为什么是"分类别的字段"，不是"给每个脚本条目标注 kind"**（例如 `entry_points = [{path="...", kind="content"}, ...]`）：

- 消费者是固定的少数几种（目前两种：装载管线、行为树引擎；未来可能加剧本系统），不是任意组合——用固定字段名，`ModManifest` 直接映射成对应的 `Vec<PathBuf>` 字段，调用方拿到的就是"我关心的那一类"，不需要先遍历再按 `kind` 过滤。
- 这与 `manifest.rs` 现有风格一致——现有字段本来就是"这一类东西的列表"（`dependencies`/`entry_points`），没有理由为了"看起来更通用"改成一份需要额外解释 `kind` 枚举取值的标注式列表。

**向后兼容与迁移**：`entry_points` 这个裸字段名直接重命名为 `scripts.content`（语义完全等价，都是"装载管线消费"），`mods/example_mod/mod.json5` 需要同步改写——这是一次性的、影响面极小的迁移（仓库里目前只有三个 mod 用到清单，`broken_syntax`/`broken_whitelist` 两个测试夹具甚至用不到 `entry_points` 之外的字段）。`behavior.scm` 从"完全不在清单里、靠调用方手写路径"变成"显式列在 `scripts.behaviors` 里"——`ScriptBehaviorSource` 的实际使用方（无论是未来的怪物生成配置还是别的什么调用点）此后可以直接从清单读到路径，不需要另外硬编码。

**扩展性**：未来新增一类消费者（例如[剧本系统设计](narrative-system.md)的叙事脚本），只需要在 `[scripts]` 表下加一个新字段（比如 `narrative = [...]`），`ModManifest` 结构体加一个对应字段即可——serde 反序列化默认忽略结构体未声明的字段，旧版本引擎遇到一份带了新字段的清单不会报错，只是读不到那部分内容（"不认识就忽略"，天然向前兼容）；新版本引擎则正常解析出这个新字段。**不需要一份开放式的 `BTreeMap<String, Vec<PathBuf>>` 来"预留任意未来分类"**——mod 清单不是存档，不存在"旧存档要能被新代码读出"这类迁移压力（清单每次装载都重新解析，不是被序列化保存下来的历史状态），加固定字段就足够，不需要为一个不存在的迁移问题预先设计灵活性。

---

## 四、资产 VFS

### mod 怎么引用自己的资产，怎么引用本体的

**资产的"地址"就是它在 VFS 里的相对路径，加上"这份路径归属哪个命名空间"**——不复用 `NamespacedId` 类型本身（`NamespacedId` 的路径段字符集不允许 `/`，见 `crates/ll-core/src/ident.rs::is_valid_segment`，无法直接表达 `sprites/terrain/lava` 这类带层级的路径），而是沿用"命名空间 + 相对路径"这个更宽松的二元组，与 `entry_points` 现有的"相对路径字符串"风格一致，不强行套用内容 ID 的字符集限制。

- **引用自己的资产**：mod 脚本/图集清单里直接写相对路径（`"sprites/lava_floor.png"`），隐式归属"当前这个 mod 自己的命名空间"——不需要在路径里重复写一遍自己的命名空间，这与 4.1 节脚本状态存储"`mod_namespace` 由宿主执行上下文决定,不是脚本传的参数"是同一个思路：既然读取动作发生在"这个 mod 自己的加载上下文"里,归属天然已知。
- **引用本体或其他 mod 的资产**：需要显式写出目标命名空间，形如 `"lostland:sprites/terrain/lava.png"`（本体命名空间 `lostland`，与「本体即 Mod」一贯的用法一致）。这类引用只应该出现在**声明了对应依赖**的 mod 里（引用 `othermod` 的资产却没有在 `[dependencies]` 里声明依赖 `othermod`，属于清单校验阶段就能拦下的错误，具体校验时机留给实现任务，本文档只定规则）。

### mod 能不能覆盖本体的资产——能，这是最常见的 mod 需求

**换贴图走"同路径覆盖"约定**，不是靠一个独立的"资产 ID 注册表"：mod 在自己的 `assets/overrides/<目标命名空间>/` 下放一个与目标资产**相对路径完全相同**的文件，即视为覆盖。

```
mods/reskin_mod/
  mod.json5
  assets/
    overrides/
      lostland/
        sprites/terrain/lava.png     # 覆盖本体的 lostland:sprites/terrain/lava.png
```

**为什么是"同路径覆盖"而不是"资产 ID + 注册表"**：换贴图这个需求的本质是"把这份资产的字节内容换掉，逻辑意义（这是哪种地形的贴图）完全不变"，不需要引入一层可以被"注册"的间接寻址——路径本身已经是稳定、可预测的标识，与 Minecraft 资源包等成熟 mod 生态的"同路径覆盖"惯例一致，作者理解成本最低，也不需要设计一套新的 ID 分配/校验机制。

**安全边界**：覆盖文件物理上仍然位于**mod 自己的目录**内（`assets/overrides/` 是 mod 目录树的一部分），不存在写到 mod 目录之外的路径——满足 [ADR 0019](../decisions/0019-denied-capability-needs-substitute-or-justification.md) J 组"限定在 mod 自己目录内"这条既有安全边界要求。**实现时需要额外校验的一点**（本文档指出，不代为实现）：VFS 解析相对路径时必须拒绝包含 `..` 段或绝对路径的输入——`manifest.rs` 当前对 `entry_points` 的路径拼接（`base_dir.join(p)`）**没有做这层过滤**，理论上一个恶意清单可以写 `entry_points = ["../../../../etc/passwd"]` 让路径跳出 mod 目录；这是一处已核实存在、但不在本任务权限范围内修复的既有缺口（本文档只如实标注，实现任务应当一并处理 `entry_points` 与资产 VFS 两处的路径穿越校验，不能只补资产 VFS 这一处）。

### 覆盖冲突：两个 mod 都想换同一张图

**规则：按依赖拓扑排序产出的加载顺序，后加载的覆盖先加载的**——直接复用 `topo_sort`（`crates/ll-mod/src/topo.rs`）已经产出的确定性总序，不为资产覆盖另设一套排序规则。

- 若 mod B 依赖 mod A（`dependencies` 里声明），B 在拓扑序里排在 A 之后，B 对 A 某资产的覆盖生效——**这给了 mod 作者一个显式的手段来保证自己的覆盖优先**：想让自己的换贴图 mod 稳定压过某个具体 mod，声明一条依赖即可，不需要额外的"mod 管理器里手动拖顺序"这类不确定性来源（本项目一贯拒绝任何依赖执行顺序的不确定性，见 `topo.rs` 模块文档对确定性的反复强调）。
- 若两个 mod 之间没有依赖关系（互不知道对方存在），`topo_sort` 的既有决胜规则（命名空间字典序）同样适用于覆盖顺序——这个结果对 mod 作者而言是"确定但基本无感知"的（不像声明依赖那样是主动选择），**这是一个已知的、诚实标注的局限**：没有依赖关系的两个 reskin mod 谁的覆盖生效，取决于命名空间字符串的字典序，不是"最后安装的那个赢"这类符合直觉的规则。若未来出现明确的"资产覆盖优先级"需求，应该是一个独立设计的扩展点，不是本文档顺手加一个新字段能解决的（YAGNI，暂不设计）。

**必须可见，不能是静默覆盖**——`LoadStatus::Warning(String)`（`crates/ll-mod/src/load_report.rs`，已核实存在且当前尚无产出路径，模块文档举的例子正是"非致命但值得作者注意的问题"）是现成的落点：资产 VFS 构建阶段发现同一个目标路径被多个 mod 覆盖时，应该产出一条 `Warning`，注明"资源 `lostland:sprites/terrain/lava.png` 被 `mod_a`/`mod_b` 同时覆盖，当前生效：`mod_b`"，展示在加载管理界面里。这与 [ADR 0016](../decisions/0016-mod-performance-tiers-by-declaration.md)「配套：把开销做成可见的」是同一条精神——"代价可见，人才会去省"这里替换成"冲突可见，作者才知道该不该处理"。**这不是本文档发明的新通道，是复用已经预留但从未被真正产出过的既有字段。**

### 被否决的方案

- **资产覆盖需要显式声明"我要覆盖谁"（清单里加一个 `overrides = [...]` 列表）**——否决：与 `entry_points` 相比，覆盖关系已经由文件路径本身（`assets/overrides/<目标命名空间>/...`）完整表达，再加一个列表字段是重复信息，且这份列表必须与实际文件同步，容易在维护中脱节（文件加了、清单忘了改，或反过来）。目录结构本身已经是唯一真相源，不需要第二份声明。
- **覆盖优先级作为清单里的一个数值字段（`override_priority = 100`）**——否决：这类"优先级数字"方案在其他 mod 生态里常见，但会引入一个新的、脱离依赖图的排序维度——两个 mod 的优先级数值相同时怎么办？需要再定一条 tie-break 规则，而"按依赖拓扑序"已经是本项目到处复用的现成确定性来源，没有理由在资产覆盖这一个角落另起一套并行的排序机制。

---

## 五、依赖与版本约束

### 结论：加版本约束，但分成两条独立的轴

```json5
{
  compatible_game_version: ">=0.5.0, <0.7.0",   // 轴一：mod ↔ 游戏引擎

  dependencies: {
    othermod: ">=1.0.0",                          // 轴二：mod ↔ mod
  },
}
```

**为什么是两条轴，不是一条**：这两类兼容性问题的检查时机、失败范围完全不同，混在一起处理反而会让报错定位更难。

#### 轴一：`compatible_game_version`——与其余 mod 无关，隔离失败

游戏引擎自身的 mod API 表面会随版本演进而变化（`register-skill` 十个位置参数这类脚本 API，历史上已经因为新增字段而调整过签名——本文档核实到的 `ClassDef`/`SkillDef` 字段随批次增长就是先例）。一个针对旧版引擎写的 mod，装到新版本上可能因为 API 形状变了而在脚本调用阶段报出一堆"参数个数不匹配"，这类错误对 mod 作者/玩家而言定位成本很高（不知道该怀疑 mod 写错了还是版本不对）。

**检查时机**：清单解析阶段（`Parse` 阶段），只需要拿到当前运行的游戏版本号做一次比较，不依赖其他任何 mod 是否存在。**失败范围**：只影响这一个 mod——与其余 mod 的依赖图完全无关，因此按现有"分阶段隔离"精神，只需要把这一个 mod 标记为 `Failed`（新增一个 `ModError` 变体，例如 `IncompatibleGameVersion`），不影响同批次其他 mod 继续加载。

#### 轴二：`[dependencies]`——版本约束校验在 Topo 阶段，沿用现有"整批中止"语义

依赖版本约束（"我需要 `othermod` 至少 1.0.0"）与"依赖是否存在"、"依赖是否成环"性质相同——都是**依赖图本身是否合法**的问题，因此校验时机与 `check_missing_dependencies`/`check_duplicate_namespaces` 放在同一处（`topo_sort`），失败时机与失败语义也应当一致。

**已核实的现状**（`crates/ll-mod/src/pipeline.rs::attribute_topo_error`）：`topo_sort` 一旦失败（无论是缺失依赖、成环还是重复命名空间），**整批** `parsed` 里的全部 mod 都会被标记为 `Failed`，不只是直接牵涉的那几个——`attribute_topo_error` 只是把"是不是直接肇事者"体现在错误文案的措辞上（肇事者拿到具体原因，其余拿到"因为其他 mod 导致整批中止"的说明），并不区分"隔离失败"与"整批失败"两种严重程度。

**版本不兼容应当沿用同一套"整批中止"语义，不是这次才需要单独设计降级方案**：新增 `ModError::IncompatibleDependencyVersion { dependent, dependency, required, actual }`，在 `check_missing_dependencies` 之后、`build_graph` 之前追加一次版本比较（依赖存在但版本不满足约束，与"依赖压根不存在"是同一类"这条边不可用"的失败，理应导致同样的整批中止后果）。

**这意味着一个真实的、值得记录的后果**：单个 mod 声明了一个过严格或过时的版本约束，可能导致**整个 mod 列表**都加载不了，而不是只有它自己失败——这与当前 `MissingDependency`/`CyclicDependency` 已经存在的行为完全一致，本文档不是引入一个新的严重性等级，只是让版本约束复用既有的严重性等级。**更精细的"只砍掉这条边受影响的子图，其余 mod 照常加载"是一个更大的架构改动**（需要在拓扑排序失败时还能计算出"哪些 mod 完全不受这条失败边影响"，并对这部分单独跑一次装载），本文档认为这超出"给依赖加版本约束"这个具体需求的范围，标注为将来可能的扩展方向，不在本批次设计。

### 与存档-mod 集合策略的配合：两个独立问题，不要混

[身份与 ID 空间](identity-and-ids.md)「六、存档与 mod 集合」已经设计了内容哈希 + 生成期/当前 mod 集合双记录，回答的是**"这份存档记录的 mod 内容，和我现在装的这批 mod 是不是同一回事"**——这是一个只有在"打开一份已有存档"这个动作发生时才有意义的问题。

本节的 `compatible_game_version`/`[dependencies]` 版本约束回答的是完全不同的问题：**"这批 mod 现在能不能一起被加载"**——在任何存档被打开之前、纯粹基于当前发现到的 mod 清单就能回答，甚至新开一局游戏（不涉及任何存档）也需要这层校验。

两者不冲突、不重叠，但有一处天然的配合点：`identity-and-ids.md` 已经指出"mod 作者改内容却不改版本号是常态"，所以**版本约束检查是一道廉价、及早暴露明显不兼容的第一道关卡，不能替代内容哈希这道更权威但只在开档时才跑得到的检查**——版本号本身可能过期或不准，`compatible_game_version`/`[dependencies]` 版本约束只保证"起码声明的版本对得上"，是否真的没有内容漂移仍然要靠 `Registry::content_hash_of` 在存档层面把关。**如果版本约束通过了，但内容哈希在读档时报出不一致，这不是本节设计失败，是两道关卡各自诚实地只回答自己那部分问题**：前者回答"装得起来吗"，后者回答"和这份存档记得的是不是同一个东西"。

### 被否决的方案

- **完整的 SAT 风格依赖求解器（自动选择满足所有版本约束的 mod 组合）**——否决：当前没有 mod 市场/多版本共存的场景（一个 mod 目录下同一个命名空间只能有一份，`DuplicateNamespace` 已经保证这一点），"选择哪个版本"这个问题在本项目里根本不存在——玩家的 mod 目录里每个命名空间恒定只有一份，需要判断的只是"这一份的版本，满足声明的约束吗"，是非布尔判断,不是选择问题,不需要求解器。
- **版本不满足时自动降级/跳过该依赖继续加载**——否决：与"缺失依赖"是同一类问题——[身份与 ID 空间](identity-and-ids.md)「② 缺失 mod 之后怎么处理」已经论证过，降级策略必须**按内容类型分类**决定（有的能丢弃、有的不能），不存在一个统一的"版本不满足就自动怎样"的默认动作；在依赖图校验这个阶段，能确定的只有"这条依赖边不可用"，不知道对方到底提供了什么内容、丢了会有什么后果，因此不应该在这里假装能安全降级，交给整批中止 + 明确报错，让 mod 作者/玩家自己决定去掉哪个 mod 或换版本。

---

## 相关文档

- [身份与 ID 空间](identity-and-ids.md) —— `NamespacedId`/`ContentIndex` 字符集与不可持久化规则、存档与 mod 集合的内容哈希/生成期-当前双记录策略
- [剧本系统设计](narrative-system.md) —— 消费本文档「入口点分类」新增的脚本分类字段（叙事声明脚本走 `scripts.content` 同一条内容注册管线）
- [脚本状态存储](script-state-storage.md) —— `state-get-foreign` 已经示范"依赖拓扑保证被依赖 mod 总是先加载完成"这条既有性质，本文档资产覆盖顺序复用同一条性质
- [0016 — mod 性能分档按声明方式](../decisions/0016-mod-performance-tiers-by-declaration.md) —— "代价可见，人才会去省"，本文档资产覆盖冲突警告与配额展示同一条精神
- [0019 — 被禁能力必须有替代品或理由](../decisions/0019-denied-capability-needs-substitute-or-justification.md) —— J 组「资产 VFS 规划存在，未实现」的既有核实结论，本文档是该缺口的具体设计
- [总纲设计规格](../../docs/superpowers/specs/2026-08-16-lostland-design.md) —— §5 crate 分层、§10.6 加载管理界面、§11.1 格式分工
