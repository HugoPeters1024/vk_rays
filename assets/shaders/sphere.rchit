#version 460
#extension GL_EXT_buffer_reference : enable
#extension GL_EXT_ray_tracing : enable
#extension GL_EXT_nonuniform_qualifier : enable

layout(location = 0) rayPayloadInEXT Payload {
  vec3 normal;
  float t;
  vec3 color;
  uint id;
  vec3 emission;
} payload;

hitAttributeEXT vec3 spherePoint;

void main() {
  payload.color = abs(spherePoint);
  payload.t = gl_HitTEXT;
}
