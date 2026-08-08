#import "TelemetryBridge.h"

@implementation TelemetryBridge

+ (BOOL)requiresMainQueueSetup {
    return NO;
}

- (instancetype)init {
    self = [super init];
    if (self) {
        _buffer = [NSMutableArray array];
    }
    return self;
}

- (void)reportAltitude:(nonnull NSNumber *)meters {
    [self enqueue:@{@"altitude": meters}];
}

- (void)enqueue:(NSDictionary *)sample {
    [self.buffer addObject:sample];
}

- (NSUInteger)pendingCount {
    return self.buffer.count;
}

- (void)drain {
    [self.buffer removeAllObjects];
}

@end
