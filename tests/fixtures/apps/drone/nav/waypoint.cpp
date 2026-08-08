#include "waypoint.hpp"

#include <cmath>
#include <sstream>

namespace nav {

double Waypoint::distanceTo(const Waypoint &other) const {
    return haversine(lat, lon, other.lat, other.lon);
}

std::string Waypoint::describe() const {
    std::ostringstream out;
    out << lat << "," << lon << "@" << alt;
    return out.str();
}

Route::Route(std::string name) : name_(std::move(name)) {}

void Route::append(const Waypoint &wp) {
    points_.push_back(wp);
}

bool Route::empty() const noexcept {
    return points_.empty();
}

double Route::length() const {
    double total = 0.0;
    for (size_t i = 1; i < points_.size(); ++i) {
        total += points_[i - 1].distanceTo(points_[i]);
    }
    return total;
}

Route Route::parse(const std::string &encoded) {
    Route route("parsed");
    std::istringstream in(encoded);
    double lat = 0, lon = 0;
    while (in >> lat >> lon) {
        route.append(Waypoint{lat, lon, 0.0});
    }
    return route;
}

double haversine(double lat1, double lon1, double lat2, double lon2) {
    constexpr double kR = 6371000.0;
    const double dLat = (lat2 - lat1) * M_PI / 180.0;
    const double dLon = (lon2 - lon1) * M_PI / 180.0;
    const double a = std::sin(dLat / 2) * std::sin(dLat / 2) +
                     std::cos(lat1 * M_PI / 180.0) * std::cos(lat2 * M_PI / 180.0) *
                         std::sin(dLon / 2) * std::sin(dLon / 2);
    return 2 * kR * std::atan2(std::sqrt(a), std::sqrt(1 - a));
}

} // namespace nav
