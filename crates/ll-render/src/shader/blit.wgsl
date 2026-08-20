// 把离屏渲染目标放大 blit 到窗口 surface 的一小块子区域。
//
// 用全屏三角形而非四边形：三角形不需要顶点缓冲与索引缓冲，顶点坐标直接
// 从 vertex_index 算出，减少一次资源绑定与一次绘制状态切换。三角形的
// 三个顶点覆盖 [-1,3] 的裁剪空间范围，超出 [-1,1] 的部分交给光栅化器
// 自动裁掉，视口内看到的就是铺满全屏的一块矩形。
//
// 采样方式由 BlitParams.sharp_bilinear 在运行期二选一（见 target.rs
// 的 BlitFilter）：
//
// - 最近邻（sharp_bilinear = 0.0）：source_sampler 绑定的是最近邻采样器，
//   fs_main 直通采样。整数倍缩放下恒锐利；非整数倍下会让相邻像素被
//   放大成宽窄不一的方块、画面轻微抖动闪烁。
// - 锐利双线性（sharp_bilinear = 1.0）：source_sampler 绑定的是线性
//   采样器，fs_main 先把 UV 重映射到「纹素内部保持平坦、只在纹素边界
//   过渡」的坐标再采样——任意倍率下像素边缘依然锐利，同时消除最近邻
//   在非整数倍下的不均匀瑕疵。算法是公开发表的 sharp-bilinear-simple
//   着色器（libretro shaders 项目，公有领域），下方 sharp_bilinear_uv
//   函数是其 WGSL 移植。
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

struct BlitParams {
    // 离屏画面放大到窗口的连续缩放倍率，恒 ≥ 1.0（见 target.rs 的
    // fit_viewport）——sharp_bilinear_uv 用它算出每个纹素在目标上有
    // 多宽，从而决定平滑过渡区该占多大比例。
    scale: f32,
    // 0.0 = 最近邻直通采样，1.0 = 启用下方的锐利双线性重映射。
    sharp_bilinear: f32,
    _padding: vec2<f32>,
}
@group(0) @binding(2)
var<uniform> params: BlitParams;

// 与 crate::target::LOGICAL_WIDTH/LOGICAL_HEIGHT 保持一致，一致性由
// target.rs 的单测 `着色器中的逻辑分辨率与rust常量保持一致`（同一套
// 字符串解析手法）锁住，理由见 batch.rs::sprite.wgsl 同名常量的测试
// 文档——两个数字谁改了都不会有编译错误，只会让采样坐标全部算错。
const SOURCE_WIDTH: f32 = 640.0;
const SOURCE_HEIGHT: f32 = 360.0;

// sharp-bilinear-simple 算法（libretro shaders 项目，公有领域）的 WGSL
// 移植：把归一化 UV 换算成纹素坐标，纹素中心附近 `region_range` 半径内
// 保持原始子像素位置不变（纹素内部平坦），只在超出这个范围、逼近纹素
// 边界的窄带内把子像素位置按 `scale` 压缩（边界处平滑过渡到相邻纹素）。
// `region_range = 0.5 - 0.5/scale` 是这套算法的核心：`scale` 越大，
// 平滑过渡带相对纹素的占比越窄，`scale` 恰为整数时过渡带宽度趋近于
// 一个目标像素的宽度，观感上与纯最近邻几乎无区别，这正是「任意倍率
// 下像素边缘依然锐利」的由来。
fn sharp_bilinear_uv(uv: vec2<f32>) -> vec2<f32> {
    let texture_size = vec2<f32>(SOURCE_WIDTH, SOURCE_HEIGHT);
    let texel = uv * texture_size;
    let texel_floor = floor(texel);
    let sub_texel = fract(texel);
    let region_range = vec2<f32>(0.5 - 0.5 / params.scale);
    let center_dist = sub_texel - vec2<f32>(0.5);
    let clamped = clamp(center_dist, -region_range, region_range);
    let f = (center_dist - clamped) * params.scale + vec2<f32>(0.5);
    return (texel_floor + f) / texture_size;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if params.sharp_bilinear < 0.5 {
        return textureSample(source_texture, source_sampler, in.uv);
    }
    return textureSample(source_texture, source_sampler, sharp_bilinear_uv(in.uv));
}
