// 纯色矩形批渲染：状态栏/角色面板/背包/装备栏的面板背景与条形都是
// 这一个着色器画出来的填色矩形，不采样任何纹理——见
// `crate::widget::quad` 模块文档「为什么不复用 ll-render 的 SpriteBatch」
// 一节：这条通道与 `ll-text` 的文本通道一样，画在窗口原生分辨率的
// surface 视图上（blit 之后、不清屏），不是 640x360 逻辑分辨率的世界层
// 管线，因此分辨率必须是运行期 uniform，不能像 sprite.wgsl 那样写死
// 640x360 常量。

struct Resolution {
    size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> resolution: Resolution;

struct QuadVertex {
    @location(0) unit_pos: vec2<f32>,
}

struct QuadInstance {
    @location(1) position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(vertex: QuadVertex, instance: QuadInstance) -> VertexOutput {
    let pixel_pos = instance.position + vertex.unit_pos * instance.size;
    let ndc_x = pixel_pos.x / resolution.size.x * 2.0 - 1.0;
    let ndc_y = 1.0 - pixel_pos.y / resolution.size.y * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
