#include "shape.hpp"
#include <cmath>

double Shape::perimeter() const {
    return 2.0 * (std::fabs(1.0) + std::fabs(1.0));
}

double describe(const Shape& s) {
    return s.area() + s.perimeter();
}
