#version 460
#extension GL_EXT_buffer_reference : enable
#extension GL_EXT_ray_tracing : enable
#extension GL_EXT_nonuniform_qualifier : enable

#include "rand.glsl"
#include "common.glsl"

layout(push_constant, std430) uniform Registers {
  UniformData un;
  VertexData  vd;
  IndexData   id;
} regs;

layout(location = 0) rayPayloadInEXT HitPayload payload;

hitAttributeEXT vec2 attribs;

void main()
{
  const vec3 barycentricCoords = vec3(1.0f - attribs.x - attribs.y, attribs.x, attribs.y);

  const Vertex v0 = regs.vd.vertices[regs.id.indices[gl_PrimitiveID * 3 + 0]];
  const Vertex v1 = regs.vd.vertices[regs.id.indices[gl_PrimitiveID * 3 + 1]];
  const Vertex v2 = regs.vd.vertices[regs.id.indices[gl_PrimitiveID * 3 + 2]];
  const vec3 normal = v0.normal * barycentricCoords.x + v1.normal * barycentricCoords.y + v2.normal * barycentricCoords.z;

  vec3 world_normal = normalize((gl_ObjectToWorldEXT * vec4(normal, 0.0)).xyz);

  payload.normal = world_normal;
  payload.t = gl_HitTEXT;
  payload.color = vec3(0.7f, 0.7, 0.3);
  g_seed = wang_hash(gl_InstanceID);
  payload.color = SampleRandomColor();
  payload.emission = vec3(0);
  if (gl_InstanceID % 23 == 4) {
    payload.emission = vec3(1.0);
  }
  payload.roughness = 1.0f;
}

