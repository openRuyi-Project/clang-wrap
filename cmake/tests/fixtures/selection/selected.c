#if !defined(FIXTURE_COMPILE_DATABASE) || !defined(FIXTURE_PER_SOURCE_OPTION)
#error "The LLVM IR compile must preserve target and per-source options."
#endif

int main(void) { return 0; }