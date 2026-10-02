//! Browser implementation: presets in `localStorage`, bundles via download and file input.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::{name_from_storage_key, storage_key, SavedLocation};
use crate::preset::bundle::{bundle_filename, from_bundle, to_bundle};
use crate::preset::Preset;

pub const SAVED_SECTION_LABEL: &str = "Browser storage";

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn list_saved() -> Vec<(String, SavedLocation)> {
    let Some(store) = local_storage() else { return Vec::new() };
    let n = store.length().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..n {
        let Ok(Some(key)) = store.key(i) else { continue };
        if name_from_storage_key(&key).is_none() {
            continue;
        }
        if let Ok(Some(text)) = store.get_item(&key)
            && let Ok(p) = from_bundle(&text)
        {
            out.push((p.meta.name, SavedLocation::Browser(key)));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn load_saved(location: &SavedLocation) -> anyhow::Result<Preset> {
    match location {
        SavedLocation::Browser(key) => {
            let store = local_storage().ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
            let text = store
                .get_item(key)
                .ok()
                .flatten()
                .ok_or_else(|| anyhow::anyhow!("no preset stored under {key}"))?;
            from_bundle(&text)
        }
        SavedLocation::Folder(_) => anyhow::bail!("folders are not available in the browser"),
    }
}

pub fn save_to(preset: &Preset, location: &SavedLocation) -> anyhow::Result<()> {
    match location {
        SavedLocation::Browser(key) => {
            let store = local_storage().ok_or_else(|| anyhow::anyhow!("localStorage unavailable"))?;
            store
                .set_item(key, &to_bundle(preset)?)
                .map_err(|_| anyhow::anyhow!("localStorage refused the write (quota?)"))
        }
        SavedLocation::Folder(_) => anyhow::bail!("folders are not available in the browser"),
    }
}

/// In the browser the location is derived from the name; the app asks for the name itself.
pub fn location_for_name(name: &str) -> (SavedLocation, bool) {
    let key = storage_key(name);
    let exists = local_storage().and_then(|s| s.get_item(&key).ok().flatten()).is_some();
    (SavedLocation::Browser(key), exists)
}

pub fn delete_saved(location: &SavedLocation) {
    if let (SavedLocation::Browser(key), Some(store)) = (location, local_storage()) {
        let _ = store.remove_item(key);
    }
}

/// Export: trigger a download of the bundle.
pub fn export_bundle(preset: &Preset) -> anyhow::Result<Option<String>> {
    let text = to_bundle(preset)?;
    let filename = bundle_filename(&preset.meta.name);
    download(&filename, "application/toml", text.as_bytes())?;
    Ok(Some(filename))
}

/// Downloads the PNG through the browser.
pub fn save_png(filename: &str, bytes: &[u8]) -> anyhow::Result<Option<String>> {
    download(filename, "image/png", bytes)?;
    Ok(Some(filename.to_string()))
}

/// Triggers a browser download of `bytes` under `filename`.
fn download(filename: &str, mime: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no window"))?;
    let document = window.document().ok_or_else(|| anyhow::anyhow!("no document"))?;
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_buffer_source_sequence_and_options(&parts, &opts)
        .map_err(|_| anyhow::anyhow!("could not create blob"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| anyhow::anyhow!("could not create object URL"))?;
    let a: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(|_| anyhow::anyhow!("could not create anchor"))?
        .dyn_into()
        .map_err(|_| anyhow::anyhow!("anchor cast failed"))?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}

thread_local! {
    static UPLOADED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Import: open the browser file picker; the file's text is queued for `poll_import`.
pub fn request_import() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(el) = document.create_element("input") else { return };
    let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() else { return };
    input.set_type("file");
    input.set_accept(".toml");
    let input_for_cb = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        let Some(files) = input_for_cb.files() else { return };
        let Some(file) = files.get(0) else { return };
        let Ok(reader) = web_sys::FileReader::new() else { return };
        let reader_for_cb = reader.clone();
        let on_load = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if let Ok(result) = reader_for_cb.result()
                && let Some(text) = result.as_string()
            {
                UPLOADED.with(|u| *u.borrow_mut() = Some(text));
            }
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_text(&file);
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
    input.click();
}

pub fn poll_import() -> Option<anyhow::Result<Preset>> {
    let text = UPLOADED.with(|u| u.borrow_mut().take())?;
    Some(from_bundle(&text))
}

thread_local! {
    static UPLOADED_IMAGE: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// Opens the browser file picker for a PNG; its bytes are queued for `poll_image_import`.
pub fn request_image_import() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(el) = document.create_element("input") else { return };
    let Ok(input) = el.dyn_into::<web_sys::HtmlInputElement>() else { return };
    input.set_type("file");
    input.set_accept("image/png");
    let input_for_cb = input.clone();
    let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        let Some(files) = input_for_cb.files() else { return };
        let Some(file) = files.get(0) else { return };
        let Ok(reader) = web_sys::FileReader::new() else { return };
        let reader_for_cb = reader.clone();
        let on_load = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if let Ok(result) = reader_for_cb.result() {
                let bytes = js_sys::Uint8Array::new(&result).to_vec();
                UPLOADED_IMAGE.with(|u| *u.borrow_mut() = Some(bytes));
            }
        });
        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        let _ = reader.read_as_array_buffer(&file);
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    on_change.forget();
    input.click();
}

pub fn poll_image_import() -> Option<Vec<u8>> {
    UPLOADED_IMAGE.with(|u| u.borrow_mut().take())
}

thread_local! {
    static DROPPED: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
}

/// Reads a dropped file asynchronously and queues it for `poll_dropped_files`.
pub fn queue_dropped_file(file: &dyn egui::DroppedFile) {
    let name = file.path().to_string_lossy().to_string();
    let Some(web_file) = file.web_file().cloned() else {
        DROPPED.with(|d| d.borrow_mut().push((format!("__error__{name}: no file handle"), Vec::new())));
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let entry = match wasm_bindgen_futures::JsFuture::from(web_file.array_buffer()).await {
            Ok(buf) => (name, js_sys::Uint8Array::new(&buf).to_vec()),
            Err(_) => (format!("__error__{name}: could not read"), Vec::new()),
        };
        DROPPED.with(|d| d.borrow_mut().push(entry));
    });
}

pub fn poll_dropped_files() -> Vec<(String, Vec<u8>)> {
    DROPPED.with(|d| std::mem::take(&mut *d.borrow_mut()))
}

/// `?preset=<id>` from the page URL.
pub fn startup_preset_from_url() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    super::preset_from_query(&search)
}

/// `#p=<code>` from the page URL.
pub fn startup_share_code() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    crate::preset::share::share_code_from_fragment(&hash)
}

/// This page without query or fragment.
pub fn share_base_url() -> String {
    let fallback = crate::preset::share::PAGES_URL.to_string();
    let Some(loc) = web_sys::window().map(|w| w.location()) else { return fallback };
    match (loc.origin(), loc.pathname()) {
        (Ok(o), Ok(p)) => format!("{o}{p}"),
        _ => fallback,
    }
}

pub fn is_web() -> bool {
    true
}
