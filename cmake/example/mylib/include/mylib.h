/*
 * MyLib - Example library for CMakeEmitLLVMIR test
 * Public header file
 */
#ifndef MYLIB_H
#define MYLIB_H

#ifdef __cplusplus
extern "C" {
#endif

/* Math operations */
int mylib_add(int a, int b);
int mylib_subtract(int a, int b);
int mylib_multiply(int a, int b);
int mylib_divide(int a, int b);

/* String operations */
int mylib_string_length(const char* str);
int mylib_string_compare(const char* s1, const char* s2);

/* Utility functions */
int mylib_factorial(int n);
int mylib_fibonacci(int n);
void mylib_print_version(void);

#ifdef __cplusplus
}
#endif

#endif /* MYLIB_H */
