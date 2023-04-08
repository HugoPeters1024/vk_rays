#version 460

layout(location = 0) in  vec2 uv;

layout (set=0, binding=0) uniform sampler2D test;

layout(location = 0) out vec4 oColor;

void main() {
    oColor = texture(test, uv);
}

