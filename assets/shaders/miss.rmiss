#version 460
#extension GL_EXT_ray_tracing : enable

#include "common.glsl"

layout(location = 0) rayPayloadInEXT HitPayload payload;

layout(set=0, binding=2) uniform sampler2D skybox;

void main()
{
  vec2 uv = vec2(
      atan(gl_WorldRayDirectionEXT.x, gl_WorldRayDirectionEXT.z)/(2 * PI),
      acos(gl_WorldRayDirectionEXT.y) / PI
  );
  uv.x += 0.8f;
  payload.t = 0.0;
  payload.emission = pow(min(texture(skybox, uv).rgb, vec3(1000)), vec3(2.2)) * 0.2;
//  payload.emission = vec3(1.7);
}

