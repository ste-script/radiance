#property description Repeating tiles

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = uv - 0.5;
    let bins = pow(2., 4. * iIntensity);
    let bins2 = bins / (mix(1., (0.7 + 0.3 * pow(defaultPulse, 2.)), smoothstep(0., 0.2, iIntensity)));
    let newUV = normCoord * bins2;
    let newUV2 = fract(newUV + 0.5) - 0.5;
    let fragColor = textureSample(iInputsTex[0], iSampler,  newUV2 + 0.5);
    return fragColor;
}
