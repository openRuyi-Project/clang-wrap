#include "libadd.h"

// Define versioned symbols
// Default version (LIBADD_1.0)
__asm__(".symver addf_impl,addf@@LIBADD_1.0");

// Implementation for version 1.0
float addf_impl(float a, float b) {
    return a + b;
}