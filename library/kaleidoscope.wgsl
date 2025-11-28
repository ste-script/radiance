#property description Mirror and repeat the pattern in a circle
#property frequency 0.5

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = 2. * (uv - 0.5);
    let normCoord2 = normCoord * (aspectCorrection);
    let r = length(normCoord2);
    let theta = atan2(normCoord2.y, normCoord2.x);

    let bins = iIntensity * 5. + 2.;
    let tStep = pi / bins;
    let theta2 = abs((theta + 10. * tStep) % (2. * tStep) - tStep);

    let theta3 = theta2 + (iTime * iFrequency * pi * 0.125) % pi;

    let newUV = r * vec2<f32>(cos(theta3), sin(theta3));
    let newUV2 = newUV * (0.707);
    let newUV3 = newUV2 / (aspectCorrection);
    let newUV4 = newUV3 * 0.5 + 0.5;

    return textureSample(iInputsTex[0], iSampler,  mix(uv, newUV4, smoothstep(0., 0.2, iIntensity)));
}
