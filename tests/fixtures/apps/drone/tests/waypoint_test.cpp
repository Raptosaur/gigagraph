#include <gtest/gtest.h>

#include "../nav/waypoint.hpp"

using nav::Route;
using nav::Waypoint;

TEST(WaypointTest, DistanceToSelfIsZero) {
    Waypoint wp{51.5, -0.1, 0.0};
    EXPECT_DOUBLE_EQ(wp.distanceTo(wp), 0.0);
}

TEST(WaypointTest, DescribeIncludesAltitude) {
    Waypoint wp{1.0, 2.0, 3.0};
    EXPECT_NE(wp.describe().find("@3"), std::string::npos);
}

TEST(RouteTest, EmptyRouteHasZeroLength) {
    Route route("empty");
    EXPECT_TRUE(route.empty());
    EXPECT_DOUBLE_EQ(route.length(), 0.0);
}

class RouteFixture : public ::testing::Test {
  protected:
    void SetUp() override { route_.append(Waypoint{0, 0, 0}); }
    Route route_{"fixture"};
};

TEST_F(RouteFixture, AppendGrowsTheRoute) {
    EXPECT_FALSE(route_.empty());
}

static Route helperRoute() {
    return Route::parse("0 0 1 1");
}
