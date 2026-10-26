/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::*;
use crate::frameworks::uikit::ui_touch::handle_event;
use crate::frameworks::foundation::NSInteger;
use crate::window::{Event, FingerId};
use std::collections::HashMap;

fn first_action(env: &mut crate::Environment, receiver: id, _: SEL, _: id) {
    let old: NSInteger = msg![env; receiver tag];
    () = msg![env; receiver setTag:(old * 10 + 1)];
}
fn second_action(env: &mut crate::Environment, receiver: id, _: SEL, _: id) {
    let old: NSInteger = msg![env; receiver tag];
    () = msg![env; receiver setTag:(old * 10 + 2)];
}
fn actions(env: &mut crate::Environment) -> (SEL, SEL) {
    let class = env.objc.get_known_class("UIView", &mut env.mem);
    env.objc.add_host_method_if_absent(class, "_testBarFirst:",
        &(first_action as fn(&mut crate::Environment, id, SEL, id)), &mut env.mem);
    env.objc.add_host_method_if_absent(class, "_testBarSecond:",
        &(second_action as fn(&mut crate::Environment, id, SEL, id)), &mut env.mem);
    (env.objc.lookup_selector("_testBarFirst:").unwrap(), env.objc.lookup_selector("_testBarSecond:").unwrap())
}
fn image(env: &mut crate::Environment) -> id {
    let image: id = msg_class![env; UIImage alloc];
    msg![env; image initWithData:nil]
}
fn mount(env: &mut crate::Environment, item: id) -> (id, id) {
    let _application: id = msg_class![env; UIApplication new];
    let frame = CGRect { origin: CGPoint::default(), size: CGSize { width: 100.0, height: 100.0 } };
    let window: id = msg_class![env; UIWindow alloc];
    let window: id = msg![env; window initWithFrame:frame];
    let toolbar: id = msg_class![env; UIToolbar alloc];
    let toolbar: id = msg![env; toolbar initWithFrame:frame];
    let items: id = msg_class![env; NSArray arrayWithObject:item];
    () = msg![env; toolbar setItems:items];
    () = msg![env; window addSubview:toolbar];
    () = msg![env; window setHidden:false];
    (window, toolbar)
}
fn tap(env: &mut crate::Environment) {
    let point = || HashMap::from([(FingerId::Mouse, (20.0, 20.0))]);
    handle_event(env, Event::TouchesDown(point()));
    handle_event(env, Event::TouchesUp(point()));
}

// Image items remain target/action objects: the image is decoration rather
// than a customView which the toolbar would mount without an interactive item.
// https://developer.apple.com/documentation/uikit/uibarbuttonitem/init(image:style:target:action:)
#[test]
fn image_bar_item_is_interactive_and_keeps_its_custom_view_nil() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let pool: id = msg_class![env; NSAutoreleasePool new];
    let (action, _) = actions(env);
    let receiver: id = msg_class![env; UIView new];
    let picture = image(env);
    let item: id = msg_class![env; UIBarButtonItem alloc];
    let item: id = msg![env; item initWithImage:picture style:(UIBarButtonItemStyle::Plain) target:receiver action:action];
    let custom: id = msg![env; item customView];
    assert_eq!(custom, nil);
    let (window, toolbar) = mount(env, item);
    let subviews: id = msg![env; toolbar subviews];
    let button_view: id = msg![env; subviews objectAtIndex:0u32];
    assert_eq!(button_view, item);
    tap(env);
    let count: NSInteger = msg![env; receiver tag];
    assert_eq!(count, 1, "the internal image view swallowed the bar item's tap");
    () = msg![env; toolbar setItems:nil];
    for object in [window, toolbar, item, picture, receiver, pool] { release(env, object); }
}

#[test]
fn bar_target_action_setters_replace_the_bridge_and_allow_nil_action() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let pool: id = msg_class![env; NSAutoreleasePool new];
    let (first, second) = actions(env);
    let receiver: id = msg_class![env; UIView new];
    let other: id = msg_class![env; UIView new];
    let picture = image(env);
    let item: id = msg_class![env; UIBarButtonItem alloc];
    let item: id = msg![env; item initWithImage:picture style:(UIBarButtonItemStyle::Plain) target:receiver action:first];
    let (window, toolbar) = mount(env, item);
    tap(env);
    () = msg![env; item setTarget:other];
    () = msg![env; item setAction:second];
    tap(env);
    let old_count: NSInteger = msg![env; receiver tag];
    let new_count: NSInteger = msg![env; other tag];
    assert_eq!((old_count, new_count), (1, 2), "setters left an obsolete or duplicate action installed");
    () = msg![env; item setAction:(SEL::null())];
    let action: SEL = msg![env; item action];
    assert!(action.is_null());
    tap(env);
    let new_count: NSInteger = msg![env; other tag];
    assert_eq!(new_count, 2);
    assert_eq!(env.objc.get_refcount(receiver).get(), 1);
    assert_eq!(env.objc.get_refcount(other).get(), 1, "the bar item retained its weak target");
    () = msg![env; toolbar setItems:nil];
    for object in [window, toolbar, item, picture, receiver, other, pool] { release(env, object); }
}

#[test]
fn a_nil_bar_target_routes_the_action_through_the_responder_chain() {
    let mut env = crate::environment::touch_test_environment();
    let env = &mut env;
    let pool: id = msg_class![env; NSAutoreleasePool new];
    let (action, _) = actions(env);
    let picture = image(env);
    let item: id = msg_class![env; UIBarButtonItem alloc];
    let item: id = msg![env; item initWithImage:picture style:(UIBarButtonItemStyle::Plain) target:nil action:action];
    let (window, toolbar) = mount(env, item);
    tap(env);
    let count: NSInteger = msg![env; item tag];
    assert_eq!(count, 1, "nil target prevented responder-chain action dispatch");
    () = msg![env; toolbar setItems:nil];
    for object in [window, toolbar, item, picture, pool] { release(env, object); }
}

// A synthetic archive keeps this regression independent of any app's assets.
// iPhone OS 3.x NIB evidence uses UIIsSystemItem + UISystemItem; the public
// enum contract is documented here:
// https://developer.apple.com/documentation/uikit/uibarbuttonitem/systemitem
#[test]
fn archived_system_items_decode_their_identity_and_do_not_draw_done_for_spacers() {
    use nibarchive::{ClassName, NIBArchive, Object, Value, ValueVariant};
    for key in ["UISystemItem", "UISystemItemIdentifier"] {
        for (value, expected, text) in [
            (5, UIBarButtonSystemItem::FlexibleSpace, None),
            (16, UIBarButtonSystemItem::Trash, Some("Trash")),
            (9, UIBarButtonSystemItem::Action, Some("Action")),
        ] {
            let archive = NIBArchive::new(
                vec![Object::new(0, 0, 1), Object::new(1, 1, 4)],
                vec!["item".into(), "UIIsSystemItem".into(), key.into(), "UIEnabled".into(), "UIBounds".into()],
                vec![
                    Value::new(0, ValueVariant::ObjectRef(1)),
                    Value::new(1, ValueVariant::Bool(true)),
                    Value::new(2, ValueVariant::Int8(value)),
                    Value::new(3, ValueVariant::Bool(true)),
                    // The emulator backs bar items with a UIView. Supply its
                    // bounds so decoding stays independent of a host window.
                    Value::new(4, ValueVariant::Data([vec![6], vec![0; 16]].concat())),
                ],
                vec![ClassName::new("NSObject".into(), vec![]), ClassName::new("UIBarButtonItem".into(), vec![])],
            ).unwrap().to_bytes();
            let mut env = crate::environment::touch_test_environment();
            let env = &mut env;
            let pool: id = msg_class![env; NSAutoreleasePool new];
            let length = archive.len() as u32;
            let buffer = env.mem.alloc(length);
            env.mem.bytes_at_mut(buffer.cast(), length).copy_from_slice(&archive);
            let data: id = msg_class![env; NSData dataWithBytesNoCopy:buffer length:length freeWhenDone:true];
            let decoder: id = msg_class![env; _touchHLE_NIBArchiveDecoder alloc];
            let decoder: id = msg![env; decoder _touchHLE_initForReadingWithData:data];
            let item_key = get_static_str(env, "item");
            let item: id = msg![env; decoder decodeObjectForKey:item_key];
            let system: UIBarButtonSystemItem = msg![env; item systemItem];
            assert_eq!(system, expected, "archive key {key} lost its system-item identity");
            let label: id = msg![env; item label];
            if let Some(expected) = text {
                assert_ne!(label, nil);
                let text: id = msg![env; label text];
                assert_eq!(to_rust_string(env, text), expected);
            } else {
                assert_eq!(label, nil, "a flexible spacer was rendered as a Done button");
            }
            release(env, decoder);
            release(env, pool);
        }
    }
}
