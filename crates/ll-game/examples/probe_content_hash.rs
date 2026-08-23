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
//! ```

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mods_root = root.join("mods");
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
