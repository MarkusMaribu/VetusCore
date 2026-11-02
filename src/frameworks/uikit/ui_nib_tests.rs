/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
use super::*;
use crate::frameworks::core_graphics::CGRect;
use crate::frameworks::foundation::ns_dictionary::dict_from_keys_and_objects;
use nibarchive::{ClassName, NIBArchive, Object, Value, ValueVariant};

fn count_awake(env: &mut Environment, receiver: id, _: crate::objc::SEL) {
    let count: crate::frameworks::foundation::NSInteger = msg![env; receiver tag];
    () = msg![env; receiver setTag:(count + 1)];
}

// File's Owner and other proxies remain available to wire outlets, but are
// excluded from the public top-level array. The returned array must own its
// real views after the decoder and its autoreleased objects are released.
// https://developer.apple.com/documentation/foundation/bundle/loadnibnamed(_:owner:options:)
#[test]
fn nib_top_level_objects_exclude_placeholders_and_outlive_decoder() {
    let bounds = |width: f32| {
        let mut data = vec![6];
        for number in [0.0f32, 0.0, width, 20.0] {
            data.extend_from_slice(&number.to_le_bytes());
        }
        ValueVariant::Data(data)
    };
    let archive = NIBArchive::new(
        vec![
            Object::new(0, 0, 1),
            Object::new(1, 1, 6),
            Object::new(2, 7, 1),
            Object::new(2, 8, 1),
            Object::new(2, 9, 1),
            Object::new(3, 10, 1),
            Object::new(3, 11, 1),
            Object::new(4, 12, 1),
            Object::new(4, 13, 1),
            Object::new(4, 14, 1),
        ],
        [
            "UINibTopLevelObjectsKey",
            "NSInlinedValue",
            "UINibEncoderEmptyKey",
            "UIProxiedObjectIdentifier",
            "UIBounds",
            "NS.bytes",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        vec![
            Value::new(0, ValueVariant::ObjectRef(1)),
            Value::new(1, ValueVariant::Bool(true)),
            Value::new(2, ValueVariant::ObjectRef(2)),
            Value::new(2, ValueVariant::ObjectRef(3)),
            Value::new(2, ValueVariant::ObjectRef(4)),
            Value::new(2, ValueVariant::ObjectRef(5)),
            Value::new(2, ValueVariant::ObjectRef(6)),
            Value::new(3, ValueVariant::ObjectRef(7)),
            Value::new(3, ValueVariant::ObjectRef(8)),
            Value::new(3, ValueVariant::ObjectRef(9)),
            Value::new(4, bounds(11.0)),
            Value::new(4, bounds(22.0)),
            Value::new(5, ValueVariant::Data(b"IBFilesOwner".to_vec())),
            Value::new(5, ValueVariant::Data(b"IBFirstResponder".to_vec())),
            Value::new(5, ValueVariant::Data(b"External".to_vec())),
        ],
        ["NSObject", "NSArray", "UIProxyObject", "UIView", "NSString"]
            .into_iter()
            .map(|name| ClassName::new(name.into(), vec![]))
            .collect(),
    )
    .unwrap()
    .to_bytes();
    let mut environment = crate::environment::touch_test_environment();
    let env = &mut environment;
    let pool: id = msg_class![env; NSAutoreleasePool new];
    let view_class = env.objc.get_known_class("UIView", &mut env.mem);
    env.objc.add_host_method_if_absent(
        view_class,
        "_testNibAwake",
        &(count_awake as fn(&mut Environment, id, crate::objc::SEL)),
        &mut env.mem,
    );
    let observer = env.objc.lookup_selector("_testNibAwake").unwrap();
    let method = crate::objc::class_getInstanceMethod(env, view_class, observer);
    let imp = crate::objc::method_getImplementation(env, method);
    let awake = env.objc.lookup_selector("awakeFromNib").unwrap();
    crate::objc::class_replaceMethod(env, view_class, awake, imp, crate::mem::ConstPtr::null());
    let owner: id = msg_class![env; UIView new];
    let external: id = msg_class![env; UIView new];
    let external_key = get_static_str(env, "External");
    let replacements = dict_from_keys_and_objects(env, &[(external_key, external)]);
    let nib_class = env.objc.get_known_class("UINib", &mut env.mem);
    let nib = env.objc.alloc_object(
        nib_class,
        Box::new(UINibHostObject {
            file_owner: owner,
            external_objects: replacements,
            ..Default::default()
        }),
        &mut env.mem,
    );
    let length = archive.len() as u32;
    let bytes = env.mem.alloc(length);
    env.mem
        .bytes_at_mut(bytes.cast(), length)
        .copy_from_slice(&archive);
    let data: id =
        msg_class![env; NSData dataWithBytesNoCopy:bytes length:length freeWhenDone:true];
    let decoder: id = msg_class![env; _touchHLE_NIBArchiveDecoder alloc];
    let decoder: id = msg![env; decoder _touchHLE_initForReadingWithData:data];
    () = msg![env; decoder setDelegate:nib];
    let result = decode_top_level_objects(env, nib, decoder);
    let count: NSUInteger = msg![env; result count];
    assert_eq!(
        count, 2,
        "a placeholder displaced the nib's actual first view"
    );
    let key = get_static_str(env, "UINibTopLevelObjectsKey");
    let raw: id = msg![env; decoder decodeObjectForKey:key];
    let decoded_owner: id = msg![env; raw objectAtIndex:0u32];
    let decoded_external: id = msg![env; raw objectAtIndex:2u32];
    assert_eq!(decoded_owner, owner);
    assert_eq!(decoded_external, external);
    awaken_nib_objects(env, nib, raw);
    let owner_awakes: crate::frameworks::foundation::NSInteger = msg![env; owner tag];
    let external_awakes: crate::frameworks::foundation::NSInteger = msg![env; external tag];
    assert_eq!(
        (owner_awakes, external_awakes),
        (0, 0),
        "a placeholder was reinitialized"
    );
    release(env, decoder);
    release(env, nib);
    () = msg![env; pool drain];
    for (index, width) in [(0u32, 11.0), (1u32, 22.0)] {
        let view: id = msg![env; result objectAtIndex:index];
        let bounds: CGRect = msg![env; view bounds];
        let actual_width = bounds.size.width;
        assert_eq!(actual_width, width, "the returned views changed order");
        let awakes: crate::frameworks::foundation::NSInteger = msg![env; view tag];
        assert_eq!(
            awakes, 1,
            "a nil proxy stopped the remaining awakeFromNib callbacks"
        );
        assert_eq!(env.objc.get_refcount(view).get(), 1);
    }
    release(env, result);
    assert_eq!(env.objc.get_refcount(owner).get(), 1);
    assert_eq!(env.objc.get_refcount(external).get(), 2);
    release(env, replacements);
    release(env, external);
    release(env, owner);
}

// A nib-built cell has one content view containing its controls. Replacing it
// with an empty overlay breaks both resizing and hit testing of those controls.
// https://developer.apple.com/documentation/uikit/uitableviewcell/contentview
#[test]
fn archived_cell_keeps_its_content_view_and_delivers_switch_touches() {
    use crate::frameworks::core_graphics::{CGPoint, CGSize};
    use crate::frameworks::uikit::ui_touch::handle_event;
    use crate::window::{Event, FingerId};
    let packed = |numbers: &[f32]| {
        let mut bytes = vec![6];
        for number in numbers {
            bytes.extend_from_slice(&number.to_le_bytes());
        }
        ValueVariant::Data(bytes)
    };
    let archive = NIBArchive::new(
        vec![
            Object::new(0, 0, 2),
            Object::new(1, 2, 4),
            Object::new(2, 6, 2),
            Object::new(3, 8, 5),
            Object::new(2, 13, 2),
            Object::new(4, 15, 4),
        ],
        [
            "cell",
            "switch",
            "UIBounds",
            "UICenter",
            "UISubviews",
            "UIContentView",
            "NSInlinedValue",
            "UINibEncoderEmptyKey",
            "UIAutoresizeSubviews",
            "UIAutoresizingMask",
            "UISwitchOn",
            "UISwitchEnabled",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        vec![
            Value::new(0, ValueVariant::ObjectRef(1)),
            Value::new(1, ValueVariant::ObjectRef(5)),
            Value::new(2, packed(&[0.0, 0.0, 200.0, 44.0])),
            Value::new(3, packed(&[100.0, 22.0])),
            Value::new(4, ValueVariant::ObjectRef(2)),
            Value::new(5, ValueVariant::ObjectRef(3)),
            Value::new(6, ValueVariant::Bool(true)),
            Value::new(7, ValueVariant::ObjectRef(3)),
            Value::new(2, packed(&[0.0, 0.0, 200.0, 44.0])),
            Value::new(3, packed(&[100.0, 22.0])),
            Value::new(4, ValueVariant::ObjectRef(4)),
            Value::new(8, ValueVariant::Bool(true)),
            Value::new(9, ValueVariant::Int8(18)),
            Value::new(6, ValueVariant::Bool(true)),
            Value::new(7, ValueVariant::ObjectRef(5)),
            Value::new(2, packed(&[0.0, 0.0, 94.0, 27.0])),
            Value::new(3, packed(&[47.0, 13.5])),
            Value::new(10, ValueVariant::Bool(false)),
            Value::new(11, ValueVariant::Bool(true)),
        ],
        [
            "NSObject",
            "UITableViewCell",
            "NSArray",
            "UITableViewCellContentView",
            "UISwitch",
        ]
        .into_iter()
        .map(|name| ClassName::new(name.into(), vec![]))
        .collect(),
    )
    .unwrap()
    .to_bytes();
    let mut environment = crate::environment::touch_test_environment();
    let env = &mut environment;
    let pool: id = msg_class![env; NSAutoreleasePool new];
    let length = archive.len() as u32;
    let bytes = env.mem.alloc(length);
    env.mem
        .bytes_at_mut(bytes.cast(), length)
        .copy_from_slice(&archive);
    let data: id =
        msg_class![env; NSData dataWithBytesNoCopy:bytes length:length freeWhenDone:true];
    let decoder: id = msg_class![env; _touchHLE_NIBArchiveDecoder alloc];
    let decoder: id = msg![env; decoder _touchHLE_initForReadingWithData:data];
    let key = get_static_str(env, "cell");
    let cell: id = msg![env; decoder decodeObjectForKey:key];
    let key = get_static_str(env, "switch");
    let control: id = msg![env; decoder decodeObjectForKey:key];
    let content: id = msg![env; cell contentView];
    let actual_content: id = msg![env; control superview];
    assert_eq!(
        content, actual_content,
        "the cell replaced its archived content view"
    );
    let children: id = msg![env; cell subviews];
    let count: NSUInteger = msg![env; children count];
    assert_eq!(count, 1, "an empty view overlays the archived controls");
    let frame = CGRect {
        origin: CGPoint::default(),
        size: CGSize {
            width: 300.0,
            height: 44.0,
        },
    };
    () = msg![env; cell setFrame:frame];
    let bounds: CGRect = msg![env; content bounds];
    let width = bounds.size.width;
    assert_eq!(width, 300.0);
    let hit: id = msg![env; cell hitTest:(CGPoint { x: 20.0, y: 12.0 }) withEvent:nil];
    assert_eq!(
        hit, control,
        "the empty content view intercepted the switch tap"
    );
    let _application: id = msg_class![env; UIApplication new];
    let window: id = msg_class![env; UIWindow alloc];
    let window: id = msg![env; window initWithFrame:frame];
    () = msg![env; window addSubview:cell];
    () = msg![env; window setHidden:false];
    let points = || std::collections::HashMap::from([(FingerId::Mouse, (20.0, 12.0))]);
    handle_event(env, Event::TouchesDown(points()));
    handle_event(env, Event::TouchesUp(points()));
    assert!(msg![env; control isOn]);
    () = msg![env; cell removeFromSuperview];
    release(env, window);
    release(env, decoder);
    () = msg![env; pool drain];
}

#[test]
fn archived_image_container_keeps_interaction_and_delivers_text_field_touches() {
    use crate::frameworks::core_graphics::{CGPoint, CGSize};
    use crate::frameworks::uikit::ui_touch::handle_event;
    use crate::window::{Event, FingerId};

    let packed = |numbers: &[f32]| {
        let mut bytes = vec![6];
        for number in numbers {
            bytes.extend_from_slice(&number.to_le_bytes());
        }
        ValueVariant::Data(bytes)
    };
    // A UIImageView may contain interactive controls when enabled in a nib.
    // Preserve the archived setting instead of applying the programmatic
    // initializer's disabled default after UIView has decoded the hierarchy.
    // https://developer.apple.com/documentation/uikit/uiimageview/isuserinteractionenabled
    // https://github.com/touchHLE/touchHLE/blob/trunk/src/frameworks/uikit/ui_view/ui_image_view.rs
    for disabled in [None, Some(false), Some(true)] {
        let mut values = vec![
            Value::new(0, ValueVariant::ObjectRef(1)),
            Value::new(1, ValueVariant::ObjectRef(3)),
            Value::new(2, packed(&[0.0, 0.0, 460.0, 300.0])),
            Value::new(3, packed(&[240.0, 160.0])),
            Value::new(4, ValueVariant::ObjectRef(2)),
        ];
        if let Some(disabled) = disabled {
            values.push(Value::new(7, ValueVariant::Bool(disabled)));
        }
        let array_start = values.len();
        values.extend([
            Value::new(5, ValueVariant::Bool(true)),
            Value::new(6, ValueVariant::ObjectRef(3)),
            Value::new(2, packed(&[0.0, 0.0, 47.0, 30.0])),
            Value::new(3, packed(&[420.5, 181.0])),
        ]);
        let archive = NIBArchive::new(
            vec![
                Object::new(0, 0, 2),
                Object::new(1, 2, (array_start - 2) as i32),
                Object::new(2, array_start as i32, 2),
                Object::new(3, (array_start + 2) as i32, 2),
            ],
            [
                "image", "field", "UIBounds", "UICenter", "UISubviews",
                "NSInlinedValue", "UINibEncoderEmptyKey", "UIUserInteractionDisabled",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            values,
            ["NSObject", "UIImageView", "NSArray", "UITextField"]
                .into_iter()
                .map(|name| ClassName::new(name.into(), vec![]))
                .collect(),
        )
        .unwrap()
        .to_bytes();
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let length = archive.len() as u32;
        let bytes = env.mem.alloc(length);
        env.mem.bytes_at_mut(bytes.cast(), length).copy_from_slice(&archive);
        let data: id =
            msg_class![env; NSData dataWithBytesNoCopy:bytes length:length freeWhenDone:true];
        let decoder: id = msg_class![env; _touchHLE_NIBArchiveDecoder alloc];
        let decoder: id = msg![env; decoder _touchHLE_initForReadingWithData:data];
        let key = get_static_str(env, "image");
        let image: id = msg![env; decoder decodeObjectForKey:key];
        let key = get_static_str(env, "field");
        let field: id = msg![env; decoder decodeObjectForKey:key];
        let interactive: bool = msg![env; image isUserInteractionEnabled];
        assert_eq!(interactive, disabled != Some(true));
        let parent: id = msg![env; field superview];
        assert_eq!(parent, image);

        let frame = CGRect {
            origin: CGPoint::default(),
            size: CGSize { width: 480.0, height: 320.0 },
        };
        let root: id = msg_class![env; UIView alloc];
        let root: id = msg![env; root initWithFrame:frame];
        () = msg![env; root addSubview:image];
        let point = CGPoint { x: 430.5, y: 191.0 };
        let hit: id = msg![env; root hitTest:point withEvent:nil];
        assert_eq!(hit, if interactive { field } else { root });

        let _application: id = msg_class![env; UIApplication new];
        let window: id = msg_class![env; UIWindow alloc];
        let window: id = msg![env; window initWithFrame:frame];
        () = msg![env; window addSubview:root];
        () = msg![env; window setHidden:false];
        let points = || std::collections::HashMap::from([
            (FingerId::Mouse, (point.x, point.y)),
        ]);
        handle_event(env, Event::TouchesDown(points()));
        handle_event(env, Event::TouchesUp(points()));
        let editing: bool = msg![env; field isFirstResponder];
        assert_eq!(editing, interactive);
        if editing { let _: bool = msg![env; field resignFirstResponder]; }

        let programmatic: id = msg_class![env; UIImageView new];
        let interactive: bool = msg![env; programmatic isUserInteractionEnabled];
        assert!(!interactive, "programmatic image views must keep their default");
        release(env, programmatic);
        () = msg![env; root removeFromSuperview];
        release(env, window);
        release(env, root);
        release(env, decoder);
        () = msg![env; pool drain];
    }
}
