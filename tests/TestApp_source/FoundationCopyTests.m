/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#ifdef DEFINE_ME_WHEN_BUILDING_ON_MACOS
#import <Foundation/Foundation.h>
#else
#import "system_headers.h"
@interface NSObject (FoundationCopyTests)
- (id)copy;
@end
@interface NSHashTable : NSObject
- (id)initWithOptions:(NSUInteger)options capacity:(NSUInteger)capacity;
- (void)addObject:(id)object;
- (void)removeObject:(id)object;
- (void)removeAllObjects;
- (NSUInteger)count;
- (id)member:(id)object;
@end
@interface NSData (FoundationBase64Tests)
+ (id)dataWithBytes:(const void *)bytes length:(NSUInteger)length;
- (id)base64EncodedDataWithOptions:(NSUInteger)options;
- (id)initWithBase64EncodedData:(NSData *)data options:(NSUInteger)options;
- (id)initWithBase64EncodedString:(NSString *)string options:(NSUInteger)options;
- (NSUInteger)length;
- (const void *)bytes;
@end
@interface NSJSONSerialization : NSObject
+ (id)JSONObjectWithData:(NSData *)data options:(NSUInteger)options error:(id *)error;
@end
@interface NSRegularExpression : NSObject
+ (id)regularExpressionWithPattern:(id)pattern options:(NSUInteger)options error:(id *)error;
- (NSUInteger)numberOfMatchesInString:(id)string options:(NSUInteger)options range:(NSRange)range;
@end
#endif
#include <string.h>
#include <stdlib.h>

// Super and Super2 differ in where lookup starts. For ARM struct returns,
// the hidden result pointer must survive while objc_super* becomes self.
// https://github.com/apple-oss-distributions/objc4/blob/main/runtime/Messengers.subproj/objc-msg-arm.s
// https://github.com/touchHLE/touchHLE/blob/trunk/src/objc/messages.rs
typedef struct { float x, y, width, height; } SuperDispatchRect;
typedef struct { id receiver; Class lookupClass; } SuperDispatchInfo;
#ifndef DEFINE_ME_WHEN_BUILDING_ON_MACOS
extern void objc_msgSendSuper(void);
extern void objc_msgSendSuper_stret(void);
#else
#import <objc/message.h>
#endif
@interface SuperDispatchBase : NSObject
- (SuperDispatchRect)adjust:(SuperDispatchRect)rect offset:(float)offset marker:(unsigned)marker;
- (unsigned)dispatchValue;
@end
@implementation SuperDispatchBase
- (SuperDispatchRect)adjust:(SuperDispatchRect)rect offset:(float)offset marker:(unsigned)marker {
  rect.x += offset; rect.y += marker; rect.width += 3; rect.height += 4;
  return rect;
}
- (unsigned)dispatchValue { return 8; }
@end
@interface SuperDispatchMiddle : SuperDispatchBase
@end
@implementation SuperDispatchMiddle
- (SuperDispatchRect)adjust:(SuperDispatchRect)rect offset:(float)offset marker:(unsigned)marker {
  rect = [super adjust:rect offset:offset marker:marker];
  rect.width += 11;
  return rect;
}
- (unsigned)dispatchValue { return [super dispatchValue] + 10; }
@end
@interface SuperDispatchLeaf : SuperDispatchMiddle
@end
@implementation SuperDispatchLeaf
- (SuperDispatchRect)adjust:(SuperDispatchRect)rect offset:(float)offset marker:(unsigned)marker {
  rect = [super adjust:rect offset:offset marker:marker];
  rect.height += 17;
  return rect;
}
@end
int test_objc_super_struct_return(void) {
  SuperDispatchLeaf *object = [SuperDispatchLeaf new];
  SuperDispatchRect input = {1, 2, 3, 4};
  SuperDispatchRect result = [object adjust:input offset:3 marker:9];
  if (result.x != 4 || result.y != 11 || result.width != 17 || result.height != 25) return -1;
  SuperDispatchInfo info = {object, [SuperDispatchMiddle class]};
  SuperDispatchRect (*sendRect)(SuperDispatchInfo *, SEL, SuperDispatchRect, float, unsigned) =
#if defined(__arm__)
      (void *)objc_msgSendSuper_stret;
#else
      (void *)objc_msgSendSuper;
#endif
  result = sendRect(&info, @selector(adjust:offset:marker:), input, 3, 9);
  if (result.x != 4 || result.y != 11 || result.width != 17 || result.height != 8) return -2;
  unsigned (*sendValue)(SuperDispatchInfo *, SEL) = (void *)objc_msgSendSuper;
  if (sendValue(&info, @selector(dispatchValue)) != 18) return -3;
  [object release];
  return 0;
}

// NSAllocateObject must allocate zeroed instance and indexed storage without
// dispatching +alloc again. Custom +allocWithZone: implementations use it.
// https://developer.apple.com/documentation/foundation/nsallocateobject
// https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/foundation/ns_object.rs
extern id NSAllocateObject(Class cls, NSUInteger extraBytes, void *zone);
static unsigned allocation_calls;
@interface IndexedAllocationProbe : NSObject {
@public
  unsigned marker;
  unsigned char tail[32];
}
@end
@implementation IndexedAllocationProbe
+ (id)allocWithZone:(void *)zone {
  if (++allocation_calls > 1) return nil;
  return NSAllocateObject(self, 80, zone);
}
@end

int test_NSAllocateObject_custom_allocator(void) {
  allocation_calls = 0;
  IndexedAllocationProbe *object = [IndexedAllocationProbe alloc];
  if (!object || allocation_calls != 1) return -1;
  if (object->marker != 0) return -2;
  // The last ivar ends at the instance boundary on 32-bit iOS. Touch all
  // extra bytes to catch discarded indexed storage.
  unsigned char *extra = object->tail + sizeof(object->tail);
  for (unsigned i = 0; i < 80; i++) {
    if (extra[i] != 0) return -3;
    extra[i] = (unsigned char)(i + 1);
  }
  object->marker = 0x12345678;
  [object release];
  return 0;
}

// An immutable NSCopying result is owned by the caller and remains usable
// after the original expression's autorelease pool drains.
// https://developer.apple.com/documentation/foundation/nsregularexpression?language=objc
// https://developer.apple.com/documentation/foundation/nscopying?language=objc
int test_NSRegularExpression_copy_survives_pool(void) {
  NSAutoreleasePool *outer = [[NSAutoreleasePool alloc] init];
  NSAutoreleasePool *inner = [[NSAutoreleasePool alloc] init];
  NSRegularExpression *source = [NSRegularExpression regularExpressionWithPattern:@"cat"
      options:0 error:NULL];
  NSRegularExpression *copy = [source copy];
  [inner drain];
  int result = 0;
  if (copy == nil) result = -1;
  else if ([copy numberOfMatchesInString:@"cat dog cat" options:0
                                   range:NSMakeRange(0, 11)] != 2) result = -2;
  [copy release];
  [outer drain];
  return result;
}

// A retained result must survive draining the pool that performed the parse.
// https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/MemoryMgmt/Articles/mmRules.html
int test_NSJSONSerialization_retained_result_survives_pool(void) {
  const char *inputs[] = {"{}", "[]", "{\"part\":{\"name\":\"Body\"}}",
                          "[\"Body\",{\"name\":\"Slide\"}]"};
  NSAutoreleasePool *outer = [[NSAutoreleasePool alloc] init];
  int result = 0;
  for (unsigned options = 0; options <= 1 && !result; options++) {
    for (unsigned i = 0; i < 4 && !result; i++) {
      NSAutoreleasePool *inner = [[NSAutoreleasePool alloc] init];
      NSData *data = [NSData dataWithBytes:inputs[i] length:strlen(inputs[i])];
      id root = [[NSJSONSerialization JSONObjectWithData:data options:options
                                                   error:NULL] retain];
      [inner drain];
      unsigned expected = i < 2 ? 0 : i == 2 ? 1 : 2;
      if (root == nil || [root count] != expected) result = -(int)(1 + i + options * 4);
      if (!result && i == 2 &&
          ![[[root objectForKey:@"part"] objectForKey:@"name"] isEqual:@"Body"])
        result = -10;
      if (!result && i == 3 &&
          ![[[root objectAtIndex:1] objectForKey:@"name"] isEqual:@"Slide"])
        result = -11;
      [root release];
    }
  }
  [outer drain];
  return result;
}

static int copy_members_freed;
@interface HashTableCopyMember : NSObject
@end
@implementation HashTableCopyMember
- (void)dealloc {
  copy_members_freed++;
  [super dealloc];
}
@end

int test_NSHashTable_copy_is_independent(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSHashTable *table = [[NSHashTable alloc] initWithOptions:0 capacity:2];
  id first = [[HashTableCopyMember alloc] init];
  id second = [[NSObject alloc] init];
  [table addObject:first];
  NSHashTable *copy = [table copy];
  int result = 0;
  if (copy == table) result = -1;
  else if ([copy count] != 1 || [copy member:first] != first) result = -2;
  [table addObject:second];
  if (!result && ([table count] != 2 || [copy count] != 1)) result = -3;
  [copy removeObject:first];
  if (!result && ([table count] != 2 || [copy count] != 0)) result = -4;
  [copy release];
  [table release];
  [first release];
  [second release];
  [pool drain];
  return result;
}

int test_NSHashTable_copy_owns_members(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSHashTable *table = [[NSHashTable alloc] initWithOptions:0 capacity:1];
  id member = [[HashTableCopyMember alloc] init];
  copy_members_freed = 0;
  [table addObject:member];
  NSAutoreleasePool *inner = [[NSAutoreleasePool alloc] init];
  NSHashTable *copy = [table copy];
  [table removeAllObjects];
  [member release];
  [table release];
  [inner drain];
  int result = 0;
  if (copy_members_freed != 0) result = -1;
  else if ([copy count] != 1 || [copy member:member] != member) result = -2;
  [copy release];
  if (!result && copy_members_freed != 1) result = -3;
  [pool drain];
  return result;
}

int test_NSHashTable_copy_empty(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSHashTable *table = [[NSHashTable alloc] initWithOptions:0 capacity:0];
  NSHashTable *copy = [table copy];
  id member = [[NSObject alloc] init];
  [copy addObject:member];
  int result = copy != table && [table count] == 0 && [copy count] == 1 ? 0 : -1;
  [copy release];
  [table release];
  [member release];
  [pool drain];
  return result;
}

int test_NSData_base64_data(void) {
  static const struct {
    const unsigned char *input;
    unsigned length;
    const char *encoded;
  } cases[] = {
      {(const unsigned char *)"", 0, ""},
      {(const unsigned char *)"f", 1, "Zg=="},
      {(const unsigned char *)"fo", 2, "Zm8="},
      {(const unsigned char *)"foo", 3, "Zm9v"},
      {(const unsigned char *)"foobar", 6, "Zm9vYmFy"},
      {(const unsigned char *)"\000\377\020", 3, "AP8Q"},
  };
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  int result = 0;
  unsigned i;
  for (i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
    NSAutoreleasePool *inner = [[NSAutoreleasePool alloc] init];
    NSData *source = [NSData dataWithBytes:cases[i].input length:cases[i].length];
    NSData *encoded = [[source base64EncodedDataWithOptions:0] retain];
    [inner drain];
    unsigned length = (unsigned)strlen(cases[i].encoded);
    if (encoded == nil || [encoded length] != length ||
        (length && memcmp([encoded bytes], cases[i].encoded, length) != 0))
      result = -(int)(i + 1);
    [encoded release];
    if (result) break;
  }
  [pool drain];
  return result;
}

// https://developer.apple.com/documentation/foundation/nsdata/init(base64encodeddata:options:)
int test_NSData_base64_decode(void) {
  static const struct {
    const char *encoded;
    unsigned options;
    const unsigned char *decoded;
    unsigned length;
  } cases[] = {
    {"", 0, (const unsigned char *)"", 0},
    {"Zg==", 0, (const unsigned char *)"f", 1},
    {"Zm8=", 0, (const unsigned char *)"fo", 2},
    {"Zm9vYmFy", 0, (const unsigned char *)"foobar", 6},
    {"AP8Q", 0, (const unsigned char *)"\000\377\020", 3},
    {" Zm9v\r\nYmFy!", 1, (const unsigned char *)"foobar", 6},
    {"Zg==\n", 0, NULL, 0},
    {"Zg", 0, NULL, 0},
    {"Zg=", 1, NULL, 0},
    {"Z=g=", 1, NULL, 0},
    {"Zm9v-___", 0, NULL, 0},
  };
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  int result = 0;
  unsigned i;
  for (i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
    NSAutoreleasePool *inner = [[NSAutoreleasePool alloc] init];
    NSData *source = [NSData dataWithBytes:cases[i].encoded length:strlen(cases[i].encoded)];
    NSData *data = [[NSData alloc] initWithBase64EncodedData:source options:cases[i].options];
    NSString *string = [NSString stringWithUTF8String:cases[i].encoded];
    NSData *fromString = [[NSData alloc] initWithBase64EncodedString:string options:cases[i].options];
    [inner drain];
    if (cases[i].decoded == NULL) {
      if (data != nil || fromString != nil) result = -(int)(i + 1);
    } else if (data == nil || fromString == nil ||
               [data length] != cases[i].length || [fromString length] != cases[i].length ||
               (cases[i].length && (memcmp([data bytes], cases[i].decoded, cases[i].length) ||
                                   memcmp([fromString bytes], cases[i].decoded, cases[i].length)))) {
      result = -(int)(i + 1);
    }
    [data release];
    [fromString release];
    if (result) break;
  }
  [pool drain];
  return result;
}

int test_NSString_long_utf8(void) {
  // NSString's null-terminated initializer must preserve a full large input.
  // https://developer.apple.com/documentation/foundation/nsstring/init(utf8string:)
  const unsigned length = 131123;
  char *input = malloc(length + 1);
  memset(input, 'x', length);
  input[length - 1] = 'z';
  input[length] = 0;
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSString *string = [NSString stringWithUTF8String:input];
  const char *roundTrip = [string UTF8String];
  int result = string == nil || [string length] != length ||
      roundTrip == NULL || memcmp(roundTrip, input, length + 1) ? -1 : 0;
  [pool drain];
  free(input);
  return result;
}
