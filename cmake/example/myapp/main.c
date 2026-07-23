/*
 * MyApp - Example application using MyLib
 */
#include "mylib.h"
#include <stdio.h>

int main(int argc, char* argv[]) {
    int a = 10, b = 5;

    printf("=== MyApp - CMakeEmitLLVMIR Test ===\n\n");

    mylib_print_version();

    printf("\n--- Math Operations ---\n");
    printf("a = %d, b = %d\n", a, b);
    printf("add(%d, %d) = %d\n", a, b, mylib_add(a, b));
    printf("subtract(%d, %d) = %d\n", a, b, mylib_subtract(a, b));
    printf("multiply(%d, %d) = %d\n", a, b, mylib_multiply(a, b));
    printf("divide(%d, %d) = %d\n", a, b, mylib_divide(a, b));

    printf("\n--- String Operations ---\n");
    const char* test_str = "Hello, LLVM IR!";
    printf("Length of \"%s\" = %d\n", test_str, mylib_string_length(test_str));

    const char* s1 = "abc";
    const char* s2 = "abd";
    printf("compare(\"%s\", \"%s\") = %d\n", s1, s2, mylib_string_compare(s1, s2));

    printf("\n--- Utility Functions ---\n");
    printf("factorial(5) = %d\n", mylib_factorial(5));
    printf("fibonacci(10) = %d\n", mylib_fibonacci(10));

    printf("\n=== Test Complete ===\n");

    return 0;
}
