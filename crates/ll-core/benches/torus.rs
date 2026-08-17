//! 环面距离的性能基准。
//!
//! 距离计算会在视野、寻路、AI 目标选择中被每帧调用成千上万次，是最
//! 容易悄悄劣化的热点。基准的目的不是追求某个绝对数字，而是让后续
//! 改动引入的性能回归立刻可见。

use criterion::{Criterion, criterion_group, criterion_main};
use ll_core::torus::TorusSize;
use std::hint::black_box;

fn 切比雪夫距离基准(c: &mut Criterion) {
    let world = TorusSize::new(4096, 4096).expect("宽高均不为零");
    let a = world.wrap(10, 20);
    let b = world.wrap(4090, 4000);

    c.bench_function("chebyshev_across_seam", |bencher| {
        bencher.iter(|| world.chebyshev(black_box(a), black_box(b)))
    });
}

criterion_group!(benches, 切比雪夫距离基准);
criterion_main!(benches);
