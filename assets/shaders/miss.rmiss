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
  payload.t = 0.0;
  payload.color = abs(gl_WorldRayDirectionEXT);
  payload.emission = texture(skybox, uv).rgb;
}

