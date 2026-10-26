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
