/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Optional host renderer for standard UIKit table menus. UIKit remains the
//! owner of controllers, cells, values and target-actions. The host receives
//! a projection and returns typed actions on the guest event thread.
//! Custom drawing/web/image content stays in the emulator renderer.
use super::{ui_view::ui_control, ui_view_controller::modal_transition};
use crate::embed::{json_escape as quote, UIKitAction};
use crate::frameworks::foundation::ns_string::to_rust_string;
use crate::objc::{id, msg, msg_class, nil};
use crate::Environment;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Switch,
    Slider,
    Text,
    Button,
    Row,
}

#[derive(Default)]
pub(super) struct State {
    next_scene: u64,
    active: Option<Scene>,
}
struct Scene {
    serial: u64,
    presenter: id,
    controller: id,
    layer: id,
    animated: bool,
    phase: &'static str,
    targets: HashMap<u32, Kind>,
    snapshot: String,
    updated: Instant,
}

fn is(env: &mut Environment, object: id, class: &str) -> bool {
    if object == nil {
        return false;
    }
    let class = env.objc.get_known_class(class, &mut env.mem);
    msg![env; object isKindOfClass:class]
}
fn string(env: &mut Environment, value: id) -> String {
    if value == nil {
        String::new()
    } else {
        to_rust_string(env, value).into_owned()
    }
}

fn standard_container(env: &mut Environment, view: id, depth: u32) -> bool {
    if depth > 32 {
        return false;
    }
    if msg![env; view isHidden] {
        return true;
    }
    if is(env, view, "UITableView") || is(env, view, "UINavigationBar") {
        return true;
    }
    if is(env, view, "UILabel")
        || is(env, view, "UIControl")
        || is(env, view, "UIWebView")
        || is(env, view, "UIImageView")
    {
        return false;
    }
    let class: crate::objc::Class = msg![env; view class];
    let base = env.objc.get_known_class("UIView", &mut env.mem);
    let draw = env.objc.lookup_selector("drawRect:").unwrap();
    if env
        .objc
        .class_overrides_method_of_superclass(class, draw, base)
    {
        return false;
    }
    children(env, view)
        .into_iter()
        .all(|child| standard_container(env, child, depth + 1))
}
fn children(env: &mut Environment, view: id) -> Vec<id> {
    let array: id = msg![env; view subviews];
    let count: u32 = msg![env; array count];
    (0..count)
        .map(|index| msg![env; array objectAtIndex:index])
        .collect()
}

/// Only accept a single standard table. Unhandled custom content is a reason
/// to retain emulator rendering, never a reason to drop part of the interface.
fn table_in(env: &mut Environment, view: id, depth: u32) -> Option<id> {
    if depth > 32 {
        return None;
    }
    if is(env, view, "UITableView") {
        return Some(view);
    }
    let mut table = None;
    for child in children(env, view) {
        if is(env, child, "UINavigationBar") || is(env, child, "UIToolbar") {
            continue;
        }
        if let Some(found) = table_in(env, child, depth + 1) {
            if table.replace(found).is_some() {
                return None;
            }
        }
    }
    table
}

fn row_content(
    env: &mut Environment,
    view: id,
    labels: &mut Vec<String>,
    controls: &mut Vec<String>,
    targets: &mut HashMap<u32, Kind>,
    depth: u32,
) -> bool {
    if depth > 32 {
        return false;
    }
    let hidden: bool = msg![env; view isHidden];
    if hidden {
        return true;
    }
    let kind = if is(env, view, "UISwitch") {
        Some(Kind::Switch)
    } else if is(env, view, "UISlider") {
        Some(Kind::Slider)
    } else if is(env, view, "UITextField") {
        Some(Kind::Text)
    } else if is(env, view, "UIButton") {
        Some(Kind::Button)
    } else {
        None
    };
    if let Some(kind) = kind {
        let enabled: bool = msg![env; view isEnabled];
        let properties = match kind {
            Kind::Switch => {
                let value: bool = msg![env; view isOn];
                format!("\"kind\":\"switch\",\"on\":{value}")
            }
            Kind::Slider => {
                let value: f32 = msg![env; view value];
                let min: f32 = msg![env; view minimumValue];
                let max: f32 = msg![env; view maximumValue];
                let continuous: bool = msg![env; view isContinuous];
                if ![value, min, max].iter().all(|x| x.is_finite()) {
                    return false;
                }
                format!("\"kind\":\"slider\",\"value\":{value},\"min\":{min},\"max\":{max},\"continuous\":{continuous}")
            }
            Kind::Text => {
                let text: id = msg![env; view text];
                let placeholder: id = msg![env; view placeholder];
                let password: bool = msg![env; view isSecureTextEntry];
                format!(
                    "\"kind\":\"text\",\"text\":{},\"placeholder\":{},\"password\":{password}",
                    quote(&string(env, text)),
                    quote(&string(env, placeholder))
                )
            }
            Kind::Button => {
                let title: id = msg![env; view currentTitle];
                format!(
                    "\"kind\":\"button\",\"text\":{}",
                    quote(&string(env, title))
                )
            }
            Kind::Row => unreachable!(),
        };
        targets.insert(view.to_bits(), kind);
        controls.push(format!(
            "{{\"id\":{},\"enabled\":{enabled},{properties}}}",
            view.to_bits()
        ));
        return true;
    }
    if is(env, view, "UILabel") {
        let text: id = msg![env; view text];
        let text = string(env, text);
        if !text.is_empty() {
            labels.push(text);
        }
        return true;
    }
    if is(env, view, "UIImageView") {
        let image: id = msg![env; view image];
        return image == nil;
    }
    let class: crate::objc::Class = msg![env; view class];
    let base = env.objc.get_known_class("UIView", &mut env.mem);
    let draw = env.objc.lookup_selector("drawRect:").unwrap();
    if env
        .objc
        .class_overrides_method_of_superclass(class, draw, base)
    {
        return false;
    }
    for child in children(env, view) {
        if !row_content(env, child, labels, controls, targets, depth + 1) {
            return false;
        }
    }
    true
}

fn button(env: &mut Environment, item: id, targets: &mut HashMap<u32, Kind>) -> Option<String> {
    if item == nil {
        return Some("null".into());
    }
    let custom: id = msg![env; item customView];
    if custom != nil {
        return None;
    }
    let title: id = msg![env; item title];
    let mut title = string(env, title);
    if title.is_empty() {
        for child in children(env, item) {
            if is(env, child, "UILabel") {
                let text: id = msg![env; child text];
                title = string(env, text);
                break;
            }
        }
    }
    if title.is_empty() {
        return None;
    }
    let enabled: bool = msg![env; item isEnabled];
    targets.insert(item.to_bits(), Kind::Button);
    Some(format!(
        "{{\"id\":{},\"text\":{},\"enabled\":{enabled}}}",
        item.to_bits(),
        quote(&title)
    ))
}

fn project(
    env: &mut Environment,
    controller: id,
    serial: u64,
    animated: bool,
    phase: &str,
) -> Option<(String, HashMap<u32, Kind>)> {
    let navigation = is(env, controller, "UINavigationController");
    let content: id = if navigation {
        msg![env; controller topViewController]
    } else {
        controller
    };
    if content == nil {
        return None;
    }
    let view: id = msg![env; content view];
    if !standard_container(env, view, 0) {
        return None;
    }
    let table = table_in(env, view, 0)?;
    let title: id = msg![env; content title];
    let item: id = msg![env; content navigationItem];
    let left: id = msg![env; item leftBarButtonItem];
    let right: id = msg![env; item rightBarButtonItem];
    let mut targets = HashMap::new();
    let left = button(env, left, &mut targets)?;
    let right = button(env, right, &mut targets)?;
    let can_back = if navigation {
        let stack: id = msg![env; controller viewControllers];
        let count: u32 = msg![env; stack count];
        count > 1
    } else {
        false
    };
    let sections = super::ui_view::ui_table_view::native_sections(env, table);
    if sections.iter().all(|(_, cells)| cells.is_empty()) {
        return None;
    }
    if sections.iter().map(|(_, cells)| cells.len()).sum::<usize>() > 500 {
        return None;
    }
    let mut groups = Vec::new();
    for (title, cells) in sections {
        let mut rows = Vec::new();
        for cell in cells {
            let root: id = msg![env; cell contentView];
            let (mut labels, mut controls) = (Vec::new(), Vec::new());
            if !row_content(env, root, &mut labels, &mut controls, &mut targets, 0) {
                return None;
            }
            if controls.len() > 1 {
                return None;
            }
            targets.insert(cell.to_bits(), Kind::Row);
            let label = labels.first().cloned().unwrap_or_default();
            let detail = labels.into_iter().skip(1).collect::<Vec<_>>().join("\n");
            rows.push(format!(
                "{{\"id\":{},\"label\":{},\"detail\":{},\"controls\":[{}]}}",
                cell.to_bits(),
                quote(&label),
                quote(&detail),
                controls.join(",")
            ));
        }
        groups.push(format!(
            "{{\"title\":{},\"rows\":[{}]}}",
            quote(&title),
            rows.join(",")
        ));
    }
    let snapshot = format!("{{\"scene\":{serial},\"controller\":{},\"content\":{},\"title\":{},\"animated\":{animated},\"phase\":{},\"canBack\":{can_back},\"left\":{left},\"right\":{right},\"sections\":[{}]}}", controller.to_bits(), content.to_bits(), quote(&string(env, title)), quote(phase), groups.join(","));
    if snapshot.len() > 400_000 {
        return None;
    }
    Some((snapshot, targets))
}

pub(crate) fn present(
    env: &mut Environment,
    presenter: id,
    controller: id,
    animated: bool,
) -> bool {
    if !crate::embed::native_ui_available() {
        return false;
    }
    if env.framework_state.uikit.native_ui.active.is_some() {
        return false;
    }
    let serial = env
        .framework_state
        .uikit
        .native_ui
        .next_scene
        .wrapping_add(1);
    let Some((snapshot, targets)) = project(env, controller, serial, animated, "presenting") else {
        return false;
    };
    if !crate::embed::present_native_ui(&snapshot) {
        return false;
    }
    let view: id = msg![env; controller view];
    let layer: id = msg![env; view layer];
    // Hide only the render layer. UIView visibility/appearance and the guest
    // hierarchy stay intact; the host overlays the live presenting app frame.
    () = msg![env; layer setHidden:true];
    let state = &mut env.framework_state.uikit.native_ui;
    state.next_scene = serial;
    state.active = Some(Scene {
        serial,
        presenter,
        controller,
        layer,
        animated,
        phase: "presenting",
        targets,
        snapshot,
        updated: Instant::now(),
    });
    true
}

pub(crate) fn prepare_presentation(env: &mut Environment) {
    let Some(scene) = env.framework_state.uikit.native_ui.active.take() else {
        return;
    };
    let layer = scene.layer;
    () = msg![env; layer setHidden:false];
    crate::embed::present_native_ui("{}");
    if scene.phase == "presenting" {
        modal_transition::complete(
            env,
            scene.presenter,
            scene.controller,
            false,
            scene.animated,
        );
    } else if scene.phase == "dismissing" {
        modal_transition::complete(env, scene.presenter, scene.controller, true, scene.animated);
    }
}

pub(crate) fn dismiss(env: &mut Environment, controller: id, animated: bool) -> bool {
    let Some(scene) = env.framework_state.uikit.native_ui.active.as_mut() else {
        return false;
    };
    if scene.controller != controller {
        return false;
    }
    scene.phase = "dismissing";
    scene.animated = animated;
    scene.updated = Instant::now() - Duration::from_secs(1);
    refresh(env);
    true
}

pub(crate) fn refresh(env: &mut Environment) {
    let Some(scene) = env.framework_state.uikit.native_ui.active.as_ref() else {
        return;
    };
    if scene.updated.elapsed() < Duration::from_millis(75) {
        return;
    }
    let (controller, serial, animated, phase) =
        (scene.controller, scene.serial, scene.animated, scene.phase);
    let Some((snapshot, targets)) = project(env, controller, serial, animated, phase) else {
        // An app replaced the table with unsupported content. Return rendering
        // to its guest layer rather than showing a partial native interface.
        let scene = env.framework_state.uikit.native_ui.active.take().unwrap();
        let layer = scene.layer;
        () = msg![env; layer setHidden:false];
        crate::embed::present_native_ui("{}");
        if scene.phase == "presenting" {
            modal_transition::complete(
                env,
                scene.presenter,
                scene.controller,
                false,
                scene.animated,
            );
        } else if scene.phase == "dismissing" {
            modal_transition::complete(
                env,
                scene.presenter,
                scene.controller,
                true,
                scene.animated,
            );
        }
        return;
    };
    let scene = env.framework_state.uikit.native_ui.active.as_mut().unwrap();
    let changed = scene.snapshot != snapshot;
    scene.targets = targets;
    scene.updated = Instant::now();
    scene.snapshot = snapshot.clone();
    if changed {
        crate::embed::present_native_ui(&snapshot);
    }
}

pub(crate) fn handle_action(env: &mut Environment, action: UIKitAction) {
    // Guest timers can reload a table between two host frames. Revalidate its
    // currently owned controls before dereferencing any exported object ID.
    let Some(scene) = env.framework_state.uikit.native_ui.active.as_mut() else {
        return;
    };
    if action.scene != scene.serial {
        return;
    }
    scene.updated = Instant::now() - Duration::from_secs(1);
    refresh(env);
    let Some(scene) = env.framework_state.uikit.native_ui.active.as_ref() else {
        return;
    };
    if action.scene != scene.serial {
        return;
    }
    let (controller, presenter, phase, animated) = (
        scene.controller,
        scene.presenter,
        scene.phase,
        scene.animated,
    );
    if action.target == controller.to_bits() {
        match action.action.as_str() {
            "presented" if phase == "presenting" => {
                env.framework_state
                    .uikit
                    .native_ui
                    .active
                    .as_mut()
                    .unwrap()
                    .phase = "visible";
                modal_transition::complete(env, presenter, controller, false, animated);
            }
            "dismissed" if phase == "dismissing" => {
                let scene = env.framework_state.uikit.native_ui.active.take().unwrap();
                let layer = scene.layer;
                () = msg![env; layer setHidden:false];
                crate::embed::present_native_ui("{}");
                modal_transition::complete(env, presenter, controller, true, animated);
                return;
            }
            "back" if phase != "dismissing" => {
                let _: id = msg![env; controller popViewControllerAnimated:true];
            }
            "close" if phase != "dismissing" => {
                () = msg![env; controller dismissModalViewControllerAnimated:true];
            }
            _ => return,
        }
    } else if phase != "dismissing" {
        let Some(kind) = scene.targets.get(&action.target).copied() else {
            return;
        };
        let target: id = crate::mem::Ptr::from_bits(action.target);
        if kind != Kind::Row && !msg![env; target isEnabled] {
            return;
        }
        match (kind, action.action.as_str()) {
            (Kind::Switch, "switch") => {
                let value = action.value == "true";
                () = msg![env; target setOn:value animated:false];
                ui_control::send_actions(env, target, nil, ui_control::UIControlEventValueChanged);
            }
            (Kind::Slider, "slider") => {
                let Ok(value) = action.value.parse::<f32>() else {
                    return;
                };
                if !value.is_finite() {
                    return;
                }
                () = msg![env; target setValue:value];
                ui_control::send_actions(env, target, nil, ui_control::UIControlEventValueChanged);
            }
            (Kind::Text, "text") => {
                ui_control::ui_text_field::handle_edit(env, target, action.value)
            }
            (Kind::Text, "focus") => {
                if action.value == "true" {
                    let _: bool = msg![env; target becomeFirstResponder];
                } else {
                    let _: bool = msg![env; target resignFirstResponder];
                }
            }
            (Kind::Button, "button") => {
                ui_control::send_actions(env, target, nil, ui_control::UIControlEventTouchUpInside)
            }
            (Kind::Row, "row") => {
                let view: id = msg![env; controller view];
                if let Some(table) = table_in(env, view, 0) {
                    let path: id = msg![env; table indexPathForCell:target];
                    let delegate: id = msg![env; table delegate];
                    let selector = env.objc.register_host_selector(
                        "tableView:didSelectRowAtIndexPath:".into(),
                        &mut env.mem,
                    );
                    if msg![env; delegate respondsToSelector:selector] {
                        () = msg![env; delegate tableView:table didSelectRowAtIndexPath:path];
                    }
                }
            }
            _ => return,
        }
    }
    if let Some(scene) = env.framework_state.uikit.native_ui.active.as_mut() {
        scene.updated = Instant::now() - Duration::from_secs(1);
    }
    refresh(env);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frameworks::core_graphics::{CGPoint, CGRect, CGSize};
    use crate::frameworks::foundation::{ns_string::get_static_str, NSInteger, NSUInteger};
    use crate::objc::{release, SEL};
    use std::cell::{Cell, RefCell};
    thread_local! {
        static CELLS: RefCell<Vec<id>> = const { RefCell::new(Vec::new()) };
        static CHANGES: Cell<u32> = const { Cell::new(0) };
    }
    fn rows(_: &mut Environment, _: id, _: SEL, _: id, _: NSInteger) -> NSInteger {
        CELLS.with(|cells| cells.borrow().len() as NSInteger)
    }
    fn cell(env: &mut Environment, _: id, _: SEL, _: id, path: id) -> id {
        let row: NSUInteger = msg![env; path row];
        CELLS.with(|cells| cells.borrow()[row as usize])
    }
    fn title(env: &mut Environment, _: id, _: SEL, _: id, _: NSInteger) -> id {
        get_static_str(env, "Connection")
    }
    fn changed(_: &mut Environment, _: id, _: SEL, _: id) {
        CHANGES.with(|count| count.set(count.get() + 1));
    }
    fn edited(env: &mut Environment, _: id, _: SEL, field: id) {
        let text: id = msg![env; field text];
        let defaults: id = msg_class![env; NSUserDefaults standardUserDefaults];
        let key = get_static_str(env, "nativeFieldTest");
        () = msg![env; defaults setObject:text forKey:key];
    }

    #[test]
    fn native_projection_routes_controls_to_real_guest_callbacks_and_rejects_stale_actions() {
        // The host is a renderer: UIControl target-action and accepted field
        // edits remain UIKit operations, including NSUserDefaults consumers.
        // https://developer.apple.com/documentation/uikit/uicontrol
        // https://developer.apple.com/documentation/uikit/uitextfielddelegate
        let mut environment = crate::environment::touch_test_environment();
        let env = &mut environment;
        let pool: id = msg_class![env; NSAutoreleasePool new];
        let _application: id = msg_class![env; UIApplication new];
        let source: id = msg_class![env; NSObject new];
        let class = env.objc.get_known_class("NSObject", &mut env.mem);
        env.objc.add_host_method_if_absent(
            class,
            "tableView:numberOfRowsInSection:",
            &(rows as fn(&mut Environment, id, SEL, id, NSInteger) -> NSInteger),
            &mut env.mem,
        );
        env.objc.add_host_method_if_absent(
            class,
            "tableView:cellForRowAtIndexPath:",
            &(cell as fn(&mut Environment, id, SEL, id, id) -> id),
            &mut env.mem,
        );
        env.objc.add_host_method_if_absent(
            class,
            "tableView:titleForHeaderInSection:",
            &(title as fn(&mut Environment, id, SEL, id, NSInteger) -> id),
            &mut env.mem,
        );
        env.objc.add_host_method_if_absent(
            class,
            "nativeTestChanged:",
            &(changed as fn(&mut Environment, id, SEL, id)),
            &mut env.mem,
        );
        env.objc.add_host_method_if_absent(
            class,
            "nativeTestEdited:",
            &(edited as fn(&mut Environment, id, SEL, id)),
            &mut env.mem,
        );
        let changed = env.objc.lookup_selector("nativeTestChanged:").unwrap();
        let edited = env.objc.lookup_selector("nativeTestEdited:").unwrap();
        let switch: id = msg_class![env; UISwitch new];
        let slider: id = msg_class![env; UISlider new];
        let field: id = msg_class![env; UITextField new];
        () = msg![env; switch addTarget:source action:changed forControlEvents:(ui_control::UIControlEventValueChanged)];
        () = msg![env; slider addTarget:source action:changed forControlEvents:(ui_control::UIControlEventValueChanged)];
        () = msg![env; field addTarget:source action:edited forControlEvents:(ui_control::UIControlEventEditingChanged)];
        let initial = get_static_str(env, "Initial");
        () = msg![env; field setText:initial];
        CELLS.with(|cells| cells.borrow_mut().clear());
        for (label, control) in [("Enabled", switch), ("Level", slider), ("Name", field)] {
            let cell: id = msg_class![env; UITableViewCell new];
            let content: id = msg![env; cell contentView];
            let text: id = msg_class![env; UILabel new];
            let label = get_static_str(env, label);
            () = msg![env; text setText:label];
            () = msg![env; content addSubview:text];
            () = msg![env; content addSubview:control];
            release(env, text);
            CELLS.with(|cells| cells.borrow_mut().push(cell));
        }
        let frame = CGRect {
            origin: CGPoint::default(),
            size: CGSize {
                width: 320.0,
                height: 480.0,
            },
        };
        let table: id = msg_class![env; UITableView alloc];
        let table: id = msg![env; table initWithFrame:frame style:(1i32)];
        () = msg![env; table setDataSource:source];
        () = msg![env; table reloadData];
        let controller: id = msg_class![env; UIViewController new];
        () = msg![env; controller setView:table];
        let title = get_static_str(env, "Preferences");
        () = msg![env; controller setTitle:title];
        let (snapshot, targets) = project(env, controller, 7, true, "visible")
            .expect("standard UIKit table was not projected");
        assert!(
            snapshot.contains("Preferences")
                && snapshot.contains("Connection")
                && snapshot.contains("Initial")
        );
        assert!(
            snapshot.contains("\"kind\":\"switch\"")
                && snapshot.contains("\"kind\":\"slider\"")
                && snapshot.contains("\"kind\":\"text\"")
        );
        assert_eq!(targets.len(), 6);
        let layer: id = msg![env; table layer];
        env.framework_state.uikit.native_ui.active = Some(Scene {
            serial: 7,
            presenter: nil,
            controller,
            layer,
            animated: false,
            phase: "visible",
            targets,
            snapshot,
            updated: Instant::now(),
        });
        CHANGES.with(|count| count.set(0));
        let send = |env: &mut Environment, scene, target: id, action: &str, value: &str| {
            handle_action(
                env,
                UIKitAction {
                    scene,
                    target: target.to_bits(),
                    action: action.into(),
                    value: value.into(),
                },
            )
        };
        send(env, 6, switch, "switch", "true");
        send(
            env,
            7,
            crate::mem::Ptr::from_bits(0xdeadbeef),
            "switch",
            "true",
        );
        assert!(!msg![env; switch isOn]);
        assert_eq!(CHANGES.with(|count| count.get()), 0);
        send(env, 7, switch, "switch", "true");
        assert!(msg![env; switch isOn]);
        send(env, 7, slider, "slider", "0.75");
        let value: f32 = msg![env; slider value];
        assert_eq!(value, 0.75);
        assert_eq!(CHANGES.with(|count| count.get()), 2);
        () = msg![env; switch setEnabled:false];
        send(env, 7, switch, "switch", "false");
        assert!(msg![env; switch isOn]);
        assert_eq!(CHANGES.with(|count| count.get()), 2);
        send(env, 7, field, "text", "Edited");
        let defaults: id = msg_class![env; NSUserDefaults standardUserDefaults];
        let key = get_static_str(env, "nativeFieldTest");
        let saved: id = msg![env; defaults objectForKey:key];
        assert_eq!(string(env, saved), "Edited");
        // An unsupported image-bearing cell must return to guest rendering.
        let image: id = msg_class![env; UIImage alloc];
        let image: id = msg![env; image initWithData:nil];
        let image_view: id = msg_class![env; UIImageView alloc];
        let image_view: id = msg![env; image_view initWithImage:image];
        let first = CELLS.with(|cells| cells.borrow()[0]);
        let content: id = msg![env; first contentView];
        () = msg![env; content addSubview:image_view];
        assert!(project(env, controller, 7, true, "visible").is_none());
        env.framework_state.uikit.native_ui.active = None;
        release(env, image_view);
        release(env, image);
        release(env, controller);
        release(env, table);
        CELLS.with(|cells| {
            for cell in cells.borrow_mut().drain(..) {
                release(env, cell);
            }
        });
        for object in [field, slider, switch, source, pool] {
            release(env, object);
        }
    }
}
