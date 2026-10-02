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

static UPLOADED_IMAGE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Asks for a PNG and queues its bytes for `poll_image_import`.
pub fn request_image_import() {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Choose an image to seed the grid")
        .add_filter("PNG image", &["png"])
        .pick_file()
    else {
        return;
    };
    if let Ok(bytes) = std::fs::read(&path) {
        *UPLOADED_IMAGE.lock().unwrap_or_else(|e| e.into_inner()) = Some(bytes);
    }
}

pub fn poll_image_import() -> Option<Vec<u8>> {
    UPLOADED_IMAGE.lock().unwrap_or_else(|e| e.into_inner()).take()
}

static DROPPED: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());

/// Queues a dropped file's name and bytes for `poll_dropped_files` (synchronous on the desktop).
pub fn queue_dropped_file(file: &dyn egui::DroppedFile) {
    let name = file.path().to_string_lossy().to_string();
    let entry = match file.bytes() {
        Ok(bytes) => (name, bytes),
        Err(e) => (format!("__error__{name}: {e}"), Vec::new()),
    };
    DROPPED.lock().unwrap_or_else(|e| e.into_inner()).push(entry);
}

/// Dropped files whose bytes are available, oldest first. Names starting with `__error__` carry
/// a read error message instead of a file.
pub fn poll_dropped_files() -> Vec<(String, Vec<u8>)> {
    std::mem::take(&mut *DROPPED.lock().unwrap_or_else(|e| e.into_inner()))
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

/// No URL fragment on the desktop.
pub fn startup_share_code() -> Option<String> {
    None
}

/// Links copied on the desktop point at the public web build.
pub fn share_base_url() -> String {
    crate::preset::share::PAGES_URL.to_string()
}

pub fn is_web() -> bool {
    false
}

#[allow(dead_code)]
fn _suffix_is_used() -> &'static str {
    BUNDLE_SUFFIX
}
