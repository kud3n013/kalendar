//! The fonts the app draws with: the system's own text font unless another
//! is picked in the settings, with the system's emoji font right behind it.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use fontique::{Collection, CollectionOptions, FamilyInfo, FontStyle, FontWeight, FontWidth, GenericFamily};
use std::sync::Arc;

fn system() -> Collection {
    Collection::new(CollectionOptions { shared: false, system_fonts: true })
}

/// Every font family installed on this computer, by name.
pub fn families() -> Vec<String> {
    let mut col = system();
    let mut names: Vec<String> = col.family_names().filter(|n| !n.starts_with('.')).map(str::to_string).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

/// One face of a family at the given weight, and whether it really has it.
fn face(fam: &FamilyInfo, weight: FontWeight) -> Option<(FontData, bool)> {
    let info = fam.match_font(FontWidth::NORMAL, FontStyle::Normal, weight, false)?;
    let blob = info.load(None)?;
    let mut data = FontData::from_owned(blob.data().to_vec());
    data.index = info.index();
    let exact = if info.has_weight_axis() {
        // one file holds every weight
        data.tweak.coords.push("wght", weight.value());
        true
    } else {
        (info.weight().value() - weight.value()).abs() < 120.0
    };
    Some((data, exact))
}

fn generic(col: &mut Collection, kind: GenericFamily) -> Option<FamilyInfo> {
    let ids: Vec<_> = col.generic_families(kind).collect();
    ids.into_iter().find_map(|id| col.family(id))
}

/// Sets the app's fonts. `family` is a family name, or empty for the
/// system's own text font. Whatever is missing falls back to the built-in
/// font, so this always leaves something to draw with.
pub fn apply(ctx: &egui::Context, family: &str) {
    let mut col = system();
    let mut fonts = FontDefinitions::default();
    let base = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut text: Vec<String> = vec![];
    let mut bold: Vec<String> = vec![];

    let chosen = if family.is_empty() { None } else { col.family_by_name(family) };
    let chosen = chosen.or_else(|| generic(&mut col, GenericFamily::SansSerif)).or_else(|| generic(&mut col, GenericFamily::SystemUi));
    if let Some(fam) = &chosen {
        if let Some((regular, _)) = face(fam, FontWeight::NORMAL) {
            fonts.font_data.insert("text".into(), Arc::new(regular));
            text.push("text".into());
        }
        // titles are a step heavier, not the full bold
        if let Some((heavy, true)) = face(fam, FontWeight::SEMI_BOLD) {
            fonts.font_data.insert("heavy".into(), Arc::new(heavy));
            bold.push("heavy".into());
        }
    }
    // colour emoji come before the built-in fonts, which have none
    if let Some((emoji, _)) = generic(&mut col, GenericFamily::Emoji).and_then(|f| face(&f, FontWeight::NORMAL)) {
        fonts.font_data.insert("emoji".into(), Arc::new(emoji));
        text.push("emoji".into());
    }
    text.extend(base);
    // the Lucide icon font covers only private-use glyphs, so it can sit at
    // the end of every family as a fallback
    fonts.font_data.insert("lucide".into(), Arc::new(FontData::from_static(include_bytes!("../assets/lucide/lucide.ttf"))));
    bold.extend(text.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, text);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    for fam in fonts.families.values_mut() {
        fam.push("lucide".to_string());
    }
    ctx.set_fonts(fonts);
}
