#property description Derivative of https://www.shadertoy.com/view/XssGD7, with tighter edges.

fn get_texture(uv: vec2<f32>, off: vec2<f32>, cor: vec2<f32>) -> vec4<f32> {
    return textureSample(iInputsTex[0], iSampler, uv + off * cor);
}

fn main(uv: vec2<f32>) -> vec4<f32> {
    // Sobel operator
    let off = max(0.0001, 8. * (1. - iIntensity * defaultPulse) + 1.);
    let cor = 1. / iResolution.xy;
    let o = vec3<f32>(-off, 0.0, off);
    let gx = vec4<f32>(0.0);
    let gy = vec4<f32>(0.0);
    let gx2 = gx + get_texture(uv, o.xz, cor);
    let gy2 = gy + gx2;
    let gx3 = gx2 + 2.0 * get_texture(uv, o.xy, cor);
    let t = get_texture(uv, o.xx, cor);
    let gx4 = gx3 + t;
    let gy3 = gy2 - t;
    let gy4 = gy3 + 2.0 * get_texture(uv, o.yz, cor);
    let gy5 = gy4 - 2.0 * get_texture(uv, o.yx, cor);
    let t2 = get_texture(uv, o.zz, cor);
    let gx5 = gx4 - t2;
    let gy6 = gy5 + t2;
    let gx6 = gx5 - 2.0 * get_texture(uv, o.zy,cor);
    let t3 = get_texture(uv, o.zx,cor);
    let gx7 = gx6 - t3;
    let gy7 = gy6 - t3;

    let grad = sqrt(gx7 * gx7 + gy7 * gy7);
    let grad2 = vec4<f32>(grad.xyz / sqrt(off), grad.a);
    let grad3 = vec4<f32>(grad2.xyz, max(max(grad2.r, grad2.g), max(grad2.b, grad2.a)));

    let original = textureSample(iInputsTex[0], iSampler, uv);

    return mix(original, grad3, smoothstep(0., 0.3, iIntensity));
}

