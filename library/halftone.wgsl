#property description CMYK halftoning, like a printer
#property frequency 1

fn rgb2cmyk(rgb: vec3<f32>) -> vec4<f32> {
    let k = 1. - max(max(rgb.r, rgb.g), rgb.b);
    let cmy = (1. - rgb - k) / max(1. - k, 0.001);
    return vec4<f32>(cmy, k);
}

fn cmyk2rgb(cmyk: vec4<f32>) -> vec3<f32> {
    return (1. - cmyk.xyz) * (1. - cmyk.w);
}

fn grid(basis: mat2x2<f32>, cmykMask: vec4<f32>, uv: vec2<f32>, offset: vec2<f32>) -> vec3<f32> {
    let points = 300. * pow(2., -9. * iIntensity) + 5.;
    //let points = points / (0.7 + 0.3 * pow(defaultPulse, 2.));
    let r = 0.5 / points;

    let invBasis = inverse2(basis);

    let pt = (uv - 0.5) * aspectCorrection;

    let newCoord = round(pt * points * invBasis - offset) + offset;
    let colorCoord = newCoord / points * basis;
    let c = textureSample(iInputsTex[0], iSampler,  colorCoord / aspectCorrection + 0.5).rgb;
    let cmyk = rgb2cmyk(c);
    let cmyk2 = cmyk * (cmykMask);
    let cmykValue = dot(cmyk2, vec4<f32>(1.));
    let r2 = r * (sqrt(cmykValue));
    let cmyk3 = cmyk2 / (max(cmykValue, 0.001));
    let c2 = mix(vec3<f32>(1.), cmyk2rgb(cmyk3), 1. - smoothstep(r2 * 0.8, r2, length(pt - colorCoord)));
    return c2;
}

fn basis(t: f32) -> mat2x2<f32> {
    let t2 = t * pi / 180.;
    return mat2x2(cos(t2), sin(t2),
                 -sin(t2), cos(t2));
}

fn main(uv: vec2<f32>) -> vec4<f32> {
    let b1 = basis(15. / 180. * pi);
    let b2 = basis(75. / 180. * pi);
    let b3 = basis(0. / 180. * pi);
    let b4 = basis(45. / 180. * pi);

    let spin = iTime * iFrequency * pi * 0.25;
    let c = cos(spin);
    let s = sin(spin);
    let c2 = cos(-spin * 0.5);
    let s2 = sin(-spin * 0.5);

    let o1 = vec2<f32>(c, s) * 0.25;
    let o2 = vec2<f32>(s, -c) * 0.25;
    let o3 = vec2<f32>(-c, -s) * 0.25;
    let o4 = vec2<f32>(c2, s2) * 0.25;

    let c1_grid = grid(b1, vec4<f32>(1., 0., 0., 0.), uv, o1 + vec2<f32>(0.));
    let m1 = grid(b2, vec4<f32>(0., 1., 0., 0.), uv, o2 + vec2<f32>(0.));
    let y1 = grid(b3, vec4<f32>(0., 0., 1., 0.), uv, o3 + vec2<f32>(0.));
    let k1 = grid(b4, vec4<f32>(0., 0., 0., 1.), uv, o4 + vec2<f32>(0.));
    let c2_grid = grid(b1, vec4<f32>(1., 0., 0., 0.), uv, o1 + vec2<f32>(0.5));
    let m2 = grid(b2, vec4<f32>(0., 1., 0., 0.), uv, o2 + vec2<f32>(0.5));
    let y2 = grid(b3, vec4<f32>(0., 0., 1., 0.), uv, o3 + vec2<f32>(0.5));
    let k2 = grid(b4, vec4<f32>(0., 0., 0., 1.), uv, o4 + vec2<f32>(0.5));
    let total = vec3<f32>(1.);
    let total2 = min(total, c1_grid);
    let total3 = min(total2, m1);
    let total4 = min(total3, y1);
    let total5 = min(total4, k1);
    let total6 = min(total5, c2_grid);
    let total7 = min(total6, m2);
    let total8 = min(total7, y2);
    let total9 = min(total8, k2);
    let total10 = vec4<f32>(total9, 1.);

    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);
    let a = max(fragColor.a, max(total10.r, max(total10.g, total10.b)));
    let total11 = vec4<f32>(total10.rgb, a);
    return mix(fragColor, total11, smoothstep(0., 0.1, iIntensity));
}
