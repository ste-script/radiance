#property description The walls are melting
#property frequency 1

fn main(uv: vec2<f32>) -> vec4<f32> {
    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);
    let c = textureSample(iChannelsTex[1], iSampler,  uv);
    let c2 = c * (smoothstep(0., 0.2, iIntensity));
    return composite(fragColor, c2);
}

#buffershader

fn main(uv: vec2<f32>) -> vec4<f32> {
    let c1 = textureSample(iInputsTex[0], iSampler,  uv);

    let perturb = sin(uv.yx * 10. + sin(vec2<f32>(iTime * iFrequency * 0.5, iTime * iFrequency * 0.75))); // Perturb a little to make the melting more wavy
    let perturb2 = perturb * (1. - smoothstep(0., 0.1, uv.y)); // Don't perturb near the top to avoid going off-texture

    let c2 = textureSample(iChannelsTex[1], iSampler,  uv + vec2<f32>(0., -0.01 * iIntensity) + 0.005 * iIntensity * perturb2);

    let fragColor = max(c1, c2); // Blend between the current frame and a slightly shifted down version of it using the max function
    let fragColor2 = max(fragColor - 0.002 - 0.02 * (1. - iIntensity), vec4<f32>(0.)); // Fade it out slightly

    let fragColor3 = fragColor2 * smoothstep(0., 0.1, iIntensity); // Clear back buffer when intensity is low
    return fragColor3;
}
