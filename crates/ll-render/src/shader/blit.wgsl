// 把离屏渲染目标放大 blit 到窗口 surface 的一小块子区域。
//
// 用全屏三角形而非四边形：三角形不需要顶点缓冲与索引缓冲，顶点坐标直接
// 从 vertex_index 算出，减少一次资源绑定与一次绘制状态切换。三角形的
// 三个顶点覆盖 [-1,3] 的裁剪空间范围，超出 [-1,1] 的部分交给光栅化器
// 自动裁掉，视口内看到的就是铺满全屏的一块矩形。
//
// 采样器必须是最近邻（见 target.rs 里 Sampler 的创建）：线性插值会在放大
// 时把像素边缘糊掉，这正是整数倍缩放要避免的瑕疵。

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((vertex_index << 1u) & 2u), f32(vertex_index & 2u));

    var out: VertexOutput;
    out.uv = uv;
    out.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

@group(0) @binding(0)
var source_texture: texture_2d<f32>;
@group(0) @binding(1)
var source_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source_texture, source_sampler, in.uv);
}
