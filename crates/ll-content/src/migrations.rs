//! 真正注册进迁移链的迁移函数——目前是空的。
//!
//! [`crate::migration`] 只搭了迁移链的机制骨架（见其模块文档「本任务
//! 只搭机制，不接入真实迁移函数」一节），本模块原本是把具体迁移函数
//! 真正注册进 `save_file::migration_chain`（模块私有，无法作为 rustdoc
//! 链接目标）的地方。发布前的
//! 开发过程中，这里累计注册过三步具体迁移（`Migration1To2`：落地探索
//! 记忆批次新增 `WorldState::exploration`/`Interior::origin`；
//! `Migration2To3`：击杀与死亡记录批次新增
//! `WorldState::history`/`next_world_id`；`Migration3To4`：无名单位击杀
//! 计数批次新增 `WorldState::kill_counts`），配套的「形状变了」镜像
//! 类型（`WorldStateV1`/`WorldStateV2`/`WorldStateV3`/`AgentV2`/
//! `InteriorV1`/`InteriorTableDataV1`）与手写字节测试夹具
//! （`encode_v1_body`/`encode_v2_body`/`encode_v3_body`）随之逐批累积。
//!
//! # 为什么现在清空，而不是继续维护这条链
//!
//! 项目所有者裁定「老存档去掉就好了」：项目尚未发布，此前累计的全部
//! 存档都是开发期产物，不存在需要保留兼容的真实玩家数据。继续维护
//! 这三步迁移意味着 `WorldState`/`Agent`/`Interior` 每新增一个字段，
//! 都要额外新增一份「形状变了」的镜像类型、一段手写字节编码测试夹具、
//! 一批断言迁移后字段取值的测试——这份维护成本此前已经把本文件撑到
//! 迁移函数落地前的 828 行，而它验证的全部内容在发布之后都不会再有
//! 真实存档用到。删除具体迁移函数、把
//! [`crate::save_file::CURRENT_SCHEMA_VERSION`] 重置为 1，是「项目尚未
//! 发布」这个阶段独有的窗口——发布之后再做同一件事就是真正的破坏性
//! 变更，会导致真实玩家的存档打不开，那时候才是这条链真正需要长期
//! 存在、需要为每次字段变更认真配一份迁移函数的时候。
//!
//! # 老存档现在会发生什么
//!
//! `save_file::migration_chain` 现在返回一条空链——不再有任何
//! 已注册的迁移路径。任何 `schema_version` 与当前唯一认识的版本不一致
//! 的存档都会在读档管线里被明确拒绝
//! （[`crate::load_error::LoadError::SchemaTooNew`]/
//! [`crate::load_error::LoadError::SchemaMigrationGap`]，见两者文档与
//! `crate::save_file` 模块「旧版本号存档被拒绝」一节的测试），不会被
//! 空链悄悄放行、也不会被当前版本的类型静默按错位的字段布局解析出
//! 一份看似合法实则损坏的数据。
//!
//! # 框架本身没有被删除
//!
//! [`crate::migration::Migration`] trait 与
//! [`crate::migration::MigrationChain`]，连同它们自己的测试（用与任何
//! 具体 `WorldState` 版本都无关的合成迁移函数验证「找到相邻/跳级路径」
//! 「找不到路径时报错」「单步失败时错误向上传播」等机制本身的正确性）
//! 原样保留在 [`crate::migration`]——发布之后真实存档需要升级时，新的
//! 迁移函数重新实现 [`crate::migration::Migration`]、往
//! `save_file::migration_chain` 里注册即可，不需要重新设计
//! 这套机制。
