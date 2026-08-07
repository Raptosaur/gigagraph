#include <stdlib.h>

/* Triple-pointer return: function_declarator nested under three
 * pointer_declarator levels — the deepest form the query claims. */
static double ***alloc_cube(size_t n) {
    double ***cube = malloc(n * sizeof(double **));
    for (size_t i = 0; i < n; i++) {
        cube[i] = malloc(n * sizeof(double *));
        for (size_t j = 0; j < n; j++) {
            cube[i][j] = calloc(n, sizeof(double));
        }
    }
    return cube;
}
