//! 玩家初始钱包的**端到端**验收（ADR 0031）——批次 34 的 B 半。
//!
//! 所有者 2026-09-02 裁定：玩家开局带 1000
//! （`ll_game::world::PLAYER_STARTING_WALLET`）。它推翻的是交易批次那条
//! 临时裁定「玩家初始钱包仍然是 0，等所有者裁定」
//! （`docs/superpowers/plans/2026-09-01-batch31-dialogue-trade.md`
//! 第十一节第 9 条），那一条的后果是**玩家开局只能卖不能买**。
//!
//! | 能力 | 本文件里对应的断言 |
//! |---|---|
//! | 新开的局真的带着那笔钱 | `玩家开局带着所有者裁定的那笔钱` |
//! | 玩家**真的买得起东西了**（旧行为下买不起） | `玩家开局就买得起一件中等价位的货` |
//! | 老存档**不受影响**（钱包是存档里的数据，不是派生量） | `老存档里的钱包按存档里的值读回来` |
//!
//! # 本文件刻意**不往玩家钱包里塞钱**
//!
//! `trade_screen.rs` 那一份夹具给玩家手动写了 10000
//! （`玩家开局的钱` 常量）——它验的是交易本身，不是「开局带多少」。
//! 本文件若照抄那一手，「玩家买得起」这条断言就会**恒绿**，与钱包初值
//! 一点关系都没有。这正是本会话反复登记的那个形状：断言恒绿，因为被
//! 断言的那件事根本没有参与。

use std::path::{Path, PathBuf};

use ll_core::ident::ContentIndex;
use ll_game::content::{LoadedContent, RuntimeCatalogs, load_content};
use ll_game::save::{LoadedGame, load_game, save_game};
use ll_game::world::{GameWorld, PLAYER_STARTING_WALLET, build_new_world};
use ll_sim::intent::Intent;
use ll_sim::timeline::Timeline;
use ll_sim::trade::TradeDirection;
use ll_sim::turn::TurnEngine;
use ll_world::entity::EntityId;
use ll_world::generate::GenParams;
use ll_world::item::ItemStack;
use ll_world::ownership::Owner;
use ll_world::state::WorldState;

/// 固定种子，理由同 `trade_screen.rs` 的同名常量。
const SEED: u64 = 20260826;

/// 拿来买卖的那件货，以及它在 `mods/lostland/items.json5` 里的基础价。
///
/// **900 不是抄来的数**：`trade_screen.rs` 的
/// `在交易屏上买下一件东西货和钱各自换手` 断言的就是这个价钱。这里再写
/// 一遍是为了让下面那句「1000 买得起、0 买不起」读得懂——两处对不上时
/// 两条测试会一起红。
const 货: &str = "lostland:roast_meat";
const 货的价钱: i64 = 900;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn test_content() -> LoadedContent {
    let root = repo_root();
    load_content(&root.join("mods"), &root.join("assets"))
        .expect("仓库真实 mods/ 目录下本体内容契约必须解析成功")
}

fn 索引(content: &LoadedContent, raw: &str) -> ContentIndex {
    content
        .registry
        .get(&ll_core::ident::NamespacedId::parse(raw).expect("固定字面量标识符恒合法"))
        .unwrap_or_else(|| panic!("{raw} 必须已注册"))
}

/// 走生产路径开一局新游戏。
fn 新开一局(content: &LoadedContent) -> GameWorld {
    build_new_world(
        content,
        GenParams {
            seed: SEED,
            ..GenParams::default()
        },
    )
    .expect("建世界应当成功")
}

#[test]
fn 玩家开局带着所有者裁定的那笔钱() {
    // Arrange & Act：**不碰钱包**，只走生产的建世界路径。
    let content = test_content();
    let game_world = 新开一局(&content);

    // Assert
    let player = game_world
        .world
        .actors
        .get(game_world.player)
        .expect("新游戏必然有玩家");
    assert_eq!(
        player.wallet, PLAYER_STARTING_WALLET,
        "玩家开局的钱必须来自 build_player_agent 那一处常量"
    );
    assert_ne!(
        PLAYER_STARTING_WALLET, 0,
        "常量本身不能是 0——那正是本批要推翻的旧行为（只能卖不能买）"
    );
}

#[test]
fn 玩家开局就买得起一件中等价位的货() {
    // 这条是本批 B 半真正的验收线：**旧行为（钱包 0）下 resolve_trade
    // 的第五道闸门会当场拦下，产出零效果**（`ll_sim::resolve::trade`
    // 那句 `if buyer_agent.wallet < price { return Vec::new(); }`）。
    //
    // 反例（已实测，见计划文档）：把 `PLAYER_STARTING_WALLET` 改回 0，
    // 本条当场红，且红在「玩家背包里没多出东西」那一行。
    // Arrange
    let content = test_content();
    let mut game_world = 新开一局(&content);
    let player = game_world.player;
    let 玩家原有的钱 = game_world.world.actors.get(player).expect("玩家在").wallet;
    // **刻意不在这里加一句「夹具前提：开局这笔钱买得起」**。那样写会把
    // 「买得起」变成一条**排在真正的判据前面、且更容易红**的断言——
    // 本会话反复登记的假绿形状之一（「一条断言前面有更容易红的断言，
    // 导致它永远轮不到执行」）。实测过：加了那一句之后，把钱包改回 0
    // 这条反例红在**那一句**上，而不是红在「货真的到手了」上。
    // 判据因此全部落在下面的产出断言里，钱包数值只进错误消息。

    // 玩家出生装备里**可能本来就有同一件货**（种族 `starting_items`），
    // 因此判据取「这件货的**总件数**多了一件」，不是「多出一堆」——
    // 归属不同的两堆不会合并，堆数会多出一堆，但那是归属的事，不是本条
    // 要验的事。
    let 手上有多少件 = |game_world: &GameWorld, who: EntityId| -> i64 {
        game_world
            .world
            .actors
            .get(who)
            .expect("实体在")
            .inventory
            .iter()
            .filter(|stack| stack.def == 索引(&content, 货))
            .map(|stack| i64::from(stack.count))
            .sum()
    };
    let 买之前 = 手上有多少件(&game_world, player);

    // 造一个手里有货的卖家：**只有卖家的东西是塞进去的**，玩家那一侧
    // 一个字节都没动。
    let 卖家 = {
        let mut agent = game_world.world.actors.get(player).expect("玩家在").clone();
        agent.affiliations = Vec::new();
        agent.wallet = 0;
        agent.inventory = vec![ItemStack {
            owner: Owner::Unowned,
            ..ItemStack::new(索引(&content, 货), 3)
        }];
        game_world.world.actors.spawn(agent)
    };

    // Act：走生产结算路径（Intent → resolve → Effect → apply）。
    提交(
        &mut game_world,
        &content,
        Intent::Trade {
            actor: player,
            partner: 卖家,
            item: 索引(&content, 货),
            direction: TradeDirection::Buy,
        },
    );

    // Assert
    assert_eq!(
        手上有多少件(&game_world, player),
        买之前 + 1,
        "玩家手上那件货应当多出一件——钱不够时 resolve 会产出零效果         （开局钱包 {玩家原有的钱}，这件货 {货的价钱}）"
    );
    assert_eq!(
        game_world.world.actors.get(player).expect("玩家在").wallet,
        玩家原有的钱 - 货的价钱,
        "钱要真的付出去"
    );
    assert_eq!(
        game_world.world.actors.get(卖家).expect("卖家在").wallet,
        货的价钱,
        "钱要真的到卖家手上（货币守恒）"
    );
}

#[test]
fn 老存档里的钱包按存档里的值读回来() {
    // 钱包是**存档里的数据**，不是读档时重算的派生量（`Agent::wallet`
    // 字段文档：「厚层直接存值，不像薄层那样走公式」）。因此改了初值
    // **不写迁移**——老存档里的玩家仍然是他存盘时的数额。
    //
    // 「老存档」在这里由「一份 `wallet` 被改成 0 的存档」代表：它与本批
    // 之前写出的存档在**这个字段上**逐字节相同，而那正是本条要验的字段。
    //
    // **本条验的是读档路径，不是版本兼容性**，两件事要分清：
    // 真正的旧版本存档（schema 7 / 内容哈希算法 34）今天**打不开**——
    // 本批把内容哈希算法版本升到了 35，而 `check_content_hash_algorithm`
    // 对版本不等一律拒绝（见 `ll_content::save_file::CURRENT_SCHEMA_VERSION`
    // 的「7 -> 8」一节）。那是每一次内容改动都有的既有代价，与钱包无关。
    // 本条要排除的是另一件事：**读档时用新的初值把存档里的值盖掉**——
    // 那才是「钱包变成了读档重算的派生量」这个缺陷的样子。
    // Arrange
    let content = test_content();
    let mut game_world = 新开一局(&content);
    game_world
        .world
        .actors
        .get_mut(game_world.player)
        .expect("玩家在")
        .wallet = 0;
    let dir = std::env::temp_dir().join(format!("ll-game-player-wallet-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("创建测试目录应当成功");
    let path = dir.join("old.sav");
    save_game(
        &path,
        &content,
        &game_world,
        "测试角色",
        "测试区域",
        "老存档",
    )
    .expect("存档应当写成功");

    // Act
    let loaded = load_game(&path, &content);

    // Assert
    let LoadedGame::Playable { world, .. } = loaded else {
        panic!("刚写出来的存档必须能读回来");
    };
    let 读回来的钱 = world
        .actors
        .get(game_world.player)
        .expect("玩家在存档里")
        .wallet;
    assert_eq!(
        读回来的钱, 0,
        "老存档里的钱包必须原样读回，不能被新的初值覆盖——若这里读到 \
         {PLAYER_STARTING_WALLET}，说明有人把钱包变成了读档重算的派生量"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 把一条意图走完整条 `Intent → resolve → Effect → apply` 管线。
/// 写法与 `trade_screen.rs` 的同名帮手一致；集成测试之间看不见彼此的
/// 私有帮手，因此这几行在这里重来一遍。
fn 提交(game_world: &mut GameWorld, content: &LoadedContent, intent: Intent) {
    let player = game_world.player;
    let clock = game_world.world.clock;
    let mut timeline = Timeline::new();
    timeline.schedule(player, clock);
    let mut engine = TurnEngine::new(timeline);
    let runtime = RuntimeCatalogs::new(content);
    let catalogs = runtime.as_resolve_catalogs();
    let mut on_effect = |_world: &WorldState, _effect: &ll_sim::effect::Effect| {};
    let mut ai =
        |_world: &WorldState, actor: EntityId, _controlled: EntityId| Intent::Wait { actor };
    engine.advance_ai(
        &mut game_world.world,
        player,
        &mut ai,
        &catalogs,
        &mut on_effect,
    );
    engine.try_player_intent(
        &mut game_world.world,
        player,
        intent,
        &catalogs,
        &mut on_effect,
    );
}
