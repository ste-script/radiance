#property description Shift the color in YUV space by rotating on the UV plane

fn main(uv: vec2<f32>) -> vec4<f32> {
    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);
    let yuv = rgb2yuv(demultiply(fragColor).rgb);

    let t = select(iIntensity * 2. * pi, iTime * iFrequency * pi, iFrequency != 0.);
    let u_v = yuv.gb * 2.;
    let u_v2 = u_v - 1.;
    let u_v3 = vec2<f32>(u_v2.x * cos(t) - u_v2.y * sin(t),
                        u_v2.x * sin(t) + u_v2.y * cos(t));
    let u_v4 = u_v3 + 1.;
    let u_v5 = u_v4 / 2.;

    let yuv2 = vec3<f32>(yuv.r, u_v5);

    return select(vec4<f32>(yuv2rgb(yuv2), 1.) * fragColor.a, mix(fragColor, vec4<f32>(yuv2rgb(yuv2), 1.) * fragColor.a, iIntensity), iFrequency != 0.);
}
