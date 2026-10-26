/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"
@interface NSEnumerator : NSObject
- (id)nextObject;
- (NSArray*)allObjects;
@end
@interface NSArray (EnumeratorTests)
- (NSEnumerator*)objectEnumerator;
- (NSEnumerator*)reverseObjectEnumerator;
@end
@interface TestCountingEnumerator : NSEnumerator { unsigned cursor; }
@end
@implementation TestCountingEnumerator
- (id)nextObject {
    switch (cursor++) {case 0:return @"a";case 1:return @"b";case 2:return @"c";default:return nil;}
}
@end
static int enumerator_item_deallocs;
@interface EnumeratorTestItem : NSObject
@end
@implementation EnumeratorTestItem
- (void)dealloc {++enumerator_item_deallocs;[super dealloc];}
@end

// allObjects returns and consumes the remaining objects, in enumeration order.
// https://developer.apple.com/documentation/foundation/nsenumerator/allobjects
int test_NSEnumerator_allObjects_consumes_remaining(void) {
    enumerator_item_deallocs=0;
    NSAutoreleasePool *pool=[NSAutoreleasePool new];
    id a=[EnumeratorTestItem new],b=[EnumeratorTestItem new],c=[EnumeratorTestItem new];
    NSArray *source=[[NSArray arrayWithObjects:a,b,c,nil] retain];
    [a release];[b release];[c release];
    NSEnumerator *enumerator=[[source objectEnumerator] retain];
    if ([enumerator nextObject]!=a) return -1;
    NSArray *remaining=[[enumerator allObjects] retain];
    if (!remaining || [remaining count]!=2 || [remaining objectAtIndex:0]!=b || [remaining objectAtIndex:1]!=c) return -2;
    if ([enumerator nextObject]!=nil || [[enumerator allObjects] count]!=0) return -3;
    [source release];[enumerator release];[pool drain];
    if (enumerator_item_deallocs!=1 || [remaining count]!=2) return -4;
    [remaining release];
    if (enumerator_item_deallocs!=3) return -5;
    source=[NSArray arrayWithObjects:@"a",@"b",@"c",nil];
    enumerator=[source reverseObjectEnumerator];
    if (![[enumerator nextObject] isEqualToString:@"c"]) return -6;
    remaining=[enumerator allObjects];
    if ([remaining count]!=2 || ![[remaining objectAtIndex:0] isEqualToString:@"b"] || ![[remaining objectAtIndex:1] isEqualToString:@"a"]) return -7;
    enumerator=[TestCountingEnumerator new];
    if (![[enumerator nextObject] isEqualToString:@"a"]) return -8;
    remaining=[enumerator allObjects];
    if ([remaining count]!=2 || ![[remaining objectAtIndex:0] isEqualToString:@"b"] || [enumerator nextObject]!=nil) return -9;
    [enumerator release];
    return 0;
}
