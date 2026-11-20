/* Built with m68k-linux-gnu-gcc -m68000 -msoft-float -Os -fno-pic
   -fomit-frame-pointer -fno-asynchronous-unwind-tables -S (GCC 15.2).
   The other half: a static of the same name as in one.c, the same
   strings, a 32-bit multiply and divide (libgcc calls), a struct copy. */
struct point { int x, y, z, w; char tag[24]; };
extern int one(int, int, int);
extern const char *greeting(int);
int shared_counter = 5;
static int writer = 3;
static struct point origin = { 1, 2, 3, 4, "origin" };

static int scale(struct point *p, int k)
{
    struct point q = origin;
    q.x *= k;
    q.y /= (k | 1);
    *p = q;
    return q.x + q.y + writer;
}

int two(int n)
{
    struct point p;
    const char *g = greeting(n);
    return one(n & 3, writer, p.x) + g[0] + (int)sizeof("world");
}
