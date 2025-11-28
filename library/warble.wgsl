#property description Makes the image warbly

fn main(uv: vec2<f32>) -> vec4<f32> {
    let newPt = (uv - 0.5) * aspectCorrection;

    let bins = max(iIntensity * 10., 1.);

    let newPtInt = floor(newPt * bins);
    let newPtFrac = fract(newPt * bins);
    let newPtFrac2 = newPtFrac * 2. - 1.;

    let parameter = iIntensity * pow(defaultPulse, 2.);

    let displacement = pow(abs(newPtFrac2), vec2<f32>(1. + 2. * parameter)) * sign(newPtFrac2);
    let newPt2 = (newPtInt + 0.5 * displacement + 0.5) / bins;

    return textureSample(iInputsTex[0], iSampler,  newPt2 / aspectCorrection + 0.5);
}
