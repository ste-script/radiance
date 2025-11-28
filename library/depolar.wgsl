#property description Convert rings to vertical lines

fn main(uv: vec2<f32>) -> vec4<f32> {
    let angle  = (uv.y + 0.25 * iTime * iFrequency) * pi;
    let lengthFactor = 1.0; // sqrt(2.);
    let rtheta = uv.x * lengthFactor * vec2<f32>(sin(angle), -cos(angle));
    let rtheta2 = rtheta / aspectCorrection;
    let rtheta3 = (rtheta2 + 1.) / 2.;

    let uv2 = mix(uv, rtheta3, iIntensity);

    return textureSample(iInputsTex[0], iSampler, uv2);
}
