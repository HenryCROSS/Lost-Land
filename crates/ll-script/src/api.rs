//! mod API 表面：脚本能拿到的能力分类。
//!
//! 每个子模块对应一类能力，见各自文档。内容注册相关的函数留给任务 7
//! 按需添加，不在这里预先造好（避免为还没有消费者的形状猜测接口）。

pub mod actor;
pub mod event;
pub mod handle;
pub mod intent;
pub mod log;
pub mod ordered;
pub mod query;
pub mod rng;
pub mod state;

#[cfg(test)]
mod tests {
    use crate::host::ScriptEngine;

    #[test]
    fn 脚本无法访问未注册的函数名() {
        // 确认排除清单里没注册的能力确实不可达：即使把 mod API 表面
        // 的四类函数全部注册好，脚本调用一个刻意拼造、从未注册过的
        // 函数名也必须得到 Err，而不是某种意外成功。
        // Arrange
        let mut engine = ScriptEngine::new();
        super::rng::register(&mut engine);
        super::query::register(&mut engine);

        // Act：Steel 在编译期就能判定这是个自由标识符（不同于「具名函数
        // 缺参」那种延迟到实际引用才报错的情况，见 ADR 0001「参数个数
        // 的真实语义」一节），因此这里直接断言 load_source 本身返回
        // Err，不需要再走一次 call_raw。
        let result = engine
            .load_source("(define (probe) (this-function-was-never-registered 1 2 3))".to_string());

        // Assert
        assert!(result.is_err());
    }
}
