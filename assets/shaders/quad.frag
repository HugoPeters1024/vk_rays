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

void main() {
    const float gamma = 2.2f;
    const float exposure = 1.0;

    vec4 bufferVal = texture(test, uv);

    vec3 hdrColor = bufferVal.xyz / bufferVal.w;
    vec3 mapped = pow(vec3(1.0f) - exp(-hdrColor * exposure), vec3(1.0f / gamma));

    oColor = vec4(mapped, 1.0f);
}

