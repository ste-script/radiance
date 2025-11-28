#property description Zoomin until you see subpixels

fn main(uv: vec2<f32>) -> vec4<f32> {
    let pt = (uv - 0.5) * aspectCorrection;
    let op = onePixel * 0.3;
    let factor = exp(-6. * iIntensity);
    let factor2 = factor / (0.7 + 0.3 * pow(defaultPulse, 2.));

    let coord = floor(pt * factor2 / op) * op;
    let f = fract(pt * factor2 / op);
    let c = textureSample(iInputsTex[0], iSampler,  coord + 0.5);

    let redSubpixel = box(vec2<f32>(0.0, 0.) + f * vec2<f32>(3.6, 1.2)) * vec4<f32>(c.r, 0., 0., c.r);
    let greenSubpixel = box(vec2<f32>(-1.2, 0.) + f * vec2<f32>(3.6, 1.2)) * vec4<f32>(0., c.g, 0., c.g);
    let blueSubpixel = box(vec2<f32>(-2.4, 0.) + f * vec2<f32>(3.6, 1.2)) * vec4<f32>(0., 0., c.b, c.b);

    let c2 = redSubpixel + greenSubpixel + blueSubpixel;
    let fragColor = textureSample(iInputsTex[0], iSampler,  (uv - 0.5) * factor2 + 0.5);
    let fragColor2 = mix(fragColor, c2, smoothstep(0.3, 0.6, iIntensity));
    return fragColor2;
}
