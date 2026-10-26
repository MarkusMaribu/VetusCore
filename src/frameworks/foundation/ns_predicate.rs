/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSPredicate`.

use crate::abi::VaList;
use crate::frameworks::foundation::{ns_string, NSInteger, NSUInteger};
use crate::objc::{
    id, msg, msg_class, nil, objc_classes, release, retain, ClassExports, HostObject, NSZonePtr,
};

// =========================================================================
// MARK: - Predicate kind
// =========================================================================

#[path = "predicate_parser.rs"]
mod parser;
use parser::{Comparison, Expression, Operand};

#[derive(Clone)]
enum PredicateKind {
    /// Always evaluates to the given constant. Also acts as the
    /// `Default` for the phantom-fallback host object (yielding a
    /// predicate that always returns `false`).
    Constant(bool),
    /// Invalid or unsupported formats remain identifiable to fetch callers.
    Format(Result<Expression, String>),
    /// NOT of another predicate.
    Not(id),
    And(Vec<id>),
    Or(Vec<id>),
    /// Block-based predicate (not supported — always returns false).
    Block,
}
impl Default for PredicateKind {
    fn default() -> Self {
        PredicateKind::Constant(false)
    }
}

#[derive(Default)]
struct NSPredicateHostObject {
    kind: PredicateKind,
    /// Original format string if created with predicateWithFormat:.
    format: Option<String>,
    /// Object operands are retained for the predicate's entire lifetime.
    arguments: Vec<id>,
}
impl HostObject for NSPredicateHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSPredicate: NSObject

// =========================================================================
// MARK: - Allocation
// =========================================================================

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSPredicateHostObject {
        kind: PredicateKind::Constant(false),
        format: None,
        arguments: Vec::new(),
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

// =========================================================================
// MARK: - Convenience constructors
// =========================================================================

// predicateWithValue: — always-true or always-false predicate.
+ (id)predicateWithValue:(bool)value {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithValue:value];
    crate::objc::autorelease(env, new)
}

// Apple's format parser keeps %@ object operands and %K key paths distinct.
// https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Predicates/Articles/pCreating.html
+ (id)predicateWithFormat:(id)format, ...args {
    let new: id = msg![env; this alloc];
    let new = init_varargs(env, new, format, args.start());
    crate::objc::autorelease(env, new)
}

// predicateWithFormat:argumentArray:
+ (id)predicateWithFormat:(id)format
           argumentArray:(id)arguments { // NSArray*
    let new: id = msg![env; this alloc];
    let new = init_arguments(env, new, format, arguments);
    crate::objc::autorelease(env, new)
}

+ (id)predicateWithFormat:(id)format arguments:(VaList)args {
    let new: id = msg![env; this alloc];
    let new = init_varargs(env, new, format, args);
    crate::objc::autorelease(env, new)
}

// predicateWithBlock: — block evaluation is not supported; the predicate
// always evaluates to false (and logs when evaluated).
+ (id)predicateWithBlock:(id)_block {
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<NSPredicateHostObject>(new).kind = PredicateKind::Block;
    crate::objc::autorelease(env, new)
}

// notPredicateWithSubpredicate:
+ (id)notPredicateWithSubpredicate:(id)predicate {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithNotPredicate:predicate];
    crate::objc::autorelease(env, new)
}

// andPredicateWithSubpredicates:
+ (id)andPredicateWithSubpredicates:(id)subpredicates { // NSArray*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithAndPredicates:subpredicates];
    crate::objc::autorelease(env, new)
}

// orPredicateWithSubpredicates:
+ (id)orPredicateWithSubpredicates:(id)subpredicates { // NSArray*
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithOrPredicates:subpredicates];
    crate::objc::autorelease(env, new)
}

// =========================================================================
// MARK: - Initializers
// =========================================================================

- (id)initWithValue:(bool)value {
    env.objc.borrow_mut::<NSPredicateHostObject>(this).kind =
        PredicateKind::Constant(value);
    this
}

- (id)initWithFormat:(id)format, ...args { init_varargs(env, this, format, args.start()) }
- (id)initWithFormat:(id)format arguments:(VaList)args { init_varargs(env, this, format, args) }
- (id)initWithFormat:(id)format argumentArray:(id)arguments { init_arguments(env, this, format, arguments) }

- (id)initWithNotPredicate:(id)subpredicate {
    retain(env, subpredicate);
    env.objc.borrow_mut::<NSPredicateHostObject>(this).kind =
        PredicateKind::Not(subpredicate);
    this
}

- (id)initWithAndPredicates:(id)subpredicates { compound(env, this, subpredicates, true) }
- (id)initWithOrPredicates:(id)subpredicates { compound(env, this, subpredicates, false) }

// =========================================================================
// MARK: - Dealloc
// =========================================================================

- (())dealloc {
    let kind = std::mem::replace(
        &mut env.objc.borrow_mut::<NSPredicateHostObject>(this).kind,
        PredicateKind::Constant(false),
    );
    match kind {
        PredicateKind::Not(p) => release(env, p),
        PredicateKind::And(children) | PredicateKind::Or(children) => {
            for p in children { release(env, p); }
        }
        _ => {}
    }
    let arguments = std::mem::take(&mut env.objc.borrow_mut::<NSPredicateHostObject>(this).arguments);
    for value in arguments { release(env, value); }
    env.objc.dealloc_object(this, &mut env.mem)
}

// =========================================================================
// MARK: - NSCopying
// =========================================================================

- (id)copyWithZone:(NSZonePtr)_zone {
    retain(env, this)
}

// =========================================================================
// MARK: - Evaluation
// =========================================================================

// evaluateWithObject: — evaluate against a single object.
- (bool)evaluateWithObject:(id)object {
    evaluate_predicate(env, this, object)
}

// evaluateWithObject:substitutionVariables: — variable substitution not
// implemented; delegates to plain evaluateWithObject:.
- (bool)evaluateWithObject:(id)object
   substitutionVariables:(id)_variables { // NSDictionary*
    evaluate_predicate(env, this, object)
}

// =========================================================================
// MARK: - Predicate string / subpredicates
// =========================================================================

- (id)predicateFormat { // NSString*
    let fmt = {
        let host = env.objc.borrow::<NSPredicateHostObject>(this);
        match &host.kind {
            PredicateKind::Constant(v) => {
                if *v { "TRUEPREDICATE".to_string() } else { "FALSEPREDICATE".to_string() }
            }
            PredicateKind::Format(_) => host.format.clone().unwrap_or_default(),
            PredicateKind::Not(_)    => "NOT (...)".to_string(),
            PredicateKind::And(_) => "(...) AND (...)".to_string(),
            PredicateKind::Or(_)  => "(...) OR (...)".to_string(),
            PredicateKind::Block     => "BLOCK".to_string(),
        }
    };
    let s: id = ns_string::from_rust_string(env, fmt);
    crate::objc::autorelease(env, s)
}

// predicateWithSubstitutionVariables: — returns self (no substitution).
- (id)predicateWithSubstitutionVariables:(id)_variables {
    retain(env, this);
    crate::objc::autorelease(env, this)
}

// =========================================================================
// MARK: - Compound predicate subpredicates accessors
// =========================================================================

- (id)subpredicates {
    let children = match &env.objc.borrow::<NSPredicateHostObject>(this).kind {
        PredicateKind::Not(p) => vec![*p],
        PredicateKind::And(p) | PredicateKind::Or(p) => p.clone(),
        _ => Vec::new(),
    };
    let arr: id = msg_class![env; NSMutableArray new];
    for child in children { () = msg![env; arr addObject:child]; }
    crate::objc::autorelease(env, arr)
}

// =========================================================================
// MARK: - Description
// =========================================================================

- (id)description {
    let fmt: id = msg![env; this predicateFormat];
    let fmt_str = ns_string::to_rust_string(env, fmt);
    let s = format!("<NSPredicate: {:?} \"{}\">", this, fmt_str);
    let cstr = env.mem.alloc_and_write_cstr(s.as_bytes());
    msg_class![env; NSString stringWithUTF8String:cstr]
}

@end

// =========================================================================
// MARK: - NSCompoundPredicate (subclass — thin wrapper)
// =========================================================================

@implementation NSCompoundPredicate: NSPredicate

+ (id)notPredicateWithSubpredicate:(id)predicate {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithNotPredicate:predicate];
    crate::objc::autorelease(env, new)
}

+ (id)andPredicateWithSubpredicates:(id)subpredicates {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithAndPredicates:subpredicates];
    crate::objc::autorelease(env, new)
}

+ (id)orPredicateWithSubpredicates:(id)subpredicates {
    let new: id = msg![env; this alloc];
    let new: id = msg![env; new initWithOrPredicates:subpredicates];
    crate::objc::autorelease(env, new)
}

@end

// =========================================================================
// MARK: - NSComparisonPredicate (subclass — stub)
// =========================================================================

@implementation NSComparisonPredicate: NSPredicate

+ (id)predicateWithLeftExpression:(id)_lhs
                  rightExpression:(id)_rhs
                         modifier:(i32)_modifier
                             type:(i32)_type
                          options:(i32)_options {
    log!("NSComparisonPredicate expression construction is unsupported");
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<NSPredicateHostObject>(new).kind = PredicateKind::Block;
    crate::objc::autorelease(env, new)
}

+ (id)predicateWithLeftExpression:(id)_lhs
                  rightExpression:(id)_rhs
               customSelector:(id)_selector {
    log!("NSComparisonPredicate custom-selector construction is unsupported");
    let new: id = msg![env; this alloc];
    env.objc.borrow_mut::<NSPredicateHostObject>(new).kind = PredicateKind::Block;
    crate::objc::autorelease(env, new)
}

@end

};

// =========================================================================
// MARK: - Evaluation helper
// =========================================================================

fn set_format<F>(env: &mut crate::Environment, this: id, format: id, mut argument: F) -> id
where
    F: FnMut(&mut crate::Environment, char) -> Result<Operand, String>,
{
    let format = if format == nil {
        String::new()
    } else {
        ns_string::to_rust_string(env, format).into_owned()
    };
    let mut objects = Vec::new();
    let parsed = parser::parse(&format, |specifier| {
        let operand = argument(env, specifier)?;
        if let Operand::Object(object) = operand {
            objects.push(object);
        }
        Ok(operand)
    });
    for &object in &objects {
        retain(env, object);
    }
    let host = env.objc.borrow_mut::<NSPredicateHostObject>(this);
    let old = std::mem::replace(&mut host.arguments, objects);
    host.kind = PredicateKind::Format(parsed);
    host.format = Some(format);
    for value in old {
        release(env, value);
    }
    this
}

fn init_varargs(env: &mut crate::Environment, this: id, format: id, mut args: VaList) -> id {
    set_format(env, this, format, |env, specifier| {
        Ok(match specifier {
            '@' => Operand::Object(args.next(env)),
            'K' => {
                let key: id = args.next(env);
                Operand::Key(ns_string::to_rust_string(env, key).into_owned())
            }
            'd' | 'i' => Operand::Number(args.next::<i32>(env) as f64),
            'u' => Operand::Number(args.next::<u32>(env) as f64),
            'f' | 'g' => Operand::Number(args.next::<f64>(env)),
            _ => return Err(format!("Unsupported format placeholder %{specifier}")),
        })
    })
}

fn init_arguments(env: &mut crate::Environment, this: id, format: id, arguments: id) -> id {
    let count: NSUInteger = msg![env; arguments count];
    let mut at = 0;
    set_format(env, this, format, |env, specifier| {
        if at >= count {
            return Err("Missing predicate argument".into());
        }
        let value: id = msg![env; arguments objectAtIndex:at];
        at += 1;
        Ok(match specifier {
            '@' => Operand::Object(value),
            'K' => Operand::Key(ns_string::to_rust_string(env, value).into_owned()),
            _ => {
                return Err(format!(
                    "Unsupported argumentArray placeholder %{specifier}"
                ))
            }
        })
    })
}

fn compound(env: &mut crate::Environment, this: id, array: id, and: bool) -> id {
    let count: NSUInteger = msg![env; array count];
    let mut children = Vec::with_capacity(count as usize);
    for index in 0..count {
        let child: id = msg![env; array objectAtIndex:index];
        retain(env, child);
        children.push(child);
    }
    env.objc.borrow_mut::<NSPredicateHostObject>(this).kind = if and {
        PredicateKind::And(children)
    } else {
        PredicateKind::Or(children)
    };
    this
}

/// Core Data must reject an unsupported predicate instead of treating it as
/// a successful fetch with no matches. The direct evaluate API retains its
/// prior logged fallback for syntax outside the implemented grammar.
pub(super) fn is_supported(env: &crate::Environment, predicate: id) -> bool {
    match &env.objc.borrow::<NSPredicateHostObject>(predicate).kind {
        PredicateKind::Format(Err(_)) | PredicateKind::Block => false,
        PredicateKind::Not(p) => is_supported(env, *p),
        PredicateKind::And(children) | PredicateKind::Or(children) => {
            children.iter().all(|p| is_supported(env, *p))
        }
        _ => true,
    }
}

fn operand_value(env: &mut crate::Environment, operand: &Operand, object: id) -> id {
    match operand {
        Operand::Key(key) if key.eq_ignore_ascii_case("SELF") => object,
        Operand::Key(key) => {
            let key = ns_string::from_rust_string(env, key.clone());
            let value: id = msg![env; object valueForKeyPath:key];
            release(env, key);
            value
        }
        Operand::Object(value) => *value,
        Operand::String(text) => {
            let text = ns_string::from_rust_string(env, text.clone());
            crate::objc::autorelease(env, text)
        }
        Operand::Number(number) => msg_class![env; NSNumber numberWithDouble:(*number)],
        Operand::Null => nil,
    }
}

fn evaluate_expression(env: &mut crate::Environment, parsed: &Expression, object: id) -> bool {
    match parsed {
        Expression::Constant(value) => *value,
        Expression::Not(child) => !evaluate_expression(env, child, object),
        Expression::And(a, b) => {
            evaluate_expression(env, a, object) && evaluate_expression(env, b, object)
        }
        Expression::Or(a, b) => {
            evaluate_expression(env, a, object) || evaluate_expression(env, b, object)
        }
        Expression::Compare(a, op, b) => {
            let a = operand_value(env, a, object);
            let b = operand_value(env, b, object);
            let null: id = msg_class![env; NSNull null];
            let a = if a == null { nil } else { a };
            let b = if b == null { nil } else { b };
            if matches!(op, Comparison::Equal | Comparison::NotEqual) {
                let equal = a == b || (a != nil && b != nil && msg![env; a isEqual:b]);
                return equal == (*op == Comparison::Equal);
            }
            if a == nil || b == nil {
                return false;
            }
            let comparison: NSInteger = msg![env; a compare:b];
            match op {
                Comparison::Less => comparison < 0,
                Comparison::LessEqual => comparison <= 0,
                Comparison::Greater => comparison > 0,
                Comparison::GreaterEqual => comparison >= 0,
                _ => unreachable!(),
            }
        }
    }
}

fn evaluate_predicate(env: &mut crate::Environment, predicate: id, object: id) -> bool {
    let kind = env
        .objc
        .borrow::<NSPredicateHostObject>(predicate)
        .kind
        .clone();
    match kind {
        PredicateKind::Constant(value) => value,
        PredicateKind::Format(Ok(parsed)) => evaluate_expression(env, &parsed, object),
        PredicateKind::Format(Err(reason)) => {
            log!("NSPredicate evaluation unsupported: {reason}");
            false
        }
        PredicateKind::Not(p) => !evaluate_predicate(env, p, object),
        PredicateKind::And(children) => children
            .into_iter()
            .all(|p| evaluate_predicate(env, p, object)),
        PredicateKind::Or(children) => children
            .into_iter()
            .any(|p| evaluate_predicate(env, p, object)),
        PredicateKind::Block => {
            log!("NSPredicate evaluation unsupported: block/expression predicate");
            false
        }
    }
}
