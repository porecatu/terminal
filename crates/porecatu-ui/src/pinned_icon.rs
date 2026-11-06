// SPDX-License-Identifier: GPL-3.0-or-later

//! The icon of a pinned tab (ADR-0088 §4 to §6, PRD-021 RF-21.17 to
//! RF-21.21): the chain `[icon]` -> `icon.png` -> the app icon, run **off the
//! main thread**, and the process-wide cache of the textures it produces.
//!
//! - [`spawn_resolve`] opens one short-lived, detached thread per batch (the
//!   startup batch, or the one request of a pin gesture). The whole chain
//!   runs there -- reading the `.porecatu`, probing `icon.png`, decoding --
//!   and what comes back is **keyed data** ([`PinnedIconResult`]), never a
//!   command (ADR-0052). A failing step only moves on to the next one, with
//!   no warning (RF-21.20): the app icon in its place is the visible sign.
//! - [`PinnedIconCache`] is the ADR-0088 §5 cache: one texture per resolved
//!   **path**, shared by every pinned tab that resolved to it, with a use
//!   count; the last user dropping it releases the texture. The app icon
//!   never takes an entry (it is `App::app_icon_image`). Generic in the GPU
//!   handle, like `BackgroundImageStore`, so it is tested without a GPU.
//! - [`contain`] is the ADR-0088 §6 geometry: the image fitted in the square
//!   of `icon_em_size`, keeping its aspect.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::thread;

use porecatu_config::{ICON_PNG_FILE_NAME, ProjectFile};
use porecatu_core::TabId;
use porecatu_render::ImageId;
use porecatu_session::{PaneNodeV1, PaneV1, TabV1};

use crate::background_image::{DecodedImage, decode};
use crate::pinned::{PinnedKey, PinnedRegistry};

/// Longest side of a pinned icon after decoding (ADR-0088 §4). An
/// implementation value, not an appearance one: the icon is drawn at
/// `icon_em_size` (20 logical px), and 128 covers a 400% scale (80 physical
/// px) with a mip level to spare, at about 87 KB of texture per icon.
pub(crate) const PINNED_ICON_MAX_DIM: u32 = 128;

/// What the thread needs to resolve the icon of one pinned tab.
#[derive(Debug, Clone)]
pub(crate) struct PinnedIconRequest {
    pub key: PinnedKey,
    /// The directory saved when the tab was pinned. `None`: an SSH tab, or no
    /// directory at all -- the app icon, with nothing to look at.
    pub dir: Option<PathBuf>,
    /// For `[project_file] enabled`: off, the `.porecatu` is not read and the
    /// chain starts at `icon.png` (RF-21.19).
    pub project_file: ProjectFile,
}

/// Where the icon of a result came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PinnedIconSource {
    /// The file `[icon]` of the `.porecatu` declared.
    Declared(PathBuf),
    /// The `icon.png` of the directory.
    IconPng(PathBuf),
    /// The app icon: nothing else decoded.
    App,
}

/// What comes back from the thread, with the key that asked for it.
#[derive(Debug)]
pub(crate) struct PinnedIconResult {
    pub key: PinnedKey,
    pub source: PinnedIconSource,
    /// `None` with `App`.
    pub image: Option<DecodedImage>,
}

/// Decodes the file at `path`, as a pinned icon: PNG or JPEG by content,
/// reduced to [`PINNED_ICON_MAX_DIM`]. A panic of the decoder is a failure
/// of this step, as in `background_image::spawn_load`.
fn load(path: &Path) -> Option<DecodedImage> {
    panic::catch_unwind(AssertUnwindSafe(|| {
        let file = File::open(path).ok()?;
        decode(BufReader::new(file), PINNED_ICON_MAX_DIM).ok()
    }))
    .ok()
    .flatten()
}

/// The whole chain for one request (ADR-0088 §4). Pure with respect to the
/// process -- it only touches the disk -- and run on the thread.
///
/// A `dir` that is a network or device path (`\\server\x`, `\\.\...`) is the
/// app icon **before anything is touched**: the `.porecatu` and `icon.png`
/// would both be read from it, and merely touching a UNC path on Windows
/// opens an SMB connection and offers the session credentials (ADR-0088 §2).
pub(crate) fn resolve_chain(request: &PinnedIconRequest) -> PinnedIconResult {
    let app = || PinnedIconResult {
        key: request.key,
        source: PinnedIconSource::App,
        image: None,
    };
    let Some(dir) = request.dir.as_deref() else {
        return app();
    };
    if porecatu_config::is_unc_or_device_path(&dir.to_string_lossy()) || !dir.is_dir() {
        return app();
    }
    if let Some(path) = porecatu_config::resolve_icon(dir, &request.project_file)
        && let Some(image) = load(&path)
    {
        return PinnedIconResult {
            key: request.key,
            source: PinnedIconSource::Declared(path),
            image: Some(image),
        };
    }
    let png = dir.join(ICON_PNG_FILE_NAME);
    if fs::metadata(&png).is_ok_and(|meta| meta.is_file())
        && let Some(image) = load(&png)
    {
        return PinnedIconResult {
            key: request.key,
            source: PinnedIconSource::IconPng(png),
            image: Some(image),
        };
    }
    app()
}

/// Opens the thread that resolves `requests` in order and hands each result
/// to `on_result` -- **on that thread**, never the main one; the caller
/// passes a closure that only sends the result through the `EventLoopProxy`.
/// Short-lived and detached, like `background_image::spawn_load`: no `join`,
/// the process leaves with it. No requests, no thread.
pub(crate) fn spawn_resolve(
    requests: Vec<PinnedIconRequest>,
    on_result: impl Fn(PinnedIconResult) + Send + 'static,
) {
    if requests.is_empty() {
        return;
    }
    thread::spawn(move || {
        for request in requests {
            on_result(resolve_chain(&request));
        }
    });
}

/// The directory an icon is looked up in for a saved tab (ADR-0088 §4): the
/// `cwd` of the **focused** pane of the snapshot (the tree's leaf, or the
/// tab's own fields when there is no tree). `None` when that pane is an SSH
/// one -- `is_ssh` is `profile_spawn::restores_as_ssh` over its saved profile
/// -- or has no directory.
pub(crate) fn saved_dir(saved: &TabV1, is_ssh: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let (cwd, profile) = match &saved.panes {
        Some(tree) => {
            let leaf = leaves(&tree.root)
                .into_iter()
                .find(|leaf| leaf.id == tree.focused)
                .or_else(|| leaves(&tree.root).into_iter().next())?;
            (leaf.cwd.as_ref(), leaf.profile.as_deref())
        }
        None => (saved.cwd.as_ref(), saved.profile.as_deref()),
    };
    if profile.is_some_and(is_ssh) {
        return None;
    }
    cwd.cloned()
}

/// The leaves of a saved pane tree, first to last.
fn leaves(node: &PaneNodeV1) -> Vec<&PaneV1> {
    match node {
        PaneNodeV1::Leaf(pane) => vec![pane],
        PaneNodeV1::Split { first, second, .. } => {
            let mut all = leaves(first);
            all.extend(leaves(second));
            all
        }
    }
}

/// A pinned icon already on the GPU: the handle and the size of level 0
/// (needed by [`contain`]). Generic in the handle only so the cache is
/// testable without a GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IconTexture<H = ImageId> {
    pub id: H,
    pub size: (u32, u32),
}

/// What [`PinnedIconCache::apply`] did with a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Applied {
    /// The key is on the app icon now (no entry in the cache).
    App,
    /// The path already had a texture: one more user, the bytes dropped.
    Shared,
    /// The bytes wait for a texture ([`PinnedIconCache::upload`]).
    Pending,
}

struct Cached<H> {
    texture: IconTexture<H>,
    /// How many keys use this texture; zero releases it.
    uses: usize,
}

struct PendingIcon {
    key: PinnedKey,
    path: PathBuf,
    image: DecodedImage,
}

/// The textures of the pinned icons of the **process** (ADR-0088 §5): one per
/// resolved path, with a use count. Pure -- the GPU enters as a closure
/// (`upload`) and as the list of handles to free (`drain_released`).
pub(crate) struct PinnedIconCache<H = ImageId> {
    textures: HashMap<PathBuf, Cached<H>>,
    /// Which path each key uses; a key on the app icon is not here.
    by_key: HashMap<PinnedKey, PathBuf>,
    /// Decoded, waiting for a texture: there was no `GpuContext` yet.
    pending: Vec<PendingIcon>,
    /// Handles the cache let go of, for the `App` to free on the GPU.
    released: Vec<H>,
}

// By hand: the derive would ask `H: Default`.
impl<H> Default for PinnedIconCache<H> {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            by_key: HashMap::new(),
            pending: Vec::new(),
            released: Vec::new(),
        }
    }
}

impl<H: Copy> PinnedIconCache<H> {
    /// The texture `key` draws, `None` for the app icon (or while the result
    /// has not arrived).
    pub(crate) fn texture_of(&self, key: PinnedKey) -> Option<IconTexture<H>> {
        let path = self.by_key.get(&key)?;
        self.textures.get(path).map(|cached| cached.texture)
    }

    /// Whether some decoded icon still waits for a texture.
    #[cfg(test)]
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Handles to free on the GPU since the last call.
    pub(crate) fn drain_released(&mut self) -> Vec<H> {
        std::mem::take(&mut self.released)
    }

    /// Applies a result the thread delivered for a key the caller already
    /// checked is still alive (a result for a gone key is dropped before
    /// reaching here, and never lifts a texture).
    pub(crate) fn apply(&mut self, result: PinnedIconResult) -> Applied {
        let PinnedIconResult { key, source, image } = result;
        let path = match source {
            PinnedIconSource::Declared(path) | PinnedIconSource::IconPng(path) => path,
            PinnedIconSource::App => {
                self.forget(key);
                return Applied::App;
            }
        };
        if self.textures.contains_key(&path) {
            self.pending.retain(|p| p.key != key);
            self.bind(key, path);
            return Applied::Shared;
        }
        let Some(image) = image else {
            self.forget(key);
            return Applied::App;
        };
        self.pending.retain(|p| p.key != key);
        self.pending.push(PendingIcon { key, path, image });
        Applied::Pending
    }

    /// Creates the texture of every decoded icon that waits for one: `create`
    /// gets the bytes and returns the handle. A path that another pending
    /// icon already lifted in this call is shared, not created twice.
    pub(crate) fn upload(&mut self, mut create: impl FnMut(&DecodedImage) -> H) {
        for PendingIcon { key, path, image } in std::mem::take(&mut self.pending) {
            if !self.textures.contains_key(&path) {
                let texture = IconTexture {
                    id: create(&image),
                    size: image.size,
                };
                self.textures
                    .insert(path.clone(), Cached { texture, uses: 0 });
            }
            self.bind(key, path);
        }
    }

    /// The key is gone (unpinned, or its tab ended): its use of a texture
    /// goes, and the texture with it when it was the last one.
    pub(crate) fn forget(&mut self, key: PinnedKey) {
        self.pending.retain(|p| p.key != key);
        if let Some(path) = self.by_key.remove(&key) {
            self.unuse(&path);
        }
    }

    /// `key` uses the texture of `path` (which exists), and stops using the
    /// one it used before.
    fn bind(&mut self, key: PinnedKey, path: PathBuf) {
        if self.by_key.get(&key) == Some(&path) {
            return;
        }
        if let Some(cached) = self.textures.get_mut(&path) {
            cached.uses += 1;
        }
        if let Some(old) = self.by_key.insert(key, path) {
            self.unuse(&old);
        }
    }

    fn unuse(&mut self, path: &Path) {
        let Some(cached) = self.textures.get_mut(path) else {
            return;
        };
        cached.uses = cached.uses.saturating_sub(1);
        if cached.uses == 0
            && let Some(cached) = self.textures.remove(path)
        {
            self.released.push(cached.texture.id);
        }
    }
}

/// The own textures of the pinned tabs of `window`, by `TabId`, for the bar
/// (`chrome::paint`). `pinned_tabs` is the zone, `Workspace::pinned()`. A tab
/// without an entry -- its result has not arrived, or it resolved to the app
/// icon -- is not in the map, and the bar draws the app icon for it.
pub(crate) fn textures_for<W: Copy + Eq + std::hash::Hash, H: Copy>(
    registry: &PinnedRegistry<W>,
    cache: &PinnedIconCache<H>,
    window: W,
    pinned_tabs: &[TabId],
) -> HashMap<TabId, IconTexture<H>> {
    pinned_tabs
        .iter()
        .filter_map(|&tab| {
            let key = registry.key_of(window, tab)?;
            Some((tab, cache.texture_of(key)?))
        })
        .collect()
}

/// ADR-0088 §6: the size of an image of `size` pixels fitted (*contain*) in a
/// square of `side`, keeping its aspect -- the longer side is `side`. A
/// degenerate size fills the square. The caller centers the result in the
/// square.
pub(crate) fn contain(side: f32, size: (u32, u32)) -> (f32, f32) {
    let (width, height) = (size.0 as f32, size.1 as f32);
    if width <= 0.0 || height <= 0.0 {
        return (side, side);
    }
    let longest = width.max(height);
    (side * width / longest, side * height / longest)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::mpsc;
    use std::time::Duration;

    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use porecatu_session::PaneTreeV1;

    use super::*;
    use crate::pinned::PinnedRegistry;
    use porecatu_session::PinnedLoadOutcome;

    // ---- keys ----------------------------------------------------------

    /// Distinct keys, from a registry (the key is opaque outside it).
    fn keys(n: usize) -> Vec<PinnedKey> {
        let file = porecatu_session::PinnedFileV1 {
            tabs: (0..n).map(|_| TabV1::default()).collect(),
            ..Default::default()
        };
        let (registry, _) = PinnedRegistry::<u8>::from_load(PinnedLoadOutcome::Loaded(file), None);
        registry.entries().iter().map(|e| e.key).collect()
    }

    // ---- the chain, on real temporary files ----------------------------

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        encode(width, height, ImageFormat::Png)
    }

    fn encode(width: u32, height: u32, format: ImageFormat) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([200, 30, 30, 255]));
        let mut bytes = Cursor::new(Vec::new());
        let dynamic = match format {
            ImageFormat::Jpeg => {
                DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(image).into_rgb8())
            }
            _ => DynamicImage::ImageRgba8(image),
        };
        dynamic.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn request(dir: Option<&Path>, enabled: bool) -> PinnedIconRequest {
        PinnedIconRequest {
            key: keys(1)[0],
            dir: dir.map(Path::to_path_buf),
            project_file: ProjectFile {
                enabled,
                ..ProjectFile::default()
            },
        }
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_valid_icon_section_is_the_declared_icon() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".porecatu", b"[icon]\nlogo.png\n");
        let logo = write(dir.path(), "logo.png", &png_bytes(16, 16));
        // `icon.png` is there too, and loses.
        write(dir.path(), "icon.png", &png_bytes(8, 8));

        let result = resolve_chain(&request(Some(dir.path()), true));

        assert_eq!(result.source, PinnedIconSource::Declared(logo));
        assert_eq!(result.image.unwrap().size, (16, 16));
    }

    #[test]
    fn a_declared_icon_that_does_not_decode_falls_to_icon_png() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".porecatu", b"[icon]\nlogo.png\n");
        write(dir.path(), "logo.png", b"not an image at all");
        let icon = write(dir.path(), "icon.png", &png_bytes(8, 8));

        let result = resolve_chain(&request(Some(dir.path()), true));

        assert_eq!(result.source, PinnedIconSource::IconPng(icon));
        assert_eq!(result.image.unwrap().size, (8, 8));
    }

    #[test]
    fn a_declared_gif_falls_to_the_next_step() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".porecatu", b"[icon]\nanim.gif\n");
        // A real GIF header: recognized, and refused (PNG and JPEG only).
        write(
            dir.path(),
            "anim.gif",
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;",
        );
        let result = resolve_chain(&request(Some(dir.path()), true));
        assert_eq!(result.source, PinnedIconSource::App);
        assert!(result.image.is_none());
    }

    #[test]
    fn nothing_to_find_is_the_app_icon() {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve_chain(&request(Some(dir.path()), true));
        assert_eq!(result.source, PinnedIconSource::App);
        assert!(result.image.is_none());
    }

    #[test]
    fn a_missing_directory_is_the_app_icon() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("sumiu");
        let result = resolve_chain(&request(Some(&gone), true));
        assert_eq!(result.source, PinnedIconSource::App);
    }

    #[test]
    fn project_file_disabled_skips_the_icon_section_but_not_icon_png() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".porecatu", b"[icon]\nlogo.png\n");
        write(dir.path(), "logo.png", &png_bytes(16, 16));
        let icon = write(dir.path(), "icon.png", &png_bytes(8, 8));

        let result = resolve_chain(&request(Some(dir.path()), false));

        assert_eq!(result.source, PinnedIconSource::IconPng(icon));
        assert_eq!(result.image.unwrap().size, (8, 8));
    }

    #[test]
    fn no_directory_is_the_app_icon() {
        let result = resolve_chain(&request(None, true));
        assert_eq!(result.source, PinnedIconSource::App);
        assert!(result.image.is_none());
    }

    #[test]
    fn a_unc_or_device_directory_is_the_app_icon_without_touching_it() {
        for dir in [
            r"\\servidor\pasta",
            "//servidor/pasta",
            r"\\.\pipe\x",
            r"\\?\UNC\srv\x",
        ] {
            let result = resolve_chain(&request(Some(Path::new(dir)), true));
            assert_eq!(result.source, PinnedIconSource::App, "{dir}");
        }
    }

    #[test]
    fn a_jpeg_icon_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".porecatu", b"[icon]\nfoto.jpg\n");
        let foto = write(dir.path(), "foto.jpg", &encode(24, 24, ImageFormat::Jpeg));
        let result = resolve_chain(&request(Some(dir.path()), true));
        assert_eq!(result.source, PinnedIconSource::Declared(foto));
        assert_eq!(result.image.unwrap().size, (24, 24));
    }

    #[test]
    fn a_large_image_is_reduced_to_the_cap_on_its_longer_side() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "icon.png", &png_bytes(1024, 512));
        let result = resolve_chain(&request(Some(dir.path()), true));
        let image = result.image.unwrap();
        assert_eq!(image.size, (PINNED_ICON_MAX_DIM, PINNED_ICON_MAX_DIM / 2));
        assert_eq!(
            (image.levels[0].width, image.levels[0].height),
            image.size,
            "level 0 is the reduced image"
        );
    }

    #[test]
    fn the_thread_delivers_every_result_with_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let png = write(dir.path(), "icon.png", &png_bytes(8, 8));
        let empty = tempfile::tempdir().unwrap();
        let ks = keys(2);
        let project_file = ProjectFile::default();
        let (tx, rx) = mpsc::channel();
        spawn_resolve(
            vec![
                PinnedIconRequest {
                    key: ks[0],
                    dir: Some(dir.path().to_path_buf()),
                    project_file: project_file.clone(),
                },
                PinnedIconRequest {
                    key: ks[1],
                    dir: Some(empty.path().to_path_buf()),
                    project_file,
                },
            ],
            move |result| {
                let _ = tx.send(result);
            },
        );
        let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let second = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(first.key, ks[0]);
        assert_eq!(first.source, PinnedIconSource::IconPng(png));
        assert_eq!(second.key, ks[1]);
        assert_eq!(second.source, PinnedIconSource::App);
    }

    #[test]
    fn no_requests_open_no_thread() {
        // Nothing to observe but that it returns and never calls back.
        let (tx, rx) = mpsc::channel::<PinnedIconResult>();
        spawn_resolve(Vec::new(), move |result| {
            let _ = tx.send(result);
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    // ---- saved_dir -----------------------------------------------------

    fn leaf(id: u32, cwd: &str, profile: Option<&str>) -> PaneV1 {
        PaneV1 {
            id,
            cwd: Some(PathBuf::from(cwd)),
            spawn_program: None,
            profile: profile.map(str::to_owned),
        }
    }

    #[test]
    fn the_directory_is_the_one_of_the_focused_leaf() {
        let saved = TabV1 {
            cwd: Some(PathBuf::from("/tab")),
            panes: Some(PaneTreeV1 {
                root: PaneNodeV1::Split {
                    axis: porecatu_session::SplitAxisV1::Horizontal,
                    ratio: 0.5,
                    first: Box::new(PaneNodeV1::Leaf(leaf(0, "/a", None))),
                    second: Box::new(PaneNodeV1::Leaf(leaf(1, "/b", None))),
                },
                focused: 1,
            }),
            ..TabV1::default()
        };
        assert_eq!(saved_dir(&saved, |_| false), Some(PathBuf::from("/b")));
    }

    #[test]
    fn without_a_tree_the_directory_is_the_tab_own() {
        let saved = TabV1 {
            cwd: Some(PathBuf::from("/tab")),
            ..TabV1::default()
        };
        assert_eq!(saved_dir(&saved, |_| false), Some(PathBuf::from("/tab")));
        assert_eq!(saved_dir(&TabV1::default(), |_| false), None);
    }

    #[test]
    fn an_ssh_pane_has_no_directory() {
        let saved = TabV1 {
            cwd: Some(PathBuf::from("/tab")),
            profile: Some("servidor".to_owned()),
            ..TabV1::default()
        };
        assert_eq!(saved_dir(&saved, |id| id == "servidor"), None);
        let tree = TabV1 {
            panes: Some(PaneTreeV1 {
                root: PaneNodeV1::Leaf(leaf(0, "/a", Some("servidor"))),
                focused: 0,
            }),
            ..TabV1::default()
        };
        assert_eq!(saved_dir(&tree, |id| id == "servidor"), None);
        assert_eq!(
            saved_dir(&tree, |_| false),
            Some(PathBuf::from("/a")),
            "a local profile keeps its directory"
        );
    }

    // ---- the cache -----------------------------------------------------

    fn tiny() -> DecodedImage {
        DecodedImage {
            size: (4, 2),
            levels: Vec::new(),
        }
    }

    fn found(key: PinnedKey, path: &str) -> PinnedIconResult {
        PinnedIconResult {
            key,
            source: PinnedIconSource::IconPng(PathBuf::from(path)),
            image: Some(tiny()),
        }
    }

    fn app(key: PinnedKey) -> PinnedIconResult {
        PinnedIconResult {
            key,
            source: PinnedIconSource::App,
            image: None,
        }
    }

    /// An upload that hands out handles 1, 2, 3... and counts the creations.
    fn upload(cache: &mut PinnedIconCache<u32>, created: &mut Vec<u32>) {
        cache.upload(|_| {
            let id = created.len() as u32 + 1;
            created.push(id);
            id
        });
    }

    #[test]
    fn a_result_waits_for_the_gpu_and_then_becomes_a_texture() {
        let ks = keys(1);
        let mut cache = PinnedIconCache::<u32>::default();
        assert_eq!(cache.apply(found(ks[0], "/p/icon.png")), Applied::Pending);
        assert!(cache.has_pending());
        assert_eq!(cache.texture_of(ks[0]), None, "no texture before the GPU");

        let mut created = Vec::new();
        upload(&mut cache, &mut created);

        assert!(!cache.has_pending());
        assert_eq!(
            cache.texture_of(ks[0]),
            Some(IconTexture {
                id: 1,
                size: (4, 2)
            })
        );
    }

    #[test]
    fn two_keys_on_the_same_path_share_one_texture() {
        let ks = keys(2);
        let mut cache = PinnedIconCache::<u32>::default();
        let mut created = Vec::new();
        cache.apply(found(ks[0], "/p/icon.png"));
        upload(&mut cache, &mut created);
        // The path is cached already: no pending, the bytes are dropped.
        assert_eq!(cache.apply(found(ks[1], "/p/icon.png")), Applied::Shared);
        assert!(!cache.has_pending());
        upload(&mut cache, &mut created);
        assert_eq!(created, [1], "one texture for the path");
        assert_eq!(cache.texture_of(ks[0]), cache.texture_of(ks[1]));
    }

    #[test]
    fn two_pending_results_for_one_path_create_one_texture() {
        let ks = keys(2);
        let mut cache = PinnedIconCache::<u32>::default();
        cache.apply(found(ks[0], "/p/icon.png"));
        cache.apply(found(ks[1], "/p/icon.png"));
        let mut created = Vec::new();
        upload(&mut cache, &mut created);
        assert_eq!(created, [1]);
        assert_eq!(cache.texture_of(ks[0]), cache.texture_of(ks[1]));
    }

    #[test]
    fn the_texture_is_released_only_when_the_last_user_goes() {
        let ks = keys(2);
        let mut cache = PinnedIconCache::<u32>::default();
        let mut created = Vec::new();
        cache.apply(found(ks[0], "/p/icon.png"));
        cache.apply(found(ks[1], "/p/icon.png"));
        upload(&mut cache, &mut created);

        cache.forget(ks[0]);
        assert!(cache.drain_released().is_empty(), "still used by the other");
        assert!(cache.texture_of(ks[1]).is_some());

        cache.forget(ks[1]);
        assert_eq!(cache.drain_released(), [1], "the last user frees it");
        assert_eq!(cache.texture_of(ks[1]), None);
        assert!(cache.drain_released().is_empty(), "freed once");
    }

    #[test]
    fn unpinning_the_last_pinned_tab_of_a_path_releases_its_image_id() {
        let ks = keys(2);
        let mut cache = PinnedIconCache::<u32>::default();
        let mut created = Vec::new();
        cache.apply(found(ks[0], "/a/icon.png"));
        cache.apply(found(ks[1], "/b/icon.png"));
        upload(&mut cache, &mut created);
        assert_eq!(created, [1, 2]);

        cache.forget(ks[0]);

        assert_eq!(cache.drain_released(), [1]);
        assert!(cache.texture_of(ks[1]).is_some(), "the other path stays");
    }

    #[test]
    fn a_new_path_for_a_key_frees_the_old_texture_when_nobody_else_has_it() {
        let ks = keys(1);
        let mut cache = PinnedIconCache::<u32>::default();
        let mut created = Vec::new();
        cache.apply(found(ks[0], "/a/icon.png"));
        upload(&mut cache, &mut created);
        cache.apply(found(ks[0], "/b/icon.png"));
        upload(&mut cache, &mut created);
        assert_eq!(cache.drain_released(), [1]);
        assert_eq!(cache.texture_of(ks[0]).unwrap().id, 2);
    }

    #[test]
    fn the_same_path_again_for_a_key_does_not_free_or_double_count() {
        let ks = keys(1);
        let mut cache = PinnedIconCache::<u32>::default();
        let mut created = Vec::new();
        cache.apply(found(ks[0], "/a/icon.png"));
        upload(&mut cache, &mut created);
        assert_eq!(cache.apply(found(ks[0], "/a/icon.png")), Applied::Shared);
        assert!(cache.drain_released().is_empty());
        cache.forget(ks[0]);
        assert_eq!(cache.drain_released(), [1], "one use, one release");
    }

    #[test]
    fn the_app_result_takes_no_entry_and_drops_a_previous_one() {
        let ks = keys(1);
        let mut cache = PinnedIconCache::<u32>::default();
        let mut created = Vec::new();
        assert_eq!(cache.apply(app(ks[0])), Applied::App);
        assert_eq!(cache.texture_of(ks[0]), None);
        assert!(!cache.has_pending());

        cache.apply(found(ks[0], "/a/icon.png"));
        upload(&mut cache, &mut created);
        assert_eq!(cache.apply(app(ks[0])), Applied::App);
        assert_eq!(cache.drain_released(), [1]);
    }

    #[test]
    fn forgetting_a_key_before_the_gpu_drops_its_pending_bytes() {
        let ks = keys(1);
        let mut cache = PinnedIconCache::<u32>::default();
        cache.apply(found(ks[0], "/a/icon.png"));
        cache.forget(ks[0]);
        assert!(!cache.has_pending());
        let mut created = Vec::new();
        upload(&mut cache, &mut created);
        assert!(created.is_empty(), "no texture for a gone key");
    }

    #[test]
    fn the_bar_gets_the_own_textures_of_its_window_only() {
        let file = porecatu_session::PinnedFileV1 {
            tabs: vec![TabV1::default(), TabV1::default(), TabV1::default()],
            ..Default::default()
        };
        let (mut registry, _) =
            PinnedRegistry::<u8>::from_load(PinnedLoadOutcome::Loaded(file), None);
        let ks: Vec<PinnedKey> = registry.entries().iter().map(|e| e.key).collect();
        let (t1, t2, t3) = (TabId::new(1), TabId::new(2), TabId::new(3));
        registry.bind_live(ks[0], 1, t1);
        registry.bind_live(ks[1], 1, t2);
        registry.bind_live(ks[2], 2, t3);

        let mut cache = PinnedIconCache::<u32>::default();
        // `t1` has its own icon, `t2` resolved to the app icon, `t3` lives in
        // the other window.
        cache.apply(found(ks[0], "/a/icon.png"));
        cache.apply(app(ks[1]));
        cache.apply(found(ks[2], "/c/icon.png"));
        let mut created = Vec::new();
        upload(&mut cache, &mut created);

        let map = textures_for(&registry, &cache, 1, &[t1, t2]);
        assert_eq!(map.len(), 1);
        assert_eq!(map[&t1].size, (4, 2));
        assert!(!map.contains_key(&t2));
        assert!(!map.contains_key(&t3));
    }

    // ---- contain -------------------------------------------------------

    #[test]
    fn a_wide_image_fits_the_width_and_is_centered_vertically_by_the_caller() {
        let (w, h) = contain(20.0, (200, 100));
        assert_eq!((w, h), (20.0, 10.0));
    }

    #[test]
    fn a_tall_image_fits_the_height() {
        let (w, h) = contain(20.0, (100, 200));
        assert_eq!((w, h), (10.0, 20.0));
    }

    #[test]
    fn a_square_image_fills_the_square_and_a_degenerate_one_does_too() {
        assert_eq!(contain(20.0, (64, 64)), (20.0, 20.0));
        assert_eq!(contain(20.0, (0, 5)), (20.0, 20.0));
        assert_eq!(contain(20.0, (5, 0)), (20.0, 20.0));
    }

    #[test]
    fn a_small_image_is_scaled_up_to_the_square_too() {
        // The side is the em of the icon, not a cap: contain, not "at most".
        assert_eq!(contain(20.0, (4, 2)), (20.0, 10.0));
    }
}
