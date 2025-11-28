#property description Fileball in the center

fn main(uv: vec2<f32>) -> vec4<f32> {
    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);

    let normCoord = (uv - 0.5) * aspectCorrection;

    let noise_input = vec3<f32>(length(normCoord) * 3. - iTime, abs(atan2(normCoord.y, normCoord.x)), iTime * 0.3);
    let shift = vec2<f32>(noise3(noise_input), noise3(noise_input + 100.)) - 0.5;
    let shift2 = shift + (vec2<f32>(noise3(2. * noise_input), noise3(2. * noise_input + 100.)) - 0.5) * 0.5;
    let shift3 = shift2 + (vec2<f32>(noise3(4. * noise_input), noise3(4. * noise_input + 100.)) - 0.5) * 0.25;
    let shift4 = (iIntensity * 0.7 + 0.3) * shift3 * (0.3 + 0.7 * pow(defaultPulse, 2.));

    let normCoord2 = normCoord + shift4;
    let color = vec4<f32>(1., clamp(length(normCoord2) * 2., 0., 1.), 0., 1.0);
    let color2 = color * smoothstep(0.4, 0.5, (1. - length(normCoord2)));
    let color3 = color2 * smoothstep(0., 0.2, iIntensity);

    return composite(fragColor, color3);
}
