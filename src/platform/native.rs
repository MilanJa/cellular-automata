//! Desktop implementation: preset folders under `./presets`, native file dialogs.

use std::path::Path;
use std::sync::Mutex;

use super::SavedLocation;
use crate::preset::bundle::{bundle_filename, from_bundle, to_bundle, BUNDLE_SUFFIX};
use crate::preset::{preset_exists, scan_presets_dir, slug, Preset};

pub const PRESETS_DIR: &str = "presets";

/// Human label for the "saved presets" section of the dropdown.
pub const SAVED_SECTION_LABEL: &str = "./presets";

pub fn list_saved() -> Vec<(String, SavedLocation)> {
    scan_presets_dir(Path::new(PRESETS_DIR))
        .into_iter()
        .map(|(name, path)| (name, SavedLocation::Folder(path)))
        .collect()
}

pub fn load_saved(location: &SavedLocation) -> anyhow::Result<Preset> {
    match location {
        SavedLocation::Folder(path) => Preset::load_dir(path),
        SavedLocation::Browser(_) => anyhow::bail!("browser storage is not available on the desktop"),
    }
}

/// Saves in place. Only folder locations exist on the desktop.
pub fn save_to(preset: &Preset, location: &SavedLocation) -> anyhow::Result<()> {
    match location {
        SavedLocation::Folder(path) => preset.save_dir(path),
        SavedLocation::Browser(_) => anyhow::bail!("browser storage is not available on the desktop"),
    }
}

/// Asks for a parent folder and returns `<parent>/<slug>` plus whether a preset already exists there.
pub fn choose_save_location(name: &str) -> Option<(SavedLocation, bool)> {
    let parent = rfd::FileDialog::new().set_title("Choose where to create the preset folder").pick_folder()?;
    let target = parent.join(slug(name));
    let exists = preset_exists(&target);
    Some((SavedLocation::Folder(target), exists))
}

/// Native confirm dialog.
pub fn confirm(title: &str, text: &str) -> bool {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

/// Export: ask for a file name and write the bundle.
pub fn export_bundle(preset: &Preset) -> anyhow::Result<Option<String>> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export preset")
        .set_file_name(bundle_filename(&preset.meta.name))
        .add_filter("Preset bundle", &["toml"])
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, to_bundle(preset)?)?;
    Ok(Some(path.display().to_string()))
}

static UPLOADED: Mutex<Option<String>> = Mutex::new(None);

/// Import: ask for a bundle file and queue its contents for `poll_import`.
pub fn request_import() {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import preset bundle")
        .add_filter("Preset bundle", &["toml"])
        .pick_file()
    else {
        return;
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| format!("__error__{}: {e}", path.display()));
    *UPLOADED.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
}

/// Returns an imported bundle once, parsed, or an error message.
pub fn poll_import() -> Option<anyhow::Result<Preset>> {
    let text = UPLOADED.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    Some(if let Some(err) = text.strip_prefix("__error__") {
        Err(anyhow::anyhow!("could not read {err}"))
    } else {
        from_bundle(&text)
    })
}

/// Asks where to save the PNG and writes it. `Ok(None)` when the user cancelled.
pub fn save_png(filename: &str, bytes: &[u8]) -> anyhow::Result<Option<String>> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Save image")
        .set_file_name(filename)
        .add_filter("PNG image", &["png"])
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, bytes)?;
    Ok(Some(path.display().to_string()))
}

/// The desktop has no URL; the startup preset comes from the command line instead.
pub fn startup_preset_from_url() -> Option<String> {
    None
}

pub fn is_web() -> bool {
    false
}

#[allow(dead_code)]
fn _suffix_is_used() -> &'static str {
    BUNDLE_SUFFIX
}
