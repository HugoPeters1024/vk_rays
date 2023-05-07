#version 460
#extension GL_EXT_ray_tracing : enable

layout(location = 0) rayPayloadInEXT Payload {
  vec3 normal;
  float t;
  vec3 color;
  float padding;
  vec3 emission;
} payload;

void main()
{
  payload.t = 0.0;
  payload.color = vec3(0.03, 0.03, 0.05) * 0.01;
  payload.emission = vec3(0);
}

