#version 460
#extension GL_EXT_ray_tracing : enable

#include "common.glsl"

layout(location = 0) rayPayloadInEXT HitPayload payload;

void main()
{
  payload.t = 0.0;
  payload.color = vec3(0.03, 0.03, 0.05);
}

