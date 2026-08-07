#import <Foundation/Foundation.h>
#import <React/RCTBridgeModule.h>
#import "PaymentsModule.h"
#include <stdio.h>
@import CoreLocation;

static NSString *const kEndpoint = @"https://api.example.com/charge";

static double clamp_amount(double amount) {
    return amount < 0 ? 0 : amount;
}

@interface PaymentsModule () <RCTBridgeModule>
@property (nonatomic, strong) NSString *token;
- (void)clearToken;
@end

@implementation PaymentsModule

RCT_EXPORT_MODULE();

RCT_EXPORT_METHOD(processPayment:(NSString *)amount
                  resolver:(RCTPromiseResolveBlock)resolve
                  rejecter:(RCTPromiseRejectBlock)reject)
{
    NSNumber *value = [self parseAmount:amount];
    if (value == nil) {
        reject(@"bad_amount", @"Could not parse", nil);
        return;
    }
    resolve(value);
}

RCT_EXPORT_METHOD(reset)
{
    [self clearToken];
}

- (NSNumber *)parseAmount:(NSString *)text {
    NSNumberFormatter *fmt = [[NSNumberFormatter alloc] init];
    [fmt setNumberStyle:NSNumberFormatterDecimalStyle];
    return [fmt numberFromString:text];
}

- (void)logAll:(NSArray *)items prefix:(NSString *)prefix {
    for (NSString *item in items) {
        NSLog(@"%@ %@", prefix, item);
    }
    [[NSNotificationCenter defaultCenter] postNotificationName:@"payments.logged"
                                                        object:self];
}

- (void)clearToken {
    if (self.token != nil) {
        [self logAll:@[] prefix:@"clear"];
    }
    printf("cleared %f", clamp_amount(-1.0));
}

+ (instancetype)shared {
    return [[PaymentsModule alloc] init];
}

@end
