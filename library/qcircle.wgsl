#property description Big purple soft circle 
#property frequency 0.5

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = (uv - 0.5) * aspectCorrection;
    let t = iTime * iFrequency * pi / 4.;
    let center = vec2<f32>(sin(t), cos(t));
    let center2 = center * (0.5);

    let a = clamp(length(center2 - normCoord), 0., 1.);
    let a2 = pow(a, 2.);
    let a3 = 1.0 - a2;
    let a4 = a3 * (iIntensity);

    let c = vec4<f32>(0.2, 0.1, 0.5, 1.) * a4;

    return composite(textureSample(iInputsTex[0], iSampler,  uv), c);
}
