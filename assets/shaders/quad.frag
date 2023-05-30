#version 460

#include "common.glsl"

layout(location = 0) in  vec2 uv;

layout (set=0, binding=0) uniform sampler2D test;

layout(push_constant, std430) uniform Registers {
  UniformData uniforms;
};

layout(location = 0) out vec4 oColor;

vec3 ACES(const vec3 x) {
    const float a = 2.51;
    const float b = 0.03;
    const float c = 2.43;
    const float d = 0.59;
    const float e = 0.14;
    return (x * (a * x + b)) / (x * (c * x + d) + e);
}

vec3 acesFilm(const vec3 x) {
    const float a = 2.51;
    const float b = 0.03;
    const float c = 2.43;
    const float d = 0.59;
    const float e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d ) + e), 0.0, 1.0);
}

// Uncharted 2 tone map
// see: http://filmicworlds.com/blog/filmic-tonemapping-operators/
vec3 toneMapUncharted2Impl(vec3 color)
{
  const float A = 0.15;
  const float B = 0.50;
  const float C = 0.10;
  const float D = 0.20;
  const float E = 0.02;
  const float F = 0.30;
  return ((color * (A * color + C * B) + D * E) / (color * (A * color + B) + D * F)) - E / F;
}

vec3 tonemapFilmic(const vec3 color) {
	vec3 x = max(vec3(0.0), color - 0.004);
	return (x * (6.2 * x + 0.5)) / (x * (6.2 * x + 1.7) + 0.06);
}

void main() {
    const float gamma = 2.2f;
    const float exposure = uniforms.exposure * uniforms.exposure;

    vec4 bufferVal = texture(test, uv);

    vec3 hdrColor = bufferVal.xyz / bufferVal.w;
    vec3 mapped = vec3(1.0) - exp(-hdrColor * exposure);
    // gamma correction
//    mapped = pow(mapped, vec3(1.0f / gamma));
    mapped = tonemapFilmic(mapped);

    oColor = vec4(mapped, 1.0f);
}

