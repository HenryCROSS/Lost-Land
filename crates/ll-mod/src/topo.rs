//! 依赖拓扑排序：把发现到的 mod 清单排成一个满足依赖关系的加载顺序。
//!
//! # 确定性
//!
//! 规格 C5 禁止 `HashMap`/`HashSet` 迭代顺序参与逻辑判断，拓扑排序对
//! 这条约束尤其敏感——[`crate::discover::discover_mods`] 的返回顺序
//! 依赖文件系统遍历，不同操作系统/文件系统上可能不同。若排序算法在
//! 「多个 mod 同时满足条件、选哪个先走」时依赖输入数组的下标顺序，
//! 相当于让发现阶段的不确定性直接渗进排序结果。
//!
//! 本模块的应对：所有「多个候选、选一个」的时刻——入度为零时先处理
//! 哪个、缺失依赖同时存在多处时先报哪个、环路检测从哪个节点起步——
//! 一律按 mod 自身的命名空间字符串字典序决定，不看它在输入切片里的
//! 下标。这样即便把 `discover_mods` 的输出打乱后再解析、再排序，
//! 只要清单内容集合不变，`topo_sort` 的结果就恒定不变。
//!
//! # 重复命名空间：曾经的已知缺口，现已修正
//!
//! 旧版实现直接 `namespace_to_idx.insert(m.id.namespace(), i)`——两个
//! 已发现的清单若声明了同一个命名空间，后处理的那个会**静默覆盖**前
//! 一个在映射表里的下标，依赖解析、拓扑排序此后全部只认得到那个「后
//! 来者」，前一个 mod 的存在感在图里彻底消失，且不产生任何错误。这是
//! 最坏的一类失败：玩家看到的行为莫名其妙（比如两个 mod 都定义了
//! `yourmod:fireball` 但属性完全不同，游戏里只表现出其中一个），mod
//! 作者也毫无察觉自己被覆盖了。
//!
//! 现在 [`topo_sort`] 在建立命名空间映射**之前**先做一次重复检测（见
//! [`check_duplicate_namespaces`]）：按命名空间字典序扫描相邻项，一旦
//! 撞见两个清单共享同一个命名空间就立即返回
//! [`ModError::DuplicateNamespace`]，不再进入依赖解析/排序，也不再有
//! 「选哪一个当权威定义」这个决策——两份定义本身就是冲突，选哪个都是
//! 错的，唯一正确的处理是让整批加载停下来，把冲突显式报给加载管理界面
//! （任务 11）。检测顺序按命名空间字典序（不是 `manifests` 原始下标），
//! 与本模块其余「多个候选选一个」的场景遵循同一条确定性规则。

use crate::manifest::{ModError, ModManifest, mod_self_id};
use ll_core::ident::NamespacedId;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// 对 `manifests` 做依赖拓扑排序，返回一个下标排列——`result[k]` 是
/// 第 `k` 个应当被加载的 mod 在 `manifests` 中的下标。
///
/// 依赖用 [`ModManifest::dependencies`] 里的裸命名空间字符串表达，
/// 按名字匹配 `manifests` 里其他清单的 [`ModManifest::id`]（用
/// [`crate::manifest::mod_self_id`] 同一套约定还原成可比较的
/// [`NamespacedId`]）。
///
/// # 错误
///
/// - 某个依赖在 `manifests` 里找不到对应的 mod：
///   [`ModError::MissingDependency`]，附带缺失的那个依赖自身的标识符。
///   这正是 [0015](../../../knowledge/decisions/0015-content-id-registration-is-parsing-not-invariant.md)
///   「注册校验是解析」分工里那一步「解析失败」的落点——`manifest.rs`
///   只校验依赖名字符是否合法，「这个依赖是否真的存在」只有等所有
///   候选都发现完、集齐 `manifests` 之后才能回答。
/// - 依赖成环：[`ModError::CyclicDependency`]，附带环路上具体的 mod
///   （按环路顺序，不是"所有卡住的 mod"这种粗粒度报告）。
pub fn topo_sort(manifests: &[ModManifest]) -> Result<Vec<usize>, ModError> {
    let n = manifests.len();

    // 所有「多个候选选一个」的场景都从这份按命名空间字典序排好的下标
    // 出发，而不是 0..n 这种依赖输入数组原始顺序的写法。
    let mut sorted_by_namespace: Vec<usize> = (0..n).collect();
    sorted_by_namespace.sort_by_key(|&i| manifests[i].id.namespace());

    // 必须在建立 namespace_to_idx 之前做：重复命名空间会让下一步的
    // HashMap 插入静默覆盖，见模块文档「重复命名空间」一节。
    check_duplicate_namespaces(manifests, &sorted_by_namespace)?;

    // 命名空间 -> 下标。用于把依赖字符串解析回具体的 mod。这里的
    // HashMap 只用于 O(1) 单键查找，从不被遍历产出顺序——遍历一律走
    // 上面按命名空间排好序的 `sorted_by_namespace`。经过上一步的重复
    // 检测，此时每个命名空间在 `manifests` 里恰好出现一次，insert 不
    // 会再发生静默覆盖。
    let mut namespace_to_idx: HashMap<&str, usize> = HashMap::with_capacity(n);
    for (i, m) in manifests.iter().enumerate() {
        namespace_to_idx.insert(m.id.namespace(), i);
    }

    check_missing_dependencies(manifests, &namespace_to_idx, &sorted_by_namespace)?;

    let (indegree, dependents) = build_graph(manifests, &namespace_to_idx);
    let order = kahn_sort(manifests, &sorted_by_namespace, indegree, &dependents);

    if order.len() == n {
        return Ok(order);
    }

    Err(ModError::CyclicDependency(find_one_cycle(
        manifests,
        &namespace_to_idx,
        &sorted_by_namespace,
    )))
}

/// 检测是否有两个（或更多）清单声明了同一个命名空间。
///
/// `sorted_by_namespace` 已经按命名空间字典序排好，重复的命名空间必然
/// 相邻——扫描一遍相邻对即可，不需要额外的 `HashSet`。命中的是字典序
/// 下第一组冲突（多组冲突同时存在时，报告哪一组不依赖 `manifests` 的
/// 原始下标顺序，与本模块其余检测的确定性规则一致）。
fn check_duplicate_namespaces(
    manifests: &[ModManifest],
    sorted_by_namespace: &[usize],
) -> Result<(), ModError> {
    for pair in sorted_by_namespace.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if manifests[a].id.namespace() == manifests[b].id.namespace() {
            return Err(ModError::DuplicateNamespace(manifests[a].id.clone()));
        }
    }
    Ok(())
}

/// 按命名空间字典序扫描依赖，第一个找不到的直接报告。字典序扫描
/// （而不是 `manifests` 原始下标顺序）保证多个 mod 同时缺依赖时，
/// 报告的永远是同一个，不受 [`crate::discover::discover_mods`] 返回
/// 顺序的影响。
fn check_missing_dependencies(
    manifests: &[ModManifest],
    namespace_to_idx: &HashMap<&str, usize>,
    sorted_by_namespace: &[usize],
) -> Result<(), ModError> {
    for &i in sorted_by_namespace {
        for dep_ns in &manifests[i].dependencies {
            if !namespace_to_idx.contains_key(dep_ns.as_str()) {
                // 依赖名字符合法性已在 parse_manifest 里校验过，这里的
                // mod_self_id 不会失败；万一失败也不该 panic 整个加载
                // 流程，退化为一个不太可能撞见的占位错误文案。
                let missing = mod_self_id(dep_ns)
                    .unwrap_or_else(|_| mod_self_id("invalid").expect("固定字面量 invalid 恒合法"));
                return Err(ModError::MissingDependency(missing));
            }
        }
    }
    Ok(())
}

/// 构建入度数组与「依赖方」邻接表：`dependents[i]` 是依赖了第 `i` 个
/// mod 的所有 mod 下标——`i` 加载完成后，这些 mod 的入度各减一。
fn build_graph(
    manifests: &[ModManifest],
    namespace_to_idx: &HashMap<&str, usize>,
) -> (Vec<u32>, Vec<Vec<usize>>) {
    let n = manifests.len();
    let mut indegree = vec![0u32; n];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];

    for (j, m) in manifests.iter().enumerate() {
        for dep_ns in &m.dependencies {
            // check_missing_dependencies 已确保这里必然能查到。
            let i = namespace_to_idx[dep_ns.as_str()];
            dependents[i].push(j);
            indegree[j] += 1;
        }
    }

    (indegree, dependents)
}

/// Kahn 算法本体，用小顶堆而不是 FIFO 队列——堆按「命名空间字符串,
/// 下标」排序，保证同一时刻有多个入度为零的候选时，谁先出队完全由
/// 命名空间字典序决定，与它们各自何时入度归零的先后顺序、以及它们在
/// `manifests` 里的原始下标都无关。
fn kahn_sort(
    manifests: &[ModManifest],
    sorted_by_namespace: &[usize],
    mut indegree: Vec<u32>,
    dependents: &[Vec<usize>],
) -> Vec<usize> {
    let mut heap: BinaryHeap<Reverse<(&str, usize)>> = BinaryHeap::new();
    for &i in sorted_by_namespace {
        if indegree[i] == 0 {
            heap.push(Reverse((manifests[i].id.namespace(), i)));
        }
    }

    let mut order = Vec::with_capacity(manifests.len());
    while let Some(Reverse((_, i))) = heap.pop() {
        order.push(i);
        for &j in &dependents[i] {
            indegree[j] -= 1;
            if indegree[j] == 0 {
                heap.push(Reverse((manifests[j].id.namespace(), j)));
            }
        }
    }

    order
}

/// 在依赖图里找出一条具体的环路，返回环上各 mod 的标识符（按环路
/// 顺序）。调用前提：调用方已经确认图中确实存在环（[`kahn_sort`] 未能
/// 排出全部节点）。
///
/// 用白/灰/黑三色标记的 DFS：灰色节点是当前递归路径上尚未退栈的
/// 节点，再次访问到灰色节点就意味着找到了一条环——从路径里该节点
/// 首次出现的位置切到末尾就是环路本身。DFS 起点按命名空间字典序
/// （而不是 `manifests` 下标顺序）尝试，避免图中同时存在多个独立环时,
/// 报告哪一个环取决于输入切片的原始顺序。
fn find_one_cycle(
    manifests: &[ModManifest],
    namespace_to_idx: &HashMap<&str, usize>,
    sorted_by_namespace: &[usize],
) -> Vec<NamespacedId> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    fn visit(
        i: usize,
        manifests: &[ModManifest],
        namespace_to_idx: &HashMap<&str, usize>,
        color: &mut [Color],
        path: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        color[i] = Color::Gray;
        path.push(i);

        for dep_ns in &manifests[i].dependencies {
            let Some(&j) = namespace_to_idx.get(dep_ns.as_str()) else {
                continue;
            };
            match color[j] {
                Color::White => {
                    if let Some(cycle) = visit(j, manifests, namespace_to_idx, color, path) {
                        return Some(cycle);
                    }
                }
                Color::Gray => {
                    let start = path
                        .iter()
                        .position(|&x| x == j)
                        .expect("灰色节点必然仍在当前递归路径上");
                    return Some(path[start..].to_vec());
                }
                Color::Black => {}
            }
        }

        path.pop();
        color[i] = Color::Black;
        None
    }

    let n = manifests.len();
    let mut color = vec![Color::White; n];
    let mut path = Vec::new();

    for &i in sorted_by_namespace {
        if color[i] == Color::White
            && let Some(cycle_idx) = visit(i, manifests, namespace_to_idx, &mut color, &mut path)
        {
            return cycle_idx
                .into_iter()
                .map(|k| manifests[k].id.clone())
                .collect();
        }
    }

    // 调用方（topo_sort）只在 kahn_sort 未能排出全部节点时才调用本
    // 函数，此时图中必然存在环，这里到不了。留空向量而不是 panic——
    // 万一前提被违反，宁可返回一个空环路（加载管理界面会显示成一个
    // 奇怪但无害的空列表），也不让 mod 加载流程直接崩溃（规格 §10.4）。
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn manifest(namespace: &str, dependencies: &[&str]) -> ModManifest {
        ModManifest {
            id: mod_self_id(namespace).expect("测试用命名空间恒合法"),
            version: "0.1.0".to_string(),
            dependencies: dependencies.iter().map(|s| s.to_string()).collect(),
            entry_points: Vec::<PathBuf>::new(),
        }
    }

    /// 把 topo_sort 的下标结果换算成命名空间序列，测试断言更好读。
    fn namespaces_in_order<'a>(manifests: &'a [ModManifest], order: &[usize]) -> Vec<&'a str> {
        order.iter().map(|&i| manifests[i].id.namespace()).collect()
    }

    #[test]
    fn 无依赖的mod保持任意顺序都能排出() {
        // Arrange
        let manifests = vec![manifest("a", &[]), manifest("b", &[])];

        // Act
        let order = topo_sort(&manifests).expect("无依赖必然能排出");

        // Assert
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn 有依赖的mod排在被依赖者之后() {
        // Arrange：b 依赖 a，a 必须先于 b 加载。
        let manifests = vec![manifest("b", &["a"]), manifest("a", &[])];

        // Act
        let order = topo_sort(&manifests).expect("这是合法的依赖关系");
        let names = namespaces_in_order(&manifests, &order);

        // Assert
        let a_pos = names.iter().position(|&n| n == "a").unwrap();
        let b_pos = names.iter().position(|&n| n == "b").unwrap();
        assert!(a_pos < b_pos);
    }

    #[test]
    fn 依赖成环时拓扑排序报告具体环路() {
        // Arrange：a 依赖 b，b 依赖 a。
        let manifests = vec![manifest("a", &["b"]), manifest("b", &["a"])];

        // Act
        let result = topo_sort(&manifests);

        // Assert
        match result {
            Err(ModError::CyclicDependency(cycle)) => {
                let names: Vec<&str> = cycle.iter().map(|id| id.namespace()).collect();
                assert_eq!(names.len(), 2);
                assert!(names.contains(&"a") && names.contains(&"b"));
            }
            other => panic!("期望 CyclicDependency，实际是 {other:?}"),
        }
    }

    #[test]
    fn 依赖缺失时拓扑排序报告缺失的具体mod() {
        // Arrange：a 依赖一个从未被发现的 ghost。
        let manifests = vec![manifest("a", &["ghost"])];

        // Act
        let result = topo_sort(&manifests);

        // Assert
        assert_eq!(
            result,
            Err(ModError::MissingDependency(
                mod_self_id("ghost").expect("测试用命名空间恒合法")
            ))
        );
    }

    #[test]
    fn 无依赖关系的多个mod按命名空间字典序排序而非输入顺序() {
        // Arrange：输入顺序是 c, a, b——字典序应该是 a, b, c。
        let manifests = vec![manifest("c", &[]), manifest("a", &[]), manifest("b", &[])];

        // Act
        let order = topo_sort(&manifests).expect("无依赖必然能排出");
        let names = namespaces_in_order(&manifests, &order);

        // Assert
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn 打乱发现顺序后拓扑排序结果仍然一致() {
        // 这是本模块要防的核心风险：discover_mods 的返回顺序依赖文件
        // 系统遍历，不同操作系统上可能不同——排序结果不能被这份不确定
        // 性污染。
        // Arrange：同一组 mod（含依赖关系），分别按两种不同的输入顺序
        // 构造。
        let order_one = vec![
            manifest("c", &["a"]),
            manifest("a", &[]),
            manifest("b", &["a"]),
        ];
        let order_two = vec![
            manifest("b", &["a"]),
            manifest("c", &["a"]),
            manifest("a", &[]),
        ];

        // Act
        let sorted_one = topo_sort(&order_one).expect("这是合法的依赖关系");
        let sorted_two = topo_sort(&order_two).expect("这是合法的依赖关系");
        let names_one = namespaces_in_order(&order_one, &sorted_one);
        let names_two = namespaces_in_order(&order_two, &sorted_two);

        // Assert
        assert_eq!(names_one, names_two);
    }

    #[test]
    fn 两个清单共享同一命名空间时报告重复而不是静默覆盖() {
        // 这是简报要求正面处理的已知缺口：旧版实现会让后一个清单静默
        // 顶替前一个在 namespace_to_idx 里的下标，两个 mod 只有一个
        // 还能被依赖解析看见。现在必须报错，不能让任何一份定义悄悄
        // "赢"。
        // Arrange：两份声明了同一个命名空间 "dup" 的清单，内容不同
        // （版本号不同），模拟两个作者各自发布了同名 mod。
        let manifests = vec![
            ModManifest {
                id: mod_self_id("dup").expect("测试用命名空间恒合法"),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                entry_points: Vec::<PathBuf>::new(),
            },
            ModManifest {
                id: mod_self_id("dup").expect("测试用命名空间恒合法"),
                version: "2.0.0".to_string(),
                dependencies: Vec::new(),
                entry_points: Vec::<PathBuf>::new(),
            },
        ];

        // Act
        let result = topo_sort(&manifests);

        // Assert
        assert_eq!(
            result,
            Err(ModError::DuplicateNamespace(
                mod_self_id("dup").expect("测试用命名空间恒合法")
            ))
        );
    }

    #[test]
    fn 重复命名空间检测先于依赖解析生效() {
        // 即使重复的那个命名空间同时也被其他 mod 依赖，报告的仍然是
        // 重复本身，不是缺失依赖或别的下游错误——重复检测必须在建立
        // namespace_to_idx 之前就拦下，顺序错了会让这条断言失败。
        // Arrange
        let manifests = vec![
            manifest("dup", &[]),
            manifest("dup", &[]),
            manifest("c", &["dup"]),
        ];

        // Act
        let result = topo_sort(&manifests);

        // Assert
        assert!(matches!(result, Err(ModError::DuplicateNamespace(_))));
    }

    #[test]
    fn 空清单列表排出空顺序() {
        // Arrange
        let manifests: Vec<ModManifest> = Vec::new();

        // Act
        let order = topo_sort(&manifests).expect("空输入不应报错");

        // Assert
        assert!(order.is_empty());
    }
}
