#include "shape.hpp"
#include "../util/format.hpp"
#include <cmath>
#include <sstream>

namespace geo {

static double unit_area(int sides) {
    return sides * 0.5;
}

Shape::~Shape() {
    // Nothing owned.
}

double Shape::area() const {
    return std::fabs(unit_area(sides_));
}

std::string Shape::label() const {
    std::ostringstream out;
    out << "shape/" << sides_;
    return out.str();
}

const char *shape_kind(const Shape &s) {
    switch (s.sides()) {
        case 3:
            return "triangle";
        case 4:
            return "quad";
        default:
            return s.sides() > 4 ? "poly" : "degenerate";
    }
}

}  // namespace geo

geo::Shape geo::Shape::unit() {
    return geo::Shape(3);
}
