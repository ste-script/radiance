#property description Pixels radiating from the center

fn main(uv: vec2<f32>) -> vec4<f32> {
    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);
    let c = textureSample(iChannelsTex[1], iSampler,  uv);
    let c2 = c * smoothstep(0., 0.2, iIntensity);
    return composite(fragColor, c2);
}
#buffershader
fn main(uv: vec2<f32>) -> vec4<f32> {
    let delta = iStep * iFrequency;
    let uvSample = (uv - 0.5) * (1.0 - delta) + 0.5;
    let fragColor = textureSample(iChannelsTex[1], iSampler,  clamp(uvSample, vec2<f32>(0.), vec2<f32>(1.)));
    let fragColor2 = fragColor * exp(-1. / 30.);
    let fragColor3 = select(fragColor2, vec4<f32>(0.), fragColor2.r < 0.02);
    let random = rand4(textureSample(iNoiseTex, iSampler,  uv) * iTime);
    let random2 = rand4(textureSample(iNoiseTex, iSampler,  uv) * iTime + 100.);
    let thresh = exp((iIntensity - 2.) * 4.);
    let fragColor4 = select(fragColor3, vec4<f32>(1.), random < thresh && random2 < thresh);
    return fragColor4;
}
