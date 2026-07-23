/* Compiled directly with gcc — no clang-wrap involvement, no .bc. */
long helper_native(long x) {
    /* Slow computation that gcc could constant-fold for trivial inputs but
     * still must keep because x is opaque. */
    long acc = x;
    for (long i = 1; i < 17; i += 2) {
        acc = (acc * (i + 1)) - (acc / (i + 2));
    }
    return acc;
}
