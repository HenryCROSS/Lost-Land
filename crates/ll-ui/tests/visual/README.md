# 视觉回归基准

``baseline/p4_acceptance.png`` 是**视觉回归基准**：640×360，P4 mod 加载界面验收 demo 的世界层一帧。
**只覆盖世界层**——文字面板画在窗口 surface 上读不回来（那张纹理只声明了
`RENDER_ATTACHMENT`、不含 `COPY_SRC`，是 `ll-text` 「两条渲染通道」架构的
结果，不是疏漏，见 `knowledge/handoff/p4-to-p5.md`）。

## 比对失败时的处置规矩

与 `crates/ll-render/tests/visual/README.md` 那份逐字同一条，适用于这张图：

1. **先判断是有意的视觉调整还是缺陷**，不要假设任何一种。
2. 只有确认是**有意调整**才更新基准，并在提交信息里说明改了什么、为什么。
3. **绝不允许「测试挂了就重新截图覆盖」**——基准是需要被保护的资产，
   不是可以随手刷新的缓存。

## 生产者已删除（2026-08-29 批次 13）

**`baseline/p4_acceptance.png`** 的唯一生产者是 ``ll-ui:p4_acceptance`（按 F2 存图）`。
2026-08-29 项目所有者裁定去掉 `examples/`（原话「我觉得应该要去掉 example。
然后有用的东西搬迁了。剩下的后面考虑。」），那个 target 随之删除，见
[ADR 0030](../../../../knowledge/decisions/0030-remove-examples-acceptance-demos.md)。

**图本身一张没删，删的是生产者。** 也就是说：这张基准现在**无法重新生成**，
在有人按上面的方式恢复生产者、或另立一套无头像素比对之前，它只能当作
**只读的历史留档**看——发现不一致时无法「重新截一张对比」，只能靠读代码判断。
（这张图要真实窗口 + 图形适配器才截得出来，无头 CI 上 `request_adapter` 会失败，
硬搬成测试只能写出「拿不到适配器就跳过」的假测试，正是 ADR 0018 要根除的东西。）

这不是一次「顺手删掉」——ADR 0030「后果」一节列了三条路（保留那一个 example /
把生成逻辑搬成测试 / 放弃这张基准）各自的代价，并明确写着**本批次不替所有者
做选择**，只做最保守、最容易反转的那一侧：图留着，生产者删掉，恢复方式写在
这里。

恢复被删的生产者（git 里逐字节留着，与提交哈希无关）：

```bash
git log --oneline --diff-filter=D -- crates/ll-ui/examples
git show <那个提交>^:crates/ll-ui/examples/p4_acceptance/main.rs
```
