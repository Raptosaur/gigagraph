#pragma once

#include <string>
#include <vector>

namespace nav {

struct Waypoint {
    double lat;
    double lon;
    double alt;

    double distanceTo(const Waypoint &other) const;
    std::string describe() const;
};

class Route {
  public:
    explicit Route(std::string name);

    void append(const Waypoint &wp);
    bool empty() const noexcept;
    double length() const;
    const std::string &name() const { return name_; }

    static Route parse(const std::string &encoded);

  private:
    std::string name_;
    std::vector<Waypoint> points_;
};

double haversine(double lat1, double lon1, double lat2, double lon2);

} // namespace nav
