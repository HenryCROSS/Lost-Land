// 贴图矩形批渲染：HUD 皮肤层的真实九宫格边框/条形贴图（`NineSliceSkin`）
// 用这个着色器画——采样 `ll-render` 打包出的同一张图集，分辨率走运行期
// uniform（不像 sprite.wgsl 写死 640x360),理由与 `quad.wgsl` 一致：本
// 通道画在窗口 surface 的原生分辨率上,不是 640x360 逻辑分辨率的世界层
// 管线。

struct Resolution {
    size: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> resolution: Resolution;
@group(0) @binding(1)
var atlas_texture: texture_2d<f32>;
@group(0) @binding(2)
var atlas_sampler: sampler;

struct QuadVertex {
    @location(0) unit_pos: vec2<f32>,
}

struct TexturedQuadInstance {
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
fn vs_main(vertex: QuadVertex, instance: TexturedQuadInstance) -> VertexOutput {
    let pixel_pos = instance.position + vertex.unit_pos * instance.size;
    let ndc_x = pixel_pos.x / resolution.size.x * 2.0 - 1.0;
    let ndc_y = 1.0 - pixel_pos.y / resolution.size.y * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = instance.uv_rect.xy + vertex.unit_pos * instance.uv_rect.zw;
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(atlas_texture, atlas_sampler, in.uv) * in.color;
}
