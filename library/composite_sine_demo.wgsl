#property description Composite with sine wave animation
#property frequency 1
#property inputCount 2

fn main(uv: vec2<f32>) -> vec4<f32> {
    let l = textureSample(iInputsTex[0], iSampler, uv);
    let r = textureSample(iInputsTex[1], iSampler, uv);
    
    // Sine wave oscillation: automatically animates based on frequency
    let oscillate = sin(iTime * pi * iFrequency) * 0.5 + 0.5;
    
    return composite(l, r * oscillate * iIntensity);
}
