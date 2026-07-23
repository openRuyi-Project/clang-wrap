/*
 * Real end-user application that:
 *   1. Links the static library `libmixed.a` and calls EVERY ONE of its
 *      three top-level symbols (native_part / ir_part1 / ir_part2).
 *   2. Also calls a per-source-file helper compiled independently:
 *        - helper_ir       (clang-wrap built)
 *        - helper_native   (gcc built)
 *   3. Uses libm by going through sin / cos / sqrt, so the executable has
 *      a real unresolved dependency on libm at link time.
 *   4. Installed via install-wrap into $(DESTDIR)$(BINDIR).
 */

#include <math.h>

extern int  native_part(int x);   /* libmixed.a · gcc module */
extern int  ir_part1(int x);      /* libmixed.a · clang-wrap module */
extern int  ir_part2(int x);      /* libmixed.a · clang-wrap module */
extern int  helper_ir(int x);     /* helper_ir.c   · clang-wrap */
extern long helper_native(long x);/* helper_native.c · gcc        */

int main(void) {
    /* Pull every libmixed.a symbol into the call graph. */
    int v = ir_part1(5);          /* 5 + 10 = 15 */
    v = ir_part2(v);              /* 15 - 4 = 11 */
    v = native_part(v);           /* 11 * 3 + 1 = 34 */

    /* Independent helpers (different compilation toolchains). */
    int  hi = helper_ir(7);
    long hn = helper_native(9);

    /* Real libm use — sin/cos/sqrt must be resolved by -lm at link time. */
    volatile double m = sin((double)v) + cos((double)v);
    volatile double r = sqrt(m * m + 1.0);

    /* Real libresolv use — res_query() must be resolved by -lresolv at
     * link time.  We hold its address in a static function pointer so
     * the symbol is referenced at runtime without ever invoking the
     * function (which would require a real DNS query answer buffer). */
    extern int res_query(const char *, int, int, unsigned char *, int);

    static void *(*volatile keep_resolv)(const char *, int, int, unsigned char *, int) =
        (void *)res_query;
    (void)keep_resolv;

    /* `volatile` keeps the optimiser from constant-folding the libm calls or
     * the helper return values away.  The actual exit code is fixed at 0 so
     * the test can assert on it deterministically; the side-effect on the
     * volatile sink plus the call graph prove every required symbol was
     * resolved at link time. */
    (void)hi; (void)hn; (void)r;
    return 0;
}
