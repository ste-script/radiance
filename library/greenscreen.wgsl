#property description Replace green parts of the first input with the second
#property inputCount 2
fn main(uv: vec2<f32>) -> vec4<f32> {
    let m = textureSample(iInputsTex[0], iSampler,  uv);
    let g = textureSample(iInputsTex[1], iSampler,  uv);

    let fragColor = m;

    // x is 1.0 in pure green areas and ~0.0 elsewhere
    let m2 = demultiply(m); // don't use alpha to detect green-ness
    let x = pow(clamp(m2.g - (m2.r + m2.b) * 3.0, 0.0, 1.0), 0.2);
    let x2 = x * m2.a; // Put alpha back in

    let parameter = iIntensity * pow(defaultPulse, 2.);
    return composite(fragColor, g * x2 * parameter);
}
