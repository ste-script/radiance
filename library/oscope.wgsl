#property description Fake waveform visualizer that looks like an oscilloscope
#property frequency 1

fn main(uv: vec2<f32>) -> vec4<f32> {
    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);

    let normCoord = (uv - 0.5) * aspectCorrection;

    let x = normCoord.x;
    let wave = 0.;
    let wave2 = wave + 0.6 * sin(x * 10. + iTime * 1.) * iAudioLow;
    let wave3 = wave2 + 0.6 * sin(x * 15. + iTime * -0.3) * iAudioLow;
    let wave4 = wave3 + 0.2 * sin(x * 40. + iTime * 8.) * iAudioMid;
    let wave5 = wave4 + 0.2 * sin(x * 70. + iTime * -3.) * iAudioLow;
    let wave6 = wave5 + 0.1 * sin(x * 120. + iTime * 16.) * iAudioHi;
    let wave7 = wave6 + 0.1 * sin(x * 180. + iTime * -10.) * iAudioHi;
    let wave8 = wave7 * iAudioLevel;
    let wave9 = wave8 * smoothstep(0., 0.3, iIntensity);
    let wave10 = wave9 * (0.5 + 0.5 * pow(defaultPulse, 2.));

    let d = abs(normCoord.y - wave10);

    let glow = 1. - smoothstep(0., (0.02 + iAudioHi * 0.3) * smoothstep(0., 0.5, iIntensity), d);
    let glow2 = glow + (0.5 * (1. - smoothstep(0., (0.3 + iAudioHi * 0.3) * iIntensity, d)));
    let glow3 = glow2 * (0.7 + 0.3 * pow(defaultPulse, 0.5));
    let c = vec4<f32>(0., 1., 0., 1.) * glow3;
    return composite(fragColor, c);
}

