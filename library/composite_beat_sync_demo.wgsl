#property description Composite with beat sync animation
#property frequency 1
#property inputCount 2

fn main(uv: vec2<f32>) -> vec4<f32> {
    let l = textureSample(iInputsTex[0], iSampler, uv);
    let r = textureSample(iInputsTex[1], iSampler, uv);
    
    // Beat-sync pulse: pulsing in sync with audio beats
    let blend = pow(defaultPulse, 2.0);
    
    return composite(l, r * blend * iIntensity);
}
