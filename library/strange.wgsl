#property description Strange tentacles
#property frequency 1

fn main(uv: vec2<f32>) -> vec4<f32> {
    let normCoord = (uv - 0.5) * aspectCorrection;

    // Build a vector to use to sample perlin noise from
    // .xy ~ coordinate, .z ~ time + audio, .w ~ fixed integer per call to noise()
    let noise_input_xy = 5. * normCoord * iIntensity;
    let noise_input_z = iTime * iFrequency * 0.2;
    // + iAudioLevel * mix(0.2, 0.7, iIntensity);
    let noise_input = vec3<f32>(noise_input_xy, noise_input_z);

    // Sample perlin noise with many octaves
    let n = noise3(noise_input) - 0.1;
    let n2 = n + (noise3(2. * noise_input) - 0.5) * 0.5;
    let n3 = n2 + (noise3(4. * noise_input) - 0.5) * 0.25;
    let n4 = n3 + (noise3(8. * noise_input) - 0.5) * 0.125;
    let n5 = n4 + (noise3(16. * noise_input) - 0.5) * 0.0625;
    // Take the noise and create "ridges", call it `a`
    let n6 = n5 - 0.5;
    let n7 = n6 * mix(20.0, -4., iAudioLevel);
    let n8 = clamp(abs(n7), 0., 1.);
    let n9 = 1.0 - pow(n8, mix(1.0, 4.0, iAudioLevel));
    let a = n9;

    // Sample perlin noise with just a few octaves, we'll use it for color
    let noise_input2 = noise_input + vec3<f32>(0., 0., 1337.);
    let n10 = noise3(noise_input2) - 0.1;
    let n11 = n10 + ((noise3(2. * noise_input2) - 0.5) * 0.5);
    let n12 = n11 + ((noise3(4. * noise_input2) - 0.5) * 0.25);
    let n13 = clamp(n12, 0., 1.);
    let n14 = pow(n13, 2.0);
    let b = n14;

    // Sample perlin noise for distortion
    let noise_input3 = noise_input2 + vec3<f32>(0., 0., 9876.);
    let n15 = noise3(noise_input3) - 0.1;
    let n16 = n15 + (noise3(2. * noise_input3) - 0.5) * 0.5;
    let n17 = n16 + (noise3(4. * noise_input3) - 0.5) * 0.25;
    let n18 = n17 + (noise3(8. * noise_input3) - 0.5) * 0.125;

    // Mix the new noise with `a` to create fringing around the tentacles
    let c = n18 * mix(0.08, 0.4, a);
    // Distort (re-use `b` as angle)
    let uvNew = uv;
    let uNew = uvNew.x + (cos(b) * c);
    let vNew = uvNew.y + (sin(b) * c);
    let uvNew2 = mix(uv, vec2<f32>(uNew, vNew), iIntensity);
    let uvNew3 = clamp(uvNew2, vec2<f32>(0.), vec2<f32>(1.));

    // Make a black-ish/blue-ish color
    let color = vec4<f32>(0.05, 0.4, 0.5, 1.);
    let color_rgb = color.rgb * b;
    let color_rg = color_rgb.rg * (mix(0.7, 1.0, c));
    let color2 = vec4<f32>(color_rg, color_rgb.b, color.a);
    let color3 = color2 * a * smoothstep(0., 0.2, iIntensity);

    let under = textureSample(iInputsTex[0], iSampler,  uvNew3);
    return composite(under, color3);
}
