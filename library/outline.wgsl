#property description Apply black outline around edges
//#property author https://www.shadertoy.com/view/XssGD7 + zbanks

fn main(uv: vec2<f32>) -> vec4<f32> {
	// Sobel operator
	let o = vec3<f32>(-1., 0., 1.);
	let gx = vec4<f32>(0.0);
	let gy = vec4<f32>(0.0);
	let gx2 = gx + textureSample(iInputsTex[0], iSampler,  uv + o.xz * onePixel);
	let gy2 = gy + gx2;
	let gx3 = gx2 + 2.0*textureSample(iInputsTex[0], iSampler,  uv + o.xy * onePixel);
	let t = textureSample(iInputsTex[0], iSampler,  uv + o.xx * onePixel);
	let gx4 = gx3 + t;
	let gy3 = gy2 - t;
	let gy4 = gy3 + 2.0*textureSample(iInputsTex[0], iSampler,  uv + o.yz * onePixel);
	let gy5 = gy4 - 2.0*textureSample(iInputsTex[0], iSampler,  uv + o.yx * onePixel);
	let t2 = textureSample(iInputsTex[0], iSampler,  uv + o.zz * onePixel);
	let gx5 = gx4 - t2;
	let gy6 = gy5 + t2;
	let gx6 = gx5 - 2.0*textureSample(iInputsTex[0], iSampler,  uv + o.zy * onePixel);
	let t3 = textureSample(iInputsTex[0], iSampler,  uv + o.zx * onePixel);
	let gx7 = gx6 - t3;
	let gy7 = gy6 - t3;
	let grad = sqrt(gx7 * gx7 + gy7 * gy7);

    let black = clamp(1.0 - length(grad) * 0.9, 0., 1.);
    let black2 = pow(black, mix(1.0, 2.0, iIntensity));

    let c = textureSample(iInputsTex[0], iSampler,  uv);
    let rgb = c.rgb * (mix(1.0, black2, iIntensity * pow(defaultPulse, 2.)));
    return add_alpha(rgb, c.a);
}
