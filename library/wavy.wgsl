#property description Rectilinear distortion
#property frequency 1

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = (uv - 0.5) * aspectCorrection;
    let shift = vec2<f32>(0.);
    let t = iTime * iFrequency + 1.;
    let shift2 = shift + cos(pi * normCoord) * sin(t * vec2<f32>(0.1, 0.13));
    let shift3 = shift2 + cos(pi * normCoord * 2.) * sin(t * vec2<f32>(0.33, -0.23)) / 2.;
    let shift4 = shift3 + cos(pi * normCoord * 3.) * sin(t * vec2<f32>(0.35, -0.53)) / 3.;
    let shift5 = shift4 + cos(pi * normCoord * 4.) * sin(t * vec2<f32>(-0.63, -0.20)) / 4.;
    let shift6 = shift5 + cos(pi * normCoord * 5.) * sin(t * vec2<f32>(-0.73, 0.44)) / 5.;
    let shift7 = shift6 + cos(pi * normCoord * 6.) * sin(t * vec2<f32>(-0.73, 0.74)) / 6.;
    let shift8 = shift7 + cos(pi * normCoord * 7.) * sin(t * vec2<f32>(-1.05, -0.52)) / 7.;
    let shift9 = shift8 + cos(pi * normCoord * 8.) * sin(t * vec2<f32>(1.45, -1.22)) / 8.;

    let shift10 = shift9 + sin(pi * normCoord * 5.) * sin(t * vec2<f32>(0.79, -0.47)) / 5.;
    let shift11 = shift10 + sin(pi * normCoord * 6.) * sin(t * vec2<f32>(0.33, 0.79)) / 6.;
    let shift12 = shift11 + sin(pi * normCoord * 7.) * sin(t * vec2<f32>(1.15, -0.53)) / 7.;
    let shift13 = shift12 + sin(pi * normCoord * 8.) * sin(t * vec2<f32>(-1.36, -1.12)) / 8.;

    let amount = 0.1 * iIntensity;

    return textureSample(iInputsTex[0], iSampler,  (normCoord + shift13 * amount) / aspectCorrection + 0.5);
}
