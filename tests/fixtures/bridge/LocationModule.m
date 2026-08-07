#import <React/RCTBridgeModule.h>

@implementation LocationModule

RCT_EXPORT_MODULE();

RCT_EXPORT_METHOD(locate:(NSString *)accuracy
                  resolver:(RCTPromiseResolveBlock)resolve
                  rejecter:(RCTPromiseRejectBlock)reject)
{
  resolve(@"ok");
}

@end
