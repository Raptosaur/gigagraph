#import <XCTest/XCTest.h>

#import "../Sources/TelemetryBridge.h"

@interface TelemetryBridgeTests : XCTestCase
@property(nonatomic, strong) TelemetryBridge *bridge;
@end

@implementation TelemetryBridgeTests

- (void)setUp {
    [super setUp];
    self.bridge = [[TelemetryBridge alloc] init];
}

- (void)tearDown {
    self.bridge = nil;
    [super tearDown];
}

- (void)testPendingCountStartsAtZero {
    XCTAssertEqual([self.bridge pendingCount], 0);
}

- (void)testEnqueueIncrementsPendingCount {
    [self.bridge enqueue:@{@"altitude": @1}];
    XCTAssertEqual([self.bridge pendingCount], 1);
}

- (TelemetryBridge *)makeBridge {
    return [[TelemetryBridge alloc] init];
}

@end
