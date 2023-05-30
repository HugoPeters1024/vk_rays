#ifndef GLSL_COMMON
#define GLSL_COMMON

#extension GL_EXT_buffer_reference : enable
#extension GL_EXT_scalar_block_layout : require

const float PI = 3.141592653589793f;
const float INVPI = 1.0f / 3.141592653589793f;
const float EPS = 0.001f;

float max3(in vec3 v) { return max(v.x, max(v.y, v.z)); }

mat3 fromAxisAngle(vec3 axis, float angle)
{
    axis = normalize(axis);
    float s = sin(angle);
    float c = cos(angle);
    float oc = 1.0 - c;
    
    return mat3(oc * axis.x * axis.x + c,           oc * axis.x * axis.y - axis.z * s,  oc * axis.z * axis.x + axis.y * s,
                oc * axis.x * axis.y + axis.z * s,  oc * axis.y * axis.y + c,           oc * axis.y * axis.z - axis.x * s,
                oc * axis.z * axis.x - axis.y * s,  oc * axis.y * axis.z + axis.x * s,  oc * axis.z * axis.z + c);
}

vec3 alignToNormalZUP(in vec3 s, in vec3 normal)
{
  const vec3 up = vec3(0.0f, 0.0f, 1.0f);
  if (dot(up, normal) > 0.999f) {
    return s;
  } 

  if (dot(up, normal) < -0.999f) {
    return -s;
  }

  float angle = acos(dot(up, normal));
  vec3 axis = cross(up, normal);
  return s * fromAxisAngle(axis, angle);
}

struct HitPayload {
  float t;
  bool inside;
  vec4 color;
  vec3 surface_normal;
  vec3 normal;
  vec3 emission;
  float absorption;
  float metallic;
  float roughness;
  float transmission;
  float refract_index;
};


struct AABB {
  float minx;
  float miny;
  float minz;
  float maxx;
  float maxy;
  float maxz;
};

vec3 aabb_center(in AABB aabb) {
  return vec3((aabb.minx + aabb.maxx) * 0.5f, (aabb.miny + aabb.maxy) * 0.5f, (aabb.minz + aabb.maxz) * 0.5f);
}

// Assumes the AABB is equal in all dimensions
float aabb_radius(in AABB aabb) {
  return (aabb.maxx - aabb.minx) * 0.5f;
}

struct Vertex {
  vec3 pos;
  vec3 normal;
  vec2 uv;
};

struct GltfMaterial {
  vec4 diffuse_factor;
  uint diffuse_texture;
  uint normal_texture;
  float metallic_factor;
  float roughness_factor;
  uint metallic_roughness_texture;
  vec3 emissive_factor;
  uint emissive_texture;
};


layout (buffer_reference, scalar, buffer_reference_align = 8) readonly buffer VertexData {
  Vertex vertices[];
};

layout (buffer_reference, scalar, buffer_reference_align = 8) readonly buffer IndexData {
  uint indices[];
};

layout (buffer_reference, scalar, buffer_reference_align = 8) readonly buffer IndexOffsetData {
  uint offsets[];
};

layout (buffer_reference, scalar, buffer_reference_align = 8) readonly buffer MaterialData {
  GltfMaterial materials[];
};

layout(buffer_reference, scalar, buffer_reference_align = 8) readonly buffer SphereData
{
	AABB aabbs[];
};

layout (buffer_reference, scalar, buffer_reference_align = 8) readonly buffer UniformData {
  mat4 inverse_view;
  mat4 inverse_proj;
  uint entropy;
  uint should_clear;
  uint mouse_x;
  uint mouse_y;
  float exposure;
};

layout (buffer_reference, std430, buffer_reference_align = 16) buffer QueryData {
  float focal_distance;
};


#endif
