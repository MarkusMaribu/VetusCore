/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use super::ns_string::get_static_str;
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject,
    NSZonePtr, SEL,
};
use crate::Environment;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum OperationState {
    #[default]
    Ready,
    Executing,
    Finished,
}

#[derive(Debug, Default)]
struct NSOperationHostObject {
    // State tracking
    state: OperationState,
    cancelled: bool,

    // Dependencies
    dependencies: id,

    // Completion block
    completion_block: id,

    // NSBlockOperation: retained NSMutableArray of execution blocks, or nil
    execution_blocks: id,

    // Properties
    name: id,
    queue_priority: i32,

    // NSInvocationOperation specific fields
    target: id,
    selector: Option<SEL>,
    arg: id,
    invocation: id,
}

impl HostObject for NSOperationHostObject {}

#[derive(Debug, Default)]
struct NSOperationQueueHostObject {
    operations: id, // NSMutableArray of operations
    suspended: bool,
    max_concurrent_operations: i32,
    name: id,
    main_queue: bool,
    worker_running: bool,
    owns_pending_reference: bool,
}

impl HostObject for NSOperationQueueHostObject {}

#[derive(Default)]
pub struct State {
    main_queue: Option<id>,
    current_queues: HashMap<usize, id>,
}

fn start_queue_driver(env: &mut Environment, queue: id) {
    let host = env.objc.borrow_mut::<NSOperationQueueHostObject>(queue);
    if host.worker_running || host.suspended || host.max_concurrent_operations == 0 {
        return;
    }
    host.worker_running = true;
    let main = host.main_queue;
    let selector = env
        .objc
        .lookup_selector("_touchHLE_runOperationQueue:")
        .unwrap();
    if main {
        () = msg![env; queue performSelectorOnMainThread:selector withObject:nil waitUntilDone:false];
    } else {
        super::ns_thread::detach_new_thread_inner(env, selector, queue, nil, false);
    }
}

/// Operations remain queued until finished, including while suspended. A
/// background queue runs on a guest worker; the main queue uses its run loop.
/// https://developer.apple.com/documentation/foundation/operationqueue
fn run_queue(env: &mut Environment, queue: id) {
    let mut idle_since = None;
    loop {
        let (operations, suspended, main) = {
            let host = env.objc.borrow::<NSOperationQueueHostObject>(queue);
            (
                host.operations,
                host.suspended || host.max_concurrent_operations == 0,
                host.main_queue,
            )
        };
        let count: usize = msg![env; operations count];
        // Reuse a background worker across sensor samples instead of creating
        // a guest thread for every reading. Retire it after a short idle spell.
        if !suspended && !main && count == 0 {
            let idle = idle_since.get_or_insert_with(Instant::now);
            if idle.elapsed() < Duration::from_millis(100) {
                env.sleep(Duration::from_millis(5));
                continue;
            }
        } else {
            idle_since = None;
        }
        if suspended || count == 0 {
            let host = env.objc.borrow_mut::<NSOperationQueueHostObject>(queue);
            host.worker_running = false;
            let release_pending = count == 0 && std::mem::take(&mut host.owns_pending_reference);
            if release_pending {
                release(env, queue);
            }
            return;
        }
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let snapshot: id = msg![env; operations copy];
        let mut selected = nil;
        let mut priority = i32::MIN;
        for i in 0..count {
            let op: id = msg![env; snapshot objectAtIndex:i];
            if msg![env; op isFinished] {
                () = msg![env; operations removeObject:op];
            } else if msg![env; op isReady] {
                let p: i32 = msg![env; op queuePriority];
                if selected == nil || p > priority {
                    selected = op;
                    priority = p;
                }
            }
        }
        if selected != nil {
            let thread = env.current_thread;
            let previous = env
                .framework_state
                .foundation
                .ns_operation
                .current_queues
                .insert(thread, queue);
            () = msg![env; selected start];
            if let Some(previous) = previous {
                env.framework_state
                    .foundation
                    .ns_operation
                    .current_queues
                    .insert(thread, previous);
            } else {
                env.framework_state
                    .foundation
                    .ns_operation
                    .current_queues
                    .remove(&thread);
            }
            if msg![env; selected isFinished] {
                () = msg![env; operations removeObject:selected];
            }
        }
        release(env, snapshot);
        release(env, pool);
        if selected == nil {
            if main {
                let selector = env
                    .objc
                    .lookup_selector("_touchHLE_runOperationQueue:")
                    .unwrap();
                () = msg![env; queue performSelector:selector withObject:nil afterDelay:0.001f64];
                return;
            }
            env.sleep(Duration::from_millis(1));
        }
    }
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSOperation: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSOperationHostObject::default());
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (())dealloc {
    // Extract all properties to temporary variables to avoid borrow checker
    // conflicts
    let (deps, completion, execution_blocks, name, target, arg, invocation) = {
        let host_object = env.objc.borrow::<NSOperationHostObject>(this);
        (
            host_object.dependencies,
            host_object.completion_block,
            host_object.execution_blocks,
            host_object.name,
            host_object.target,
            host_object.arg,
            host_object.invocation,
        )
    };

    release(env, deps);
    release(env, completion);
    release(env, execution_blocks);
    release(env, name);
    release(env, target);
    release(env, arg);
    release(env, invocation);

    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Execution

- (())start {
    // Check if already executing or finished
    let state = env.objc.borrow::<NSOperationHostObject>(this).state;
    if state != OperationState::Ready {
        return;
    }

    // Check if cancelled before starting
    let cancelled = env.objc.borrow::<NSOperationHostObject>(this).cancelled;
    if cancelled {
        let finished_str: id = get_static_str(env, "isFinished");

        () = msg![env; this willChangeValueForKey:finished_str];
        env.objc.borrow_mut::<NSOperationHostObject>(this).state = OperationState::Finished;
        () = msg![env; this didChangeValueForKey:finished_str];
        return;
    }

    // Wait for dependencies
    let deps = env.objc.borrow::<NSOperationHostObject>(this).dependencies;
    if deps != nil {
        let count: usize = msg![env; deps count];
        for i in 0..count {
            let dep: id = msg![env; deps objectAtIndex:i];
            if dep != nil {
                let is_finished: bool = msg![env; dep isFinished];
                if !is_finished {
                    log!("Warning: NSOperation starting with unfinished dependency");
                }
            }
        }
    }

    // Transition to executing
    let executing_str: id = get_static_str(env, "isExecuting");

    () = msg![env; this willChangeValueForKey:executing_str];
    env.objc.borrow_mut::<NSOperationHostObject>(this).state = OperationState::Executing;
    () = msg![env; this didChangeValueForKey:executing_str];

    // Execute main
    () = msg![env; this main];

    // Transition to finished
    let finished_str: id = get_static_str(env, "isFinished");
    let executing_str_2: id = get_static_str(env, "isExecuting");

    () = msg![env; this willChangeValueForKey:executing_str_2];
    () = msg![env; this willChangeValueForKey:finished_str];
    env.objc.borrow_mut::<NSOperationHostObject>(this).state = OperationState::Finished;
    () = msg![env; this didChangeValueForKey:executing_str_2];
    () = msg![env; this didChangeValueForKey:finished_str];

    // Call completion block if present
    let completion = env.objc.borrow::<NSOperationHostObject>(this).completion_block;
    if completion != nil {
        // Invoke the block
        crate::libc::dispatch::call_void_block(env, completion.cast_void());
    }
}

- (())main {
    // Base implementation does nothing - subclasses override this
}

// MARK: - Cancellation

- (())cancel {
    let was_cancelled = env.objc.borrow::<NSOperationHostObject>(this).cancelled;
    if !was_cancelled {
        let cancelled_str: id = get_static_str(env, "isCancelled");

        () = msg![env; this willChangeValueForKey:cancelled_str];
        env.objc.borrow_mut::<NSOperationHostObject>(this).cancelled = true;
        () = msg![env; this didChangeValueForKey:cancelled_str];
    }
}

- (bool)isCancelled {
    env.objc.borrow::<NSOperationHostObject>(this).cancelled
}

// MARK: - State queries

- (bool)isReady {
    let state = env.objc.borrow::<NSOperationHostObject>(this).state;
    if state != OperationState::Ready {
        return false;
    }

    // Check if all dependencies are finished
    let deps = env.objc.borrow::<NSOperationHostObject>(this).dependencies;
    if deps != nil {
        let count: usize = msg![env; deps count];
        for i in 0..count {
            let dep: id = msg![env; deps objectAtIndex:i];
            if dep != nil {
                let is_finished: bool = msg![env; dep isFinished];
                if !is_finished {
                    return false;
                }
            }
        }
    }

    true
}

- (bool)isExecuting {
    env.objc.borrow::<NSOperationHostObject>(this).state == OperationState::Executing
}

- (bool)isFinished {
    env.objc.borrow::<NSOperationHostObject>(this).state == OperationState::Finished
}

- (bool)isConcurrent {
    false // We don't support concurrent operations in HLE
}

- (bool)isAsynchronous {
    false // We don't support async operations in HLE
}

// MARK: - Dependencies

- (())addDependency:(id)op {
    if op == nil {
        return;
    }

    // Don't add dependency if already executing or finished
    let state = env.objc.borrow::<NSOperationHostObject>(this).state;
    if state != OperationState::Ready {
        log!("Warning: Attempting to add dependency to operation that is not ready");
        return;
    }

    let deps = env.objc.borrow::<NSOperationHostObject>(this).dependencies;

    let deps_arr = if deps == nil {
        let new_arr: id = msg_class![env; NSMutableArray alloc];
        let new_arr: id = msg![env; new_arr init];
        env.objc.borrow_mut::<NSOperationHostObject>(this).dependencies = new_arr;
        new_arr
    } else {
        deps
    };

    // Check if dependency already exists
    let contains: bool = msg![env; deps_arr containsObject:op];
    if !contains {
        let _: () = msg![env; deps_arr addObject:op];
    }
}

- (())removeDependency:(id)op {
    if op == nil {
        return;
    }
    let deps = env.objc.borrow::<NSOperationHostObject>(this).dependencies;
    if deps != nil {
        let _: () = msg![env; deps removeObject:op];
    }
}

- (id)dependencies {
    let deps = env.objc.borrow::<NSOperationHostObject>(this).dependencies;
    if deps == nil {
        msg_class![env; NSArray array]
    } else {
        let copy: id = msg![env; deps copy];
        autorelease(env, copy)
    }
}

// MARK: - Completion block

- (())setCompletionBlock:(id)block {
    let block: id = if block != nil { msg![env; block copy] } else { nil };
    let old = std::mem::replace(&mut env.objc.borrow_mut::<NSOperationHostObject>(this).completion_block, block);
    release(env, old);
}

- (id)completionBlock {
    let block = env.objc.borrow::<NSOperationHostObject>(this).completion_block;
    block // Already retained in our object
}

// MARK: - Properties

- (())setName:(id)name {
    let old_name = env.objc.borrow::<NSOperationHostObject>(this).name;
    release(env, old_name);

    if name != nil {
        retain(env, name);
    }
    env.objc.borrow_mut::<NSOperationHostObject>(this).name = name;
}

- (id)name {
    env.objc.borrow::<NSOperationHostObject>(this).name
}

- (())setQueuePriority:(i32)priority {
    env.objc.borrow_mut::<NSOperationHostObject>(this).queue_priority = priority;
}

- (i32)queuePriority {
    env.objc.borrow::<NSOperationHostObject>(this).queue_priority
}

// MARK: - Waiting

- (())waitUntilFinished {
    // In HLE, operations are synchronous, so this is a no-op
    // In a real implementation, this would block until isFinished is true
}

@end

@implementation NSInvocationOperation: NSOperation

// allocWithZone: and dealloc are now inherited from NSOperation

- (id)initWithTarget:(id)target selector:(SEL)sel object:(id)arg {
    let this: id = msg![env; this init];
    if this != nil {
        retain(env, target);
        if arg != nil {
            retain(env, arg);
        }

        let host_object = env.objc.borrow_mut::<NSOperationHostObject>(this);
        host_object.target = target;
        host_object.selector = Some(sel);
        host_object.arg = arg;
    }
    this
}

- (id)initWithInvocation:(id)invocation {
    let this: id = msg![env; this init];
    if this != nil && invocation != nil {
        retain(env, invocation);
        env.objc.borrow_mut::<NSOperationHostObject>(this).invocation = invocation;
    }
    this
}

- (())main {
    // Check for cancellation
    let cancelled = env.objc.borrow::<NSOperationHostObject>(this).cancelled;
    if cancelled {
        return;
    }

    let (target, sel_opt, arg, invocation) = {
        let host_object = env.objc.borrow::<NSOperationHostObject>(this);
        (
            host_object.target,
            host_object.selector,
            host_object.arg,
            host_object.invocation,
        )
    };

    if invocation != nil {
        () = msg![env; invocation invoke];
        return;
    }

    if target != nil {
        if let Some(sel) = sel_opt {
            if arg != nil {
                let _: id = msg![env; target performSelector:sel withObject:arg];
            } else {
                let _: id = msg![env; target performSelector:sel];
            }
        }
    }
}

- (id)result {
    // Would need to store the result from the invocation
    nil
}

@end

// Implementation per Apple's documentation for NSBlockOperation:
// - `blockOperationWithBlock:` returns a new operation whose `main`
//   executes the given block.
// - `addExecutionBlock:` appends a block; execution blocks run in the
//   order they were added.
// - `executionBlocks` returns a copy of the list of blocks.
// Per the docs, adding a block after execution has started or finished
// raises NSInvalidArgumentException; in HLE that would abort the guest
// process, so adding after start logs a warning and ignores the block.
@implementation NSBlockOperation: NSOperation

+ (id)blockOperationWithBlock:(id)block {
    let op: id = msg_class![env; NSBlockOperation alloc];
    () = msg![env; op init];
    if block != nil {
        () = msg![env; op addExecutionBlock:block];
    }
    autorelease(env, op)
}

- (())addExecutionBlock:(id)block {
    if block == nil {
        return;
    }

    let state = env.objc.borrow::<NSOperationHostObject>(this).state;
    if state != OperationState::Ready {
        log!(
            "Warning: [NSBlockOperation addExecutionBlock:] called after execution started; ignoring"
        );
        return;
    }

    let blocks = if env
        .objc
        .borrow::<NSOperationHostObject>(this)
        .execution_blocks
        == nil
    {
        let new_arr: id = msg_class![env; NSMutableArray alloc];
        let new_arr: id = msg![env; new_arr init];
        env.objc
            .borrow_mut::<NSOperationHostObject>(this)
            .execution_blocks = new_arr;
        new_arr
    } else {
        env.objc
            .borrow::<NSOperationHostObject>(this)
            .execution_blocks
    };
    let copied: id = msg![env; block copy];
    () = msg![env; blocks addObject:copied];
    release(env, copied);
}

- (id)executionBlocks {
    let blocks = env.objc.borrow::<NSOperationHostObject>(this).execution_blocks;
    if blocks == nil {
        msg_class![env; NSArray array]
    } else {
        let copy: id = msg![env; blocks copy];
        autorelease(env, copy)
    }
}

- (())main {
    // Check for cancellation
    let cancelled = env.objc.borrow::<NSOperationHostObject>(this).cancelled;
    if cancelled {
        return;
    }

    let blocks = env.objc.borrow::<NSOperationHostObject>(this).execution_blocks;

    if blocks == nil {
        return;
    }

    // Per the docs, the execution blocks run in the order they were added.
    let count: usize = msg![env; blocks count];
    for i in 0..count {
        // Re-check cancellation before each block, like a well-behaved
        // operation should.
        let cancelled = env.objc.borrow::<NSOperationHostObject>(this).cancelled;
        if cancelled {
            return;
        }

        let block: id = msg![env; blocks objectAtIndex:i];
        if block != nil {
            crate::libc::dispatch::call_void_block(env, block.cast_void());
        }
    }
}

@end

@implementation NSOperationQueue: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSOperationQueueHostObject::default());
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

+ (id)mainQueue {
    if let Some(queue) = env.framework_state.foundation.ns_operation.main_queue {
        return queue;
    }
    let queue: id = msg![env; this new];
    env.objc.borrow_mut::<NSOperationQueueHostObject>(queue).main_queue = true;
    env.framework_state.foundation.ns_operation.main_queue = Some(queue);
    queue
}

+ (id)currentQueue {
    env.framework_state.foundation.ns_operation.current_queues
        .get(&env.current_thread).copied().unwrap_or_else(|| {
            if env.current_thread == 0 { msg![env; this mainQueue] } else { nil }
        })
}

- (id)init {
    let operations: id = msg_class![env; NSMutableArray alloc];
    let operations: id = msg![env; operations init];
    env.objc.borrow_mut::<NSOperationQueueHostObject>(this).operations = operations;
    env.objc.borrow_mut::<NSOperationQueueHostObject>(this).max_concurrent_operations = -1; // Default: unlimited
    this
}

- (())dealloc {
    let (operations, name) = {
        let host_object = env.objc.borrow::<NSOperationQueueHostObject>(this);
        (host_object.operations, host_object.name)
    };

    release(env, operations);
    release(env, name);

    env.objc.dealloc_object(this, &mut env.mem)
}

// MARK: - Adding operations

- (())addOperation:(id)op {
    if op == nil { return; }
    let operations = env.objc.borrow::<NSOperationQueueHostObject>(this).operations;
    () = msg![env; operations addObject:op];
    let host = env.objc.borrow_mut::<NSOperationQueueHostObject>(this);
    if !host.owns_pending_reference {
        host.owns_pending_reference = true;
        retain(env, this);
    }
    start_queue_driver(env, this);
}

- (())_touchHLE_runOperationQueue:(id)_unused {
    run_queue(env, this);
}

- (())addOperations:(id)ops waitUntilFinished:(bool)wait {
    if ops == nil {
        return;
    }

    let count: usize = msg![env; ops count];
    for i in 0..count {
        let op: id = msg![env; ops objectAtIndex:i];
        () = msg![env; this addOperation:op];
    }

    if wait {
        () = msg![env; this waitUntilAllOperationsAreFinished];
    }
}

- (())addOperationWithBlock:(id)block {
    if block == nil {
        return;
    }

    let op: id = msg_class![env; NSBlockOperation blockOperationWithBlock:block];
    () = msg![env; this addOperation:op];
}

// MARK: - Queue control

- (())setSuspended:(bool)suspended {
    env.objc.borrow_mut::<NSOperationQueueHostObject>(this).suspended = suspended;
    if !suspended { start_queue_driver(env, this); }
}

- (bool)isSuspended {
    env.objc.borrow::<NSOperationQueueHostObject>(this).suspended
}

- (())setMaxConcurrentOperationCount:(i32)count {
    env.objc.borrow_mut::<NSOperationQueueHostObject>(this).max_concurrent_operations = count;
    if count != 0 { start_queue_driver(env, this); }
}

- (i32)maxConcurrentOperationCount {
    env.objc.borrow::<NSOperationQueueHostObject>(this).max_concurrent_operations
}

// MARK: - Cancellation

- (())cancelAllOperations {
    let operations = env.objc.borrow::<NSOperationQueueHostObject>(this).operations;
    let count: usize = msg![env; operations count];

    for i in 0..count {
        let op: id = msg![env; operations objectAtIndex:i];
        if op != nil {
            () = msg![env; op cancel];
        }
    }
}

// MARK: - Waiting

- (())waitUntilAllOperationsAreFinished {
    loop {
        let count: usize = msg![env; this operationCount];
        if count == 0 { break; }
        env.sleep(Duration::from_millis(1));
    }
}

// MARK: - Properties

- (id)operations {
    let operations = env.objc.borrow::<NSOperationQueueHostObject>(this).operations;
    let copy: id = msg![env; operations copy];
    autorelease(env, copy)
}

- (usize)operationCount {
    let operations = env.objc.borrow::<NSOperationQueueHostObject>(this).operations;
    msg![env; operations count]
}

- (())setName:(id)name {
    let old_name = env.objc.borrow::<NSOperationQueueHostObject>(this).name;
    release(env, old_name);

    if name != nil {
        retain(env, name);
    }
    env.objc.borrow_mut::<NSOperationQueueHostObject>(this).name = name;
}

- (id)name {
    env.objc.borrow::<NSOperationQueueHostObject>(this).name
}

// MARK: - Quality of service (stub)

- (())setQualityOfService:(i32)_qos {
    // Ignored in HLE
}

- (i32)qualityOfService {
    0 // NSQualityOfServiceDefault
}

@end

};
