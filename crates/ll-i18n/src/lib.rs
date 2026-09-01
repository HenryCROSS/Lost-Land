//! 本地化查表层：把 `_key` 字段（`display_name_key`/`title_key`/
//! `description_key`/`message_key`）解析成 Fluent `.ftl` 里的真实字串。
//!
//! # 为什么单独成 crate，不放进 `ll-text`
//!
//! `ll-text`（`crates/ll-text`）是排版/栅格化地基：cosmic-text 断行整形、
//! glyphon 上屏，依赖 `ll-render` 拿 GPU 设备。本 crate 做的是完全
//! 不同的另一件事——「给一个键、一个语言标签，查出应该显示哪段文字」
//! ——不摸任何 GPU 资源，也不关心文字最终怎么画到屏幕上。把两者混进
//! 同一个 crate，会让 `ll-text` 平白多出一条与渲染无关的 Fluent 解析
//! 依赖；反过来，任何只需要查表（比如存档校验错误信息、mod 清单校验）
//! 而不需要渲染的调用方，也会被迫拖进 cosmic-text/glyphon/wgpu 这整条
//! 渲染管线。拆开后 `ll-text` 管「怎么画」，本 crate 管「画什么」，
//! 各自可以独立被消费。
//!
//! # 依赖方向
//!
//! 本 crate 只依赖 `fluent`/`tracing`，不依赖任何本项目上游 crate——
//! 这是刻意的：本地化是纯表现层关注点（哪种语言、哪个键对应哪段文字），
//! `ll-world`/`ll-sim` 的确定性世界模拟不应该、也不需要知道任何字符串
//! 长什么样。谁消费本 crate 完全由上层决定（目前是 `ll-game`），本
//! crate 自身不知道也不关心调用方是谁。
//!
//! # 缺键与缺语言：返回键名本身，不 panic、不空白
//!
//! [`Catalog::resolve`]/[`Catalog::resolve_with_args`] 在键不存在、
//! 语言未装载、或 Fluent 格式化本身失败时，一律回退到把**原始键**当
//! 显示文本返回，并记一条 `tracing::warn!`。理由：
//!
//! - **不能静默**——缺键是内容制作过程中的真实缺陷（忘了写翻译、
//!   `.ftl` 文件手改坏了、mod 声明了键却没配套翻译），必须在日志里
//!   看得见，否则永远没人会去补。
//! - **不能 panic**——本地化资源是运行时可替换的用户/mod 数据（见
//!   `knowledge/design/mod-package-structure.md`「本地化文件」一节），
//!   与 `ll-platform` 配置文件系统（`ll_platform::config` 模块文档
//!   「损坏时的退化策略」一节）同一条纪律：外部数据损坏不该让游戏
//!   直接崩掉。
//! - **回退到键名而非空字符串**——键名虽然对玩家不友好（会看到形如
//!   `lostland:keybind.action.up` 的原始文本），但一眼就能看出「这里
//!   缺了翻译」，比空白或占位符更容易在测试/游玩中被发现并追溯到具体
//!   缺失的是哪一条。
//!
//! # 命名空间是查表的一部分，不是可以剥掉的装饰
//!
//! [`Catalog`] 按 **`(命名空间, 语言标签)`** 分桶（实现上是两级
//! `HashMap`），[`Catalog::resolve`] 用 key 的命名空间前缀决定去哪个桶
//! 查。此前的实现把前缀**整个剥掉**再查一张只按语言分桶的扁平表，那有
//! 两个后果，缺一不可修：
//!
//! 1. 两个 mod 各自定义同名内容（两边都有 `elf` 种族）时，
//!    `mymod:race.elf.display_name` 与 `lostland:race.elf.display_name`
//!    折成同一个 Fluent id，**后装载的静默覆盖前一个**，没有任何东西
//!    会报错；
//! 2. 与之配套的装载端此前只读本体一个目录，第三方 mod 的 `.ftl`
//!    **根本没有被读过**。
//!
//! 两条都是 `knowledge/design/dialogue-system.md` 三节 3.2 点名的致命
//! 缺口，本 crate 与 `ll_mod::locale_vfs`、`ll_game` 的装载点同批修完。
//!
//! # 语言回退：不许让玩家看见键名
//!
//! 一个 mod 只提供了 `zh-CN.ftl`，玩家用 `en` 玩——此前的行为是整屏
//! 显示 `mymod:item.foo.display_name` 这样的原始键名。
//! [`Catalog::resolve`] 因此有一条回退链：
//!
//! ```text
//! 请求的语言 → FALLBACK_LANGUAGE（en）→ 该命名空间其余语言（字典序）→ 键名
//! ```
//!
//! - **回退不跨命名空间**：`mymod:greet` 缺译时绝不去看本体的
//!   `greet`——那正是上一节要消灭的撞键行为换一种形式。
//! - 回退到**另一种语言的真实文案**是「看得懂但语言不对」，回退到键名
//!   是「看不懂」。前者玩家能继续玩，且一眼能看出是翻译缺失。
//! - 每次落到回退都记一条 `warn`，缺译仍然在日志里看得见。
//! - 需要判断「这个键在这种语言下到底有没有译文」的调用方（覆盖率门禁）
//!   用 [`Catalog::try_resolve`]，它精确、不回退。**这条配套不是可选
//!   的**：没有它，回退链会把已经在生效的覆盖率断言全部变哑。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fluent::{FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

/// Fluent 的实参表，[`Catalog::resolve_with_args`] 的入参类型。
///
/// **必须重新导出**：`resolve_with_args` 从一开始就是 `pub`，但它的
/// 参数类型此前只在本 crate 内部 `use`，crate 外没有任何合法途径构造
/// 一个 `FluentArgs`——那个方法因此是「公开但调不动」的。第一个真实
/// 调用方（`ll_ui::hud::character_panel` 的规则修正行）出现时补上这条
/// 导出，而不是让下游各自再声明一份 `fluent` 依赖：多一份独立的版本
/// 声明就多一个「同名不同类型」的漂移风险点，理由与 `ll-ui` 走
/// `ll-render` 重新导出的 `wgpu` 一致。
pub use fluent::FluentArgs;

/// 一个 mod（或本体）的本地化目录：命名空间 + 该命名空间的 `locales/`
/// 目录。
///
/// **本体不是特例**：本体那一条与任何 mod 那一条是同一个类型、走同一
/// 条装载路径，唯一的差别是 `dir` 指向 `assets/locales/` 而不是
/// `mods/<id>/locales/`——与 `ll_mod::asset_vfs::build` 把本体资产根目录
/// 与 `mods_root` 并列传入是同一形状，也正是
/// `knowledge/design/mod-package-structure.md`「本地化文件」一节
/// 「规格 §5 `locales/` 目录本身就可以理解成本体这个虚拟 mod 自己的
/// `locales/`」的直接实现。这里**没有**一条「本体专用」的装载入口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleSource {
    /// 这个目录里的全部 `.ftl` 属于哪个命名空间。
    pub namespace: String,
    /// 该命名空间的 `locales/` 目录，内含 `<语言标签>.ftl`。
    pub dir: PathBuf,
}

impl LocaleSource {
    /// 构造一条本地化来源。
    pub fn new(namespace: impl Into<String>, dir: impl Into<PathBuf>) -> LocaleSource {
        LocaleSource {
            namespace: namespace.into(),
            dir: dir.into(),
        }
    }
}

/// 查不到请求语言时，回退链里**优先**尝试的语言标签。
///
/// 见模块文档「语言回退」一节：本项目首发中英双语，`en` 是 mod 作者最
/// 可能提供的那一份，也是最可能被最多人看懂的那一份，因此它排在其余
/// 语言（字典序）之前。
pub const FALLBACK_LANGUAGE: &str = "en";

/// 全部已装载的本地化消息，按**命名空间 → 语言标签 → `FluentBundle`**
/// 两级分桶。
///
/// # 为什么必须有命名空间这一维
///
/// 见模块文档「命名空间是查表的一部分」一节。一句话：没有这一维，
/// `mymod:race.elf.display_name` 与 `lostland:race.elf.display_name`
/// 会折成同一个 Fluent id，落进同一个 bundle。
///
/// 语言标签是 `.ftl` 文件名去掉扩展名，如 `zh-CN.ftl` 对应标签
/// `"zh-CN"`——与
/// `knowledge/design/mod-package-structure.md`「本地化文件」一节的
/// `locales/<语言标签>.ftl` 约定完全一致，不需要在文件名之外再维护
/// 一份语言标签清单。
pub struct Catalog {
    /// 裸键（不含冒号，如 `window.title`、`hud-status-time-label`）归属
    /// 的命名空间。这些键属于引擎/HUD 自身而不属于任何内容表，今天就
    /// 没有前缀，此处保持它们的既有行为。
    base_namespace: String,
    bundles: HashMap<String, HashMap<String, FluentBundle<FluentResource>>>,
}

impl Catalog {
    /// 按顺序装载全部本地化来源。
    ///
    /// **不返回 `Result`，永不失败**：目录不存在、某个文件语法有误、
    /// 文件名不是合法语言标签——这些情况各自跳过对应的语言/文件并记一条
    /// `tracing::warn!`，装载流程整体继续。理由与模块文档「缺键与缺
    /// 语言」一节一致：本地化资源是外部数据，任何单点损坏都不该阻塞
    /// 启动。极端情况下（目录整个不存在）会得到一个空 `Catalog`——
    /// 此后每次 `resolve` 都会走「回退到键名」分支，游戏仍然能跑，只是
    /// 玩家会看到键名而不是译文，这比直接起不来更好。
    ///
    /// `sources` 的顺序**不参与任何判断**：每一条各自落进自己命名空间
    /// 的桶，互不覆盖（C5——顺序不是逻辑，因此也不需要排序）。同一个
    /// 命名空间被给出两次是调用方的缺陷（`ll_mod::topo::topo_sort` 会
    /// 先一步拒绝重复命名空间），这里记一条 warn 并让后来者生效。
    pub fn load(base_namespace: &str, sources: &[LocaleSource]) -> Catalog {
        let mut bundles: HashMap<String, HashMap<String, FluentBundle<FluentResource>>> =
            HashMap::new();
        for source in sources {
            let loaded = load_namespace_dir(&source.namespace, &source.dir);
            let slot = bundles.entry(source.namespace.clone()).or_default();
            for (language, bundle) in loaded {
                if slot.insert(language.clone(), bundle).is_some() {
                    tracing::warn!(
                        namespace = %source.namespace,
                        %language,
                        "同一个命名空间被装载了两次，后一份覆盖前一份"
                    );
                }
            }
        }
        Catalog {
            base_namespace: base_namespace.to_string(),
            bundles,
        }
    }

    /// 只装载一个命名空间的一个目录——测试与「只关心某一份内容的文案」
    /// 的调用点用。
    ///
    /// 它**不是**给本体开的捷径：命名空间是显式入参，本体要用它就得和
    /// 任何 mod 一样把 `"lostland"` 写出来。
    pub fn load_one(namespace: &str, dir: &Path) -> Catalog {
        Catalog::load(namespace, &[LocaleSource::new(namespace, dir)])
    }

    /// 本体命名空间下已成功装载的语言标签数量——只用于启动日志与测试
    /// 断言，不参与任何查表逻辑。
    pub fn loaded_language_count(&self) -> usize {
        self.languages().len()
    }

    /// 全部 `(命名空间, 语言)` 桶的数量——启动日志用它回答「有几个 mod
    /// 的本地化真的被装进来了」，[`Self::loaded_language_count`] 回答不了
    /// 这个问题。
    pub fn loaded_bundle_count(&self) -> usize {
        self.bundles.values().map(HashMap::len).sum()
    }

    /// **本体命名空间**已装载的全部语言标签，**按字典序排好**。
    ///
    /// # 为什么只报本体这一个命名空间
    ///
    /// 这份清单的唯一消费者是设置界面的语言切换（见
    /// `ll_game::menu_screen` 里那条「顺序本身就是逻辑」的文档）。一个
    /// mod 提供了 `ja.ftl` 并不意味着**游戏本体 UI** 有日文——把 `ja`
    /// 放进设置里，玩家选中后看到的是一整屏走回退链的英文 UI 加一小撮
    /// 日文 mod 文案。「mod 能不能给游戏新增一种可选语言」是一个独立的
    /// 产品问题，本 crate 不替它做决定，保持本地化命名空间化之前的行为
    /// 逐字不变。
    ///
    /// # 为什么必须排序（C5）
    ///
    /// 内部是 `HashMap`，直接遍历它的键就是让哈希桶序参与逻辑判断——而
    /// 这份清单的**顺序本身就是逻辑**：设置界面按左右键在这个清单里
    /// 循环，顺序决定「按一下右键切到哪一种语言」。
    /// `docs/architecture/03-invariants.md` C5 一节给的判据在这里逐字
    /// 成立：「这个值会不会被用来决定处理顺序……会，就是错的」。
    ///
    /// 排字典序而不是装载顺序：装载顺序来自
    /// [`std::fs::read_dir`]，那个顺序在不同文件系统上并不保证一致，
    /// 同样是一个隐藏的非确定输入。
    pub fn languages(&self) -> Vec<String> {
        let Some(slot) = self.bundles.get(&self.base_namespace) else {
            return Vec::new();
        };
        let mut tags: Vec<String> = slot.keys().cloned().collect();
        tags.sort();
        tags
    }

    /// 查 `key` 在 `language` 下的文本，**精确查找，不走任何回退**：
    /// 查不到返回 `None`。
    ///
    /// # 为什么需要它，而不是让所有人都用 [`Self::resolve`]
    ///
    /// [`Self::resolve`] 有语言回退链（见模块文档「语言回退」一节），
    /// 于是「某个键漏了 zh-CN 译文」在它那里表现为一句英文，而不是键名。
    /// 那对玩家是改善，对**门禁**是灾难：本 crate 里那三条真实资产
    /// 覆盖率测试的判据正是「解析结果 == 键名即视为缺译」，回退链会让
    /// 它们全部变哑。凡是要判断「这个键在这种语言下到底有没有译文」的
    /// 调用方，用这一个，不要用 [`Self::resolve`]。
    pub fn try_resolve(&self, language: &str, key: &str) -> Option<String> {
        self.try_resolve_with_args(language, key, None)
    }

    /// [`Self::try_resolve`] 的带参版本。
    pub fn try_resolve_with_args(
        &self,
        language: &str,
        key: &str,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        let (namespace, fluent_id) = split_key(key, &self.base_namespace);
        self.format(namespace, language, &fluent_id, args)
    }

    /// 查 `key` 在 `language` 下的文本，不带参数插值。
    ///
    /// `key` 既可以是裸 Fluent 路径（如 `"window.title"`，
    /// `ll_platform::window::WindowConfig::title_key` 的既有形状——本
    /// crate 不依赖 `ll-platform`，此处只是文字引用，不做可解析的文档
    /// 内链），也可以是带命名空间前缀的完整键（如
    /// `"lostland:race.human.display_name"`，`ll-mod` 内容表的
    /// `display_name_key: NamespacedId` 既有形状）。**命名空间前缀决定
    /// 去哪个 mod 的 `locales/` 里查**，裸键落到本体命名空间。
    pub fn resolve(&self, language: &str, key: &str) -> String {
        self.resolve_with_args(language, key, None)
    }

    /// 查 `key` 在 `language` 下的文本，`args` 提供 Fluent 消息里
    /// `{ $变量 }` 占位符的实参（例如
    /// `ll_content::load_error::ModSetMismatch` 的 `namespace`/
    /// `required_version`）。
    ///
    /// 查不到时走模块文档「语言回退」一节的回退链，全部落空才回退到
    /// 键名本身。
    pub fn resolve_with_args(
        &self,
        language: &str,
        key: &str,
        args: Option<&FluentArgs>,
    ) -> String {
        let (namespace, fluent_id) = split_key(key, &self.base_namespace);

        if let Some(text) = self.format(namespace, language, &fluent_id, args) {
            return text;
        }

        for fallback in self.fallback_languages(namespace, language) {
            let Some(text) = self.format(namespace, &fallback, &fluent_id, args) else {
                continue;
            };
            tracing::warn!(
                language,
                fallback = %fallback,
                key,
                namespace,
                "该键在请求的语言下查不到，回退到同一命名空间的另一种语言"
            );
            return text;
        }

        tracing::warn!(
            language,
            key,
            namespace,
            fluent_id,
            "该键在这个命名空间的任何一种语言下都查不到，回退到键名本身"
        );
        key.to_string()
    }

    /// `namespace` 下除 `requested` 之外的全部语言，[`FALLBACK_LANGUAGE`]
    /// 排最前，其余按字典序——**回退链的顺序必须确定**（C5），否则同一份
    /// 内容在两次运行里可能回退到不同的语言。
    fn fallback_languages(&self, namespace: &str, requested: &str) -> Vec<String> {
        let Some(slot) = self.bundles.get(namespace) else {
            return Vec::new();
        };
        let mut tags: Vec<String> = slot
            .keys()
            .filter(|tag| tag.as_str() != requested)
            .cloned()
            .collect();
        tags.sort();
        if let Some(position) = tags.iter().position(|tag| tag == FALLBACK_LANGUAGE) {
            let preferred = tags.remove(position);
            tags.insert(0, preferred);
        }
        tags
    }

    /// 在一个确定的 `(命名空间, 语言)` 桶里格式化一条消息；桶不存在、
    /// 键不存在、键没有可显示的值，三种情况一律返回 `None`——**由调用方
    /// 决定降级到什么**，本方法不自己决定。
    fn format(
        &self,
        namespace: &str,
        language: &str,
        fluent_id: &str,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        let bundle = self.bundles.get(namespace)?.get(language)?;
        let pattern = bundle.get_message(fluent_id)?.value()?;

        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned();

        if !errors.is_empty() {
            tracing::warn!(
                namespace,
                language,
                fluent_id,
                ?errors,
                "本地化文本格式化出现错误，结果可能不完整"
            );
        }

        Some(text)
    }
}

/// 把一个 `_key` 字段的取值拆成 `(命名空间, Fluent 消息 id)`。
///
/// # 命名空间：决定去哪个桶查，不再被丢弃
///
/// 冒号前缀是 `NamespacedId` 的命名空间（`lostland:race.elf.display_name`
/// 的 `lostland`）。此前它在查表前被**整个剥掉**，于是两个 mod 各自
/// 定义的同名内容会折成同一个 Fluent id：`mymod:race.elf.display_name`
/// 与 `lostland:race.elf.display_name` 剥完都是 `race-elf-display_name`。
/// 现在它决定去哪个命名空间的桶里查。没有冒号的裸键（`window.title`）
/// 落到 `base_namespace`。
///
/// # 为什么消息 id 需要转换，不能直接拿路径当 id
///
/// 两条既有约束叠在一起，逼出这次转换：
///
/// 1. `NamespacedId`（`ll-mod` 内容表用它存 `display_name_key`）的
///    `路径` 部分允许包含点号（如 `race.human.display_name`，见
///    `ll_core::ident::NamespacedId::parse` 的合法字符集），这是内容
///    ID 命名空间自身的分层约定。
/// 2. Fluent 的消息标识符语法（`[a-zA-Z][a-zA-Z0-9_-]*`）**不允许
///    点号**——`.` 在 `.ftl` 语法里是消息「属性」（`message.attr`）的
///    分隔符，只能出现在专门声明的属性行里，不能作为主消息 id 的一
///    部分。若把 `race.human.display_name` 原样当 Fluent id 写进
///    `.ftl`，解析会在第一个点号处失败。
///
/// 解法：查表前把点号换成连字符（`race.human.display_name` →
/// `race-human-display_name`），下划线保留不动——连字符和下划线都是
/// Fluent id 合法字符，只有点号不合法。`.ftl` 文件里的条目因此用
/// 连字符分隔而不是点号，见 `assets/locales/zh-CN.ftl`。
fn split_key<'a>(key: &'a str, base_namespace: &'a str) -> (&'a str, String) {
    match key.split_once(':') {
        Some((namespace, path)) => (namespace, path.replace('.', "-")),
        None => (base_namespace, key.replace('.', "-")),
    }
}

/// 装载一个命名空间的 `locales/` 目录，产出 `语言标签 → FluentBundle`。
///
/// 目录不存在只记一条 warn 并返回空表——见 [`Catalog::load`] 文档
/// 「不返回 `Result`」一节。
fn load_namespace_dir(namespace: &str, dir: &Path) -> Vec<(String, FluentBundle<FluentResource>)> {
    let mut loaded = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(
                namespace,
                dir = %dir.display(),
                %error,
                "本地化目录不存在或无法读取，这个命名空间没有任何已装载的语言"
            );
            return loaded;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("ftl") {
            continue;
        }
        let Some(language) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        match load_bundle(&path, language) {
            Ok(bundle) => loaded.push((language.to_string(), bundle)),
            Err(error) => {
                tracing::warn!(
                    namespace,
                    path = %path.display(),
                    %error,
                    "本地化文件装载失败，跳过这一种语言"
                );
            }
        }
    }

    loaded
}

/// 装载单个 `.ftl` 文件为一个语言的 `FluentBundle`。
fn load_bundle(path: &Path, language: &str) -> Result<FluentBundle<FluentResource>, LoadError> {
    let text = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    let resource = FluentResource::try_new(text).map_err(|(_, errors)| LoadError::Syntax {
        error_count: errors.len(),
    })?;

    let lang_id: LanguageIdentifier = language
        .parse()
        .map_err(|_| LoadError::InvalidLanguageTag)?;
    let mut bundle = FluentBundle::new(vec![lang_id]);
    // use_isolating = false：本地化文本会与游戏内其余 UI 文字混排，
    // Fluent 默认插入的双向文本隔离符（U+2068/U+2069）在纯左到右的
    // 中英混排场景下没有实际作用，反而可能被下游文本渲染当成不可见
    // 的异常字形处理——见 `ll-text` 的 CJK 排版取舍（
    // `knowledge/pipelines/text-and-font-rendering.md`），本项目暂不
    // 支持阿拉伯语/希伯来语等真正需要双向隔离的语言。
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .map_err(|errors| LoadError::Syntax {
            error_count: errors.len(),
        })?;

    Ok(bundle)
}

/// [`load_bundle`] 的失败原因，只用于日志诊断（见
/// [`Catalog::load`] 文档「不返回 `Result`」一节）——不向调用方
/// 暴露，调用方只关心装载后 [`Catalog::loaded_language_count`] 少了
/// 一种语言，具体哪里错、错在哪一行留给日志。
#[derive(Debug)]
enum LoadError {
    Io(std::io::Error),
    Syntax { error_count: usize },
    InvalidLanguageTag,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(error) => write!(f, "读取文件失败：{error}"),
            LoadError::Syntax { error_count } => {
                write!(f, "Fluent 语法解析失败，共 {error_count} 处错误")
            }
            LoadError::InvalidLanguageTag => write!(f, "文件名不是合法的语言标签"),
        }
    }
}

impl std::error::Error for LoadError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试里统一用本体的命名空间——**不是**因为本体特殊，而是因为
    /// 真实资产覆盖率那几条测试读的就是本体的 `assets/locales/`。
    /// `ll_game::content::BASE_NAMESPACE` 是它的生产真相源，本 crate
    /// 不依赖 `ll-game`（依赖方向见模块文档），此处只能重写一份字面量。
    const BASE: &str = "lostland";

    /// 在临时目录写两个最小 `.ftl` 文件，模拟 `assets/locales/` 的
    /// 真实形状——用临时目录而非直接读仓库里的 `assets/locales/`，
    /// 是为了让本测试不依赖仓库内容是否被后续改动，只验证
    /// [`Catalog`] 自身的查表逻辑。真实内容的端到端验证见
    /// `ll-game` 对 `assets/locales/` 的装载测试。
    fn write_fixture_catalog(dir: &Path) {
        std::fs::write(dir.join("zh-CN.ftl"), "greeting = 你好\n").expect("测试用写入应当成功");
        std::fs::write(dir.join("en.ftl"), "greeting = Hello\n").expect("测试用写入应当成功");
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ll-i18n-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("测试用建目录应当成功");
        dir
    }

    #[test]
    fn 已装载语言清单按字典序排列() {
        // C5：这份清单的顺序**就是**逻辑（设置界面按左右键在它里面
        // 循环）。直接遍历 HashMap 的键会让哈希桶序决定「按一下右键
        // 切到哪一种语言」。
        //
        // **为什么铺八种语言而不是两种**：ADR 0022 的反例验证实测抓到
        // 过——只有两种语言时，去掉排序之后本断言六次里只红两次
        // （哈希桶序有约一半概率恰好就是字典序），是一条会漏网的断言。
        // 八种语言把「碰巧有序」的概率压到 1/8!（约万分之零点二），
        // 断言才真的咬得住。这正是 C5 一节警告的那种「测试照样全绿」
        // 的形状，只是这次发生在守护它的测试自己身上。
        // Arrange
        let dir = temp_dir("languages-sorted");
        let tags = ["zh-CN", "en", "ja", "de", "fr", "ko", "ru", "es"];
        for tag in tags {
            std::fs::write(
                dir.join(format!("{tag}.ftl")),
                "greeting = x
",
            )
            .expect("测试用写入应当成功");
        }
        let catalog = Catalog::load_one(BASE, &dir);

        // Act
        let languages = catalog.languages();

        // Assert
        let mut expected: Vec<String> = tags.iter().map(|tag| tag.to_string()).collect();
        expected.sort();
        assert_eq!(languages, expected);
    }

    #[test]
    fn 同一份目录两次装载给出逐条相同的语言清单() {
        // 上一条只能证明「这一次是排好的」；本条证明它不随进程内的
        // 哈希种子变化——两个内容相同的 HashMap 给出不同迭代顺序是
        // 本仓库 P4 期间实测确认过的事实（C5 一节原话）。
        // Arrange
        let dir = temp_dir("languages-stable");
        write_fixture_catalog(&dir);

        // Act
        let 第一次 = Catalog::load_one(BASE, &dir).languages();
        let 第二次 = Catalog::load_one(BASE, &dir).languages();

        // Assert
        assert_eq!(第一次, 第二次);
    }

    #[test]
    fn 目录不存在时语言清单为空而不是恐慌() {
        // Arrange
        let dir = std::env::temp_dir().join("ll-i18n-test-no-such-dir-中文");

        // Act
        let catalog = Catalog::load_one(BASE, &dir);

        // Assert
        assert!(catalog.languages().is_empty());
    }

    #[test]
    fn 给定键和语言解析出该语言对应的字串() {
        // Arrange
        let dir = temp_dir("resolve-basic");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(BASE, &dir);

        // Act
        let text = catalog.resolve("zh-CN", "greeting");

        // Assert
        assert_eq!(text, "你好");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 切换语言后同一个键解析出不同的字串() {
        // 这是本地化系统区别于「查了张表」的关键验证点：不是随便查出
        // 点什么都算数，必须是**同一个键**在**不同语言**下真的产出
        // 两段不同的文本。
        // Arrange
        let dir = temp_dir("resolve-switch-language");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(BASE, &dir);

        // Act
        let zh_text = catalog.resolve("zh-CN", "greeting");
        let en_text = catalog.resolve("en", "greeting");

        // Assert
        assert_ne!(zh_text, en_text);
        assert_eq!(zh_text, "你好");
        assert_eq!(en_text, "Hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 缺键时回退到键名本身而不是空字符串() {
        // Arrange
        let dir = temp_dir("resolve-missing-key");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(BASE, &dir);

        // Act
        let text = catalog.resolve("zh-CN", "no.such.key");

        // Assert
        assert_eq!(text, "no.such.key");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 未装载的语言回退到同命名空间的另一种语言而不是键名() {
        // 本条**推翻**了本地化命名空间化之前的同名断言（原断言：查不到
        // 语言就返回键名）。理由见模块文档「语言回退」一节：一个只提供
        // zh-CN 的 mod 在英文玩家那里会整屏显示原始键名，那是玩家可见的
        // 乱码。原断言原样保留在 git 历史里，此处不删来由。
        // Arrange
        let dir = temp_dir("resolve-missing-language");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(BASE, &dir);

        // Act
        let text = catalog.resolve("fr", "greeting");

        // Assert：fr 没装载，回退链先试 FALLBACK_LANGUAGE（en）
        assert_eq!(text, "Hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 只提供一种语言的模组在别的语言下给出它自己的文案而不是键名() {
        // 任务书的硬约束：**不许静默显示原始键名**。这一条咬住的正是
        // 「mod 只写了 zh-CN，玩家用 en」这个真实场景。
        // Arrange
        let dir = temp_dir("resolve-mod-single-language");
        std::fs::write(dir.join("zh-CN.ftl"), "item-foo-display_name = 魔杖\n")
            .expect("测试用写入应当成功");
        let catalog = Catalog::load_one("mymod", &dir);

        // Act
        let text = catalog.resolve("en", "mymod:item.foo.display_name");

        // Assert
        assert_eq!(text, "魔杖");
        assert_ne!(text, "mymod:item.foo.display_name");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 回退链优先英文而不是字典序第一名() {
        // C5：回退链的顺序必须确定。这一条同时证明它**不是**字典序——
        // 只按字典序的话 de 会赢过 en。
        // Arrange
        let dir = temp_dir("resolve-fallback-prefers-en");
        std::fs::write(dir.join("de.ftl"), "greeting = Hallo\n").expect("测试用写入应当成功");
        std::fs::write(dir.join("en.ftl"), "greeting = Hello\n").expect("测试用写入应当成功");
        let catalog = Catalog::load_one("mymod", &dir);

        // Act
        let text = catalog.resolve("ja", "mymod:greeting");

        // Assert
        assert_eq!(text, "Hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 语言回退不跨命名空间() {
        // 跨命名空间回退等于把本批要消灭的撞键行为换一种形式重新引入：
        // 一个 mod 忘了写译文，玩家看到的会是本体同路径那条内容的名字，
        // 而且没有任何东西会说这不对。
        // Arrange
        let base_dir = temp_dir("fallback-scope-base");
        std::fs::write(base_dir.join("en.ftl"), "race-elf-display_name = Elf\n")
            .expect("测试用写入应当成功");
        let mod_dir = temp_dir("fallback-scope-mod");
        std::fs::write(
            mod_dir.join("zh-CN.ftl"),
            "race-gnome-display_name = 侏儒\n",
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load(
            BASE,
            &[
                LocaleSource::new(BASE, &base_dir),
                LocaleSource::new("mymod", &mod_dir),
            ],
        );

        // Act：mymod 没有 elf 这条键，本体有
        let text = catalog.resolve("en", "mymod:race.elf.display_name");

        // Assert：回退到键名，**不是**本体的 "Elf"
        assert_eq!(text, "mymod:race.elf.display_name");

        // Cleanup
        let _ = std::fs::remove_dir_all(&base_dir);
        let _ = std::fs::remove_dir_all(&mod_dir);
    }

    #[test]
    fn 两个命名空间下的同名键互不覆盖() {
        // 这是缺口 ② 的单元级断言（端到端那条在 ll-game 的集成测试里，
        // 用真实的 mods/ 目录）。把命名空间分流去掉，两条会解析成同一段
        // 文本，本条必红。
        // Arrange
        let base_dir = temp_dir("collision-base");
        std::fs::write(base_dir.join("zh-CN.ftl"), "race-elf-display_name = 精灵\n")
            .expect("测试用写入应当成功");
        let mod_dir = temp_dir("collision-mod");
        std::fs::write(
            mod_dir.join("zh-CN.ftl"),
            "race-elf-display_name = 高等精灵\n",
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load(
            BASE,
            &[
                LocaleSource::new(BASE, &base_dir),
                LocaleSource::new("mymod", &mod_dir),
            ],
        );

        // Act
        let 本体 = catalog.resolve("zh-CN", "lostland:race.elf.display_name");
        let 模组 = catalog.resolve("zh-CN", "mymod:race.elf.display_name");

        // Assert
        assert_eq!(本体, "精灵");
        assert_eq!(模组, "高等精灵");
        assert_ne!(本体, 模组);

        // Cleanup
        let _ = std::fs::remove_dir_all(&base_dir);
        let _ = std::fs::remove_dir_all(&mod_dir);
    }

    #[test]
    fn 装载来源的先后顺序不改变任何解析结果() {
        // C5：`sources` 的顺序不参与判断。两个 mod 各自落进自己的桶，
        // 谁先谁后都一样——这与精灵资产的「覆盖按拓扑序生效」是两件不同
        // 的事，本地化结构上就没有覆盖。
        // Arrange
        let a_dir = temp_dir("order-a");
        std::fs::write(a_dir.join("zh-CN.ftl"), "greeting = 甲\n").expect("测试用写入应当成功");
        let b_dir = temp_dir("order-b");
        std::fs::write(b_dir.join("zh-CN.ftl"), "greeting = 乙\n").expect("测试用写入应当成功");
        let 正序 = Catalog::load(
            BASE,
            &[
                LocaleSource::new("amod", &a_dir),
                LocaleSource::new("bmod", &b_dir),
            ],
        );
        let 逆序 = Catalog::load(
            BASE,
            &[
                LocaleSource::new("bmod", &b_dir),
                LocaleSource::new("amod", &a_dir),
            ],
        );

        // Act & Assert
        for key in ["amod:greeting", "bmod:greeting"] {
            assert_eq!(正序.resolve("zh-CN", key), 逆序.resolve("zh-CN", key));
        }
        assert_eq!(正序.resolve("zh-CN", "amod:greeting"), "甲");
        assert_eq!(正序.resolve("zh-CN", "bmod:greeting"), "乙");

        // Cleanup
        let _ = std::fs::remove_dir_all(&a_dir);
        let _ = std::fs::remove_dir_all(&b_dir);
    }

    #[test]
    fn 精确查找不走回退链所以覆盖率门禁不会被弄哑() {
        // `try_resolve` 是语言回退链的必要配套：没有它，「某个键漏了
        // zh-CN 译文」会被回退成一句英文，而覆盖率断言的判据（结果 ==
        // 键名即视为缺译）就再也不会红。
        // Arrange
        let dir = temp_dir("try-resolve-exact");
        write_fixture_catalog(&dir);
        let catalog = Catalog::load_one(BASE, &dir);

        // Act & Assert
        assert_eq!(
            catalog.try_resolve("zh-CN", "greeting").as_deref(),
            Some("你好")
        );
        assert_eq!(catalog.try_resolve("fr", "greeting"), None);
        assert_eq!(catalog.try_resolve("zh-CN", "no.such.key"), None);
        // 对照：同一个输入走 resolve 会被回退链救回来
        assert_eq!(catalog.resolve("fr", "greeting"), "Hello");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 目录不存在时装载出空目录而不是崩溃() {
        // Arrange
        let dir =
            std::env::temp_dir().join(format!("ll-i18n-test-nonexistent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // Act
        let catalog = Catalog::load_one(BASE, &dir);

        // Assert
        assert_eq!(catalog.loaded_language_count(), 0);
    }

    #[test]
    fn 命名空间前缀决定去哪个桶查而裸键落到本体命名空间() {
        // 验证 ll-mod 内容表的 NamespacedId 键形状
        // （"lostland:race.human.display_name"）与 ll-platform 的裸键
        // 形状（"window.title"）都能解析——前者按前缀选桶，后者落到
        // base_namespace，见 `split_key` 文档。
        // Arrange
        let dir = temp_dir("resolve-namespaced");
        std::fs::write(
            dir.join("zh-CN.ftl"),
            "race-human-display_name = 人类\nwindow-title = 迷途大陆\n",
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load_one(BASE, &dir);

        // Act
        let text = catalog.resolve("zh-CN", "lostland:race.human.display_name");
        let 裸键 = catalog.resolve("zh-CN", "window.title");

        // Assert
        assert_eq!(text, "人类");
        assert_eq!(裸键, "迷途大陆");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 参数插值把变量替换进最终文本() {
        // Arrange
        let dir = temp_dir("resolve-with-args");
        std::fs::write(
            dir.join("zh-CN.ftl"),
            "save-mod-missing = 缺少模组 { $namespace }\n",
        )
        .expect("测试用写入应当成功");
        let catalog = Catalog::load_one(BASE, &dir);
        let mut args = FluentArgs::new();
        args.set("namespace", "examplemod");

        // Act
        let text = catalog.resolve_with_args("zh-CN", "save-mod-missing", Some(&args));

        // Assert
        assert_eq!(text, "缺少模组 examplemod");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 全部约七十处 `_key` 使用点背后实际用到的键——本体三个种族、
    /// 三个职业、两个转职、十二个键位动作、窗口标题、三条 mod/存档
    /// 校验消息 id，逐一核实真实 `assets/locales/` 里都配了对应译文。
    /// 与其余用临时目录 fixture 的测试不同，这条故意读仓库真实资产，
    /// 就是要在 CI 里当场发现「代码里新增了一个 `_key` 常量，但忘了
    /// 补对应 `.ftl` 条目」这类遗漏。
    ///
    /// P7 第一批（只读观测 HUD，`ll-ui::hud`）追加了状态栏/角色面板/
    /// 背包/装备面板一批键——这批键不来自任何 `_key` 字段（它们是 UI
    /// 布局自身的标签与 `AttributeKind`/`EquipSlot` 的展示名，不是内容
    /// 表字段），但同样必须在真实 `.ftl` 里有译文，纳入同一条覆盖率
    /// 检查，理由一致：不要让「新增一个 HUD 常量，但忘了补 `.ftl`」这
    /// 类遗漏在 CI 里潜伏下去。
    const PRODUCTION_KEYS: &[&str] = &[
        "window.title",
        "lostland:keybind.action.up",
        "lostland:keybind.action.down",
        "lostland:keybind.action.left",
        "lostland:keybind.action.right",
        "lostland:keybind.action.confirm",
        "lostland:keybind.action.cancel",
        "lostland:keybind.action.menu",
        "lostland:keybind.action.map",
        "lostland:keybind.action.wait",
        "lostland:keybind.action.screenshot",
        "lostland:keybind.action.zoom_in",
        "lostland:keybind.action.zoom_out",
        "lostland:race.human.display_name",
        "lostland:race.dwarf.display_name",
        "lostland:race.elf.display_name",
        "lostland:class.warrior.display_name",
        "lostland:class.mage.display_name",
        "lostland:class.ranger.display_name",
        // 卫兵 + 六条据点职业（NPC 生成批次）：`mods/lostland/classes.json5`
        // 现在注册十条职业，前三条以外的七条此前只在 .ftl 里有译文而不在
        // 本清单上，因此「新增一条职业内容却忘了补 .ftl」这类遗漏对它们
        // 一直测不出来。据点名册（`ll_mod::roster`）真的会把这七条职业挂
        // 在生成出来的 NPC 身上，展示层随时会去取它们的名字，本批次把它们
        // 一并收进这道覆盖检查。
        "lostland:class.guard.display_name",
        "lostland:class.steward.display_name",
        "lostland:class.militia.display_name",
        "lostland:class.farmer.display_name",
        "lostland:class.hunter.display_name",
        "lostland:class.butcher.display_name",
        "lostland:class.blacksmith.display_name",
        // 渔夫 / 牧羊人 / 石匠（按职业选行为树 + 资源两层分类批次）：
        // 三条新据点职业，理由与上面七条逐字相同——据点名册真的会把
        // 它们挂在生成出来的 NPC 身上。
        "lostland:class.fisher.display_name",
        "lostland:class.shepherd.display_name",
        "lostland:class.mason.display_name",
        "lostland:subclass.duelist.display_name",
        "lostland:subclass.apprentice.display_name",
        // 上面十一条（三个种族、四个职业、剑舞者/学徒两个副职）现在
        // **全部**是真实生产内容，都走 mod 脚本注册：本体职业/技能/
        // 副职/任务迁移批次把 `materialize_base_{classes,skills,
        // subclasses,quests}` 四个函数连同它们的测试夹具一并删除，
        // 那四个函数此前从来不在生产装载路径上（见 `ll_mod::class`
        // 模块文档同名一节），于是这批键此前指向的内容根本没被装载
        // 过。下面八条（四个制作类副职 + 四个配方类别）来自副职获得
        // 机制批次，一直就是真实生产内容。
        //
        // 技能与任务没有出现在本清单里，因为 `SkillAttrs`/`QuestAttrs`
        // 根本没有 `display_name_key` 字段——那不是遗漏，是这两类内容
        // 至今没有任何展示层需要它们的名字。
        "lostland:subclass.artisan.display_name",
        "lostland:subclass.tailor.display_name",
        "lostland:subclass.alchemist.display_name",
        "lostland:subclass.cook.display_name",
        "lostland:recipe_category.forging.display_name",
        "lostland:recipe_category.tailoring.display_name",
        "lostland:recipe_category.alchemy.display_name",
        "lostland:recipe_category.cooking.display_name",
        // 四档地形形态预设的名字与说明（世界生成参数落地批次）：
        // `ll_content::world_identity::TERRAIN_PRESETS` 的
        // `display_name_key`/`description_key`。它们是玩家在建档时真正
        // 会看到的文案（当前经日志与配置文件说明，P7 开局界面落地后经
        // 界面），与上面那批内容名字同一条纪律：新增一档预设却忘了补
        // .ftl，必须在这里红。
        "lostland:worldgen.preset.continent.display_name",
        "lostland:worldgen.preset.continent.description",
        "lostland:worldgen.preset.archipelago.display_name",
        "lostland:worldgen.preset.archipelago.description",
        "lostland:worldgen.preset.highland.display_name",
        "lostland:worldgen.preset.highland.description",
        "lostland:worldgen.preset.inland.display_name",
        "lostland:worldgen.preset.inland.description",
        "save-mod-missing",
        "save-mod-version-mismatch",
        "mod-dependency-version-mismatch",
        "hud-status-time-label",
        "hud-status-health-label",
        "hud-status-mana-label",
        "lostland:season.spring.display_name",
        "lostland:season.summer.display_name",
        "lostland:season.autumn.display_name",
        "lostland:season.winter.display_name",
        "hud-character-panel-title",
        "hud-character-level-label",
        "hud-character-experience-label",
        "hud-character-modifiers-title",
        "hud-character-modifiers-empty",
        "lostland:attribute.strength.display_name",
        "lostland:attribute.dexterity.display_name",
        "lostland:attribute.constitution.display_name",
        "lostland:attribute.intelligence.display_name",
        "lostland:attribute.willpower.display_name",
        "lostland:attribute.charisma.display_name",
        "hud-inventory-panel-title",
        "hud-inventory-empty",
        "hud-inventory-durability-label",
        "hud-equipment-panel-title",
        "hud-equipment-empty-slot",
        "lostland:equip_slot.main_hand.display_name",
        "lostland:equip_slot.off_hand.display_name",
        "lostland:equip_slot.head.display_name",
        "lostland:equip_slot.face.display_name",
        "lostland:equip_slot.eyes.display_name",
        "lostland:equip_slot.neck.display_name",
        "lostland:equip_slot.body.display_name",
        "lostland:equip_slot.outer.display_name",
        "lostland:equip_slot.back.display_name",
        "lostland:equip_slot.shoulder_l.display_name",
        "lostland:equip_slot.shoulder_r.display_name",
        "lostland:equip_slot.arm_l.display_name",
        "lostland:equip_slot.arm_r.display_name",
        "lostland:equip_slot.hand_l.display_name",
        "lostland:equip_slot.hand_r.display_name",
        "lostland:equip_slot.belt.display_name",
        "lostland:equip_slot.tasset.display_name",
        "lostland:equip_slot.legs.display_name",
        "lostland:equip_slot.boot_l.display_name",
        "lostland:equip_slot.boot_r.display_name",
        "lostland:equip_slot.ring_l.display_name",
        "lostland:equip_slot.ring_r.display_name",
        "lostland:equip_slot.unknown.display_name",
    ];

    /// 仓库根目录——`ll-i18n` 位于 `crates/ll-i18n`，向上两级到根。
    fn repo_locales_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("assets")
            .join("locales")
    }

    #[test]
    fn 真实资产目录覆盖全部本体键的中文翻译() {
        // Arrange
        let catalog = Catalog::load_one(BASE, &repo_locales_dir());

        // Act & Assert：用 `try_resolve` 而不是 `resolve`——后者有语言
        // 回退链，一条只缺 zh-CN 的键会被回退成英文，这条断言就再也不会
        // 红了。见 `Catalog::try_resolve` 文档。
        for key in PRODUCTION_KEYS {
            assert!(
                catalog.try_resolve("zh-CN", key).is_some(),
                "键 {key} 在 zh-CN.ftl 里没有对应译文"
            );
        }
    }

    #[test]
    fn 真实资产目录覆盖全部本体键的英文翻译() {
        // Arrange
        let catalog = Catalog::load_one(BASE, &repo_locales_dir());

        // Act & Assert：理由同上一条，用精确查找。
        for key in PRODUCTION_KEYS {
            assert!(
                catalog.try_resolve("en", key).is_some(),
                "键 {key} 在 en.ftl 里没有对应译文"
            );
        }
    }

    #[test]
    fn 真实资产目录里同一批键的中英文文本逐一互不相同() {
        // 比前两条更强的断言：不只是「两种语言各自都有译文」，而是
        // 同一个键在两种语言下产出的文本确实不同——否则无法排除
        // 「两份 .ftl 手滑复制成了同一份内容」这种两条测试各自都通过
        // 但本地化其实没有真正切换的情形。
        // Arrange
        let catalog = Catalog::load_one(BASE, &repo_locales_dir());

        // Act & Assert
        for key in PRODUCTION_KEYS {
            let zh_text = catalog.try_resolve("zh-CN", key);
            let en_text = catalog.try_resolve("en", key);
            assert_ne!(zh_text, en_text, "键 {key} 的中英文译文相同，怀疑内容重复");
        }
    }
}
