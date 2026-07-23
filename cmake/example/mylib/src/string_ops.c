/*
 * MyLib - String operations implementation
 */
#include "mylib.h"

int mylib_string_length(const char* str) {
    int len = 0;
    if (str == 0) {
        return 0;
    }
    while (str[len] != '\0') {
        len++;
    }
    return len;
}

int mylib_string_compare(const char* s1, const char* s2) {
    if (s1 == 0 || s2 == 0) {
        return -1;
    }
    while (*s1 && (*s1 == *s2)) {
        s1++;
        s2++;
    }
    return *(unsigned char*)s1 - *(unsigned char*)s2;
}
