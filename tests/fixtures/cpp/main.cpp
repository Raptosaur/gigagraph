#include "shape.hpp"
#include <cstdio>
#include <vector>

struct Timer {
    Timer() : ticks_(0) {}
    ~Timer() { std::printf("timer done\n"); }
    void tick() { ++ticks_; }
    long ticks_;
};

template <typename T>
T clamp_min(T value, T floor) {
    return value < floor ? floor : value;
}

static int total_sides(const std::vector<geo::Shape> &shapes) {
    int total = 0;
    for (const auto &s : shapes) {
        total += s.sides();
    }
    return total;
}

static void describe(const geo::Shape *shape) {
    if (shape->sides() > 3) {
        std::printf("polygon\n");
    }
}

int apply_op(int (*op)(int, int), int a, int b) {
    return (*op)(a, b);
}

int main() {
    Timer timer;
    std::vector<geo::Shape> shapes;
    shapes.push_back(geo::Shape::unit());
    shapes.push_back(geo::Shape(4));
    describe(&shapes.front());
    timer.tick();
    int sides = clamp_min(total_sides(shapes), 0);
    sides = clamp_min<int>(sides, 3);
    geo::Shape *heap = new geo::Shape(5);
    while (sides > 0) {
        --sides;
    }
    std::printf("%d %d\n", heap->sides(), sides);
    delete heap;
    return 0;
}
