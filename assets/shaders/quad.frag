#version 460

layout(location = 0) in  vec2 uv;

layout (set=0, binding=0) uniform sampler2D test;

layout(location = 0) out vec4 oColor;

void main() {
    const float gamma = 2.0f;
    const float exposure = 1.0;

    vec4 bufferVal = texture(test, uv);

    vec3 hdrColor = bufferVal.xyz / bufferVal.w;
    vec3 mapped = pow(vec3(1.0f) - exp(-hdrColor * exposure), vec3(1.0f / gamma));

    oColor = vec4(mapped, 1.0f);
}

