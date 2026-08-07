#pragma once

class Shape {
public:
    double area() const { return width * height; }
    double perimeter() const;

private:
    double width = 1.0;
    double height = 1.0;
};
