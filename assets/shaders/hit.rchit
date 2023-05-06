#version 460
#extension GL_EXT_buffer_reference : enable
#extension GL_EXT_ray_tracing : enable
#extension GL_EXT_nonuniform_qualifier : enable

struct Vertex {
  vec3 pos;
  vec3 normal;
};

layout (buffer_reference, std430, buffer_reference_align = 16) buffer VertexData {
  Vertex vertices[];
};

layout (buffer_reference, std430, buffer_reference_align = 16) buffer IndexData {
  uint indices[];
};

layout (buffer_reference, std430, buffer_reference_align = 16) buffer UniformData {
  mat4 inverse_view;
  mat4 inverse_proj;
  uint entropy;
};

layout(push_constant, std430) uniform Registers {
  UniformData un;
  VertexData  vd;
  IndexData   id;
} regs;

layout(location = 0) rayPayloadInEXT Payload {
  vec3 normal;
  float t;
} payload;

hitAttributeEXT vec2 attribs;

void main()
{
  const vec3 barycentricCoords = vec3(1.0f - attribs.x - attribs.y, attribs.x, attribs.y);

  const Vertex v0 = regs.vd.vertices[regs.id.indices[gl_PrimitiveID * 3 + 0]];
  const Vertex v1 = regs.vd.vertices[regs.id.indices[gl_PrimitiveID * 3 + 1]];
  const Vertex v2 = regs.vd.vertices[regs.id.indices[gl_PrimitiveID * 3 + 2]];
  const vec3 normal = v0.normal * barycentricCoords.x + v1.normal * barycentricCoords.y + v2.normal * barycentricCoords.z;

  const vec3 world_normal = normalize((gl_ObjectToWorldEXT * vec4(normal, 0.0)).xyz);

  payload.normal = world_normal;
  payload.t = gl_HitTEXT;
}

