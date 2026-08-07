#ifndef SHAPE_HPP
#define SHAPE_HPP

#include <string>
#include <cstdint>

namespace geo {

class Shape {
public:
    explicit Shape(int sides) : sides_(sides) {}
    ~Shape();

    int sides() const { return sides_; }

    double scaled_area(double factor) const {
        double base = area();
        return factor > 0 ? base * factor : base;
    }

    virtual double area() const;
    std::string label() const;
    static Shape unit();

private:
    int32_t sides_;
};

}  // namespace geo

#endif  // SHAPE_HPP
