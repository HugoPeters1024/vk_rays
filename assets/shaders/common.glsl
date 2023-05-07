#ifndef GLSL_COMMON
#define GLSL_COMMON
const float PI = 3.141592653589793f;
const float INVPI = 1.0f / 3.141592653589793f;
const float EPS = 0.001f;

float max3(in vec3 v) { return max(v.x, max(v.y, v.z)); }

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

uint g_seed = 22;

uint rand() {
    g_seed = rand_xorshift(g_seed);
    return g_seed;
}

float randf()
{
    g_seed = rand_xorshift(g_seed);
    return g_seed * 2.3283064365387e-10f;
}

float randf_seed(in uint seed) {
    return wang_hash(seed) * 2.3283064365387e-10f;
}


mat3 rotationMatrix(vec3 axis, float angle)
{
    axis = normalize(axis);
    float s = sin(angle);
    float c = cos(angle);
    float oc = 1.0 - c;
    
    return mat3(oc * axis.x * axis.x + c,           oc * axis.x * axis.y - axis.z * s,  oc * axis.z * axis.x + axis.y * s,
                oc * axis.x * axis.y + axis.z * s,  oc * axis.y * axis.y + c,           oc * axis.y * axis.z - axis.x * s,
                oc * axis.z * axis.x - axis.y * s,  oc * axis.y * axis.z + axis.x * s,  oc * axis.z * axis.z + c);
}

mat3 AlignToNormalM(in vec3 normal) {
    const vec3 w = normal;
    const vec3 u = normalize(cross((abs(w.x) > .1f ? vec3(0, 1, 0) : vec3(1, 0, 0)), w));
    const vec3 v = normalize(cross(w,u));
    return mat3(u, v, w);
}

vec3 alignSampleVector(in vec3 s, in vec3 up, in vec3 normal)
{
  if (dot(up, normal) > 0.999f) {
    return s;
  } 

  if (dot(up, normal) < -0.999f) {
    return -s;
  }

  float angle = acos(dot(up, normal));
  vec3 axis = cross(up, normal);
  return s * rotationMatrix(axis, angle);
}

vec3 AlignToNormal(in vec3 normal, in vec3 i) {
    return AlignToNormalM(normal) * i;
}


vec3 SampleHemisphereCosine()
{
    float r0 = randf();
    float r1 = randf();
    const float r = sqrt(r0);
    const float theta = 2.0f * PI * r1;
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



#endif
