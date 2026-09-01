# 批次 25 实施计划：ADR 引用编号勘误普扫（0018 → 0022）

- **工作树**：`wt-adrfix`（分支 `wt-adrfix`，基线 `origin/main` = `c6ea879`）
- **不碰**：`LostLand`（main 工作树）、`wt-races`（并行批次，改
  `crates/ll-world/src/race.rs`、`mods/lostland/races.json5`、
  `mods/lostland/cultures.json5`、角色创建屏、两份 `.ftl`、
  `crates/ll-ui/tests/i18n_text_width.rs`），以及磁盘上其余已合并的旧工作树
- **本批不改任何行为**：只动文档、注释与 ADR 编号引用，零逻辑改动、零新增/删除测试
- **不 push、不合并 main**

---

## 零、这一批为什么存在

`knowledge/handoff/2026-08-27-session-handoff.md` 第一节第 6 条把「反例验证是硬
要求」这条纪律记在 **ADR 0018** 名下。该条已由
[`2026-08-31-batch22-populated-baseline.md`](2026-08-31-batch22-populated-baseline.md)
四之三节原地更正过（原文保留 + 更正段），但**那条错误编号在此之前已经被全仓库
抄了上百次**——每一份新计划文档、每一条新测试注释都照抄了它。

复核（本批自己重跑，不引用任何口头转述）：

```bash
grep -rn "ADR 0018" --include=*.md --include=*.rs --include=*.py \
  --include=*.json5 --include=*.toml --include=*.sh . | grep -v "^./target" | wc -l
grep -c 反例 knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md
```

两个事实：

1. **ADR 0018** =
   `knowledge/decisions/0018-engine-layer-vs-gameplay-layer-scripting-boundary.md`，
   标题「脚本层边界改按系统类型划分（引擎层 / 玩法层）」。**全文与反例验证无关**
   （`grep -c 反例` 为 **0**；本批 B 部分在它末尾追加订正节之后这个数不再是 0，
   复核时请只看正文与两节订正之外的部分）。
   它讲的是三步归类判据 + 「本体即 Mod」限定于玩法层；其 2026-08-23 订正段把
   验收要求改写为「每种玩法层内容类型都能从 mod 的 JSON5 内容数据文件声明，且要有
   真实 mod 内容为证」。
2. **ADR 0022** =
   `knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md`，
   标题「覆盖不全的确定性哈希，等于没有确定性哈希（三个独立实例）」。它正文里
   有「故意改坏」验证的原始记录（`sea_level` 400 → 500 → 400），并明确把自己
   的主张概括成「**覆盖不全的守护等于没有守护**」。**反例验证 / 假绿 / 覆盖盲区
   这条纪律的依据是它。**

---

## 一、方法：三类分类，逐处看上下文，不做无差别替换

**禁止 `sed -i 's/ADR 0018/ADR 0022/g'`。** 每一处都读上下文，按下面这张判据表
归类。判据只有一条轴：**这一处引用想说的是哪条纪律**。

| 类 | 判据（引用处的语境） | 处置 |
|---|---|---|
| **① 错引** | 讲**反例验证 / 故意改坏 / 断言改坏了会不会红 / 假绿 / 永远绿的假测试 / 判据覆盖有盲区** | 改成 **ADR 0022** |
| **② 正确** | 讲**引擎层 vs 玩法层 / 三步归类判据 / 「本体即 Mod」/ 玩法层内容必须能由 mod 声明且要有真实 mod 内容为证** | **不动** |
| **③ 拿不准** | 讲**「新能力必须有经真实内容的端到端证据」/「必须走生产路径、不用简化夹具」**，且对象不是「mod 可声明的玩法层内容类型」 | **不动**，列进报告 |

### 为什么 ③ 单独成一类

0018 的订正段只要求「**玩法层内容类型**能由 mod 声明，且要有真实 mod 内容为证」。
把它读成「**任何新能力**都要有端到端证据」是一次悄悄的外推：存档槽位、世界生成
参数、图集打包这些在 0018 自己的逐项验证表里被明确判为**引擎层**，0018 的验收
要求根本管不到它们。而 0022 讲的是判据覆盖，改过去同样不通顺。

**这个洞不是本批发明的**：`knowledge/decisions/0028-...md:214` 已经有一段原地
订正写着「调查过程中有人把这条做法归给 ADR 0018，实际不是……这条做法只体现在
`crates/ll-mod/tests/*.rs` 的写法里，**没有任何 ADR 为它背书**」。本批复核确认
该结论仍然成立，如实报告并**建议**是否新开一份 ADR 承载它——**本批不自己新建
ADR**，那需要所有者裁定。

---

## 二、历史档案 vs 活文档：判据

纪律第 9 条要求「原文一个字都不改，在它自己身上加标记」。适用面按下表切分。

**判据**：*这份文件是「某个时刻的记录」，还是「现在还在被人照着做事的规范」？*
——记录某个时刻的，改原文就等于篡改历史；现在还在指导行为的，留着错编号就会
继续误导下一个人。

| 类别 | 具体目录/文件 | 处置 |
|---|---|---|
| **历史档案** | `docs/superpowers/plans/*.md`（每一份都是「某批次开工时的计划」）、`knowledge/audit/*.md`（某次审计的快照）、`knowledge/handoff/*.md`（某次会话的交接）、`knowledge/decisions/*.md`（ADR：0018 自己的「与 0016 的关系」一节已论证「已发布的 ADR 应当被取代而非篡改」） | 原文一字不改，**加更正标记**（顶部横幅或原地追加段） |
| **活文档 / 代码** | `knowledge/design/*.md`（持续修订的设计文档）、`knowledge/README.md`、`knowledge/design/README.md`、`docs/superpowers/specs/2026-08-16-lostland-design.md`（规格，本来就是原地修订的，§10.3 就有一次）、`docs/architecture/*.md`、`crates/**/*.rs` 注释与文档、`crates/**/tests/visual/README.md`、`mods/**/*.json5` 注释 | **直接改**编号 |

`knowledge/handoff/2026-08-27-session-handoff.md` 第 6 条**已经**被 batch22 原地
更正过，本批不重复标注。

---

## 三、B 部分：更正写回被更正方（纪律第 9 条）

两边互相指向，缺一不可：

- **ADR 0018** 末尾追加一节「【2026-08-31 订正】本 ADR 长期被误引为『反例验证』
  纪律的依据」，写明：什么时候、证据（`grep -c 反例` 为 0 / 被误引处的数量）、
  结论现在是什么（依据是 0022），并**链接到 0022**。正文一字不改。
- **ADR 0022** 末尾追加一节，说明这条纪律长期被记成 0018、本次已普扫更正，
  并**链接回 0018** 与本计划。

链接必须能被 `check_markdown_links.py` 与 `check_doc_links.sh` 解析。
**rustdoc 内部链接不要指向 `#[cfg(test)]` 里的函数**（上一批踩过）：代码注释里
一律写纯文本 `ADR 0022`，不写 rustdoc 链接。

---

## 四、C 部分：顺带清掉的两处

1. **`crates/ll-game/src/player_action.rs:379`**：`craft_entries` 的文档首行被
   重复了一遍（同一行里两份 `/// 制作菜单这一帧的行——全部已注册配方，按索引
   升序。`）。grep 确认后删掉重复的那一份。
2. **别的被引错的 ADR 编号**：用同一套方法（比对被引 ADR 的实际标题与引用处
   的语境）扫一遍。已发现的候选：「**ADR 0018 三档分级**」——三档分级是
   [0016](../../../knowledge/decisions/0016-mod-performance-tiers-by-declaration.md)/[0017](../../../knowledge/decisions/0017-tiered-declarations-materialize-columnar.md)
   的内容，0018 只在第三步判据里**引用**了 0016 的一/二档。活文档与代码里的按
   同一套规则改，历史档案加标记。

---

## 五、提交划分

| # | 内容 |
|---|---|
| A | 逐处核实并更正错引编号（代码注释、活文档直接改；历史档案加更正标记） |
| B | ADR 0018 / ADR 0022 互相指向的两段订正 |
| C | `craft_entries` 文档重复行 + 其余被引错的 ADR 编号 |

中文提交信息，`docs:` 前缀。

---

## 六、验证清单

- [ ] `bash scripts/ci/run_all.sh` exit 0
- [ ] 报告**自己跑的**改前/改后测试数与二进制数（不照抄任何人的数字）
- [ ] 文档断链门禁 + Markdown 死链门禁绿（新加的互相指向链接真的能解析）
- [ ] `git diff --stat` 复核：零 `.rs` 逻辑行改动（只有注释/文档行）
- [ ] 三类各多少处、拿不准的逐条列出并说明理由

**本批不做反例验证**（[ADR 0022](../../../knowledge/decisions/0022-guard-coverage-gap-defeats-the-guard.md)
那条纪律的适用对象是**新增断言**）：本批零新增断言、零行为改动，改前改后测试数
必须**逐个相同**——那才是本批的正确判据。
