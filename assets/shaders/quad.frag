#version 460

layout(location = 0) in  vec2 uv;

layout (set=0, binding=0) uniform sampler2D test;

layout(location = 0) out vec4 oColor;

vec3 ACES(const vec3 x) {
    const float a = 2.51;
    const float b = 0.03;
    const float c = 2.43;
    const float d = 0.59;
    const float e = 0.14;
    return (x * (a * x + b)) / (x * (c * x + d) + e);
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

void main() {
    const float gamma = 2.2f;
    const float exposure = 4.6;

    vec4 bufferVal = texture(test, uv);

    vec3 hdrColor = bufferVal.xyz / bufferVal.w;
    vec3 corrected = pow(vec3(1.0f) - exp(-hdrColor * exposure), vec3(1.0f / gamma));
    vec3 mapped = ACES(corrected);

    oColor = vec4(mix(mapped, corrected, 0.4), 1.0f);
}

