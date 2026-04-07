#include "libadd.h"

// Define versioned symbols
// Default version (LIBADD_1.0)
__asm__(".symver add_impl,add@@LIBADD_1.0");

// Implementation for version 1.0
int add_impl(int a, int b) {
    return a + b;
}