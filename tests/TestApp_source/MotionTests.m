/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. https://mozilla.org/MPL/2.0/ */
#import "SyncTester.h"
#include <CoreFoundation/CFRunLoop.h>
#include <pthread.h>
#include <unistd.h>
#include <math.h>

@interface NSOperationQueue : NSObject
+ (id)mainQueue;
+ (id)currentQueue;
- (void)setSuspended:(BOOL)value;
- (NSUInteger)operationCount;
- (void)waitUntilAllOperationsAreFinished;
- (void)addOperationWithBlock:(void (^)(void))block;
- (void)addOperation:(id)operation;
@end

@interface NSBlockOperation : NSObject
+ (id)blockOperationWithBlock:(void (^)(void))block;
- (void)addDependency:(id)operation;
- (void)setCompletionBlock:(void (^)(void))block;
@end

typedef struct { double x, y, z; } CMAcceleration;
@interface CMAccelerometerData : NSObject
- (CMAcceleration)acceleration;
- (double)timestamp;
@end
@interface CMMotionManager : NSObject
- (void)setAccelerometerUpdateInterval:(double)value;
- (void)startAccelerometerUpdatesToQueue:(NSOperationQueue *)queue
                           withHandler:(void (^)(CMAccelerometerData *, id))handler;
- (void)stopAccelerometerUpdates;
@end

// Core Motion must copy the handler, enqueue every sample on the supplied
// queue, and stop producing samples on stop. A suspended queue retains work.
// https://developer.apple.com/documentation/coremotion/cmmotionmanager/startaccelerometerupdates(to:withhandler:)
// https://developer.apple.com/documentation/foundation/operationqueue
static int motion_handler_check(BOOL main) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  CMMotionManager *manager = [[CMMotionManager alloc] init];
  NSOperationQueue *queue = main ? [[NSOperationQueue mainQueue] retain] : [[NSOperationQueue alloc] init];
  pthread_t caller = pthread_self();
  __block int count = 0, error = 0;
  __block double last = -1;
  [queue setSuspended:YES];
  [manager setAccelerometerUpdateInterval:0.01];
  [manager startAccelerometerUpdatesToQueue:queue withHandler:^(CMAccelerometerData *data, id failure) {
    CMAcceleration a = [data acceleration];
    if (!data || failure || [data timestamp] <= last || fabs(a.z + 1) > 0.01 ||
        [NSOperationQueue currentQueue] != queue ||
        !!pthread_equal(caller, pthread_self()) != !!main) error = 1;
    last = [data timestamp];
    count++;
  }];
  usleep(60000);
  int result = count != 0 || [queue operationCount] < 2 ? -1 : 0;
  [queue setSuspended:NO];
  if (main) {
    for (int i = 0; i < 20 && count < 3; i++)
      CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0);
  } else {
    usleep(60000);
  }
  [manager stopAccelerometerUpdates];
  if (main) {
    for (int i = 0; i < 10 && [queue operationCount]; i++)
      CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0);
  } else [queue waitUntilAllOperationsAreFinished];
  if (count < 3 || error) result = -2;
  int stopped = count;
  usleep(40000);
  if (count != stopped) result = -3;
  [queue release];
  [manager release];
  [pool drain];
  return result;
}
int test_CoreMotion_handler_background(void) { return motion_handler_check(NO); }
int test_CoreMotion_handler_main_queue(void) { return motion_handler_check(YES); }

// A queue must keep a copied stack block until asynchronous execution, and
// identify itself inside that operation. Previously these ran inline or were
// removed immediately when suspended.
static void enqueue_captured_block(NSOperationQueue *queue, int *result, pthread_t caller) {
  int captured = 37;
  [queue addOperationWithBlock:^{
    *result = [NSOperationQueue currentQueue] == queue &&
        !pthread_equal(caller, pthread_self()) ? captured : -1;
  }];
}
int test_NSOperationQueue_suspended_blocks(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSOperationQueue *queue = [[NSOperationQueue alloc] init];
  [queue setSuspended:YES];
  int value = 0;
  enqueue_captured_block(queue, &value, pthread_self());
  usleep(20000);
  int result = value != 0 || [queue operationCount] != 1 ? -1 : 0;
  [queue setSuspended:NO];
  [queue waitUntilAllOperationsAreFinished];
  if (value != 37) result = -2;
  [queue release];
  [pool drain];
  return result;
}

// Suspended queues must preserve dependencies and completion blocks through
// the submitting stack frame. Equal-priority ready work executes in order.
static void enqueue_dependent_blocks(NSOperationQueue *queue, int *order) {
  int first = 1, second = 2, completion = 3;
  NSBlockOperation *a = [NSBlockOperation blockOperationWithBlock:^{
    *order = *order * 10 + first;
  }];
  NSBlockOperation *b = [NSBlockOperation blockOperationWithBlock:^{
    *order = *order * 10 + second;
  }];
  [b addDependency:a];
  [b setCompletionBlock:^{ *order = *order * 10 + completion; }];
  [queue addOperation:b];
  [queue addOperation:a];
}
int test_NSOperationQueue_dependencies_and_completion(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSOperationQueue *queue = [[NSOperationQueue alloc] init];
  [queue setSuspended:YES];
  int order = 0;
  enqueue_dependent_blocks(queue, &order);
  [queue setSuspended:NO];
  [queue waitUntilAllOperationsAreFinished];
  int result = order == 123 ? 0 : -1;
  [queue release];
  [pool drain];
  return result;
}
