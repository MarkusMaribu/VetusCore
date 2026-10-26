/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
#ifdef DEFINE_ME_WHEN_BUILDING_ON_MACOS
#import <Foundation/Foundation.h>
#else
#import "system_headers.h"
@interface NSDictionary (DictionarySubclassTests)
+ (id)dictionaryWithObject:(id)object forKey:(id)key;
- (id)keyEnumerator;
@end
@interface NSObject (DictionarySubclassTests)
- (id)nextObject;
- (BOOL)isEqual:(id)object;
@end
@interface NSMutableDictionary : NSDictionary
- (void)setObject:(id)object forKey:(id)key;
- (void)removeObjectForKey:(id)key;
@end
#endif

// Apple's class-cluster subclasses keep their own identity and implement the
// documented dictionary primitives using a separate backing dictionary.
// https://developer.apple.com/documentation/foundation/nsdictionary?language=objc
// https://developer.apple.com/documentation/foundation/nsmutabledictionary?language=objc
@interface PrimitiveDictionary : NSDictionary {
  NSDictionary *backing;
}
- (id)initWrappingDictionary:(NSDictionary *)dictionary;
@end
@implementation PrimitiveDictionary
- (id)initWrappingDictionary:(NSDictionary *)dictionary {
  self = [super init];
  if (self) backing = [dictionary retain];
  return self;
}
- (id)initWithObjects:(const id *)objects forKeys:(const id *)keys count:(NSUInteger)count {
  self = [super init];
  if (self) {
    NSMutableDictionary *dictionary = [[NSMutableDictionary alloc] init];
    for (NSUInteger i = 0; i < count; i++) [dictionary setObject:objects[i] forKey:keys[i]];
    backing = dictionary;
  }
  return self;
}
- (NSUInteger)count { return [backing count]; }
- (id)objectForKey:(id)key { return [backing objectForKey:key]; }
- (id)keyEnumerator { return [backing keyEnumerator]; }
- (void)dealloc { [backing release]; [super dealloc]; }
@end

@interface PrimitiveMutableDictionary : NSMutableDictionary {
  NSMutableDictionary *backing;
}
- (id)initWrappingDictionary:(NSDictionary *)dictionary;
@end
@implementation PrimitiveMutableDictionary
- (id)initWrappingDictionary:(NSDictionary *)dictionary {
  self = [super init];
  if (self) {
    backing = [[NSMutableDictionary alloc] init];
    id keys = [dictionary keyEnumerator];
    id key;
    while ((key = [keys nextObject])) [backing setObject:[dictionary objectForKey:key] forKey:key];
  }
  return self;
}
- (id)initWithObjects:(const id *)objects forKeys:(const id *)keys count:(NSUInteger)count {
  self = [super init];
  if (self) {
    backing = [[NSMutableDictionary alloc] init];
    for (NSUInteger i = 0; i < count; i++) [backing setObject:objects[i] forKey:keys[i]];
  }
  return self;
}
- (NSUInteger)count { return [backing count]; }
- (id)objectForKey:(id)key { return [backing objectForKey:key]; }
- (id)keyEnumerator { return [backing keyEnumerator]; }
- (void)setObject:(id)object forKey:(id)key { [backing setObject:object forKey:key]; }
- (void)removeObjectForKey:(id)key { [backing removeObjectForKey:key]; }
- (void)dealloc { [backing release]; [super dealloc]; }
@end

int test_dictionary_subclasses_preserve_identity_and_storage(void) {
  NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
  NSDictionary *source = [NSDictionary dictionaryWithObject:@"value" forKey:@"key"];
  PrimitiveDictionary *immutable = [[PrimitiveDictionary alloc] initWrappingDictionary:source];
  PrimitiveMutableDictionary *mutable = [[PrimitiveMutableDictionary alloc] initWrappingDictionary:source];
  int result = 0;
  if (!immutable || !mutable) result = -1;
  else if ([immutable count] != 1 || [mutable count] != 1) result = -2;
  else if (![[immutable objectForKey:@"key"] isEqual:@"value"]) result = -3;
  else {
    [mutable setObject:@"second" forKey:@"other"];
    [mutable removeObjectForKey:@"key"];
    if ([mutable count] != 1 || [mutable objectForKey:@"key"] != nil ||
        ![[mutable objectForKey:@"other"] isEqual:@"second"]) result = -4;
    if ([immutable count] != 1 || [source count] != 1) result = -5;
  }
  [immutable release];
  [mutable release];
  [pool drain];
  return result;
}
