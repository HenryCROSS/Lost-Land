//! Mod 状态存储：值类型与批量写入描述。
//!
//! 存储位置本身（挂在 [`crate::entity::Agent::mod_state`]）与命名空间
//! 隔离（键即 `(mod_namespace, key)`）不在本模块——本模块只交付「一条
//! 状态值长什么样」「一次写入的批量描述」这两件事，供 `ll-sim` 使用
//! （[`ModStateWrite`] 经 `Effect::SetModState` 写入，见其文档）。
//!
//! # 谁在用
//!
//! 这张表是 mod 可命名空间隔离地挂在单个实体上的通用键值存储。当前
//! 有两个内建使用者，都经由 `ll_sim::effect::Effect::SetModState` 这
//! 条唯一写入口落地：
//!
//! - **任务进度**（`ll_sim::quest`）：击杀计数与任务完成标记。
//! - **副职解锁**（`ll_sim::subclass`）：分类制作计数。
//!
//! 历史注记：本模块曾服务于已移除的 Steel 脚本系统（因此旧名叫
//! `mod_state`），当时还带一份全局存储与一套按 mod 计算的字节配额。
//! 脚本系统移除后，全局存储再无任何写入方、配额也再无判定点，两者
//! 已一并删除；留下的这张每实体表并非脚本残留，而是上面两个系统的
//! 真实存储后端。
//!
//! # 为什么这两样东西放在 `ll-world` 而不是 `ll-sim`
//!
//! [`ModStateValue`] 是 [`crate::entity::Agent`] 的字段类型——存档格式
//! 的一部分，必须与它同一个 crate（依赖方向 `ll-world` ← `ll-sim`，
//! `ll-world` 不能反过来依赖下游 crate）。[`ModStateWrite`] 出现在
//! `ll_sim::effect::Effect` 的变体里，放在 `ll-world` 让上下游引用同一
//! 份定义。

use std::collections::BTreeMap;

use crate::entity::EntityId;

/// Mod 能读写的存储值。**明确排除浮点**——`ll_world::state` 模块文档
/// 「全程禁止浮点数」这条底线不因 mod 状态而松动，分数走 `Milli`
/// （`i64` 底层，调用方自行处理整数/小数部分）。
///
/// `List`/`Map` 递归携带 [`ModStateValue`] 本身——嵌套深度不做限制。
/// `Map` 用 `BTreeMap`：约束 C5 禁止 `HashMap`/`HashSet` 的迭代顺序
/// 参与逻辑判断，`BTreeMap` 给出确定的字典序遍历。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ModStateValue {
    /// 整数——分数用 `Milli` 承载，`Milli.0` 直接装进这个变体。
    Int(i64),
    /// 布尔。
    Bool(bool),
    /// 任意字符串。
    Str(Box<str>),
    /// 内容引用：`NamespacedId` 的字符串形式（`namespace:path`），读取
    /// 时经当前会话的 `ContentIndex` 重新解析——存的是字符串本身，不是
    /// 索引（`ContentIndex` 依赖 mod 加载顺序，不可持久化，见
    /// `ll_core::ident` 模块文档）。
    Ref(Box<str>),
    /// 厚层实体引用。只限 `Arena<Agent>`——薄层不支持个体寻址。读取时
    /// 若目标已死亡（世代号不符），由调用方按「读取型查询返回哨兵值」
    /// 的既有约定处理，本类型本身不做失效检测——那是 `Arena::get` 的
    /// 职责。
    Entity(EntityId),
    /// 列表，元素同构或异构均可。
    List(Vec<ModStateValue>),
    /// 键值表，键必须是字符串。
    Map(BTreeMap<Box<str>, ModStateValue>),
}

/// 一条待写入的 mod 状态记录：目标实体、命名空间、键、值。
///
/// # 为什么是「一批」而不是「一条」
///
/// 写入不得绕过 `apply` 直接改 `WorldState`（约束 C1）——mod 状态存在
/// `WorldState` 里，那就是世界状态的一部分，写入必须经唯一写入口。但
/// 每次写入都发一条独立 `Effect` 会为每次写入多付一条 `Effect` 的
/// 开销；因此**一次决策期间的多次写入收集成一批，作为一条
/// `Effect::SetModState` 携带的 `Vec<ModStateWrite>` 一次性发出**
/// （见 `ll_sim::quest::kill_progress_effects` 的批量打包）。
///
/// 本类型不要求可序列化（与 `ll_sim::effect::Effect` 本身同一个理由：
/// 它是决策到 `apply` 之间同一进程内的瞬时产物，不需要跨进程留存
/// ——真正长期保留用于重放的是产生它的 `Intent` 序列）。
#[derive(Debug, Clone, PartialEq)]
pub struct ModStateWrite {
    /// 写入目标实体。只限厚层 `Arena<Agent>`；实体在 `apply` 时已不
    /// 存在则静默跳过，见 `ll_sim::apply::apply` 的既有纪律。
    pub entity: EntityId,
    /// 发起写入的 mod 命名空间——由发起方固化，写入内容无法覆盖它，
    /// 因此这里存的命名空间恒等于发起写入的 mod 自己。这是「A mod 写
    /// 不到 B mod 的键上」这条隔离承诺的载体。
    pub mod_namespace: String,
    /// 键。
    pub key: String,
    /// 值。
    pub value: ModStateValue,
}

/// `BTreeMap<(String, String), ModStateValue>` 的 serde 表示。
///
/// JSON 等基于文本的格式要求 map 的键本身序列化成字符串——元组键
/// `(String, String)` 不满足这个要求（实测 `serde_json` 直接报错
/// "key must be a string"，见本模块测试）。这里手写序列化成
/// `Vec<((String, String), ModStateValue)>`（一张有序的条目列表，任何
/// 格式都能表达），反序列化时重新收进 `BTreeMap`——与 `ll-world` 既有
/// 的 `ChunkGrid`（`crate::state` 模块）手写 serde 实现同一个理由：
/// 字段本身的内存表示（`BTreeMap`，需要确定性遍历，约束 C5）与它的
/// 序列化表示不必绑在一起。[`crate::entity::Agent::mod_state`] 用
/// `#[serde(with = "crate::mod_state::serde_map")]` 接到这里。
pub mod serde_map {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::ModStateValue;

    /// 把 map 序列化成有序条目列表。
    pub fn serialize<S>(
        map: &BTreeMap<(String, String), ModStateValue>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let entries: Vec<(&(String, String), &ModStateValue)> = map.iter().collect();
        entries.serialize(serializer)
    }

    /// 从有序条目列表重建 map。
    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<(String, String), ModStateValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries: Vec<((String, String), ModStateValue)> = Vec::deserialize(deserializer)?;
        Ok(entries.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本模块 `serde_map` 存在的全部理由：元组键直接交给 `serde_json`
    /// 会失败。这条测试把「所以才需要手写 serde」钉成实测结论，而不是
    /// 模块文档里的一句断言——若哪天 serde 放宽了这条限制，这条测试会
    /// 红，提醒重新评估 `serde_map` 是否还有存在必要。
    #[test]
    fn 元组键的裸map无法序列化成json() {
        // Arrange
        let mut map: BTreeMap<(String, String), ModStateValue> = BTreeMap::new();
        map.insert(
            ("lostland".to_string(), "kills".to_string()),
            ModStateValue::Int(3),
        );

        // Act
        let result = serde_json::to_string(&map);

        // Assert
        assert!(result.is_err(), "元组键应当被 serde_json 拒绝");
    }

    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Holder {
        #[serde(with = "serde_map")]
        state: BTreeMap<(String, String), ModStateValue>,
    }

    #[test]
    fn serde_map让元组键的map能走json往返() {
        // Arrange
        let mut state: BTreeMap<(String, String), ModStateValue> = BTreeMap::new();
        state.insert(
            ("lostland".to_string(), "kills".to_string()),
            ModStateValue::Int(3),
        );
        state.insert(
            ("yourmod".to_string(), "flag".to_string()),
            ModStateValue::Bool(true),
        );
        let holder = Holder { state };

        // Act
        let json = serde_json::to_string(&holder).expect("serde_map 应当能编码元组键");
        let restored: Holder = serde_json::from_str(&json).expect("应当能解码回来");

        // Assert
        assert_eq!(restored, holder);
    }

    #[test]
    fn serde_map往返保留全部值变体() {
        // Arrange：逐个覆盖七个变体，确保没有哪个变体在往返中丢失。
        let mut state: BTreeMap<(String, String), ModStateValue> = BTreeMap::new();
        let values = [
            ModStateValue::Int(-7),
            ModStateValue::Bool(false),
            ModStateValue::Str("名字".into()),
            ModStateValue::Ref("lostland:iron_sword".into()),
            ModStateValue::Entity(EntityId::new(0, 0)),
            ModStateValue::List(vec![ModStateValue::Int(1), ModStateValue::Bool(true)]),
            ModStateValue::Map(BTreeMap::from([("k".into(), ModStateValue::Int(2))])),
        ];
        for (index, value) in values.iter().enumerate() {
            state.insert(("lostland".to_string(), format!("k{index}")), value.clone());
        }
        let holder = Holder { state };

        // Act
        let json = serde_json::to_string(&holder).expect("应当能编码");
        let restored: Holder = serde_json::from_str(&json).expect("应当能解码");

        // Assert
        assert_eq!(restored, holder);
    }
}
