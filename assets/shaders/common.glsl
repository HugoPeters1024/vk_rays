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

vec3 AlignToNormalZUp(in vec3 v, in vec3 normal) {
  const vec3 UP = vec3(0,0,1);

  const float up_dot_normal = dot(UP, normal);

  if (up_dot_normal > 0.999) {
    return v;
  } else if (up_dot_normal < -0.999) {
    return -v;
  }


  const float angle = acos(dot(UP, normal));
  const vec3 axis = cross(UP, normal);
  return rotationMatrix(axis, angle) * v;
}

vec3 SampleHemisphereCosine(in vec3 normal)
{
    float r0 = randf();
    float r1 = randf();
    const float r = sqrt(r0);
    const float theta = 2.0f * PI * r1;
    const float x = r * cos(theta);
    const float y = r * sin(theta);
    const vec3 ret = vec3(x, y, sqrt(1.0f - r0));
    return AlignToNormalZUp(normal, ret);
}



#endif
