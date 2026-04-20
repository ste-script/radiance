#property description Mix between the two inputs
#property inputCount 2
fn main(uv: vec2<f32>) -> vec4<f32> {
    let l = textureSample(iInputsTex[0], iSampler, uv);
    let r = textureSample(iInputsTex[1], iSampler, uv);
    // Use full-range iIntensity so animation/manual control can reach pure input 0 or input 1.
    return mix(l, r, clamp(iIntensity, 0.0, 1.0));
}
