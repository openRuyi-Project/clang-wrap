/* Compiled with clang-wrap — produces both .o and the .bc sidecar. */
int helper_ir(int x) {
    /* Deliberately non-trivial so the optimiser cannot collapse it away. */
    int acc = x;
    for (int i = 0; i < 8; ++i) {
        acc = (acc * 3 + (i & 1)) ^ (acc >> 1);
    }
    return acc;
}
