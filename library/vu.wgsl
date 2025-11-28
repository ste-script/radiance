#property description Blue vertical VU meter

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = (uv - 0.5) * aspectCorrection;
    let audio = vec3<f32>(iAudioLow, iAudioMid, iAudioHi);

    let audio2 = audio * (2. * iIntensity);
    let audio3 = audio2 * (0.5 + 0.5 * pow(defaultPulse, 2.));

    let draw = 1. - smoothstep(audio3 - onePixel.x, audio3, vec3(abs(normCoord.x)));
    let dLow = vec4<f32>(0.0, 0.0, 0.5, 1.0) * draw.x;
    let dMid = vec4(0.0, 0.0, 1.0, 1.0) * draw.y;
    let dHi  = vec4(0.3, 0.3, 1.0, 1.0) * draw.z;

    let d = composite(composite(dLow, dMid), dHi);
    let d2 = clamp(d, vec4<f32>(0.), vec4<f32>(1.));

    return composite(textureSample(iInputsTex[0], iSampler, uv), d2);
}
