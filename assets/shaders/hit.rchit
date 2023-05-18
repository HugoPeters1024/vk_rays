#version 460
#extension GL_EXT_buffer_reference2 : enable
#extension GL_EXT_ray_tracing : enable
#extension GL_EXT_nonuniform_qualifier : enable

#include "rand.glsl"
#include "common.glsl"

layout(set=1, binding=16) uniform sampler2D textures[];

layout(location = 0) rayPayloadInEXT HitPayload payload;

hitAttributeEXT vec2 attribs;

layout(shaderRecordEXT, std430) buffer ShaderRecord
{
	VertexData v;
  IndexData  i;
  IndexOffsetData o;
  GeometryToTextureIdx gt;
};

void main()
{
  const vec3 barycentricCoords = vec3(1.0f - attribs.x - attribs.y, attribs.x, attribs.y);

  const uint index_offset = o.offsets[gl_GeometryIndexEXT];
  const uint texture_idx = gt.texture_ids[gl_GeometryIndexEXT];

  const Vertex v0 = v.vertices[i.indices[index_offset + gl_PrimitiveID * 3 + 0]];
  const Vertex v1 = v.vertices[i.indices[index_offset + gl_PrimitiveID * 3 + 1]];
  const Vertex v2 = v.vertices[i.indices[index_offset + gl_PrimitiveID * 3 + 2]];

  const vec3 normal = v0.normal * barycentricCoords.x + v1.normal * barycentricCoords.y + v2.normal * barycentricCoords.z;
  const vec2 uv = v0.uv * barycentricCoords.x + v1.uv * barycentricCoords.y + v2.uv * barycentricCoords.z;

  vec3 world_normal = normalize((gl_ObjectToWorldEXT * vec4(normal, 0.0)).xyz);

  payload.normal = world_normal;
  payload.t = gl_HitTEXT;
  payload.color = pow(texture(textures[texture_idx], uv).xyz, vec3(2.2));
  payload.emission = vec3(0);
  payload.roughness = 1.0f;
  payload.transmission = 0.0f;
  payload.refract_index = 1.05;
}

