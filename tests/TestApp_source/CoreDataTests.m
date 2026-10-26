/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
#import "system_headers.h"
#include <unistd.h>
@interface NSManagedObject : NSObject
- (id)valueForKey:(id)key;
- (void)setValue:(id)value forKey:(id)key;
@end
@interface CoreDataTestRecord : NSManagedObject
@property(nonatomic,retain) NSString *title;
@property(nonatomic,retain) id stamp;
@end
@implementation CoreDataTestRecord
@dynamic title, stamp;
@end
@interface NSAttributeDescription : NSObject
- (void)setName:(id)name;
- (void)setAttributeType:(int)type;
- (void)setOptional:(BOOL)optional;
- (void)setDefaultValue:(id)value;
@end
@interface NSEntityDescription : NSObject
+ (id)entityForName:(id)name inManagedObjectContext:(id)context;
+ (id)insertNewObjectForEntityForName:(id)name inManagedObjectContext:(id)context;
- (void)setName:(id)name;
- (void)setManagedObjectClassName:(id)name;
- (void)setProperties:(id)properties;
@end
@interface NSManagedObjectModel : NSObject
- (void)setEntities:(id)entities;
@end
@interface NSPersistentStoreCoordinator : NSObject
- (id)initWithManagedObjectModel:(id)model;
- (id)addPersistentStoreWithType:(id)type configuration:(id)configuration URL:(id)url options:(id)options error:(id*)error;
@end
@interface NSManagedObjectContext : NSObject
- (void)setPersistentStoreCoordinator:(id)coordinator;
- (BOOL)save:(id*)error;
- (BOOL)hasChanges;
- (id)executeFetchRequest:(id)request error:(id*)error;
- (void)deleteObject:(id)object;
- (void)rollback;
@end
@interface NSFetchRequest : NSObject
- (void)setEntity:(id)entity;
- (void)setPredicate:(id)predicate;
- (void)setSortDescriptors:(id)descriptors;
@end
@interface NSPredicate : NSObject
+ (id)predicateWithFormat:(id)format, ...;
@end
@interface NSSortDescriptor : NSObject
+ (id)sortDescriptorWithKey:(id)key ascending:(BOOL)ascending;
@end
@interface NSURL (CoreDataTestURL)
+ (id)fileURLWithPath:(id)path;
@end
NSString *NSHomeDirectory(void);
@interface NSString (CoreDataTestPath)
- (NSString*)stringByAppendingPathComponent:(NSString*)component;
- (const char*)UTF8String;
@end
@interface NSDate : NSObject
+ (id)dateWithTimeIntervalSinceReferenceDate:(double)interval;
@end

// Core Data Programming Guide: Creating and Saving Managed Objects, Fetching
// Objects. Saving must commit values; a new stack must see them after reopen.
// https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/CoreData/CreatingObjects.html
int test_CoreData_attributes_fetch_and_sqlite_reopen(void) {
    NSString *path = [NSHomeDirectory() stringByAppendingPathComponent:@"Documents/CoreDataCompatibility.sqlite"];
    unlink([path UTF8String]);
    NSAttributeDescription *title = [NSAttributeDescription new];
    [title setName:@"title"]; [title setAttributeType:700]; [title setDefaultValue:@"untitled"];
    NSAttributeDescription *rank = [NSAttributeDescription new];
    [rank setName:@"rank"]; [rank setAttributeType:300]; [rank setOptional:YES];
    NSAttributeDescription *stamp = [NSAttributeDescription new];
    [stamp setName:@"stamp"]; [stamp setAttributeType:900]; [stamp setOptional:YES];
    NSEntityDescription *entity = [NSEntityDescription new];
    [entity setName:@"Record"]; [entity setManagedObjectClassName:@"CoreDataTestRecord"];
    [entity setProperties:[NSArray arrayWithObjects:title,rank,stamp,nil]];
    NSManagedObjectModel *model = [NSManagedObjectModel new];
    [model setEntities:[NSArray arrayWithObject:entity]];
    NSPersistentStoreCoordinator *psc = [[NSPersistentStoreCoordinator alloc] initWithManagedObjectModel:model];
    id url = [NSURL fileURLWithPath:path];
    id error = nil;
    if (![psc addPersistentStoreWithType:@"SQLite" configuration:nil URL:url options:nil error:&error] || error) return -1;
    NSManagedObjectContext *context = [NSManagedObjectContext new];
    [context setPersistentStoreCoordinator:psc];
    CoreDataTestRecord *first = [NSEntityDescription insertNewObjectForEntityForName:@"Record" inManagedObjectContext:context];
    if (!first) return -2;
    if (![first.title isEqualToString:@"untitled"]) return -9;
    first.title = @"first";
    first.stamp = [NSDate dateWithTimeIntervalSinceReferenceDate:42];
    [first setValue:[NSNumber numberWithInt:1] forKey:@"rank"];
    CoreDataTestRecord *second = [NSEntityDescription insertNewObjectForEntityForName:@"Record" inManagedObjectContext:context];
    second.title = @"second";
    second.stamp = [NSDate dateWithTimeIntervalSinceReferenceDate:84];
    [second setValue:[NSNumber numberWithInt:2] forKey:@"rank"];
    if (![context hasChanges] || ![context save:&error] || error || [context hasChanges]) return -3;
    first.title = @"pending";
    [context rollback];
    if ([context hasChanges] || ![first.title isEqualToString:@"first"]) return -10;
    [context release]; [psc release];
    psc = [[NSPersistentStoreCoordinator alloc] initWithManagedObjectModel:model];
    if (![psc addPersistentStoreWithType:@"SQLite" configuration:nil URL:url options:nil error:&error]) return -4;
    context = [NSManagedObjectContext new]; [context setPersistentStoreCoordinator:psc];
    NSFetchRequest *request = [NSFetchRequest new];
    [request setEntity:[NSEntityDescription entityForName:@"Record" inManagedObjectContext:context]];
    [request setSortDescriptors:[NSArray arrayWithObject:[NSSortDescriptor sortDescriptorWithKey:@"stamp" ascending:NO]]];
    NSArray *result = [context executeFetchRequest:request error:&error];
    if (!result || [result count] != 2 || ![[(CoreDataTestRecord*)[result objectAtIndex:0] title] isEqualToString:@"second"]) return -5;
    [request setPredicate:[NSPredicate predicateWithFormat:@"title == %@", @"first"]];
    result = [context executeFetchRequest:request error:&error];
    if ([result count] != 1) return -6;
    [context deleteObject:[result objectAtIndex:0]];
    if (![context save:&error]) return -7;
    [request setPredicate:nil]; result = [context executeFetchRequest:request error:&error];
    if ([result count] != 1) return -8;
    [request release]; [context release]; [psc release];
    [model release]; [entity release]; [title release]; [rank release]; [stamp release];
    unlink([path UTF8String]);
    return 0;
}
