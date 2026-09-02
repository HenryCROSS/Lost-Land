#!/usr/bin/env bash
# 禁止第二份「面板相对屏幕怎么落位」的实现：对应 .github/workflows/ci.yml
# `single-anchor-impl` job。
#
# # 为什么要这道门禁，而不是收敛完就算了
#
# 规格 L2（knowledge/design/ui-and-navigation.md §6.3）要求把「已知屏幕
# 尺寸、锚点、自身尺寸、边距，求原点」这一句算术收敛成一个函数
# （`ll_ui::widget::geometry::Rect::anchored`）。落地时它被抄了**五遍**，
# 而五遍在边界钳制上还各不相同（反馈行对 y 做了 `.max(0.0)`，另外四处
# 刻意不钳，没有任何一处说明为什么不同）。
#
# **收敛一次而不设防，它会重新长出来**——这不是洁癖，是本仓库已经发生
# 过的事，而且不止一次：
#
# - 规格写这条时数的是「四遍」。等到批次 30 真去收敛，实际是**五份**：
#   批次 23 一边把反馈行与按键提示行合并成一份，一边在
#   `placed_action_menu` 的 `ScreenCenter` 分支里**新写了一份**垂直居中。
#   同一批人、同一周、同一个文件夹。
# - `crates/ll-game/src/layout.rs` 的地形清单漏掉沙漠与冻原，从落地起就
#   在盲区里，直到批次 28 才被发现；而它旁边的 `atlas_coverage` 当初正是
#   因为同一种「手写清单会漏」被重写过。收敛解决当下那几份，**设防才
#   解决这一类**。
#
# 形态照抄仓库既有先例 `check_no_manual_euclidean_distance.sh`——那道门禁
# 做的正是同一件事（禁手写公式、逼人用共享的那个），本脚本不发明新范式，
# 连「`//` 开头的行跳过」这条豁免都是逐字照搬的。
#
# # 判据：四种形状
#
# 在 `crates/ll-ui/src`、`crates/ll-game/src` 下，除
# `crates/ll-ui/src/widget/geometry.rs`（`anchored` 自己的家）之外，任何
# 一行命中以下之一即红：
#
#   1. `(screen_width  - …) * 0.5` / `… / 2.0`   —— 水平居中
#   2. `(screen_height - …) * 0.5` / `… / 2.0`   —— 垂直居中
#   3. `screen_width  - … - …`（**两次**减法）  —— 贴右
#   4. `screen_height - … - …`                  —— 贴下
#
# 标识符按 `screen_width|screen_height|window_width|window_height|屏宽|屏高`
# 匹配。
#
# ## 为什么贴边那两条要求「两次减法」
#
# 一次减法是**算尺寸**，不是算位置：`hud::placement::world_map_rect` 里的
# `screen_width - margin_x * 2.0` 求的是地图面板有多宽。而贴边定位必然是
# 「屏宽 − 边距 − 自身宽」两次减法。这条区分让门禁不误伤尺寸计算。
#
# ## 判据盯的是**变量名**，因此测试里写具体像素不会被拦
#
# `assert_eq!(rect.x, 1280.0 - EQUIPMENT_RIGHT_MARGIN - EQUIPMENT_WIDTH)`
# 这类「把被收敛掉的旧算术写成期望值」的回归断言（规格 L2 明确要求每处
# 改写点各留一条）不会命中——它没有 `screen_width` 这个名字。那是夹具，
# 不是第二份实现：生产代码里不可能出现写死的屏幕尺寸。
#
# # 它拦不住什么（如实写在这里）
#
# 有人先 `let half = screen_width * 0.5;` 再自己减，本门禁看不见。所以
# 这条**不是单点防线**：规格 L1 的「常驻层不占屏幕中段」与 L0 的「整帧
# 边界都是整数」两条断言跑在 `build_hud_frame` 的**产出**上（见
# `crates/ll-ui/src/hud/render_layout_tests.rs`），新面板绕开 `anchored`
# 手写落位，仍然会在那两条行为判据上显形。三条一起才是「防第五份」。
#
# # 没有豁免清单
#
# 与欧氏距离那道门禁一样，本脚本不提供「把文件加进某张清单就能消红」的
# 口子——有清单，清单就会变成下一个手写清单。唯一的消红方式是调用
# `Rect::anchored`。
set -euo pipefail

# `anchored` 自己住在这里，它当然要写那几句算术。
ALLOWED='crates/ll-ui/src/widget/geometry.rs'

# 屏幕尺寸在本仓库里的几种叫法。
DIMS='screen_width|screen_height|window_width|window_height|屏宽|屏高'

# 形状 1/2：括号里减一下，出括号乘 0.5 或除以 2。
CENTER="\\(($DIMS)[[:space:]]*-[^()]*\\)[[:space:]]*([*][[:space:]]*0\\.5|/[[:space:]]*2(\\.0)?)"
# 形状 3/4：同一个表达式里连着减两次。
EDGE="($DIMS)[[:space:]]*-[^;,)]*[[:space:]]-[[:space:]]"

violations=0
while IFS= read -r -d '' file; do
  if [ "$(printf '%s' "$file" | tr '\\' '/')" = "$ALLOWED" ]; then
    continue
  fi
  while IFS=: read -r line_no content; do
    trimmed="$(printf '%s' "$content" | sed -e 's/^[[:space:]]*//')"
    # 与 check_no_manual_euclidean_distance.sh 逐字相同的注释豁免。
    case "$trimmed" in
      //*) continue ;;
    esac
    if printf '%s' "$content" | grep -qE "$CENTER"; then
      shape='形状 1/2（居中）'
    else
      shape='形状 3/4（贴边）'
    fi
    echo "::error file=$file,line=$line_no::检测到第二份「面板相对屏幕怎么落位」的实现——${shape}：${content}。规格 L2 要求这一句算术只有一份，改用 ll_ui::widget::geometry::Rect::anchored（Anchor::TopLeft/TopRight/TopCenter/Center/BottomCenter）。理由见本脚本头注释与 knowledge/design/ui-and-navigation.md §6.3。"
    violations=$((violations + 1))
  done < <(grep -nE "$CENTER|$EDGE" "$file" || true)
done < <(find crates/ll-ui/src crates/ll-game/src -type f -name '*.rs' -print0)

if [ "$violations" -gt 0 ]; then
  echo "共发现 $violations 处第二份落位实现，见上方逐条标注。"
  exit 1
fi
echo "未发现第二份「面板相对屏幕怎么落位」的实现。"
