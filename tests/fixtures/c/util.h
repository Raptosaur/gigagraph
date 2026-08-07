#ifndef UTIL_H
#define UTIL_H

#include <stddef.h>

#define UTIL_VERSION "1.2.0"

struct Vec;

size_t util_hash(const char *key, size_t len);
char *util_strdup(const char *src);
int util_max(int a, int b);

extern int util_verbose;

#endif
