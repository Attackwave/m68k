/* Built with m68k-linux-gnu-gcc -m68000 -msoft-float -Os -fno-pic
   -fomit-frame-pointer -fno-asynchronous-unwind-tables -S (GCC 15.2).
   Constructs GCC emits for the m68000: a switch table, saved registers,
   a static that another file has too, local commons, strings
   shared between files, a byte table GCC writes as .base64. */
extern int two(int);
extern int shared_counter;
int counter_common;
static int writer;
static int pending;
static const unsigned char table[] = { 0, 1, 2, 3, 200, 7, 9, 0x1b, 13, 10, 255, 128, 4, 5, 6 };
static const char *names[] = { "alpha", "beta", "gamma" };

static int classify(int c)
{
    switch (c) {
    case 0: return 11;
    case 1: return 23;
    case 2: return two(c);
    case 3: return 47;
    case 4: return 59;
    case 5: return writer;
    case 6: return 71;
    default: return -1;
    }
}

int one(int a, int b, int c)
{
    int i, sum = 0;
    for (i = 0; i < a; i++)
        sum += classify(i + b) * c + table[i % sizeof table];
    writer = sum;
    pending += sum & 0x7f;
    counter_common++;
    shared_counter += (signed char)sum;
    return sum + (names[sum & 1][0] == 'a');
}

const char *greeting(int n)
{
    return n ? "hello, world" : "world";
}
