#!/usr/bin/env bash
# 真的运行验收 demo 的 main()：对应 .github/workflows/ci.yml `test` job
# 的「运行验收 demo」步骤。
#
# 为什么需要这一步：编译通过 ≠ 断言通过
#
# `cargo test --workspace`（scripts/ci/run_tests.sh）与
# `cargo clippy --all-targets`（scripts/ci/check_clippy.sh）都会**编译**
# 每一个 example，但都不会**运行**它的 `main()`。少数 example 在
# `Cargo.toml` 里加了 `test = true`（`ll-content` 的
# `p5_gameplay_acceptance`、`ll-sim` 的 `p5_coordinate_acceptance` 等），
# 那个开关也只让 `cargo test` 跑该文件里的 `#[test]` 函数，同样不碰
# `main()`。
#
# 后果在 2026-08 被实际撞上：`ll-content` 的 `p5_save_acceptance` 因为
# 「决策二」硬门禁（`ll_content::load_error::check_mod_set`）落地而失效
# ——demo 仍在断言旧的「生成期 mod 整个卸载 → 只读」，而新门禁会抢先
# 判定 `ModSetMismatch` 直接拒绝，于是 `cargo run --example
# p5_save_acceptance` 在 main 上**恒定 panic**。整条 CI 却全绿：它编译
# 得过，而验收断言全写在 `main()` 里，没有任何一处会去执行它。一个
# 「验收 demo」在主干上坏掉几个批次无人发现，正是因为没有任何东西运行
# 它。本脚本补上这一步。
#
# 为什么是白名单，不是「跑所有 example」
#
# 本仓库的 example 分两类，只有一类能在 CI 里跑：
#
# - **无头、纯数据、自己会退出**——`ll-content` 的两个 P5 验收 demo。
#   它们不开窗、不碰 GPU，全部结论来自 `assert!`，跑完即退出，断言失败
#   以非零状态退出。这类适合进 CI。
# - **开窗/需要 GPU/等待人操作**——P0~P4 与 `p5_coordinate_acceptance`
#   要开真实窗口并等 WASD/Esc 输入，`mixed_text_demo` 要求机器有可用
#   图形适配器。这类在无头 CI runner 上要么直接失败、要么永远等下去，
#   不能纳入。
#
# 因此下面维护两份显式清单。**新增 example 必须落进其中一份，否则本
# 脚本报错**——这条完整性检查（见 `未分类` 一段）是本门禁真正的价值
# 所在：它让「又加了一个没人跑的验收 demo」这件事在 CI 上直接变红，而
# 不是像这次一样，安静地烂在主干上等人偶然发现。
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"

# 会被真正运行的 demo：`<crate>:<example>`。
RUN_LIST="ll-content:p5_save_acceptance
ll-content:p5_gameplay_acceptance"

# 不能在 CI 运行的 example：`<crate>:<example>|<理由>`。理由必须写清楚
# 是「开窗」「要 GPU」「等人输入」哪一种，不接受「暂时不跑」这类空话
# ——写不出具体理由的，多半就是该进 RUN_LIST 的。
SKIP_LIST="ll-platform:p0_acceptance|开真实窗口并等待方向键/Esc 输入，无头 runner 上会一直等下去
ll-render:p1_acceptance|开窗渲染，需要图形适配器
ll-world:p2_acceptance|开窗渲染真实地形/光照，需要图形适配器
ll-sim:p3_acceptance|开窗渲染战斗过程，需要图形适配器
ll-ui:p4_acceptance|开窗渲染 mod 加载界面，需要图形适配器
ll-sim:p5_coordinate_acceptance|开窗并等待 WASD/Enter 输入
ll-text:mixed_text_demo|离屏渲染仍要求机器有可用 wgpu 适配器，无头 runner 上 request_adapter 会失败
ll-game:npc_roster_preview|开发期人工观察用的预览工具，不是验收 demo，无断言可言
ll-game:settlement_preview|同上，人工观察用预览
ll-game:surface_preview|同上，人工观察用预览
ll-game:probe_aistall|开发期一次性排查探针，用完即弃，无稳定断言
ll-game:probe_conquest|同上，一次性排查探针
ll-game:probe_content_hash|同上，一次性排查探针"

# ---------------------------------------------------------------------
# 完整性检查：每一个 example 必须被显式分类
# ---------------------------------------------------------------------
# 用 `cargo metadata` 而不是 `ls crates/*/examples`：前者是 cargo 自己
# 认定的 target 清单，`[[example]]` 里用 `path =` 改过位置、或以目录
# （`examples/<name>/main.rs`）形式组织的 target 都能正确枚举到，后者
# 会漏。
# LC_ALL=C：`comm` 要求两份输入按**同一套**排序规则有序，而 `sort` 的
# 排序结果依赖 locale。不钉死它，本机与 CI runner 的 locale 一旦不同，
# 比较就会给出无声的错误结果。
export LC_ALL=C

# 走 `sys.stdout.buffer` 而不是 `print`：Windows 上 Python 的文本模式
# stdout 会把换行翻译成 CRLF，行尾多出来的那个 \r 会让下面的 `comm`
# 认为所有条目都对不上（本脚本第一版就是这么翻车的，现象是每一个
# example 都被报成"未分类"）。写 bytes 绕开文本模式的换行翻译，两个
# 平台上都只出 \n。
ALL_EXAMPLES="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c '
import sys, json
meta = json.loads(sys.stdin.buffer.read().decode("utf-8"))
for package in meta["packages"]:
    for target in package["targets"]:
        if "example" in target["kind"]:
            line = "%s:%s\n" % (package["name"], target["name"])
            sys.stdout.buffer.write(line.encode("utf-8"))
' | sort)"

CLASSIFIED="$( { echo "${RUN_LIST}"; echo "${SKIP_LIST}" | sed 's/|.*//'; } | sed '/^$/d' | sort)"

UNCLASSIFIED="$(comm -23 <(echo "${ALL_EXAMPLES}") <(echo "${CLASSIFIED}"))"
if [ -n "${UNCLASSIFIED}" ]; then
  echo "错误：以下 example 没有在 scripts/ci/run_acceptance_demos.sh 里分类：" >&2
  echo "${UNCLASSIFIED}" | sed 's/^/  - /' >&2
  echo "" >&2
  echo "请把它加进 RUN_LIST（无头、有断言、会自己退出的验收 demo），" >&2
  echo "或加进 SKIP_LIST 并写明不能在 CI 跑的具体理由。" >&2
  exit 1
fi

# 反向检查：清单里列了、但 target 已经不存在（重命名/删除后忘了同步）。
STALE="$(comm -13 <(echo "${ALL_EXAMPLES}") <(echo "${CLASSIFIED}"))"
if [ -n "${STALE}" ]; then
  echo "错误：以下条目在 scripts/ci/run_acceptance_demos.sh 的清单里，但已经不是任何 example target：" >&2
  echo "${STALE}" | sed 's/^/  - /' >&2
  echo "（example 被重命名或删除后，请同步更新本脚本的清单。）" >&2
  exit 1
fi

# ---------------------------------------------------------------------
# 真正运行
# ---------------------------------------------------------------------
# 用 for 而不是 `echo ... | while read`：后者让循环体落进管道子 shell，
# 循环里 cargo 失败时退出码能否传出去取决于 pipefail/最后一条命令这些
# 细节，是一个容易写出「demo 挂了 CI 却绿」的地方——而那恰恰是本脚本
# 要根除的失败模式。条目里没有空格，直接靠词分割遍历最稳妥。
for entry in ${RUN_LIST}; do
  crate="${entry%%:*}"
  example="${entry##*:}"
  echo ""
  echo "--> cargo run -p ${crate} --example ${example}"
  # 不吞输出：demo 打印的每一行 [验收 N/M] 都是这次运行真的走到了哪一步
  # 的证据，CI 日志里留着它们，失败时不必重跑就能看出断在哪一节。
  cargo run --quiet -p "${crate}" --example "${example}"
done

echo ""
echo "验收 demo 全部运行通过（$(echo "${RUN_LIST}" | sed '/^$/d' | wc -l | tr -d ' ') 个；" \
     "另有 $(echo "${SKIP_LIST}" | sed '/^$/d' | wc -l | tr -d ' ') 个因开窗/需要 GPU/等待输入而不在 CI 运行，理由见脚本清单）。"
