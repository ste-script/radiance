#property description Wrap the parent texture on a spinning cylinder
#property frequency 1

fn main(uv: vec2<f32>) -> vec4<f32> {
    let angle = (iTime * iFrequency - 0.5) * pi;
    let angle2 = angle + 2. * asin(2. * (uv.x - 0.5));

    let x = modulo(angle2 / pi, 2.0);
    let x2 = x - 1.0;
    let x3 = abs(x2);

    let new_uv = vec2(x3, uv.y);
    let new_uv2 = mix(uv, new_uv, iIntensity);
    return textureSample(iInputsTex[0], iSampler, new_uv2);
}
