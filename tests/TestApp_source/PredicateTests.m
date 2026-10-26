/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#ifdef DEFINE_ME_WHEN_BUILDING_ON_MACOS
#import <Foundation/Foundation.h>
#else
#import "system_headers.h"
@interface NSNumber (PredicateTests)
+ (NSNumber *)numberWithInt:(int)value;
@end
@interface NSDate : NSObject
+ (instancetype)dateWithTimeIntervalSinceReferenceDate:(NSTimeInterval)value;
@end
@interface NSPredicate : NSObject
+ (instancetype)predicateWithFormat:(NSString *)format, ...;
+ (instancetype)predicateWithFormat:(NSString *)format argumentArray:(NSArray *)arguments;
+ (instancetype)predicateWithValue:(BOOL)value;
- (BOOL)evaluateWithObject:(id)object;
@end
@interface NSCompoundPredicate : NSPredicate
+ (instancetype)andPredicateWithSubpredicates:(NSArray *)predicates;
+ (instancetype)orPredicateWithSubpredicates:(NSArray *)predicates;
@end
#endif

// Typed %@ values, %K key paths and complete compound arrays follow Apple's
// Predicate Programming Guide. A quoted placeholder is a literal, and an
// object's string contents must never be parsed as predicate syntax.
// https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Predicates/Articles/pCreating.html
int test_NSPredicate_typed_comparisons_and_compounds(void) {
    NSArray *keys = [NSArray arrayWithObjects:@"fileName", @"count", @"creationDate", nil];
    NSDate *date = [NSDate dateWithTimeIntervalSinceReferenceDate:42];
    NSDictionary *row = [NSDictionary dictionaryWithObjects:
        [NSArray arrayWithObjects:@"x' OR TRUEPREDICATE", [NSNumber numberWithInt:3], date, nil]
        forKeys:keys];
    NSPredicate *p = [NSPredicate predicateWithFormat:@"%K == %@", @"fileName", @"x' OR TRUEPREDICATE"];
    if (![p evaluateWithObject:row]) return -1;
    p = [NSPredicate predicateWithFormat:@"fileName == %@", @"not the same"];
    if ([p evaluateWithObject:row]) return -2;
    p = [NSPredicate predicateWithFormat:@"count >= 3 AND count < %d AND NOT (missing != nil)", 4];
    if (![p evaluateWithObject:row]) return -3;
    p = [NSPredicate predicateWithFormat:@"count == 2 OR count == 3 AND fileName == 'wrong'"];
    if ([p evaluateWithObject:row]) return -4;
    p = [NSPredicate predicateWithFormat:@"creationDate >= %@ AND creationDate <= %@", date, date];
    if (![p evaluateWithObject:row]) return -5;
    p = [NSPredicate predicateWithFormat:@"%K != %@" argumentArray:
        [NSArray arrayWithObjects:@"count", [NSNumber numberWithInt:2], nil]];
    if (![p evaluateWithObject:row]) return -6;
    p = [NSPredicate predicateWithFormat:@"SELF == '%@'"];
    if (![p evaluateWithObject:@"%@"]) return -7;
    NSPredicate *yes = [NSPredicate predicateWithValue:YES];
    NSPredicate *no = [NSPredicate predicateWithValue:NO];
    p = [NSCompoundPredicate andPredicateWithSubpredicates:[NSArray arrayWithObjects:yes, yes, no, nil]];
    if ([p evaluateWithObject:row]) return -8;
    p = [NSCompoundPredicate orPredicateWithSubpredicates:[NSArray arrayWithObjects:no, no, yes, nil]];
    if (![p evaluateWithObject:row]) return -9;
    NSAutoreleasePool *pool = [[NSAutoreleasePool alloc] init];
    NSString *owned = [[NSString alloc] initWithString:@"retained operand"];
    p = [[NSPredicate predicateWithFormat:@"SELF == %@", owned] retain];
    [owned release];
    [pool drain];
    BOOL retained = [p evaluateWithObject:@"retained operand"];
    [p release];
    if (!retained) return -10;
    return 0;
}
