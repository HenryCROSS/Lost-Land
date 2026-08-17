// 把离屏渲染目标放大 blit 到窗口 surface 的一小块子区域。
//
// 用全屏三角形而非四边形：三角形不需要顶点缓冲与索引缓冲，顶点坐标直接
// 从 vertex_index 算出，减少一次资源绑定与一次绘制状态切换。三角形的
// 三个顶点覆盖 [-1,3] 的裁剪空间范围，超出 [-1,1] 的部分交给光栅化器
// 自动裁掉，视口内看到的就是铺满全屏的一块矩形。
//
// 采样器必须是最近邻（见 target.rs 里 Sampler 的创建）：线性插值会在放大
// 时把像素边缘糊掉，这正是整数倍缩放要避免的瑕疵。
//
// fs_main 是直通采样，不做任何色彩空间转换——这依赖一个调用方必须维持
// 的隐含前提：source_texture（离屏目标，固定 target.rs::TARGET_FORMAT，
// 一个 sRGB 变体）采样时被 GPU 按 sRGB 语义自动解码成线性值，这个线性值
// 只有在写入的 color target（这个管线画的目的地，即窗口 surface）本身
// 也是 *Srgb 变体时，才会被 GPU 在写入时自动重新编码回 sRGB。若谁改了
// gpu.rs 里 surface 格式的选择逻辑、不再优先挑 sRGB 变体，这里的直通
// 采样就会失去这个前提，画面会整体偏暗——但改法不是在这里加手动 gamma
// 转换兜底：正常的 sRGB 路径会被牵连着变成双重转换，画面又会过亮，
// 比不管它更糟。正确的地方是 gpu.rs 选格式那一步。

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
