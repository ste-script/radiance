#property description Slide the screen left-to-right
#property frequency 1

fn main(uv: vec2<f32>) -> vec4<f32> {
    let deviation = iTime * iFrequency * 0.5 - 0.5;
    let uv2 = (uv - 0.5) * aspectCorrection;
    let x = abs((uv2.x + deviation + 1.5) % 2. - 1.) - 0.5;
    let uv3 = vec2<f32>(x, uv2.y);
    let uv4 = uv3 / aspectCorrection + 0.5;

    let oc = textureSample(iInputsTex[0], iSampler,  uv);
    let c = textureSample(iInputsTex[0], iSampler,  uv4);

    let oc2 = oc * (1. - smoothstep(0.1, 0.2, iIntensity));
    let c2 = c * smoothstep(0., 0.1, iIntensity);

    return composite(oc2, c2);
}
