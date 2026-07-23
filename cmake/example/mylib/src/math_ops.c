/*
 * MyLib - Math operations implementation
 */
#include "mylib.h"

int mylib_add(int a, int b) {
    return a + b;
}

int mylib_subtract(int a, int b) {
    return a - b;
}

int mylib_multiply(int a, int b) {
    return a * b;
}

int mylib_divide(int a, int b) {
    if (b == 0) {
        return 0;  /* Simple error handling */
    }
    return a / b;
}
