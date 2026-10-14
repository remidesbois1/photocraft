//! Text sessions through the real Layers panel: select commits before changing the active
//! layer, while double-clicks, multi-selection, shortcuts and Type Masks keep their behavior.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use photocraft_doc::{LayerContent, LayerId};
use serde_json::json;

use crate::PhotocraftApp;
use crate::state::Tool;

fn harness() -> (Harness<'static, PhotocraftApp>, [LayerId; 3]) {
    let mut session = photocraft_engine::Session::new();
    session.execute("file.new", json!({"width": 480, "height": 320})).unwrap();
    let text = LayerId(session.execute("type.create", json!({"text": "Original", "size": 36, "x": 80, "y": 140})).unwrap()["layer"].as_u64().unwrap());
    let others = ["Middle", "Top"].map(|name| LayerId(session.execute("layer.new.layer", json!({"name": name})).unwrap()["layer"].as_u64().unwrap()));
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_step_dt(1.0 / 60.0).with_max_steps(64).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, crate::theme::ThemeKind::ProMedium);
        let mut app = PhotocraftApp::new(session, crate::Services::default());
        app.ui.theme = crate::theme::ThemeKind::ProMedium;
        app.ui.dock.collapsed = vec![crate::dock::Group::Color, crate::dock::Group::Properties, crate::dock::Group::History, crate::dock::Group::Navigator];
        app
    });
    h.run_steps(8);
    (h, [text, others[0], others[1]])
}

fn row(h: &Harness<'_, PhotocraftApp>, id: LayerId) -> crate::layer_row_ui::RowRects {
    crate::layer_row_ui::recorded(&h.ctx).into_iter().find(|row| row.layer == id.0).expect("layer row drawn")
}

fn pointer_click(h: &mut Harness<'_, PhotocraftApp>, at: Pos2, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.hover_at(at);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers });
        h.step();
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn click_layer(h: &mut Harness<'_, PhotocraftApp>, id: LayerId, modifiers: Modifiers) {
    // A later single click must not become a double-click on the preceding click's row name.
    h.run_steps(40);
    let at = row(h, id).name.expect("layer name drawn").center();
    pointer_click(h, at, modifiers);
}

fn double_click_type(h: &mut Harness<'_, PhotocraftApp>, id: LayerId) {
    h.run_steps(40);
    let rect = row(h, id).row;
    // Root-layer thumbnail centre in the Pro theme (24 pt thumbnail after the eye column).
    let at = pos2(rect.left() + 46.0, rect.center().y);
    pointer_click(h, at, Modifiers::NONE);
    pointer_click(h, at, Modifiers::NONE);
    let ed = h.state().ui.text_edit.as_ref().expect("double-click enters text editing");
    assert_eq!(ed.layer, id.0);
    assert!(!ed.mask);
}

fn type_text(h: &mut Harness<'_, PhotocraftApp>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run_steps(2);
}

fn text(h: &Harness<'_, PhotocraftApp>, id: LayerId) -> String {
    let layer = h.state().session.active().unwrap().doc.layer(id).expect("text layer preserved");
    let LayerContent::Text(text) = &layer.content else { panic!("layer must remain editable text") };
    text.text.clone()
}

fn history_len(h: &Harness<'_, PhotocraftApp>) -> usize {
    h.state().session.active().unwrap().history.past_len()
}

fn assert_committed(h: &Harness<'_, PhotocraftApp>, edited: LayerId, selected: LayerId) {
    assert!(h.state().ui.text_edit.is_none(), "layer click ends editing");
    assert_eq!(text(h, edited), "Changed");
    let st = h.state().session.active().unwrap();
    assert_eq!(st.active_layer, Some(selected));
    assert_eq!(st.selected_layers(), [selected]);
    assert!(!h.state().ui.status_error, "{}", h.state().ui.status);
}

#[test]
fn same_layer_click_commits_and_keeps_editable_text_selected() {
    for vertical in [false, true] {
        let (mut h, [a, ..]) = harness();
        if vertical {
            h.state_mut().run("type.orientation.vertical", json!({"layer": a.0})).unwrap();
            h.run_steps(2);
        }
        double_click_type(&mut h, a);
        type_text(&mut h, "Changed");
        click_layer(&mut h, a, Modifiers::NONE);
        assert_committed(&h, a, a);
        assert_eq!(h.state().ui.tool, if vertical { Tool::VerticalType } else { Tool::Type });
    }
}

#[test]
fn different_layer_click_commits_before_selecting_it() {
    let (mut h, [a, b, ..]) = harness();
    double_click_type(&mut h, a);
    type_text(&mut h, "Changed");
    click_layer(&mut h, b, Modifiers::NONE);
    assert_committed(&h, a, b);
    assert_eq!(h.state().ui.tool, Tool::Type);
}

#[test]
fn double_click_enters_editing_and_reopens_committed_text_once() {
    let (mut h, [a, ..]) = harness();
    let before = history_len(&h);
    double_click_type(&mut h, a);
    let session = h.state().ui.text_edit.as_ref().unwrap().session.clone();
    type_text(&mut h, "Changed");
    double_click_type(&mut h, a);
    assert_eq!(text(&h, a), "Changed");
    assert_ne!(h.state().ui.text_edit.as_ref().unwrap().session, session, "first click commits; second starts one fresh session");
    assert_eq!(history_len(&h), before + 1, "reopening adds no history");
    h.run_steps(10);
    assert!(h.state().ui.text_edit.is_some(), "later frames keep the new session");
    type_text(&mut h, "Second");
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(text(&h, a), "Second");
}

#[test]
fn layer_click_commit_is_one_undo_step_and_redoes() {
    for same_layer in [true, false] {
        let (mut h, [a, b, ..]) = harness();
        let before = history_len(&h);
        double_click_type(&mut h, a);
        type_text(&mut h, "Change");
        type_text(&mut h, "d");
        assert_eq!(history_len(&h), before + 1, "typing is coalesced");
        click_layer(&mut h, if same_layer { a } else { b }, Modifiers::NONE);
        assert_eq!(history_len(&h), before + 1, "commit and select add no duplicate step");
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        h.run_steps(2);
        assert_eq!(text(&h, a), "Original");
        assert_eq!(history_len(&h), before);
        h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        h.run_steps(2);
        assert_eq!(text(&h, a), "Changed");
        assert_eq!(history_len(&h), before + 1);
        assert!(h.state().ui.text_edit.is_none());
    }
}

#[test]
fn escape_and_control_enter_keep_their_commit_and_cancel_semantics() {
    for (escape_commits, modifiers, key, expected) in
        [(true, Modifiers::NONE, Key::Escape, "Changed"), (false, Modifiers::NONE, Key::Escape, "Original"), (false, Modifiers::COMMAND, Key::Enter, "Changed")]
    {
        let (mut h, [a, ..]) = harness();
        h.state_mut().run("prefs.set", json!({"path": "type.useEscToCommit", "value": escape_commits})).unwrap();
        double_click_type(&mut h, a);
        type_text(&mut h, "Changed");
        h.key_press_modifiers(modifiers, key);
        h.run_steps(2);
        assert!(h.state().ui.text_edit.is_none());
        assert_eq!(text(&h, a), expected);
        assert_eq!(h.state().ui.tool, Tool::Type);
    }
}

#[test]
fn options_bar_buttons_keep_their_commit_and_cancel_semantics() {
    for cancel in [false, true] {
        let (mut h, [a, ..]) = harness();
        double_click_type(&mut h, a);
        type_text(&mut h, "Changed");
        let label =
            if cancel { "Cancel any current edits (Esc)".into() } else { format!("Commit any current edits ({})", crate::shortcuts::pretty("Cmd+Enter")) };
        let at = h.get_by_label(&label).rect().center();
        pointer_click(&mut h, pos2(at.x as f32, at.y as f32), Modifiers::NONE);
        assert!(h.state().ui.text_edit.is_none());
        assert_eq!(text(&h, a), if cancel { "Original" } else { "Changed" });
    }
}

#[test]
fn modifier_clicks_commit_and_preserve_layer_multi_selection() {
    for modifiers in [Modifiers::COMMAND, Modifiers::SHIFT] {
        let (mut h, [a, b, c]) = harness();
        double_click_type(&mut h, a);
        type_text(&mut h, "Changed");
        let before = history_len(&h);
        click_layer(&mut h, c, modifiers);
        assert!(h.state().ui.text_edit.is_none());
        assert_eq!(text(&h, a), "Changed");
        let st = h.state().session.active().unwrap();
        assert_eq!(st.active_layer, Some(c));
        assert_eq!(st.selected_layers(), if modifiers.shift { vec![a, b, c] } else { vec![a, c] });
        assert_eq!(history_len(&h), before);
        // A further Ctrl/Cmd-click removes the clicked layer from the selection as before.
        click_layer(&mut h, c, Modifiers::COMMAND);
        assert_eq!(h.state().session.active().unwrap().selected_layers(), if modifiers.shift { vec![a, b] } else { vec![a] });
    }
}

fn create_with_canvas(h: &mut Harness<'_, PhotocraftApp>, tool: Tool) -> LayerId {
    h.state_mut().ui.tool = tool;
    h.run_steps(2);
    let app = h.state();
    let v = &app.ui.views[0];
    let xf = crate::canvas::ViewXform {
        rect: crate::rulers::content_rect(app, app.last_canvas_rect),
        zoom: v.zoom / app.canvas_ppp(),
        center: v.center,
        flip: app.ui.view.flip_horizontal,
        rotation: v.rotation,
        aspect: app.ui.view.display_aspect(),
    };
    pointer_click(h, xf.to_screen(320.0, 240.0), Modifiers::NONE);
    let ed = h.state().ui.text_edit.as_ref().expect("canvas click creates text session");
    assert!(ed.created);
    LayerId(ed.layer)
}

#[test]
fn new_text_is_named_on_layer_click_and_creation_stays_one_undo_step() {
    let (mut h, [_, b, ..]) = harness();
    let before = history_len(&h);
    let id = create_with_canvas(&mut h, Tool::Type);
    type_text(&mut h, "Changed");
    click_layer(&mut h, b, Modifiers::NONE);
    assert_committed(&h, id, b);
    assert_eq!(h.state().session.active().unwrap().doc.layer(id).unwrap().name, "Changed");
    assert_eq!(history_len(&h), before + 1);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert!(h.state().session.active().unwrap().doc.layer(id).is_none());
    h.state_mut().run("edit.redo", json!({})).unwrap();
    assert_eq!(text(&h, id), "Changed");
}

#[test]
fn empty_new_text_is_removed_on_same_or_different_layer_click() {
    for same_layer in [true, false] {
        let (mut h, [_, b, c]) = harness();
        h.state_mut().run("prefs.set", json!({"path": "type.fillNewTypeLayersWithPlaceholder", "value": false})).unwrap();
        let layers = h.state().session.active().unwrap().doc.layer_count();
        let id = create_with_canvas(&mut h, Tool::Type);
        assert_eq!(text(&h, id), "");
        click_layer(&mut h, if same_layer { id } else { b }, Modifiers::NONE);
        let st = h.state().session.active().unwrap();
        assert!(h.state().ui.text_edit.is_none());
        assert!(st.doc.layer(id).is_none(), "normal commit deletes an empty new layer");
        assert_eq!(st.doc.layer_count(), layers);
        assert_eq!(st.active_layer, Some(if same_layer { c } else { b }));
        assert!(!h.state().ui.status_error, "{}", h.state().ui.status);
    }
}

#[test]
fn empty_existing_text_is_preserved_on_layer_click() {
    let (mut h, [a, ..]) = harness();
    double_click_type(&mut h, a);
    h.key_press(Key::Backspace);
    h.run_steps(2);
    assert_eq!(text(&h, a), "");
    click_layer(&mut h, a, Modifiers::NONE);
    assert!(h.state().ui.text_edit.is_none());
    assert_eq!(text(&h, a), "");
    assert_eq!(h.state().session.active().unwrap().active_layer, Some(a));
    assert!(!h.state().ui.status_error, "{}", h.state().ui.status);
}

#[test]
fn unrelated_visibility_click_leaves_text_session_open() {
    let (mut h, [a, b, ..]) = harness();
    double_click_type(&mut h, a);
    type_text(&mut h, "Changed");
    let session = h.state().ui.text_edit.as_ref().unwrap().session.clone();
    let rect = row(&h, b).row;
    pointer_click(&mut h, pos2(rect.left() + 17.0, rect.center().y), Modifiers::NONE);
    assert_eq!(h.state().ui.text_edit.as_ref().unwrap().session, session);
    assert_eq!(text(&h, a), "Changed");
    assert_eq!(h.state().session.active().unwrap().active_layer, Some(a));
    assert!(!h.state().session.active().unwrap().doc.layer(b).unwrap().visible);
}

#[test]
fn type_mask_layer_click_commits_glyphs_and_removes_only_the_working_layer() {
    for tool in [Tool::HorizontalTypeMask, Tool::VerticalTypeMask] {
        for modifiers in [Modifiers::NONE, Modifiers::COMMAND, Modifiers::SHIFT] {
            let (mut h, [_, b, c]) = harness();
            h.state_mut().run("select.all", json!({})).unwrap();
            let selection = h.state().session.active().unwrap().doc.selection.as_ref().unwrap().content_bounds();
            let layers = h.state().session.active().unwrap().doc.layer_count();
            let before = history_len(&h);
            let working = create_with_canvas(&mut h, tool);
            assert!(h.state().ui.text_edit.as_ref().unwrap().mask);
            assert!(crate::layer_row_ui::recorded(&h.ctx).iter().all(|row| row.layer != working.0), "working layer has no panel row");
            type_text(&mut h, "I");
            let glyphs = h.state().session.active().unwrap().doc.layer(working).unwrap().surface().unwrap().content_bounds();
            click_layer(&mut h, b, modifiers);
            let st = h.state().session.active().unwrap();
            assert!(h.state().ui.text_edit.is_none());
            assert_eq!(st.doc.layer_count(), layers);
            assert!(st.doc.layer(working).is_none());
            assert_eq!(st.doc.selection.as_ref().unwrap().content_bounds(), glyphs);
            assert_eq!(st.active_layer, Some(b));
            assert_eq!(st.selected_layers(), if modifiers == Modifiers::NONE { vec![b] } else { vec![b, c] });
            assert_eq!(h.state().ui.tool, tool);
            assert_eq!(history_len(&h), before + 1, "normal Type Mask commit coalesces");
            h.state_mut().run("edit.undo", json!({})).unwrap();
            let st = h.state().session.active().unwrap();
            assert_eq!(st.doc.selection.as_ref().unwrap().content_bounds(), selection);
            assert_eq!(st.active_layer, Some(c));
            assert_eq!(st.doc.layer_count(), layers);
            assert!(!h.state().ui.status_error, "{}", h.state().ui.status);
        }
    }
}

#[test]
fn empty_type_mask_layer_click_keeps_the_selection_and_selects_the_clicked_layer() {
    let (mut h, [_, b, ..]) = harness();
    h.state_mut().run("select.all", json!({})).unwrap();
    let selection = h.state().session.active().unwrap().doc.selection.as_ref().unwrap().content_bounds();
    let working = create_with_canvas(&mut h, Tool::HorizontalTypeMask);
    click_layer(&mut h, b, Modifiers::NONE);
    let st = h.state().session.active().unwrap();
    assert!(h.state().ui.text_edit.is_none());
    assert!(st.doc.layer(working).is_none());
    assert_eq!(st.doc.selection.as_ref().unwrap().content_bounds(), selection);
    assert_eq!(st.active_layer, Some(b));
    assert!(!h.state().ui.status_error, "{}", h.state().ui.status);
}
