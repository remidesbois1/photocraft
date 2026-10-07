//! Photoshop-style Layer Style dialog: one row per effect instance on the left,
//! parameters on the right. State lives in the dialog's `fields` (JSON), so
//! automation can drive it like any other dialog:
//!
//! - `layer`: layer id; `globalLight`: the document's light angle (degrees)
//! - `selected`: `"blendingOptions"` or an instance id from `effects`
//! - `effects`: `[{id, kind, on, params, fx?}]` in the layer's effect order.
//!   `fx` is the effect snapshot the dialog loaded: the apply edits that
//!   snapshot with `params` (engine-side overlay), so everything the dialog
//!   doesn't model — gradient strokes, imported contours, PSD data — survives.
//!   An entry without `fx` is a new instance built from `params`.
//! - `p:blendingOptions`: the layer's blend mode, opacity and fill opacity.

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use photocraft_doc::effects::{FxPaint, StrokePosition};
use photocraft_doc::{Effect, Layer};
use serde_json::{Map, Value, json};

use crate::theme::Tokens;
use crate::{PhotocraftApp, widgets};

#[derive(Clone, Copy)]
enum P {
    Slider(f32, f32, &'static str),
    Color,
    Blend,
    Choice(&'static [(&'static str, &'static str)]),
    Check,
    /// A pattern from the dialog's `patternList` field (`[[id, name], …]`).
    Pattern,
}

/// Effect kinds in Photoshop's list order: (command kind, label, params).
/// The Blending Options page id (not an effect).
pub const BLENDING: &str = "blendingOptions";

pub const KINDS: &[(&str, &str)] = &[
    ("bevelEmboss", "Bevel & Emboss"),
    ("stroke", "Stroke"),
    ("innerShadow", "Inner Shadow"),
    ("innerGlow", "Inner Glow"),
    ("satin", "Satin"),
    ("colorOverlay", "Color Overlay"),
    ("gradientOverlay", "Gradient Overlay"),
    ("patternOverlay", "Pattern Overlay"),
    ("outerGlow", "Outer Glow"),
    ("dropShadow", "Drop Shadow"),
];

/// Effect kinds Photoshop offers several instances of (stored as `…Multi` lists in the PSD).
const MULTI: &[&str] = &["stroke", "dropShadow", "innerShadow", "colorOverlay", "gradientOverlay"];

fn multi(kind: &str) -> bool {
    MULTI.contains(&kind)
}

fn kind_of(e: &Effect) -> &'static str {
    photocraft_engine::layer_style::kind_of(e)
}

fn defaults(kind: &str) -> Value {
    photocraft_engine::layer_style::effect_defaults(kind)
}

/// Factory params for a new instance, with the shared light angle filled in.
fn fresh_params(kind: &str, light: f32) -> Value {
    let mut p = defaults(kind);
    if p.get("useGlobalLight").and_then(Value::as_bool) == Some(true) {
        p["angle"] = json!(light);
    }
    p
}

fn spec(kind: &str) -> &'static [(&'static str, &'static str, P)] {
    const POS: &[(&str, &str)] = &[("outside", "Outside"), ("inside", "Inside"), ("center", "Center")];
    const GSTYLE: &[(&str, &str)] = &[("linear", "Linear"), ("radial", "Radial"), ("angle", "Angle"), ("reflected", "Reflected"), ("diamond", "Diamond")];
    const BSTYLE: &[(&str, &str)] = &[("inner", "Inner Bevel"), ("outer", "Outer Bevel"), ("emboss", "Emboss"), ("pillow", "Pillow Emboss")];
    const DIR: &[(&str, &str)] = &[("up", "Up"), ("down", "Down")];
    const SRC: &[(&str, &str)] = &[("edge", "Edge"), ("center", "Center")];
    match kind {
        "dropShadow" => &[
            ("blend", "Blend Mode", P::Blend),
            ("color", "Color", P::Color),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("angle", "Angle", P::Slider(-180.0, 180.0, "°")),
            ("useGlobalLight", "Use Global Light", P::Check),
            ("distance", "Distance", P::Slider(0.0, 300.0, "px")),
            ("spread", "Spread", P::Slider(0.0, 100.0, "%")),
            ("size", "Size", P::Slider(0.0, 250.0, "px")),
            ("knocksOut", "Layer Knocks Out Drop Shadow", P::Check),
        ],
        "innerShadow" => &[
            ("blend", "Blend Mode", P::Blend),
            ("color", "Color", P::Color),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("angle", "Angle", P::Slider(-180.0, 180.0, "°")),
            ("useGlobalLight", "Use Global Light", P::Check),
            ("distance", "Distance", P::Slider(0.0, 300.0, "px")),
            ("choke", "Choke", P::Slider(0.0, 100.0, "%")),
            ("size", "Size", P::Slider(0.0, 250.0, "px")),
        ],
        "outerGlow" => &[
            ("blend", "Blend Mode", P::Blend),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("color", "Color", P::Color),
            ("spread", "Spread", P::Slider(0.0, 100.0, "%")),
            ("size", "Size", P::Slider(0.0, 250.0, "px")),
            ("range", "Range", P::Slider(1.0, 100.0, "%")),
        ],
        "innerGlow" => &[
            ("blend", "Blend Mode", P::Blend),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("color", "Color", P::Color),
            ("source", "Source", P::Choice(SRC)),
            ("choke", "Choke", P::Slider(0.0, 100.0, "%")),
            ("size", "Size", P::Slider(0.0, 250.0, "px")),
        ],
        "stroke" => &[
            ("size", "Size", P::Slider(1.0, 250.0, "px")),
            ("position", "Position", P::Choice(POS)),
            ("blend", "Blend Mode", P::Blend),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("color", "Color", P::Color),
        ],
        "colorOverlay" => &[("blend", "Blend Mode", P::Blend), ("color", "Color", P::Color), ("opacity", "Opacity", P::Slider(0.0, 100.0, "%"))],
        "gradientOverlay" => &[
            ("blend", "Blend Mode", P::Blend),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("from", "From", P::Color),
            ("to", "To", P::Color),
            ("reverse", "Reverse", P::Check),
            ("style", "Style", P::Choice(GSTYLE)),
            ("angle", "Angle", P::Slider(-180.0, 180.0, "°")),
            ("scale", "Scale", P::Slider(10.0, 150.0, "%")),
        ],
        "patternOverlay" => &[
            ("blend", "Blend Mode", P::Blend),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("pattern", "Pattern", P::Pattern),
            ("angle", "Angle", P::Slider(-180.0, 180.0, "°")),
            ("scale", "Scale", P::Slider(1.0, 1000.0, "%")),
            ("link", "Link with Layer", P::Check),
        ],
        "bevelEmboss" => &[
            ("style", "Style", P::Choice(BSTYLE)),
            ("depth", "Depth", P::Slider(1.0, 1000.0, "%")),
            ("direction", "Direction", P::Choice(DIR)),
            ("size", "Size", P::Slider(0.0, 250.0, "px")),
            ("soften", "Soften", P::Slider(0.0, 16.0, "px")),
            ("angle", "Angle", P::Slider(-180.0, 180.0, "°")),
            ("useGlobalLight", "Use Global Light", P::Check),
            ("altitude", "Altitude", P::Slider(0.0, 90.0, "°")),
        ],
        "satin" => &[
            ("blend", "Blend Mode", P::Blend),
            ("color", "Color", P::Color),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("angle", "Angle", P::Slider(-180.0, 180.0, "°")),
            ("distance", "Distance", P::Slider(0.0, 250.0, "px")),
            ("size", "Size", P::Slider(0.0, 250.0, "px")),
            ("invert", "Invert", P::Check),
        ],
        BLENDING => &[
            ("blend", "Blend Mode", P::Blend),
            ("opacity", "Opacity", P::Slider(0.0, 100.0, "%")),
            ("fillOpacity", "Fill Opacity", P::Slider(0.0, 100.0, "%")),
        ],
        _ => &[],
    }
}

fn hex(c: &photocraft_doc::Color) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Current values of an existing effect, in the command's parameter units —
/// only the keys this dialog models. Everything else (gradient strokes, multi-stop
/// gradients, imported contours) is left out so the apply overlay can't clobber it.
/// `light` is the document's global light angle: an effect that uses it shows that angle.
fn values_of(e: &Effect, light: f32) -> Value {
    let mut v = defaults(kind_of(e));
    let set = |v: &mut Value, k: &str, x: Value| {
        v[k] = x;
    };
    let drop_keys = |v: &mut Value, keys: &[&str]| {
        if let Some(o) = v.as_object_mut() {
            for k in keys {
                o.remove(*k);
            }
        }
    };
    match e {
        Effect::DropShadow(s) | Effect::InnerShadow(s) => {
            set(&mut v, "blend", json!(s.common.blend.label()));
            set(&mut v, "opacity", json!((s.common.opacity * 100.0).round()));
            set(&mut v, "color", json!(hex(&s.color)));
            set(&mut v, "angle", json!(if s.use_global_light { light } else { s.angle }));
            set(&mut v, "useGlobalLight", json!(s.use_global_light));
            set(&mut v, "distance", json!(s.distance));
            set(&mut v, if matches!(e, Effect::DropShadow(_)) { "spread" } else { "choke" }, json!((s.spread * 100.0).round()));
            set(&mut v, "size", json!(s.size));
            set(&mut v, "contour", photocraft_engine::layer_style::contour_param(&s.contour));
            set(&mut v, "noise", json!((s.noise * 100.0).round()));
            if matches!(e, Effect::DropShadow(_)) {
                set(&mut v, "knocksOut", json!(s.knocks_out));
            }
        }
        Effect::OuterGlow(g) | Effect::InnerGlow(g) => {
            set(&mut v, "blend", json!(g.common.blend.label()));
            set(&mut v, "opacity", json!((g.common.opacity * 100.0).round()));
            if let FxPaint::Color(c) = &g.paint {
                set(&mut v, "color", json!(hex(c)));
            } else {
                drop_keys(&mut v, &["color"]);
            }
            set(&mut v, if matches!(e, Effect::OuterGlow(_)) { "spread" } else { "choke" }, json!((g.spread * 100.0).round()));
            set(&mut v, "size", json!(g.size));
            set(&mut v, "range", json!((g.range * 100.0).round()));
            set(&mut v, "contour", photocraft_engine::layer_style::contour_param(&g.contour));
            set(&mut v, "noise", json!((g.noise * 100.0).round()));
        }
        Effect::Stroke(s) => {
            set(&mut v, "blend", json!(s.common.blend.label()));
            set(&mut v, "opacity", json!((s.common.opacity * 100.0).round()));
            set(&mut v, "size", json!(s.size));
            set(
                &mut v,
                "position",
                json!(match s.position {
                    StrokePosition::Inside => "inside",
                    StrokePosition::Center => "center",
                    _ => "outside",
                }),
            );
            if let FxPaint::Color(c) = &s.paint {
                set(&mut v, "color", json!(hex(c)));
            } else {
                // A gradient/pattern stroke: the dialog's colour field doesn't model
                // the paint, so the key is left out and the snapshot carries it.
                drop_keys(&mut v, &["color"]);
            }
        }
        Effect::ColorOverlay { common, color } => {
            set(&mut v, "blend", json!(common.blend.label()));
            set(&mut v, "opacity", json!((common.opacity * 100.0).round()));
            set(&mut v, "color", json!(hex(color)));
        }
        Effect::GradientOverlay { common, gradient: g, .. } => {
            set(&mut v, "blend", json!(common.blend.label()));
            set(&mut v, "opacity", json!((common.opacity * 100.0).round()));
            // The From/To fields model a plain two-stop gradient; anything richer
            // (more stops, transparency stops) is carried by the snapshot instead.
            if g.stops.len() == 2 && g.opacity_stops.is_empty() {
                set(&mut v, "from", json!(hex(&g.stops[0].1)));
                set(&mut v, "to", json!(hex(&g.stops[1].1)));
                set(&mut v, "reverse", json!(g.reverse));
                set(
                    &mut v,
                    "style",
                    json!(match g.style {
                        photocraft_doc::effects::GradientStyle::Radial => "radial",
                        photocraft_doc::effects::GradientStyle::Angle => "angle",
                        photocraft_doc::effects::GradientStyle::Reflected => "reflected",
                        photocraft_doc::effects::GradientStyle::Diamond => "diamond",
                        photocraft_doc::effects::GradientStyle::Linear => "linear",
                    }),
                );
                set(&mut v, "angle", json!(g.angle));
                set(&mut v, "scale", json!((g.scale * 100.0).round()));
            } else {
                drop_keys(&mut v, &["from", "to", "reverse", "style", "angle", "scale"]);
            }
        }
        Effect::PatternOverlay { common, name, id, scale, angle, link, phase } => {
            set(&mut v, "blend", json!(common.blend.label()));
            set(&mut v, "opacity", json!((common.opacity * 100.0).round()));
            set(&mut v, "pattern", json!(if id.is_empty() { name } else { id }));
            set(&mut v, "scale", json!((scale * 100.0).round()));
            set(&mut v, "angle", json!(angle));
            set(&mut v, "link", json!(link));
            set(&mut v, "phaseX", json!(phase.0));
            set(&mut v, "phaseY", json!(phase.1));
        }
        Effect::Satin(s) => {
            set(&mut v, "blend", json!(s.common.blend.label()));
            set(&mut v, "opacity", json!((s.common.opacity * 100.0).round()));
            set(&mut v, "color", json!(hex(&s.color)));
            set(&mut v, "angle", json!(s.angle));
            set(&mut v, "distance", json!(s.distance));
            set(&mut v, "size", json!(s.size));
            set(&mut v, "invert", json!(s.invert));
            set(&mut v, "contour", photocraft_engine::layer_style::contour_param(&s.contour));
        }
        Effect::BevelEmboss(b) => {
            set(
                &mut v,
                "style",
                json!(match b.style {
                    photocraft_doc::effects::BevelStyle::OuterBevel => "outer",
                    photocraft_doc::effects::BevelStyle::Emboss => "emboss",
                    photocraft_doc::effects::BevelStyle::PillowEmboss => "pillow",
                    photocraft_doc::effects::BevelStyle::StrokeEmboss => "stroke",
                    photocraft_doc::effects::BevelStyle::InnerBevel => "inner",
                }),
            );
            set(
                &mut v,
                "technique",
                json!(match b.technique {
                    photocraft_doc::effects::BevelTechnique::ChiselHard => "chiselHard",
                    photocraft_doc::effects::BevelTechnique::ChiselSoft => "chiselSoft",
                    photocraft_doc::effects::BevelTechnique::Smooth => "smooth",
                }),
            );
            set(&mut v, "depth", json!((b.depth * 100.0).round()));
            set(&mut v, "size", json!(b.size));
            set(&mut v, "soften", json!(b.soften));
            set(&mut v, "angle", json!(if b.use_global_light { light } else { b.angle }));
            set(&mut v, "useGlobalLight", json!(b.use_global_light));
            set(&mut v, "altitude", json!(b.altitude));
            set(&mut v, "direction", json!(if b.up { "up" } else { "down" }));
            set(&mut v, "glossContour", photocraft_engine::layer_style::contour_param(&b.gloss_contour));
        }
    }
    v
}

/// The dialog's effect entries (JSON array of `{id, kind, on, params, fx?}`).
fn effects_of(f: &Map<String, Value>) -> &[Value] {
    f.get("effects").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn entry<'a>(f: &'a Map<String, Value>, id: &str) -> Option<&'a Value> {
    effects_of(f).iter().find(|e| e.get("id").and_then(Value::as_str) == Some(id))
}

fn entry_mut<'a>(f: &'a mut Map<String, Value>, id: &str) -> Option<&'a mut Value> {
    f.get_mut("effects").and_then(Value::as_array_mut).and_then(|a| a.iter_mut().find(|e| e.get("id").and_then(Value::as_str) == Some(id)))
}

/// Stores one edited parameter of an instance (or of the blending page).
fn set_param(f: &mut Map<String, Value>, id: &str, key: &str, value: Value) {
    if id == BLENDING {
        if let Some(o) = f.get_mut("p:blendingOptions").and_then(Value::as_object_mut) {
            o.insert(key.into(), value);
        }
        return;
    }
    if let Some(e) = entry_mut(f, id)
        && let Some(o) = e.get_mut("params").and_then(Value::as_object_mut)
    {
        o.insert(key.into(), value);
    }
}

/// A fresh instance id: one past the highest numbered `fxN` in the list.
fn next_id(f: &Map<String, Value>) -> String {
    let max = effects_of(f)
        .iter()
        .filter_map(|e| e.get("id").and_then(Value::as_str))
        .filter_map(|id| id.strip_prefix("fx"))
        .filter_map(|n| n.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("fx{}", max + 1)
}

/// Adds an instance of `kind`, right after the last one of its kind (the list,
/// and so the rendered stack, keeps the layer's effect order), and selects it.
fn add_instance(f: &mut Map<String, Value>, kind: &str) {
    let light = f.get("globalLight").and_then(Value::as_f64).unwrap_or(120.0) as f32;
    let id = next_id(f);
    let new = json!({"id": id, "kind": kind, "on": true, "params": fresh_params(kind, light)});
    if let Some(a) = f.get_mut("effects").and_then(Value::as_array_mut) {
        let pos = a.iter().rposition(|e| e.get("kind").and_then(Value::as_str) == Some(kind)).map_or(a.len(), |i| i + 1);
        a.insert(pos, new);
    }
    f.insert("selected".into(), json!(id));
}

/// Removes an instance; the selection moves to a sibling of the same kind,
/// else to the blending page.
fn remove_instance(f: &mut Map<String, Value>, id: &str) {
    let kind = entry(f, id).and_then(|e| e.get("kind").and_then(Value::as_str)).map(str::to_string);
    if let Some(a) = f.get_mut("effects").and_then(Value::as_array_mut) {
        a.retain(|e| e.get("id").and_then(Value::as_str) != Some(id));
    }
    let selected = f.get("selected").and_then(Value::as_str).unwrap_or_default().to_string();
    if selected == id {
        let sibling = kind
            .as_deref()
            .and_then(|kind| effects_of(f).iter().find(|e| e.get("kind").and_then(Value::as_str) == Some(kind)))
            .and_then(|e| e.get("id").and_then(Value::as_str))
            .map(str::to_string);
        f.insert("selected".into(), json!(sibling.unwrap_or_else(|| BLENDING.into())));
    }
}

/// Dialog fields for a layer: one entry per effect instance (all of them, in
/// the layer's order), the blending options and the selected page. `select`
/// names a kind to open: its first instance is selected (and enabled), or one
/// is created. `light` is the document's global light angle.
pub fn initial_fields(layer: &Layer, select: Option<&str>, light: f32) -> Map<String, Value> {
    let mut f = Map::new();
    f.insert("layer".into(), json!(layer.id.0));
    f.insert("globalLight".into(), json!(light));
    f.insert("preview".into(), json!(true));
    f.insert(
        format!("p:{BLENDING}"),
        json!({"blend": layer.blend.label(), "opacity": (layer.opacity * 100.0).round(), "fillOpacity": (layer.fill_opacity * 100.0).round()}),
    );
    let mut effects = Vec::new();
    for (i, e) in layer.effects.items.iter().enumerate() {
        effects.push(json!({
            "id": format!("fx{}", i + 1),
            "kind": kind_of(e),
            "on": e.enabled(),
            "params": values_of(e, light),
            "fx": serde_json::to_value(e).unwrap_or(Value::Null),
        }));
    }
    f.insert("effects".into(), Value::Array(effects));
    let selected = match select {
        None => effects_of(&f).first().and_then(|e| e.get("id").and_then(Value::as_str)).map(str::to_string).unwrap_or_else(|| "dropShadow".into()),
        Some(BLENDING) => BLENDING.to_string(),
        Some(kind) => {
            let existing = effects_of(&f)
                .iter()
                .find(|e| e.get("kind").and_then(Value::as_str) == Some(kind))
                .map(|e| e.get("id").and_then(Value::as_str).unwrap_or_default().to_string());
            match existing {
                Some(id) => {
                    if let Some(e) = entry_mut(&mut f, &id) {
                        e["on"] = json!(true);
                    }
                    id
                }
                None => {
                    add_instance(&mut f, kind);
                    f.get("selected").and_then(Value::as_str).unwrap_or_default().to_string()
                }
            }
        }
    };
    f.insert("selected".into(), json!(selected));
    f
}

pub fn open(app: &mut PhotocraftApp, select: Option<&str>) -> Option<u64> {
    let st = app.session.active()?;
    let layer = st.doc.layer(st.active_layer?)?.clone();
    let light = st.doc.global_light.angle;
    let mut f = initial_fields(&layer, select, light);
    f.insert("patternList".into(), pattern_list(app));
    Some(app.ui.open_dialog(crate::state::DialogKind::LayerStyle, f))
}

/// `[[id, name], …]` of the patterns a style can use (document's, then the library's).
pub fn pattern_list(app: &PhotocraftApp) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let doc_pats = app.session.active().map(|d| d.doc.patterns.clone()).unwrap_or_default();
    for p in doc_pats.iter().chain(app.session.patterns.items.iter()) {
        if seen.insert(p.id.clone()) {
            out.push(json!([p.id, p.display_name()]));
        }
    }
    Value::Array(out)
}

/// Apply the dialog: replace the layer's effects with the dialog's instances.
pub fn confirm(app: &mut PhotocraftApp, f: &Map<String, Value>) -> Result<Value, String> {
    apply(f, |id, p| app.run(id, p))
}

/// Runs the dialog's commands through `run`: blending options, then the whole
/// effect list in one `layer.layerStyle.replace` (instances keep their order;
/// their snapshots carry what the dialog doesn't model).
fn apply(f: &Map<String, Value>, mut run: impl FnMut(&str, Value) -> Result<Value, String>) -> Result<Value, String> {
    let layer = f.get("layer").cloned().unwrap_or(Value::Null);
    let initial_light_angle = f.get("globalLight").and_then(Value::as_f64);
    if let Some(Value::Object(bo)) = f.get(&format!("p:{BLENDING}")) {
        let mut p = Value::Object(bo.clone());
        p["layer"] = layer.clone();
        run("layer.layerStyle.blendingOptions", p)?;
    }
    let mut entries = Vec::new();
    for e in effects_of(f) {
        let (Some(kind), Some(_)) = (e.get("kind").and_then(Value::as_str), e.get("id").and_then(Value::as_str)) else { continue };
        let on = e.get("on").and_then(Value::as_bool).unwrap_or(true);
        let params = e.get("params").cloned().unwrap_or_else(|| json!({}));
        let mut params = params;
        params["enabled"] = json!(on);
        let entry = match e.get("fx").filter(|fx| fx.is_object()) {
            Some(fx) => json!({"kind": kind, "fx": fx, "params": params}),
            None => {
                // A new instance: the page shows the factory defaults merged with
                // the edits, and the engine builds from the full set.
                let mut p = defaults(kind);
                if let (Some(o), Some(pv)) = (p.as_object_mut(), params.as_object()) {
                    for (k, v) in pv {
                        o.insert(k.clone(), v.clone());
                    }
                }
                json!({"kind": kind, "params": p})
            }
        };
        entries.push(entry);
    }
    run("layer.layerStyle.replace", json!({"layer": layer, "effects": entries}))?;
    // An effect lit by the global light follows the document's light angle, so the Angle slider
    // drives that shared angle rather than the per-effect angle the compositor ignores.
    let mut light_angle: Option<f64> = None;
    for e in effects_of(f) {
        if e.get("on").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let params = e.get("params").cloned().unwrap_or_else(|| json!({}));
        if params.get("useGlobalLight").and_then(Value::as_bool) == Some(true)
            && let Some(angle) = params.get("angle").and_then(Value::as_f64)
            && initial_light_angle.is_none_or(|initial| angle != initial)
        {
            light_angle = Some(angle);
        }
    }
    if let Some(angle) = light_angle {
        run("layer.layerStyle.globalLight", json!({"angle": angle}))?;
    }
    Ok(Value::Null)
}

/// Hash of the fields that change the rendered style (not the selected page).
pub fn preview_hash(f: &Map<String, Value>) -> u64 {
    f.iter()
        .filter(|(k, _)| k.as_str() == "layer" || k.as_str() == "effects" || k.as_str() == "p:blendingOptions")
        .flat_map(|(k, v)| k.bytes().chain(v.to_string().into_bytes()))
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// `doc` with the dialog's style applied, run on a scratch session (no history) for the live preview.
pub fn preview_document(
    doc: &photocraft_doc::Document,
    patterns: &photocraft_engine::pattern_cmds::PatternLibrary,
    f: &Map<String, Value>,
) -> Option<photocraft_doc::Document> {
    let mut s = photocraft_engine::Session::new();
    s.patterns = patterns.clone();
    s.add_document(doc.clone(), None);
    apply(f, |id, p| s.execute(id, p).map_err(|e| e.to_string())).ok()?;
    s.active().map(|d| (*d.doc).clone())
}

/// Dialog body (left list, right parameters).
pub fn body(ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    let t = Tokens::get(ui.ctx());
    let selected = f.get("selected").and_then(Value::as_str).unwrap_or("dropShadow").to_string();
    ui.horizontal_top(|ui| {
        // Left: effect list.
        ui.vertical(|ui| {
            ui.set_width(190.0);
            ui.label(RichText::new(tl!("Styles")).color(t.text_faint));
            // Blending Options page (layer blend mode, opacity and fill opacity).
            let bo = ui.add(
                egui::Label::new(RichText::new(tl!("Blending Options")).color(if selected == BLENDING { t.text } else { t.text_dim })).sense(Sense::click()),
            );
            if bo.clicked() {
                f.insert("selected".into(), json!(BLENDING));
            }
            ui.add_space(4.0);
            for &(kind, label) in KINDS {
                let ids: Vec<String> = effects_of(f)
                    .iter()
                    .filter(|e| e.get("kind").and_then(Value::as_str) == Some(kind))
                    .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect();
                if ids.is_empty() {
                    // The effect isn't on the layer: a greyed row that adds it.
                    let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 26.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, t.radius_sm, t.hover.gamma_multiply(0.5));
                    }
                    ui.painter().text(
                        rect.left_center() + vec2(28.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        tl!(label),
                        egui::FontId::proportional(12.5),
                        t.text_faint,
                    );
                    if resp.clicked() {
                        add_instance(f, kind);
                    }
                    continue;
                }
                let count = ids.len();
                for id in &ids {
                    let Some(e) = entry(f, id) else { continue };
                    let kind_label = KINDS.iter().find(|k| k.0 == kind).map(|k| k.1).unwrap_or(label);
                    let mut on = e.get("on").and_then(Value::as_bool).unwrap_or(false);
                    let is_sel = id == &selected;
                    let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 26.0), Sense::click());
                    if is_sel {
                        ui.painter().rect_filled(rect, t.radius_sm, t.row_selected.gamma_multiply(if t.pro { 1.0 } else { 0.0 }).max_alpha(t.hover));
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, t.radius_sm, t.hover.gamma_multiply(0.5));
                    }
                    let cb = egui::Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(14.0, 14.0));
                    let cresp = ui.interact(cb, ui.id().with(("fxcb", kind, id)), Sense::click());
                    if on {
                        ui.painter().rect_filled(cb, 2.0, t.accent);
                        ui.painter().line_segment([cb.left_center() + vec2(3.0, 0.5), cb.center_bottom() + vec2(-1.0, -3.5)], Stroke::new(1.8, Color32::WHITE));
                        ui.painter().line_segment([cb.center_bottom() + vec2(-1.0, -3.5), cb.right_top() + vec2(-3.0, 3.5)], Stroke::new(1.8, Color32::WHITE));
                    } else {
                        ui.painter().rect_stroke(cb, 2.0, Stroke::new(1.5, t.text_faint), StrokeKind::Inside);
                    }
                    ui.painter().text(
                        rect.left_center() + vec2(28.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        tl!(kind_label),
                        egui::FontId::proportional(12.5),
                        if on || is_sel { t.text } else { t.text_dim },
                    );
                    // + adds another instance; − removes one once there are several.
                    if multi(kind) {
                        let btn = |ui: &egui::Ui, at: egui::Pos2, glyph: &str, tag: &str| {
                            let r = egui::Rect::from_center_size(at, vec2(18.0, 18.0));
                            let hresp = ui.interact(r, ui.id().with(("fxbtn", tag, id)), Sense::click());
                            let col = if hresp.hovered() { t.accent } else { t.text_faint };
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(13.0), col);
                            hresp.clicked()
                        };
                        if count > 1 && btn(ui, rect.right_center() - vec2(22.0, 0.0), "\u{2212}", "minus") {
                            remove_instance(f, id);
                            continue;
                        }
                        if btn(ui, rect.right_center() - vec2(8.0, 0.0), "+", "plus") {
                            add_instance(f, kind);
                        }
                    }
                    if cresp.clicked() {
                        on = !on;
                        if let Some(e) = entry_mut(f, id) {
                            e["on"] = json!(on);
                        }
                        f.insert("selected".into(), json!(id));
                    } else if resp.clicked() {
                        f.insert("selected".into(), json!(id));
                    }
                }
            }
        });
        widgets::vline(ui, 330.0);
        // Right: parameters of the selected page.
        ui.vertical(|ui| {
            ui.set_width(330.0);
            let sel_kind: String = if selected == BLENDING {
                BLENDING.to_string()
            } else {
                entry(f, &selected).and_then(|e| e.get("kind").and_then(Value::as_str)).unwrap_or("").to_string()
            };
            let label = KINDS.iter().find(|k| k.0 == sel_kind).map(|k| k.1).unwrap_or(if selected == BLENDING { "Blending Options" } else { "" });
            ui.horizontal(|ui| {
                ui.label(RichText::new(tl!(&label)).font(crate::theme::semibold(14.0)).color(t.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut preview = f.get("preview").and_then(Value::as_bool).unwrap_or(true);
                    if widgets::checkbox(ui, &mut preview, tl!("Preview")).changed() {
                        f.insert("preview".into(), json!(preview));
                    }
                });
            });
            ui.add_space(6.0);
            // The page shows the factory defaults under the instance's values,
            // but only real edits reach the stored params.
            let mut disp = if selected == BLENDING {
                f.get("p:blendingOptions").cloned().unwrap_or_else(|| json!({}))
            } else {
                entry(f, &selected).and_then(|e| e.get("params").cloned()).unwrap_or_else(|| json!({}))
            };
            if selected != BLENDING {
                let base = defaults(&sel_kind);
                if let (Some(o), Some(b)) = (disp.as_object_mut(), base.as_object()) {
                    for (k, v) in b {
                        o.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
            }
            for &(key, label, kind_p) in spec(&sel_kind) {
                match kind_p {
                    P::Slider(min, max, unit) => {
                        let mut v = disp.get(key).and_then(Value::as_f64).unwrap_or(min as f64) as f32;
                        if widgets::slider_row(ui, label, &mut v, min..=max, unit, None).changed() {
                            let value = json!(v.round());
                            disp[key] = value.clone();
                            set_param(f, &selected, key, value);
                        }
                    }
                    P::Color => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl!(&label)).color(t.text_dim));
                            let hexs = disp.get(key).and_then(Value::as_str).unwrap_or("#000000").to_string();
                            let mut c = parse_hex(&hexs);
                            if ui.color_edit_button_srgba(&mut c).changed() {
                                let value = json!(format!("#{r:02x}{g:02x}{b:02x}", r = c.r(), g = c.g(), b = c.b()));
                                disp[key] = value.clone();
                                set_param(f, &selected, key, value);
                            }
                        });
                    }
                    P::Blend => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl!(&label)).color(t.text_dim));
                            let mut cur = disp.get(key).and_then(Value::as_str).unwrap_or("Normal").to_string();
                            let opts: Vec<(String, &str)> =
                                photocraft_color::BlendMode::LAYER_MODES.iter().map(|m| (m.label().to_string(), m.label())).collect();
                            if widgets::dropdown(ui, &format!("fx-blend-{sel_kind}"), &mut cur, &opts, 150.0) {
                                let value = json!(cur);
                                disp[key] = value.clone();
                                set_param(f, &selected, key, value);
                            }
                        });
                    }
                    P::Choice(options) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl!(&label)).color(t.text_dim));
                            let mut cur = disp.get(key).and_then(Value::as_str).unwrap_or(options[0].0).to_string();
                            let opts: Vec<(String, &str)> = options.iter().map(|(v, l)| (v.to_string(), *l)).collect();
                            if widgets::dropdown(ui, &format!("fx-{sel_kind}-{key}"), &mut cur, &opts, 150.0) {
                                let value = json!(cur);
                                disp[key] = value.clone();
                                set_param(f, &selected, key, value);
                            }
                        });
                    }
                    P::Pattern => {
                        let list: Vec<(String, String)> = f
                            .get("patternList")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(|e| Some((e.get(0)?.as_str()?.to_string(), e.get(1)?.as_str()?.to_string()))).collect())
                            .unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tl!(&label)).color(t.text_dim));
                            let mut cur = disp
                                .get(key)
                                .and_then(Value::as_str)
                                .filter(|c| !c.is_empty())
                                .map(str::to_string)
                                .or_else(|| list.first().map(|l| l.0.clone()))
                                .unwrap_or_default();
                            let opts: Vec<(String, &str)> = list.iter().map(|(id, n)| (id.clone(), n.as_str())).collect();
                            if widgets::dropdown(ui, &format!("fx-{sel_kind}-{key}"), &mut cur, &opts, 180.0)
                                || disp.get(key).and_then(Value::as_str).is_none_or(str::is_empty)
                            {
                                let value = json!(cur);
                                disp[key] = value.clone();
                                set_param(f, &selected, key, value);
                            }
                        });
                    }
                    P::Check => {
                        let mut b = disp.get(key).and_then(Value::as_bool).unwrap_or(false);
                        if widgets::checkbox(ui, &mut b, label).changed() {
                            let value = json!(b);
                            disp[key] = value.clone();
                            set_param(f, &selected, key, value);
                        }
                    }
                }
                ui.add_space(2.0);
            }
        });
    });
}

fn parse_hex(s: &str) -> Color32 {
    let s = s.trim_start_matches('#');
    let b = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    Color32::from_rgb(b(0), b(2), b(4))
}

trait MaxAlpha {
    fn max_alpha(self, other: Color32) -> Color32;
}

impl MaxAlpha for Color32 {
    /// Fall back to `other` when this colour is fully transparent (non-Pro themes have no row colour).
    fn max_alpha(self, other: Color32) -> Color32 {
        if self.a() == 0 { other } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_doc::StrokeFx;

    fn session() -> photocraft_engine::Session {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width": 64, "height": 64})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        s
    }

    fn strokes_of(s: &photocraft_engine::Session) -> Vec<StrokeFx> {
        let d = s.active().unwrap();
        let id = d.active_layer.unwrap();
        d.doc
            .layer(id)
            .unwrap()
            .effects
            .items
            .iter()
            .filter_map(|e| match e {
                Effect::Stroke(st) => Some(st.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_kind_has_spec_and_defaults() {
        for &(kind, _) in KINDS {
            assert!(!spec(kind).is_empty(), "{kind}");
            let d = defaults(kind);
            for &(key, _, _) in spec(kind) {
                assert!(d.get(key).is_some(), "{kind}.{key} has no default");
            }
        }
    }

    #[test]
    fn initial_fields_lists_every_instance_and_selects() {
        let l = Layer::raster("x", photocraft_doc::PixelFormat::RGBA8);
        let f = initial_fields(&l, Some("stroke"), 120.0);
        assert_eq!(f["globalLight"], json!(120.0));
        // The kind isn't on the layer yet: selecting it creates the instance.
        assert_eq!(f["effects"].as_array().unwrap().len(), 1);
        assert_eq!(f["effects"][0]["kind"], json!("stroke"));
        assert_eq!(f["effects"][0]["on"], json!(true));
        assert_eq!(f["selected"], json!("fx1"));
        // Opening the blending page selects no instance.
        let f = initial_fields(&l, Some(BLENDING), 120.0);
        assert_eq!(f["selected"], json!(BLENDING));
        assert!(effects_of(&f).is_empty());
    }

    #[test]
    fn open_and_confirm_preserves_two_strokes() {
        // The manga/typesetting case: a 3 px black stroke under a 7 px white one.
        let mut s = session();
        s.execute("layer.layerStyle.stroke", json!({"size": 3, "color": "#000000"})).unwrap();
        s.execute("layer.layerStyle.stroke", json!({"size": 7, "color": "#ffffff", "add": true})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        assert_eq!(effects_of(&f).len(), 2, "both instances load");
        // Confirm without touching anything: both strokes, same order and params.
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let strokes = strokes_of(&s);
        assert_eq!(strokes.len(), 2);
        assert_eq!((strokes[0].size, strokes[1].size), (3.0, 7.0));
        assert_eq!(strokes[0].paint, FxPaint::Color(photocraft_color::Color::BLACK));
        assert_eq!(strokes[1].paint, FxPaint::Color(photocraft_color::Color::WHITE));
    }

    #[test]
    fn editing_one_instance_leaves_its_sibling_alone() {
        let mut s = session();
        s.execute("layer.layerStyle.stroke", json!({"size": 3, "color": "#000000"})).unwrap();
        s.execute("layer.layerStyle.stroke", json!({"size": 7, "color": "#ffffff", "add": true})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        set_param(&mut f, "fx1", "size", json!(5));
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let strokes = strokes_of(&s);
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[0].size, 5.0);
        assert_eq!(strokes[1].size, 7.0, "the second stroke is untouched");
        assert_eq!(strokes[1].paint, FxPaint::Color(photocraft_color::Color::WHITE));
    }

    #[test]
    fn add_and_remove_instances() {
        let mut s = session();
        s.execute("layer.layerStyle.stroke", json!({"size": 3, "color": "#000000"})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        add_instance(&mut f, "stroke");
        add_instance(&mut f, "stroke");
        assert_eq!(effects_of(&f).len(), 3);
        assert_eq!(f["selected"], json!("fx3"));
        remove_instance(&mut f, "fx2");
        assert_eq!(effects_of(&f).len(), 2);
        assert_eq!(f["selected"], json!("fx3"), "selection moves to a sibling");
        remove_instance(&mut f, "fx3");
        assert_eq!(f["selected"], json!("fx1"), "the last sibling takes over");
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let strokes = strokes_of(&s);
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].size, 3.0);
    }

    #[test]
    fn disabled_instances_are_kept() {
        let mut s = session();
        s.execute("layer.layerStyle.colorOverlay", json!({"color": "#ff0000"})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        if let Some(e) = entry_mut(&mut f, "fx1") {
            e["on"] = json!(false);
        }
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let d = s.active().unwrap();
        let items = &d.doc.layer(id).unwrap().effects.items;
        assert_eq!(items.len(), 1, "configured but switched off stays");
        assert!(!items[0].enabled());
    }

    #[test]
    fn gradient_strokes_survive_the_dialog() {
        let mut s = session();
        s.execute("layer.layerStyle.stroke", json!({"size": 4, "from": "#ff0000", "to": "#0000ff"})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        // The dialog doesn't model the gradient, so no colour key is stored for it.
        assert!(effects_of(&f)[0]["params"].get("color").is_none());
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let strokes = strokes_of(&s);
        assert_eq!(strokes.len(), 1);
        assert!(
            matches!(&strokes[0].paint, FxPaint::Gradient(g) if g.stops.len() == 2
                && g.stops[0].1 == photocraft_color::Color::rgb(1.0, 0.0, 0.0)
                && g.stops[1].1 == photocraft_color::Color::rgb(0.0, 0.0, 1.0)),
            "the gradient is carried, not reset",
        );
    }

    #[test]
    fn preview_applies_the_style_without_touching_the_document() {
        let s = session();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), Some("colorOverlay"), st.doc.global_light.angle);
        add_instance(&mut f, "stroke");
        let h = preview_hash(&f);
        f.insert("selected".into(), json!("fx2"));
        assert_eq!(preview_hash(&f), h, "switching pages doesn't re-render");
        set_param(&mut f, "fx2", "size", json!(5));
        assert_ne!(preview_hash(&f), h);
        let shown = preview_document(&st.doc, &s.patterns, &f).unwrap();
        let fx = |doc: &photocraft_doc::Document| doc.layer(id).unwrap().effects.items.len();
        assert_eq!((fx(&shown), fx(&st.doc)), (2, 0));
    }

    #[test]
    fn percent_fields_round_trip_through_the_engine() {
        let mut s = session();
        s.execute("layer.layerStyle.outerGlow", json!({"spread": 6, "range": 40})).unwrap();
        s.execute("layer.layerStyle.dropShadow", json!({"spread": 12, "add": true})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        let fx = effects_of(&f);
        assert_eq!(fx.len(), 2);
        assert_eq!(fx[0]["params"]["spread"], json!(6.0));
        assert_eq!(fx[0]["params"]["range"], json!(40.0));
        assert_eq!(fx[1]["params"]["spread"], json!(12.0));
    }

    #[test]
    fn drop_shadow_angle_reaches_the_effect() {
        // #350: the Angle slider was dead — the dialog never sent `useGlobalLight`, so the engine
        // defaulted it to true and the compositor used the fixed global light angle, ignoring the slider.
        let mut s = session();
        let shadow = |s: &photocraft_engine::Session| {
            let d = s.active().unwrap();
            let id = d.active_layer.unwrap();
            d.doc.layer(id).unwrap().effects.items.iter().find_map(|e| match e {
                Effect::DropShadow(sh) => Some(sh.clone()),
                _ => None,
            })
        };

        // Use Global Light off: the per-effect angle is stored and used.
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), Some("dropShadow"), st.doc.global_light.angle);
        set_param(&mut f, "fx1", "angle", json!(45.0));
        set_param(&mut f, "fx1", "useGlobalLight", json!(false));
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let eff = shadow(&s).unwrap();
        assert!(!eff.use_global_light, "Use Global Light off keeps the per-effect angle");
        assert_eq!(eff.angle, 45.0);

        // Use Global Light on: the Angle slider drives the document's shared light angle.
        let st = s.active().unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), Some("dropShadow"), st.doc.global_light.angle);
        set_param(&mut f, "fx1", "angle", json!(30.0));
        set_param(&mut f, "fx1", "useGlobalLight", json!(true));
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        assert_eq!(s.active().unwrap().doc.global_light.angle, 30.0, "the Angle slider moves the shared light");
        assert!(shadow(&s).unwrap().use_global_light);

        // Reopening the dialog shows the effective angle and the checkbox state.
        let st = s.active().unwrap();
        let f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        let fx = effects_of(&f);
        let shadow_entry = fx.iter().find(|e| e["kind"] == json!("dropShadow")).unwrap();
        assert_eq!(shadow_entry["params"]["useGlobalLight"], json!(true));
        assert_eq!(shadow_entry["params"]["angle"], json!(30.0));
    }

    #[test]
    fn bevel_angle_change_is_not_overridden_by_unchanged_drop_shadow_angle() {
        let mut s = session();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), None, st.doc.global_light.angle);
        add_instance(&mut f, "bevelEmboss");
        set_param(&mut f, "fx1", "angle", json!(45.0));
        add_instance(&mut f, "dropShadow");
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        assert_eq!(s.active().unwrap().doc.global_light.angle, 45.0);
        let d = s.active().unwrap();
        assert_eq!(d.doc.layer(id).unwrap().effects.items.len(), 2);
    }

    #[test]
    fn unchanged_global_light_angle_is_not_applied() {
        let layer = Layer::raster("x", photocraft_doc::PixelFormat::RGBA8);
        let mut f = initial_fields(&layer, None, 47.5);
        add_instance(&mut f, "bevelEmboss");
        assert_eq!(effects_of(&f)[0]["params"]["angle"].as_f64(), Some(47.5), "a fresh effect starts from the shared light",);
        let mut commands = Vec::new();
        apply(&f, |cmd, _| {
            commands.push(cmd.to_string());
            Ok(Value::Null)
        })
        .unwrap();
        assert!(!commands.iter().any(|cmd| cmd == "layer.layerStyle.globalLight"));
    }

    #[test]
    fn preview_off_shows_the_original_until_reenabled() {
        let mut s = session();
        s.execute("layer.layerStyle.colorOverlay", json!({"color": "#ff0000"})).unwrap();
        let st = s.active().unwrap();
        let id = st.active_layer.unwrap();
        let mut f = initial_fields(st.doc.layer(id).unwrap(), Some("colorOverlay"), st.doc.global_light.angle);
        // Preview off is the dialog default's complement; edits still change the state.
        f.insert("preview".into(), json!(false));
        let h_off = preview_hash(&f);
        set_param(&mut f, "fx1", "color", json!("#00ff00"));
        assert_ne!(preview_hash(&f), h_off, "editing while preview is off still updates the dialog state");
        // Re-enabling preview applies the pending state (the canvas reads `preview`; the
        // scratch-session apply is the same one the checkbox-on path uses).
        f.insert("preview".into(), json!(true));
        let shown = preview_document(&st.doc, &s.patterns, &f).unwrap();
        let d = s.active().unwrap();
        let color = |doc: &photocraft_doc::Document| match &doc.layer(id).unwrap().effects.items[0] {
            Effect::ColorOverlay { color, .. } => *color,
            other => panic!("{other:?}"),
        };
        assert_eq!(color(&shown), photocraft_color::Color::rgb(0.0, 1.0, 0.0));
        assert_ne!(color(&d.doc), color(&shown), "the document itself is untouched until OK");
        // OK commits regardless of the checkbox.
        f.insert("preview".into(), json!(false));
        apply(&f, |cmd, p| s.execute(cmd, p).map_err(|e| e.to_string())).unwrap();
        let d = s.active().unwrap();
        assert_eq!(color(&d.doc), photocraft_color::Color::rgb(0.0, 1.0, 0.0), "OK commits with Preview off");
    }
}
