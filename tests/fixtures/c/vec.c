#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <string.h>
#include "util.h"
#include "../shared/compat.h"

#define BUF_SIZE 128

typedef struct Vec {
    double *data;
    size_t len;
    size_t cap;
} Vec;

typedef struct Allocator {
    void *(*alloc)(size_t size);
    void (*release)(void *ptr);
} Allocator;

typedef struct Backend Backend;

struct BackendOps {
    int (*flush)(Backend *b);
    void (*close)(Backend *b);
};

struct Backend {
    const struct BackendOps *ops;
    int fd;
};

typedef struct Handler {
    int (*on_event)(int code);
} Handler;

static size_t default_cap(void) {
    return 8;
}

static size_t grow_cap(size_t cap) {
    return cap < 8 ? 8 : cap * 2;
}

Vec *vec_new(const Allocator *a) {
    Vec *v = a->alloc(sizeof(Vec));
    v->data = NULL;
    v->len = 0;
    v->cap = default_cap();
    return v;
}

char *vec_to_string(const Vec *v) {
    char *buf = malloc(BUF_SIZE);
    size_t off = 0;
    for (size_t i = 0; i < v->len; i++) {
        off += snprintf(buf + off, BUF_SIZE - off, "%.2f,", v->data[i]);
    }
    return buf;
}

char **split_lines(const char *text, size_t *count) {
    char **lines = calloc(default_cap(), sizeof(char *));
    size_t n = 0;
    const char *cursor = text;
    while (*cursor != '\0') {
        if (*cursor == '\n') {
            lines[n++] = strndup(text, (size_t)(cursor - text));
            text = cursor + 1;
        }
        cursor++;
    }
    *count = n;
    return lines;
}

void log_msg(const char *fmt, ...) {
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
}

static int apply(int (*op)(int), int x) {
    return op(x);
}

static int apply_deref(int (*op)(int), int x) {
    return (*op)(x);
}

int dispatch(Handler h, int code) {
    if (h.on_event) {
        return h.on_event(code);
    }
    return -1;
}

int backend_flush(Backend *b) {
    if (b->ops->flush) {
        return b->ops->flush(b);
    }
    return 0;
}

size_t count_markers(const char *s) {
    size_t n = 0;
    size_t i = 0;
    while (s[i] != '\0') {
        if (s[i] == '#') {
            n++;
        }
        i++;
    }
    for (size_t j = 0; j < n; j++) {
        putchar('.');
    }
    i = 0;
    do {
        putchar('*');
        i++;
    } while (i < n);
    return n;
}

const char *status_name(int code) {
    switch (code) {
        case 0:
            return "ok";
        case 1:
            return "warn";
        default:
            return code < 0 ? "invalid" : "error";
    }
}

int main(int argc, char **argv) {
    (void)argv;
    Handler h = { .on_event = NULL };
    log_msg("rounded to %zu\n", grow_cap(default_cap()));
    printf("%s\n", status_name(dispatch(h, argc)));
    return apply(NULL, argc) + apply_deref(NULL, argc);
}
