#import <Foundation/Foundation.h>

@interface TelemetryBridge : NSObject

@property(nonatomic, strong) NSMutableArray *buffer;

- (void)reportAltitude:(nonnull NSNumber *)meters;
- (void)enqueue:(NSDictionary *)sample;
- (NSUInteger)pendingCount;
- (void)drain;

@end
