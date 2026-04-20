#property description Composite with ramp animation
#property inputCount 2

fn main(uv: vec2<f32>) -> vec4<f32> {
    let l = textureSample(iInputsTex[0], iSampler, uv);
    let r = textureSample(iInputsTex[1], iSampler, uv);
    
    // Smooth ramp: slowly accumulates over time
    let ramp = fract(iIntensityIntegral / 1024.0);
    
    return composite(l, r * ramp);
}
