//! 内容值哈希探针：跑一遍生产装载路径（`ll_game::content::load_content`），
//! 把每个命名空间的 `Registry::content_hash_of` 原样打出来。
//!
//! **这是测量工具，不是产品代码**——与 `ll-script` 下的 `probe_*.rs`
//! 同一个定位。存在的理由是：任何改动装载方式（而不是改动内容本身）
//! 的批次，都必须证明内容值哈希**逐位不变**，而在此之前没有一个不用
//! 起游戏就能拿到这串数的办法。
//!
//! ```text
//! cargo run --example probe_content_hash -p ll-game
//! cargo run --example probe_content_hash -p ll-game -- <另一个 mods 目录>
//! ```
//!
//! 第二种写法用来做**基线对比**：把改动前的 `mods/` 取出来（例如
//! `git archive <改动前的提交> mods | tar -x -C <临时目录>`），用同一个
//! 二进制跑两次，就能回答「这次改的是装载方式还是内容本身」——同一个
//! 二进制意味着两次的哈希算法逐位相同，差异只可能来自 mods 目录。
//! 不加参数时用的是仓库自己的 `mods/`，与此前完全一致。

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mods_root = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("mods"));
    let assets_root = root.join("assets");

    let loaded = match ll_game::content::load_content(&mods_root, &assets_root) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("装载失败：{err}");
            std::process::exit(1);
        }
    };

    // 命名空间取自装载报告，按字典序输出——报告本身的顺序来自拓扑
    // 排序，这里再排一次是为了让两次运行的输出可以直接 diff，与
    // 约束 C5 同一条精神（不让容器顺序参与判断）。
    let mut namespaces: Vec<String> = loaded
        .report
        .entries
        .iter()
        .map(|(id, _)| id.namespace().to_string())
        .collect();
    namespaces.sort();
    namespaces.dedup();

    for namespace in &namespaces {
        match loaded.registry.content_hash_of(namespace) {
            Some(hash) => println!("{namespace} = {hash}"),
            None => println!("{namespace} = <无内容>"),
        }
    }
}
