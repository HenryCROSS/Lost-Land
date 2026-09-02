#!/usr/bin/env bash
# 跨表撞名门禁：**一个 ContentIndex 至多被一张内容表 define**。
# 对应 .github/workflows/ci.yml 的 `content-index-table-exclusivity` job。
#
# # 判据在哪、为什么不在这个脚本里
#
# 判据本身是 Rust：`ll_mod::content_audit::detect_table_define_collisions`
# 在**装载完成后**逐个索引问 `ll_mod::content_hash::tables_defining`
# 「哪些表定义了它」，长度 ≥ 2 即撞名。它必须在装载后跑——「谁定义了这个
# 索引」这个问题在读 json5 的那一刻还没有答案（ADR 0015：注册是解析，
# 不是不变式，答案取决于本次装载了哪些 mod）。因此本脚本不去正则扫
# `mods/**/*.json5`：文本扫描既看不见 Rust 侧注册的本体内容（地形、天气、
# 空间层），也看不见 mod 之间的跨命名空间撞名，会是一道有覆盖缺口的门禁
# ——ADR 0022 说的正是「判据覆盖不全等于没有判据」。
#
# `ll_game::content::load_content` 在生产路径上直接 `?` 掉它：撞名了，
# 游戏就不启动，与「引用指向不存在的内容」同一档（理由见
# `ContentAuditReport::table_exclusivity` 文档「为什么归在②」一节）。
#
# # 那这个脚本还做什么：把三条判据**点名**跑一遍，防它们悄悄消失
#
# `run_tests.sh` 的 `cargo test --workspace` 当然也会跑到这三条测试，
# 但那是在几千条测试里——某天有人把它们删了、改名了、或者 `#[ignore]`
# 了，工作区测试仍然一片绿，没有任何人会注意到少了三行。本脚本用
# `--exact` 逐条点名，并**核对每条确实跑了 1 条**：测试没了或改名了，
# 这里报"0 passed"当场红，而不是静默变成一道不存在的门禁。这是同一条
# 「非空转」纪律在脚本层的一次应用。
#
# 三条测试各自守着判据的一半，缺一不可：
#
#   1. 真实内容上判据为绿，且**逐个索引都看过**（不是循环一次没跑）。
#   2. 人为造一个撞名，判据**真的会红**（ADR 0022 的反例验证：改坏了
#      也不红的断言等于没有断言）。
#   3. 报红时的文案点名两张表、给得出改名与删定义两条出路。
#
# # 它拦不住什么（如实写在这里）
#
# 判据的覆盖面等于 `ll_mod::content_hash::ContentValueTables` 的字段集
# ——**新增一张内容表却压根没把它接进那个结构体**，本门禁看不见它，
# 撞到它头上的名字照样过。那一步没有通用的编译期手段（`classify_index`
# 文档「局限」一节记的是同一件事），兜底的是
# `ll_game::content` 的 `真实内容装载后仅本体占位种族被值哈希判定为无归属表`
# ——新表全部条目会落进 `Opaque`，那条测试变红。两条一起才算覆盖住。
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

export RUSTFLAGS="${RUSTFLAGS:--D warnings}"

# 逐条点名跑，并核对"确实跑了 1 条"。
run_exact() {
  local package="$1"
  local test_path="$2"
  local why="$3"

  echo "  - ${test_path}（${why}）"
  local output
  if ! output="$(cargo test -p "${package}" --lib -- --exact "${test_path}" 2>&1)"; then
    echo "${output}"
    echo "::error::撞名门禁的判据 ${test_path} 跑红了。"
    return 1
  fi
  if ! printf '%s' "${output}" | grep -qE '^test result: ok\. 1 passed'; then
    echo "${output}"
    echo "::error::撞名门禁点名的判据 ${test_path} 没有跑到（0 passed）——它被删掉、改名或 #[ignore] 了。判据消失是比判据变红更严重的事：门禁会静默变成不存在。请恢复它，或在本脚本里同步更新名字并说明为什么。"
    return 1
  fi
}

echo "跨表撞名门禁（一个 ContentIndex 至多被一张内容表 define）："
run_exact ll-game \
  'content::tests::真实内容里一个索引至多被一张内容表定义且不是空转' \
  '真实内容为绿，且逐个索引都看过'
run_exact ll-mod \
  'table_exclusivity::tests::同一个索引被两张表定义时撞名检查报出两张表' \
  '人为造撞名时判据真的会红'
run_exact ll-mod \
  'table_exclusivity::tests::每个索引各归一张表时撞名检查通过且不是空转' \
  '各归各表时不误报'
run_exact ll-mod \
  'table_exclusivity::tests::撞名错误文案点名两张表并给出改名与删定义两条出路' \
  '报红时说的话可行动'

echo "跨表撞名门禁四条判据全部跑到且通过。"
