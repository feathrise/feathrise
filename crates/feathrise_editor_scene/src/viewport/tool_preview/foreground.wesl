// 作者手柄由原拾取相机投影，在视口颜色上混合，不采样场景深度。
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tint: vec4<f32>,
}

@vertex
fn vertex(@location(0) position: vec2<f32>, @location(1) tint: vec4<f32>) -> VertexOutput {
    return VertexOutput(vec4<f32>(position, 0.0, 1.0), tint);
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.tint;
}
