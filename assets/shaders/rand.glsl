#ifndef GLSL_RAND
#define GLSL_RAND

uint rand_xorshift(in uint seed)
{
    seed ^= (seed << 13);
    seed ^= (seed >> 17);
    seed ^= (seed << 5);
    return seed;
}

uint wang_hash(in uint seed)
{
    seed = (seed ^ 61) ^ (seed >> 16);
    seed *= 9;
    seed = seed ^ (seed >> 4);
    seed *= 0x27d4eb2d;
    seed = seed ^ (seed >> 15);
    return seed;
}

// https://www.shadertoy.com/view/cll3R4
uint other_hash(in uint seed)
{
  seed = seed*0x343fd+0x269ec3;
  return (seed>>16)&32767;
}

uint rand(inout uint seed) {
    seed = rand_xorshift(seed);
    return seed;
}

float randf(inout uint seed)
{
    seed = rand_xorshift(seed);
    return seed * 2.3283064365387e-10f;
}

vec3 SampleHemisphereCosine(inout uint seed)
{
    const float TWO_PI = 6.28318530718;
    float r0 = randf(seed);
    float r1 = randf(seed);
    const float r = sqrt(r0);
    const float theta = TWO_PI * r1;
    const float x = r * cos(theta);
    const float y = r * sin(theta);
    return vec3(x, y, sqrt(1.0f - r0));
}

vec3 hsv2rgb(vec3 c)
{
    vec4 K = vec4(1.0, 2.0 / 3.0, 1.0 / 3.0, 3.0);
    vec3 p = abs(fract(c.xxx + K.xyz) * 6.0 - K.www);
    return c.z * mix(K.xxx, clamp(p - K.xxx, 0.0, 1.0), c.y);
}

vec3 SampleRandomColor(inout uint seed)
{
  float h = randf(seed);
  float s = 0.5 + 0.5 * randf(seed);
  float v = 0.5 + 0.5 * randf(seed);
  return hsv2rgb(vec3(h, s, v));
}


#endif // GLSL_RAND
