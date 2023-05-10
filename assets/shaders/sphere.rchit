#version 460
#extension GL_EXT_buffer_reference : enable
#extension GL_EXT_ray_tracing : enable
#extension GL_EXT_nonuniform_qualifier : enable

#include "common.glsl"

layout(location = 0)rayPayloadInEXT HitPayload payload;

layout(push_constant, std430) uniform Registers {
  UniformData un;
  VertexData vd;
  IndexData id;
  SphereData spheres;
} regs;

hitAttributeEXT vec3 spherePoint;

void main() {
  const AABB sphereAABB = regs.spheres.aabbs[0];
  const vec3 center = aabb_center(sphereAABB);

  const vec3 normal = normalize(spherePoint - center);


  payload.color = abs(spherePoint);
  payload.t = gl_HitTEXT;
  payload.normal = normal;
}
