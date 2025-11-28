#property description Yellow blob that spins to the beat
#property frequency 1

// I think I ported this correctly; this effect is bad

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = (uv - 0.5) * aspectCorrection;
    let t = iFrequency * iTime * 0.0625 * pi;
    let center = vec2(sin(t), cos(t));
    let center2 = center * iAudioLevel * 0.9 + 0.1;

    let a = clamp(length(center2 - normCoord), 0., 1.);
    let a2 = pow(a, iAudioHi * 3. + 0.1);
    let a3 = 1.0 - a2;
    let a4 = a3 * iIntensity;
    let c = vec4(1., 1., 0., 1.) * a4;

    return composite(textureSample(iInputsTex[0], iSampler, uv), c);
}

