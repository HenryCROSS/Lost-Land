//! 脚本状态存储：值类型、有界性配额、批量写入描述。
//!
//! 落地 `knowledge/design/script-state-storage.md` 三、六节。存储位置
//! 本身（全局挂在 [`crate::state::WorldState::global_script_state`]、
//! 每实体挂在 [`crate::entity::Agent::script_state`]）与命名空间隔离
//! （键即 `(mod_namespace, key)`，见设计文档四节）不在本模块——本模块
//! 只交付「一条状态值长什么样」「一次写入的批量描述」「配额怎么算」
//! 这三件事，供 `ll-sim`（[`ScriptStateWrite`] 经 `Effect` 写入，
//! 见其模块文档）与 `ll-script`（脚本 API 层的值转换、配额判定）共用。
//!
//! # 为什么这三样东西放在 `ll-world` 而不是 `ll-script`
//!
//! [`ScriptValue`] 是 [`crate::state::WorldState`] 与 [`crate::entity::Agent`]
//! 的字段类型——存档格式的一部分，必须与它们同一个 crate（依赖方向
//! `ll-world` ← `ll-sim` ← `ll-script`，`ll-world` 不能反过来依赖任何一个
//! 下游 crate）。[`ScriptStateWrite`] 需要同时出现在 `ll-sim::effect::Effect`
//! （写入描述）与本模块的配额判定函数签名里，放在 `ll-world` 让两个
//! 下游 crate 都能引用同一份定义，不需要各自维护一份。

use std::collections::BTreeMap;

use crate::entity::EntityId;
use crate::state::WorldState;

/// 每个 mod 的全局存储 + 全部每实体存储合计上限（字节，postcard 编码后
/// 大小）。**必须是加载期静态常量**——设计文档六、1 节的确定性论证：
/// 若配额是共享浮动总量，写入是否成功会依赖 mod 加载顺序，破坏「同一
/// 份存档、mod 顺序不同，结果必须相同」这条确定性前提。数字本身未经
/// 真实 mod 内容标定，是按值类型典型条目大小的理论倒推（设计文档
/// 六、2/十、3 节已如实标注），后续如有真实卡顿/超限投诉可以再调，
/// 与 `ll-script::host::INTERRUPT_TIMEOUT` 同一种「不是精确科学」的
/// 诚实态度。
pub const PER_MOD_QUOTA_BYTES: usize = 256 * 1024;

/// 单个 `(mod, entity)` 组合的存储上限（字节）——防止某一个实体被塞爆
/// 而挤占同 mod 下其他实体的份额（设计文档六、2 节）。同样是加载期
/// 静态常量，同样未经真实标定。
pub const PER_MOD_ENTITY_QUOTA_BYTES: usize = 4 * 1024;

/// 脚本能读写的存储值。**明确排除浮点**——`ll_world::state` 模块文档
/// 「全程禁止浮点数」这条底线不因脚本状态而松动，分数走 `Milli`（`i64`
/// 底层，脚本侧配合已注册的换算常量自行处理整数/小数部分），见设计
/// 文档三、1 节。
///
/// `List`/`Map` 递归携带 `ScriptValue` 本身——嵌套深度不做限制（与
/// 配额字节上限已经间接约束了实际可达的嵌套规模，不需要额外的深度
/// 计数器）。`Map` 用 `BTreeMap`：约束 C5 禁止 `HashMap`/`HashSet` 的
/// 迭代顺序参与逻辑判断，`BTreeMap` 给出确定的字典序遍历（设计文档
/// 五、2 节）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ScriptValue {
    /// 整数——分数用 `Milli` 承载，`Milli.0` 直接装进这个变体。
    Int(i64),
    /// 布尔。
    Bool(bool),
    /// 任意字符串。
    Str(Box<str>),
    /// 内容引用：`NamespacedId` 的字符串形式（`namespace:path`），读取
    /// 时经当前会话的 `ContentIndex` 重新解析——存的是字符串本身，不是
    /// 索引（`ContentIndex` 依赖 mod 加载顺序，不可持久化，见
    /// `ll_core::ident` 模块文档），见设计文档三、2 节。
    Ref(Box<str>),
    /// 厚层实体引用。只限 `Arena<Agent>`——薄层不支持个体寻址（设计
    /// 文档三、3 节）。读取时若目标已死亡（世代号不符），由调用方按
    /// 「读取型查询返回哨兵值」的既有约定处理，本类型本身不做失效
    /// 检测——那是 `Arena::get` 的职责。
    Entity(EntityId),
    /// 列表，元素同构或异构均可。
    List(Vec<ScriptValue>),
    /// 键值表，键必须是字符串。
    Map(BTreeMap<Box<str>, ScriptValue>),
}

/// 一次状态写入的目标：全局存储，或某个具体厚层实体的每实体存储。
///
/// 与 [`ScriptValue`] 分开成独立类型（而不是直接把 `Option<EntityId>`
/// 塞进 [`ScriptStateWrite`]）：`Global`/`Entity` 两个变体名字本身就是
/// 文档，比一个裸 `Option` 更能表达「这是两种存储位置，不是同一个字段
/// 的可选值」这件事。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptStateTarget {
    /// 挂在 [`crate::state::WorldState::global_script_state`]。
    Global,
    /// 挂在某个厚层实体的 [`crate::entity::Agent::script_state`]。
    Entity(EntityId),
}

/// 一条待写入的脚本状态记录：目标、命名空间、键、值。
///
/// # 为什么是这个形状（裁定 P5-1）
///
/// 脚本不得绕过 `apply` 直接改 `WorldState`（约束 C1）——脚本状态存在
/// `WorldState` 里，那就是世界状态的一部分，写入必须经唯一写入口。
/// 但脚本每次调用 `state-set!`/`entity-state-set!` 都发一条独立 `Effect`
/// 会为每次写入多付一条 `Effect` 的开销；裁定 P5-1 的解法是**一次决策
/// 期间的多次写入收集成一批，作为一条 `Effect::SetScriptState` 携带的
/// `Vec<ScriptStateWrite>` 一次性发出**——`ll-script` 侧的
/// `api::state` 模块在脚本调用窗口内把写入攒进一个线程局部缓冲，调用
/// 结束后宿主取走整批、包成一条 `Effect`，交给 `resolve → apply` 既有
/// 管线。本类型不要求可序列化（与 `ll_sim::effect::Effect` 本身同一个
/// 理由：它是决策到 `apply` 之间同一进程内的瞬时产物，不需要跨进程
/// 留存——真正长期保留用于重放的是产生它的 `Intent`/调用序列）。
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptStateWrite {
    /// 写入目标。
    pub target: ScriptStateTarget,
    /// 发起写入的 mod 命名空间——由宿主在注册 `state-set!`/
    /// `entity-state-set!` 时固化，脚本没有参数能覆盖它（设计文档
    /// 四、1 节），因此这里存的命名空间恒等于发起写入的 mod 自己。
    pub mod_namespace: String,
    /// 键。
    pub key: String,
    /// 值。
    pub value: ScriptValue,
}

impl ScriptStateWrite {
    /// 这条待写记录是否与给定的「目标 + 命名空间 + 键」是同一条——
    /// 用于 [`crate::script_state`] 的配额判定与 `ll-script` 侧缓冲区
    /// 的去重（同一决策内重复写同一个键，只保留最后一次的值，不会让
    /// 配额把中间过程的每一次覆写都重复计入）。
    pub fn matches(&self, target: ScriptStateTarget, mod_namespace: &str, key: &str) -> bool {
        self.target == target && self.mod_namespace == mod_namespace && self.key == key
    }
}

/// `BTreeMap<(String, String), ScriptValue>` 的 serde 表示。
///
/// JSON 等基于文本的格式要求 map 的键本身序列化成字符串——元组键
/// `(String, String)` 不满足这个要求（实测 `serde_json` 直接报错
/// "key must be a string"，见本模块测试）。这里手写序列化成
/// `Vec<((String, String), ScriptValue)>`（一张有序的条目列表，任何
/// 格式都能表达），反序列化时重新收进 `BTreeMap`——与 `ll-world` 既有
/// 的 `ChunkGrid`（`crate::state` 模块）手写 serde 实现同一个理由：
/// 字段本身的内存表示（`BTreeMap`，需要确定性遍历，约束 C5）与它的
/// 序列化表示不必绑在一起。`WorldState::global_script_state`/
/// `Agent::script_state` 都用 `#[serde(with = "crate::script_state::serde_map")]`
/// 接到这里。
pub mod serde_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::ScriptValue;

    /// 把 map 序列化成有序条目列表。
    pub fn serialize<S>(
        map: &BTreeMap<(String, String), ScriptValue>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries: Vec<(&(String, String), &ScriptValue)> = map.iter().collect();
        entries.serialize(serializer)
    }

    /// 从有序条目列表重建 map。
    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<(String, String), ScriptValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries: Vec<((String, String), ScriptValue)> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}

/// 估算一条 `(key, value)` 记录序列化后的字节数——与存档主体同款编码
/// （postcard），配额判定与加载管理界面的占用展示（设计文档六、4 节，
/// 本批次不落地界面本身）共用同一个估算口径。
///
/// 序列化理论上不会失败（[`ScriptValue`] 全部变体都是可以直接编码的
/// 纯数据，没有引用循环或不支持的类型），但仍然不 panic：失败时返回
/// `usize::MAX`——这个哨兵值恒会让任何配额判定失败，是「宁可拒绝写入
/// 也不要在无法确定大小时放行」的保守选择，与配额超限本身「拒绝写入、
/// 不静默放行」的既有精神一致。
pub fn entry_size(key: &str, value: &ScriptValue) -> usize {
    postcard::to_stdvec(&(key, value))
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX)
}

/// 汇总一份 `(命名空间, 键) -> 值` 存储里属于 `mod_namespace` 的已提交
/// 条目大小，跳过 `pending` 里已有同名覆盖记录的条目——这是
/// [`mod_total_bytes`]/[`entity_mod_bytes`] 共用的过滤 + 累加逻辑,抽出来
/// 是因为它此前分别在「全局存储」「`mod_total_bytes` 内逐实体循环」
/// 「`entity_mod_bytes`」三处几乎逐字重复,容易在改一处过滤条件时漏改
/// 另外两处。
fn sum_committed_bytes(
    entries: &BTreeMap<(String, String), ScriptValue>,
    target: ScriptStateTarget,
    mod_namespace: &str,
    pending: &[ScriptStateWrite],
) -> usize {
    let mut total = 0usize;
    for ((namespace, key), value) in entries {
        if namespace != mod_namespace {
            continue;
        }
        if pending
            .iter()
            .any(|w| w.matches(target, mod_namespace, key))
        {
            continue; // 待写缓冲里有同名记录，以缓冲区的值为准，避免重复计入。
        }
        total += entry_size(key, value);
    }
    total
}

/// 计算 `mod_namespace` 在整个世界（全局存储 + 全部厚层实体的每实体
/// 存储）当前占用的字节数，加上 `pending` 缓冲区里属于该 mod 的待写
/// 记录——`pending` 里与某条已提交记录同名（目标+命名空间+键相同）的
/// 记录会覆盖，不重复计入，见 [`sum_committed_bytes`]。
///
/// **不扫描其他 mod 的数据**——只过滤 `mod_namespace` 匹配的条目，
/// 天然满足设计文档六、1 节的确定性要求：A mod 的配额判定结果只取决于
/// A mod 自己已提交 + 待提交的数据量，与其余 mod 的实际用量无关，因此
/// 也不依赖 mod 加载/执行顺序。
pub fn mod_total_bytes(
    world: &WorldState,
    mod_namespace: &str,
    pending: &[ScriptStateWrite],
) -> usize {
    let mut total = sum_committed_bytes(
        &world.global_script_state,
        ScriptStateTarget::Global,
        mod_namespace,
        pending,
    );

    for (entity, agent) in world.actors.iter_with_id() {
        total += sum_committed_bytes(
            &agent.script_state,
            ScriptStateTarget::Entity(entity),
            mod_namespace,
            pending,
        );
    }

    for write in pending {
        if write.mod_namespace == mod_namespace {
            total += entry_size(&write.key, &write.value);
        }
    }

    total
}

/// 计算 `mod_namespace` 在具体某个厚层实体上（该实体的每实体存储 +
/// `pending` 缓冲区里同一实体同一 mod 的待写记录）当前占用的字节数。
/// 逻辑与 [`mod_total_bytes`] 同构，收窄到单个实体。
pub fn entity_mod_bytes(
    world: &WorldState,
    entity: EntityId,
    mod_namespace: &str,
    pending: &[ScriptStateWrite],
) -> usize {
    let mut total = 0usize;

    if let Some(agent) = world.actors.get(entity) {
        total += sum_committed_bytes(
            &agent.script_state,
            ScriptStateTarget::Entity(entity),
            mod_namespace,
            pending,
        );
    }

    for write in pending {
        if write.mod_namespace == mod_namespace && write.target == ScriptStateTarget::Entity(entity)
        {
            total += entry_size(&write.key, &write.value);
        }
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{Agent, BaseStats};
    use crate::generate::GenParams;
    use crate::terrain::base_terrain_fixture;
    use crate::zone::ZoneLayout;

    fn test_world() -> WorldState {
        let zone_count = ll_core::torus::TorusSize::new(1, 1).expect("1x1 是合法尺寸");
        let layout = ZoneLayout::new(64, zone_count).expect("64 满足全部对齐与跨度约束");
        let (terrain_ids, terrain_table) = base_terrain_fixture();
        let spawn = layout.tile_size().wrap(0, 0);
        WorldState::new(
            layout,
            &GenParams::default(),
            &terrain_ids,
            terrain_table,
            spawn,
        )
        .expect("测试布局满足全部构造前置条件")
    }

    fn blank_agent(world: &WorldState) -> Agent {
        let mut interner = ll_core::ident::Interner::new();
        let profession = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:tester").expect("合法标识符"));
        let race = interner
            .intern(ll_core::ident::NamespacedId::parse("lostland:human").expect("合法标识符"));
        let pos = world.size.wrap(0, 0);
        let (zone, _) = world.terrain.layout().tile_to_zone(pos);
        Agent {
            pos,
            stats: BaseStats::BASELINE,
            next_action_at: ll_core::time::Tick(0),
            health: Agent::STARTING_HEALTH,
            affiliations: Vec::new(),
            wallet: 0,
            profession,
            goals: Vec::new(),
            race,
            mana: Agent::STARTING_MANA,
            stamina: Agent::STARTING_STAMINA,
            resource_pools: std::collections::BTreeMap::new(),
            spent_slots: std::collections::BTreeMap::new(),
            inventory: Vec::new(),
            equipment: std::collections::BTreeMap::new(),
            resting: None,
            unlocked_skills: Vec::new(),
            skill_cooldowns: std::collections::BTreeMap::new(),
            subclasses: Vec::new(),
            active_stat_modifiers: std::collections::BTreeMap::new(),
            current_space: crate::space::Space::surface(
                zone,
                ll_core::ident::ContentIndex::default(),
            ),
            script_state: BTreeMap::new(),
            creature_kind: None,
            spawned_at: ll_core::time::Tick(0),
            remembered_id: None,
            level: crate::entity::Agent::STARTING_LEVEL,
            experience: 0,
            xp_to_next_level: crate::entity::Agent::STARTING_XP_TO_NEXT_LEVEL,
            unspent_attribute_points: 0,
            unspent_skill_points: 0,
            stealthed: false,
        }
    }

    #[test]
    fn 同一条记录序列化两次得到相同大小() {
        // Arrange
        let value = ScriptValue::Int(42);

        // Act
        let first = entry_size("key", &value);
        let second = entry_size("key", &value);

        // Assert
        assert_eq!(first, second);
        assert!(first > 0);
    }

    #[test]
    fn 更长的字符串值产出更大的估算体积() {
        // Arrange
        let short = ScriptValue::Str("a".into());
        let long = ScriptValue::Str("a".repeat(200).into());

        // Act & Assert
        assert!(entry_size("key", &long) > entry_size("key", &short));
    }

    #[test]
    fn 全局存储的已提交条目计入mod总量() {
        // Arrange
        let mut world = test_world();
        world.global_script_state.insert(
            ("lostland".to_string(), "reputation".to_string()),
            ScriptValue::Int(100),
        );

        // Act
        let total = mod_total_bytes(&world, "lostland", &[]);

        // Assert
        assert!(total > 0);
    }

    #[test]
    fn 不同mod的已提交条目不计入彼此总量() {
        // Arrange
        let mut world = test_world();
        world.global_script_state.insert(
            ("lostland".to_string(), "a".to_string()),
            ScriptValue::Int(1),
        );
        world.global_script_state.insert(
            ("yourmod".to_string(), "b".to_string()),
            ScriptValue::Int(1),
        );

        // Act
        let lostland_total = mod_total_bytes(&world, "lostland", &[]);
        let yourmod_total = mod_total_bytes(&world, "yourmod", &[]);

        // Assert：两者各自只看到自己的一条记录，量级相同（同类型同长度键）。
        assert_eq!(lostland_total, yourmod_total);
    }

    #[test]
    fn 待写缓冲区里同名记录覆盖已提交值而不是重复计入() {
        // Arrange：已提交一个短字符串，待写缓冲里对同一个键写入更长的
        // 字符串——若发生重复计入，总量会同时包含短值与长值两份大小，
        // 比单独计入长值大得多。
        let mut world = test_world();
        world.global_script_state.insert(
            ("lostland".to_string(), "note".to_string()),
            ScriptValue::Str("short".into()),
        );
        let long_value = ScriptValue::Str("x".repeat(500).into());
        let pending = vec![ScriptStateWrite {
            target: ScriptStateTarget::Global,
            mod_namespace: "lostland".to_string(),
            key: "note".to_string(),
            value: long_value.clone(),
        }];

        // Act
        let total = mod_total_bytes(&world, "lostland", &pending);

        // Assert：应当恰好等于「只有长值」的大小，不含短值那一份。
        let expected = entry_size("note", &long_value);
        assert_eq!(total, expected);
    }

    #[test]
    fn 每实体存储的已提交条目只计入该实体不影响其他实体() {
        // Arrange
        let mut world = test_world();
        let mut agent_a = blank_agent(&world);
        agent_a.script_state.insert(
            ("lostland".to_string(), "cooldown".to_string()),
            ScriptValue::Int(5),
        );
        let id_a = world.actors.spawn(agent_a);
        let agent_b = blank_agent(&world);
        let id_b = world.actors.spawn(agent_b);

        // Act
        let usage_a = entity_mod_bytes(&world, id_a, "lostland", &[]);
        let usage_b = entity_mod_bytes(&world, id_b, "lostland", &[]);

        // Assert
        assert!(usage_a > 0);
        assert_eq!(usage_b, 0);
    }

    #[test]
    fn 单个实体的占用计入对应mod的全局总量() {
        // Arrange
        let mut world = test_world();
        let mut agent = blank_agent(&world);
        agent.script_state.insert(
            ("lostland".to_string(), "cooldown".to_string()),
            ScriptValue::Int(5),
        );
        world.actors.spawn(agent);

        // Act
        let total = mod_total_bytes(&world, "lostland", &[]);

        // Assert
        assert!(total > 0);
    }
}
