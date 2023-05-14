#version 460
#extension GL_EXT_ray_tracing : enable

#include "common.glsl"

layout(location = 0) rayPayloadInEXT HitPayload payload;

void main()
{
  payload.t = 0.0;
  payload.color = abs(gl_WorldRayDirectionEXT);
  payload.emission = vec3(1) * 0.08;
}

