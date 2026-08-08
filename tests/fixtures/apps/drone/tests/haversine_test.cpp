#include <catch2/catch_test_macros.hpp>

#include "../nav/waypoint.hpp"

TEST_CASE("haversine returns zero for identical points", "[nav]") {
    REQUIRE(nav::haversine(1.0, 2.0, 1.0, 2.0) == 0.0);
}

TEST_CASE("haversine is symmetric", "[nav][math]") {
    const double a = nav::haversine(0.0, 0.0, 1.0, 1.0);
    const double b = nav::haversine(1.0, 1.0, 0.0, 0.0);
    REQUIRE(a == b);
}
