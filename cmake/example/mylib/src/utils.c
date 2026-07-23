/*
 * MyLib - Utility functions implementation
 */
#include "mylib.h"
#include <stdio.h>

#define MYLIB_VERSION_MAJOR 1
#define MYLIB_VERSION_MINOR 0
#define MYLIB_VERSION_PATCH 0

int mylib_factorial(int n) {
    if (n <= 1) {
        return 1;
    }
    return n * mylib_factorial(n - 1);
}

int mylib_fibonacci(int n) {
    if (n <= 0) {
        return 0;
    }
    if (n == 1) {
        return 1;
    }
    return mylib_fibonacci(n - 1) + mylib_fibonacci(n - 2);
}

void mylib_print_version(void) {
    printf("MyLib version %d.%d.%d\n",
           MYLIB_VERSION_MAJOR,
           MYLIB_VERSION_MINOR,
           MYLIB_VERSION_PATCH);
}
