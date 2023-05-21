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
  MaterialData m;
};

vec3 calc_tangent(in Vertex v0, in Vertex v1, in Vertex v2) {
  vec3 edge1 = v1.pos - v0.pos;
  vec3 edge2 = v2.pos - v0.pos;
  vec2 deltaUV1 = v1.uv - v0.uv;
  vec2 deltaUV2 = v2.uv - v0.uv;


  float denom = deltaUV1.x * deltaUV2.y - deltaUV2.x * deltaUV1.y;
  if (abs(denom) < 0.0001f) {
    return vec3(1.0, 0.0, 0.0);
  }

  vec3 tangent;
  float f = 1.0 / denom;
  tangent.x = f * (deltaUV2.y * edge1.x - deltaUV1.y * edge2.x);
  tangent.y = f * (deltaUV2.y * edge1.y - deltaUV1.y * edge2.y);
  tangent.z = f * (deltaUV2.y * edge1.z - deltaUV1.y * edge2.z);
  return normalize(tangent);
}

void main()
{
  vec3 barycentricCoords = vec3(1.0f - attribs.x - attribs.y, attribs.x, attribs.y);

  const uint index_offset = o.offsets[gl_GeometryIndexEXT];
  const GltfMaterial material = m.materials[gl_GeometryIndexEXT];

  const Vertex v0 = v.vertices[i.indices[index_offset + gl_PrimitiveID * 3 + 0]];
  const Vertex v1 = v.vertices[i.indices[index_offset + gl_PrimitiveID * 3 + 1]];
  const Vertex v2 = v.vertices[i.indices[index_offset + gl_PrimitiveID * 3 + 2]];


  vec3 surface_normal = normalize(cross(v1.pos - v0.pos, v2.pos - v0.pos));
  vec3 normal = normalize(
      v0.normal * barycentricCoords.x +
      v1.normal * barycentricCoords.y + 
      v2.normal * barycentricCoords.z
  );

  payload.inside = dot(surface_normal, gl_ObjectRayDirectionEXT) > 0;
  if (payload.inside) {
    surface_normal = -surface_normal;
    normal = -normal;
  }



  const vec2 uv = v0.uv * barycentricCoords.x + v1.uv * barycentricCoords.y + v2.uv * barycentricCoords.z;


  payload.t = gl_HitTEXT;

  if (material.diffuse_texture != 0xFFFFFFFF) {
    payload.color = pow(textureLod(textures[material.diffuse_texture], uv, 0).xyz, vec3(2.2));
  } else {
    payload.color = vec3(0.6);
  }

  payload.emission = material.emissive_factor;
  if (material.emissive_texture != 0xFFFFFFFF) {
    payload.emission *= textureLod(textures[material.emissive_texture], uv, 0).xyz;
  }



  if (material.normal_texture != 0xFFFFFFFF) {
    const vec3 tangent = calc_tangent(v0, v1, v2);
    const vec3 bitangent = normalize(cross(normal, tangent));
    mat3 TBN = mat3(tangent, bitangent, normal);

    // normalize due to linear filtering
    const vec3 tex_normal = textureLod(textures[material.normal_texture], uv, 0).xyz * 2.0 - 1.0;
    payload.normal = normalize(TBN * tex_normal);
  } else {
    payload.normal = normal;
  }

  payload.surface_normal = normalize((gl_ObjectToWorldEXT * vec4(surface_normal, 0.0)).xyz);
  payload.normal = normalize((gl_ObjectToWorldEXT * vec4(payload.normal, 0.0)).xyz);

  payload.absorption = 0;
  payload.roughness = 0.1f;
  payload.transmission = 0.0f;
  payload.refract_index = 1.05;

  if (gl_GeometryIndexEXT == 9) {
//    payload.emission = vec3(1.0, 0.8, 0.2) * 10;
  }

  payload.metallic = material.metallic_factor;
  payload.roughness = material.roughness_factor;
  if (material.metallic_roughness_texture != 0xFFFFFFFF) {
    vec2 roughness_and_metallic = textureLod(textures[material.metallic_roughness_texture], uv, 0).gb;
    payload.roughness *= roughness_and_metallic.x;
    payload.metallic *= roughness_and_metallic.y;
  }

  if (gl_GeometryIndexEXT == 8) {
    payload.transmission = 1.0f;
  }
}

