#version 460
#extension GL_EXT_ray_tracing : enable

layout(location = 0) rayPayloadInEXT Payload {
  vec3 normal;
  float t;
} payload;

void main()
{
  payload.t = 0.0;
}

