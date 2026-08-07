#import "geometry.h"
#include <math.h>

static double square(double x) {
    return x * x;
}

double geo_distance(double ax, double ay, double bx, double by) {
    return sqrt(square(bx - ax) + square(by - ay));
}

@implementation GeoBox

- (double)area {
    return fabs(self.width * self.height);
}

- (BOOL)contains:(double)x y:(double)y {
    if (x < self.minX || y < self.minY) {
        return NO;
    }
    return YES;
}

- (NSString *)describe {
    NSString *label = [NSString stringWithFormat:@"box %f", [self area]];
    do {
        label = [label uppercaseString];
    } while (0);
    return label;
}

@end
