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

vec3 SampleHemisphereCosine()
{
    const float TWO_PI = 6.28318530718;
    float r0 = randf();
    float r1 = randf();
    const float r = sqrt(r0);
    const float theta = TWO_PI * r1;
    const float x = r * cos(theta);
    const float y = r * sin(theta);
    return vec3(x, y, sqrt(1.0f - r0));
}

#endif // GLSL_RAND
