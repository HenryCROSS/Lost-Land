// 精灵批渲染：一张单位四边形顶点缓冲 + 一份逐精灵实例缓冲，一次 draw
// call 画完一批精灵。实例的 position/size 已经是调用方按 Pivot、相机
// 换算好的离屏目标像素坐标，这个着色器只做「像素坐标 -> 裁剪空间」的
// 仿射变换与纹理采样，不掺任何世界逻辑。
//
// 逻辑分辨率写死为 640×360，必须与 crate::target::LOGICAL_WIDTH /
// LOGICAL_HEIGHT 保持一致。没有通过 immediate data（wgpu 30 的 push
// constant）传入，是因为那需要在设备上开启 Features::IMMEDIATES，而
// GpuContext::new（gpu.rs）没有开启该特性，改动它超出本任务范围；离屏
// 目标尺寸本身是全局固定常量、不会运行期变化，写死不算破坏可维护性。
//
// 但两处数字各写一份终究可能漂移，光靠这段注释约束不住——
// `batch.rs` 里的 `tests::着色器中的逻辑分辨率与Rust常量保持一致`
// 解析这个文件里的 `LOGICAL_WIDTH`/`LOGICAL_HEIGHT` 字面量并与 Rust 侧
// 常量断言相等，改这两行任何一个数字而忘了同步，这个测试会红。

const LOGICAL_WIDTH: f32 = 640.0;
const LOGICAL_HEIGHT: f32 = 360.0;

struct QuadVertex {
    @location(0) unit_pos: vec2<f32>,
}

struct SpriteInstance {
    @location(1) position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) uv_rect: vec4<f32>,
    @location(4) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(vertex: QuadVertex, instance: SpriteInstance) -> VertexOutput {
    // 单位四边形的 [0,1] 顶点先按实例的 size 缩放、position 平移，
    // 落到离屏目标的像素空间，再换算成裁剪空间：X 轴照常，Y 轴翻转，
    // 因为像素坐标系原点在左上、Y 向下，裁剪空间原点在中心、Y 向上。
    let pixel_pos = instance.position + vertex.unit_pos * instance.size;
    let ndc_x = pixel_pos.x / LOGICAL_WIDTH * 2.0 - 1.0;
    let ndc_y = 1.0 - pixel_pos.y / LOGICAL_HEIGHT * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    // uv_rect 是 (u, v, width, height) 的归一化矩形，单位四边形插值出
    // 矩形内部的采样坐标。
    out.uv = instance.uv_rect.xy + vertex.unit_pos * instance.uv_rect.zw;
    out.color = instance.color;
    return out;
}

@group(0) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(0) @binding(1)
var atlas_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // 颜色调制（受击变红、渐隐等）用逐分量相乘叠加到采样结果上。
    return textureSample(atlas_texture, atlas_sampler, in.uv) * in.color;
}
