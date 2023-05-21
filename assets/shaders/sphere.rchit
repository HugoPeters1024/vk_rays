#version 460
#extension GL_EXT_buffer_reference : enable
#extension GL_EXT_ray_tracing : enable
#extension GL_EXT_nonuniform_qualifier : enable

#include "common.glsl"

layout(location = 0)rayPayloadInEXT HitPayload payload;

hitAttributeEXT vec3 spherePoint;

void main() {
  const vec3 center = vec3(0);
  vec3 normal = normalize(spherePoint - center);

  payload.inside = dot(normal, gl_ObjectRayDirectionEXT) > 0.0f;
  if (payload.inside) {
    normal = -normal;
  }

  const vec3 world_normal = normal; //normalize((gl_ObjectToWorldEXT * vec4(normal, 0.0)).xyz);



  payload.absorption = 1.5f;
  payload.color = vec4(1.0f);
  payload.t = gl_HitTEXT;
  payload.surface_normal = world_normal;
  payload.normal = world_normal;
  payload.emission = vec3(0.0);
  payload.metallic = 0.00f;
  payload.roughness = 0.00f;
  payload.refract_index = 1.33f;

  payload.transmission = 0.0f;
  payload.roughness = float(gl_InstanceID) / 10.0f;
  payload.metallic = 1.0f;
}
