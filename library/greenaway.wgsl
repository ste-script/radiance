#property description Shift colors away from green (green is not a creative color)

fn main(uv: vec2<f32>) -> vec4<f32> {
    let fragColor = textureSample(iInputsTex[0], iSampler,  uv);
    let hsv = rgb2hsv(fragColor.rgb);
    let h = hsv.x;
    let parameter = iIntensity * pow(defaultPulse, 2.);
    let h2 = (h + 4. / 6.) % 1.0 - 3. / 6.;
    let h3 = h2 * ((1. - parameter / 3.));
    let h4 = (h3 - 1. / 6.) % 1.0;
    let rgb = hsv2rgb(vec3<f32>(h4, hsv.yz));
    return vec4<f32>(rgb, max(max(max(fragColor.a, rgb.r), rgb.g), rgb.b));
}
