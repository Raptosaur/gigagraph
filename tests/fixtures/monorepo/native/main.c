#include <stdio.h>
#include "util.h"

static int scale(int v) {
    return clamp_int(v * 2, 0, 100);
}

int main(int argc, char **argv) {
    int v = scale(argc);
    printf("%d\n", v);
    return 0;
}
