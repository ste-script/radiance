#property description From https://www.shadertoy.com/view/XssGD7

fn get_texture(uv: vec2<f32>) -> vec4<f32> {
    return textureSample(iInputsTex[0], iSampler,  uv);
}

fn main(uv: vec2<f32>) -> vec4<f32> {
	// Sobel operator
	let offx = onePixel.x;
	let offy = onePixel.y;
	let gx = vec4<f32>(0.0);
	let gy = vec4<f32>(0.0);
	let gx2 = gx + get_texture(uv + vec2<f32>(-offx, offy));
	let gy2 = gy + gx2;
	let gx3 = gx2 + 2.0*get_texture(uv + vec2<f32>(-offx, 0.));
	let t = get_texture(uv + vec2<f32>(-offx, -offy));
	let gx4 = gx3 + t;
	let gy3 = gy2 - t;
	let gy4 = gy3 + 2.0*get_texture(uv + vec2<f32>(0., offy));
	let gy5 = gy4 - 2.0*get_texture(uv + vec2<f32>(0., -offy));
	let t2 = get_texture(uv + vec2<f32>(offx, offy));
	let gx5 = gx4 - t2;
	let gy6 = gy5 + t2;
	let gx6 = gx5 - 2.0*get_texture(uv + vec2<f32>(offx, 0.));
	let t3 = get_texture(uv + vec2<f32>(-offx, offy));
	let gx7 = gx6 - t3;
	let gy7 = gy6 - t3;
	let grad = sqrt(gx7 * gx7 + gy7 * gy7);
    let grad_a = max(max(grad.r, grad.g), max(grad.b, grad.a));
    let grad2 = vec4<f32>(grad.xyz, grad_a);

    let original = textureSample(iInputsTex[0], iSampler,  uv);
    let parameter = iIntensity * pow(defaultPulse, 2.);
    let grad3 = grad2 * smoothstep(0., 0.5, parameter);
    let original2 = original * (1. - smoothstep(0.5, 1., parameter));

    return composite(original2, grad3);
}
