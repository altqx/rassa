use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    fmt, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

mod legacy_arabic_charmap;
mod legacy_charmap;

pub use legacy_charmap::{
    FontCharmap, font_data_glyph_index, font_face_charmap, font_face_glyph_index,
    font_face_uses_legacy_charmap,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum FontFaceScope {
    Any,
    Face(u32),
}

type FontCharSupportCache = HashMap<(PathBuf, FontFaceScope, char), bool>;

static FONT_CHAR_SUPPORT_CACHE: OnceLock<Mutex<FontCharSupportCache>> = OnceLock::new();

fn font_char_support_cache() -> &'static Mutex<FontCharSupportCache> {
    FONT_CHAR_SUPPORT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(not(target_arch = "wasm32"))]
use fontdb::{Database, Family, Query, Source, Stretch, Style as FontdbStyle, Weight};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontAttachment {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontQuery {
    pub family: String,
    pub style: Option<String>,
    pub weight: Option<i32>,
}

impl FontQuery {
    pub fn new(family: impl Into<String>) -> Self {
        Self {
            family: family.into(),
            style: None,
            weight: None,
        }
    }

    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    pub fn with_weight(mut self, weight: i32) -> Self {
        self.weight = Some(weight);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum FontProviderKind {
    #[default]
    Null,
    Fontconfig,
    Attached,
    DefaultFile,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontMatch {
    pub family: String,
    pub path: Option<PathBuf>,
    pub face_index: Option<u32>,
    pub style: Option<String>,
    pub synthetic_bold: bool,
    pub synthetic_italic: bool,
    pub provider: FontProviderKind,
}

impl FontMatch {
    pub fn unresolved(
        family: impl Into<String>,
        style: Option<String>,
        provider: FontProviderKind,
    ) -> Self {
        Self {
            family: family.into(),
            path: None,
            face_index: None,
            style,
            synthetic_bold: false,
            synthetic_italic: false,
            provider,
        }
    }
}

/// Collision-free identity for a font provider whose answers are stable.
///
/// The opaque allocation is compared by identity, so independent providers cannot accidentally
/// alias one another. Providers must replace the key whenever an observable resolution result can
/// change.
#[doc(hidden)]
#[derive(Clone)]
pub struct FontProviderCacheKey(Arc<()>);

impl FontProviderCacheKey {
    #[doc(hidden)]
    pub fn new() -> Self {
        Self(Arc::new(()))
    }
}

impl Default for FontProviderCacheKey {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for FontProviderCacheKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("FontProviderCacheKey(..)")
    }
}

impl PartialEq for FontProviderCacheKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FontProviderCacheKey {}

/// Best-effort identity for one live font-provider object.
///
/// Stable providers should prefer [`FontProvider::layout_cache_key`]. This fallback lets renderers
/// keep short-lived state for an uncacheable provider without sharing it with another live object.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontProviderInstanceKey {
    type_name: &'static str,
    address: usize,
}

impl FontProviderInstanceKey {
    fn of<T: ?Sized>(provider: &T) -> Self {
        Self {
            type_name: std::any::type_name::<T>(),
            address: provider as *const T as *const () as usize,
        }
    }
}

pub trait FontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch;

    /// Stable, collision-free identity for timestamp-independent layout caching.
    ///
    /// Custom providers default to no caching because their answers may change through
    /// interior state. An implementation that opts in must create a fresh key whenever any
    /// resolution answer can change.
    #[doc(hidden)]
    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        None
    }

    /// Identity of this live provider object for state that cannot be shared across providers.
    #[doc(hidden)]
    fn instance_cache_key(&self) -> FontProviderInstanceKey {
        FontProviderInstanceKey::of(self)
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        let resolved = self.resolve(query);
        if resolved.path.is_some() && font_match_supports_text(&resolved, text) {
            resolved
        } else {
            FontMatch::unresolved(query.family.clone(), query.style.clone(), resolved.provider)
        }
    }

    fn resolve_family(&self, family: &str) -> FontMatch {
        self.resolve(&FontQuery::new(family))
    }
}

impl<T: FontProvider + ?Sized> FontProvider for Box<T> {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        (**self).resolve(query)
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        (**self).resolve_for_text(query, text)
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        (**self).layout_cache_key()
    }

    fn instance_cache_key(&self) -> FontProviderInstanceKey {
        (**self).instance_cache_key()
    }
}

impl<T: FontProvider + ?Sized> FontProvider for &T {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        (**self).resolve(query)
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        (**self).resolve_for_text(query, text)
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        (**self).layout_cache_key()
    }

    fn instance_cache_key(&self) -> FontProviderInstanceKey {
        (**self).instance_cache_key()
    }
}

#[derive(Default)]
pub struct NullFontProvider;

impl FontProvider for NullFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        FontMatch::unresolved(
            query.family.clone(),
            query.style.clone(),
            FontProviderKind::Null,
        )
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        static KEY: OnceLock<FontProviderCacheKey> = OnceLock::new();
        Some(KEY.get_or_init(FontProviderCacheKey::new).clone())
    }
}

pub struct CrossfontProvider {
    fallback_family: Option<String>,
    config_path: Option<PathBuf>,
    resolve_cache: Mutex<HashMap<FontResolveKey, FontMatch>>,
    layout_cache_key: FontProviderCacheKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FontResolveKey {
    family: String,
    style: Option<String>,
    weight: Option<i32>,
}

impl From<&FontQuery> for FontResolveKey {
    fn from(query: &FontQuery) -> Self {
        Self {
            family: query.family.clone(),
            style: query.style.clone(),
            weight: query.weight,
        }
    }
}

pub type FontconfigProvider = CrossfontProvider;

impl Default for CrossfontProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CrossfontProvider {
    pub fn new() -> Self {
        Self {
            fallback_family: Some("Arial".to_string()),
            config_path: None,
            resolve_cache: Mutex::new(HashMap::new()),
            layout_cache_key: FontProviderCacheKey::new(),
        }
    }

    pub fn with_fallback_family(fallback_family: impl Into<String>) -> Self {
        Self {
            fallback_family: Some(fallback_family.into()),
            config_path: None,
            resolve_cache: Mutex::new(HashMap::new()),
            layout_cache_key: FontProviderCacheKey::new(),
        }
    }

    /// Use a host fc-match config; fall back to fontdb if it fails.
    pub fn with_config(config_path: impl Into<PathBuf>) -> Self {
        Self {
            fallback_family: Some("Arial".to_string()),
            config_path: Some(config_path.into()),
            resolve_cache: Mutex::new(HashMap::new()),
            layout_cache_key: FontProviderCacheKey::new(),
        }
    }

    pub fn with_config_and_fallback_family(
        config_path: impl Into<PathBuf>,
        fallback_family: impl Into<String>,
    ) -> Self {
        Self {
            fallback_family: Some(fallback_family.into()),
            config_path: Some(config_path.into()),
            resolve_cache: Mutex::new(HashMap::new()),
            layout_cache_key: FontProviderCacheKey::new(),
        }
    }

    #[cfg(test)]
    fn resolve_cache_len_for_tests(&self) -> usize {
        self.resolve_cache
            .lock()
            .expect("font resolve cache mutex poisoned")
            .len()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn find_font(
        &self,
        family: String,
        style: Option<String>,
        weight: Option<i32>,
    ) -> Option<FontMatch> {
        resolve_system_font(
            &family,
            style.as_deref(),
            weight,
            self.config_path.as_deref(),
        )
        .map(|(resolved_family, resolved_path, face_index)| {
            let resolved_style = resolved_path
                .as_deref()
                .and_then(|path| load_face_metadata(path).and_then(|(_, style)| style));
            let (synthetic_bold, synthetic_italic) =
                synthetic_style_flags(style.as_deref(), weight, resolved_style.as_deref());

            FontMatch {
                family: resolved_family,
                path: resolved_path,
                face_index,
                style,
                synthetic_bold,
                synthetic_italic,
                provider: FontProviderKind::Fontconfig,
            }
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn find_font(
        &self,
        _family: String,
        _style: Option<String>,
        _weight: Option<i32>,
    ) -> Option<FontMatch> {
        let _ = &self.config_path;
        None
    }
}

impl FontProvider for CrossfontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let cache_key = FontResolveKey::from(query);
        if let Some(cached) = self
            .resolve_cache
            .lock()
            .expect("font resolve cache mutex poisoned")
            .get(&cache_key)
            .cloned()
        {
            return cached;
        }

        let resolved = if let Some(font) =
            self.find_font(query.family.clone(), query.style.clone(), query.weight)
        {
            font
        } else if let Some(fallback_family) = &self.fallback_family {
            self.find_font(fallback_family.clone(), query.style.clone(), query.weight)
                .unwrap_or_else(|| {
                    FontMatch::unresolved(
                        query.family.clone(),
                        query.style.clone(),
                        FontProviderKind::Fontconfig,
                    )
                })
        } else {
            FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::Fontconfig,
            )
        };

        self.resolve_cache
            .lock()
            .expect("font resolve cache mutex poisoned")
            .insert(cache_key, resolved.clone());
        resolved
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        Some(self.layout_cache_key.clone())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_system_font(
    family: &str,
    style: Option<&str>,
    weight: Option<i32>,
    config_path: Option<&Path>,
) -> Option<(String, Option<PathBuf>, Option<u32>)> {
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = config_path;

    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some((path, face_index)) =
        fontconfig_match_path(family, style, weight, None, config_path)
    {
        let resolved_family = load_face_metadata(&path)
            .map(|(family, _)| family)
            .unwrap_or_else(|| family.to_owned());
        if config_path.is_some() || fontconfig_match_is_acceptable(family, &resolved_family) {
            return Some((resolved_family, Some(path), face_index));
        }
    }

    let mut database = Database::new();
    database.load_system_fonts();

    let requested_style = style.map(normalize_font_key);
    let wants_bold = requested_style
        .as_deref()
        .is_some_and(|style| style.contains("bold"))
        || weight.is_some_and(bold_weight_is_active);
    let fontdb_style = requested_style
        .as_deref()
        .map(|style| {
            if style.contains("italic") || style.contains("oblique") {
                FontdbStyle::Italic
            } else {
                FontdbStyle::Normal
            }
        })
        .unwrap_or(FontdbStyle::Normal);

    let normalized_family = normalize_font_key(family);
    let family_query = match normalized_family.as_str() {
        "sans" | "sansserif" => Family::SansSerif,
        "serif" => Family::Serif,
        "mono" | "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(family),
    };

    let query = Query {
        families: &[family_query],
        weight: weight.map(fontdb_weight).unwrap_or(if wants_bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        }),
        stretch: Stretch::Normal,
        style: fontdb_style,
    };
    let Some(id) = database.query(&query).or_else(|| {
        let fallback = Query {
            families: &[family_query],
            weight: Weight::NORMAL,
            stretch: Stretch::Normal,
            style: FontdbStyle::Normal,
        };
        database.query(&fallback)
    }) else {
        return windows_known_font_path(family).map(|path| (family.to_owned(), Some(path), None));
    };
    let face = database.face(id)?;
    let resolved_family = face
        .families
        .first()
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| family.to_owned());
    let (path, face_index) = match &face.source {
        Source::File(path) => (
            Some(path.clone()),
            Some(face.index).filter(|index| *index > 0),
        ),
        Source::SharedFile(path, _) => (
            Some(path.clone()),
            Some(face.index).filter(|index| *index > 0),
        ),
        _ => (None, Some(face.index).filter(|index| *index > 0)),
    };
    let path = path
        .or_else(|| windows_known_font_path(&resolved_family))
        .or_else(|| windows_known_font_path(family));
    Some((resolved_family, path, face_index))
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
pub fn resolve_system_font_for_char(
    family: &str,
    style: Option<&str>,
    character: char,
) -> Option<(String, Option<PathBuf>, Option<u32>)> {
    let (path, face_index) = fontconfig_match_path(family, style, None, Some(character), None)?;
    if !font_file_face_supports_char(&path, face_index.unwrap_or(0), character) {
        return None;
    }
    let resolved_family = load_face_metadata(&path)
        .map(|(family, _)| family)
        .unwrap_or_else(|| family.to_owned());
    Some((resolved_family, Some(path), face_index))
}

#[cfg(not(all(unix, not(target_os = "macos"), not(target_arch = "wasm32"))))]
pub fn resolve_system_font_for_char(
    _family: &str,
    _style: Option<&str>,
    _character: char,
) -> Option<(String, Option<PathBuf>, Option<u32>)> {
    None
}

pub fn font_match_supports_text(font: &FontMatch, text: &str) -> bool {
    let Some(path) = &font.path else {
        return false;
    };
    font_file_supports_text_in_scope(
        path,
        FontFaceScope::Face(font.face_index.unwrap_or(0)),
        text,
    )
}

fn font_file_supports_text_in_scope(path: &Path, scope: FontFaceScope, text: &str) -> bool {
    let characters = text
        .chars()
        .filter(|character| !character.is_whitespace() && !character.is_control())
        .collect::<HashSet<_>>();
    let mut missing = Vec::new();
    let mut all_supported = true;
    {
        let cache = font_char_support_cache()
            .lock()
            .expect("font char support cache mutex poisoned");
        for character in characters {
            match cache.get(&(path.to_path_buf(), scope, character)) {
                Some(false) => all_supported = false,
                Some(true) => {}
                None => missing.push(character),
            }
        }
    }
    if missing.is_empty() {
        return all_supported;
    }

    let data = fs::read(path).ok();
    let results = missing
        .into_iter()
        .map(|character| {
            let supports = data
                .as_deref()
                .is_some_and(|data| font_data_supports_char(data, scope, character));
            (character, supports)
        })
        .collect::<Vec<_>>();
    all_supported &= results.iter().all(|(_, supports)| *supports);
    let mut cache = font_char_support_cache()
        .lock()
        .expect("font char support cache mutex poisoned");
    for (character, supports) in results {
        cache.insert((path.to_path_buf(), scope, character), supports);
    }
    all_supported
}

pub fn font_file_supports_char(path: &Path, character: char) -> bool {
    font_file_supports_char_in_scope(path, FontFaceScope::Any, character)
}

pub fn font_file_face_supports_char(path: &Path, face_index: u32, character: char) -> bool {
    font_file_supports_char_in_scope(path, FontFaceScope::Face(face_index), character)
}

fn font_file_supports_char_in_scope(path: &Path, scope: FontFaceScope, character: char) -> bool {
    let cache_key = (path.to_path_buf(), scope, character);
    if let Some(supports_char) = font_char_support_cache()
        .lock()
        .expect("font char support cache mutex poisoned")
        .get(&cache_key)
        .copied()
    {
        return supports_char;
    }

    let supports_char = font_file_supports_char_uncached(path, scope, character);
    font_char_support_cache()
        .lock()
        .expect("font char support cache mutex poisoned")
        .insert(cache_key, supports_char);
    supports_char
}

fn font_file_supports_char_uncached(path: &Path, scope: FontFaceScope, character: char) -> bool {
    let Ok(data) = fs::read(path) else {
        return false;
    };
    font_data_supports_char(&data, scope, character)
}

fn font_data_supports_char(data: &[u8], scope: FontFaceScope, character: char) -> bool {
    match scope {
        FontFaceScope::Any => {
            let face_count = ttf_parser::fonts_in_collection(data).unwrap_or(1).max(1);
            (0..face_count).any(|index| {
                font_data_glyph_index(data, index, character).is_some_and(|glyph| glyph.0 != 0)
            })
        }
        FontFaceScope::Face(index) => {
            font_data_glyph_index(data, index, character).is_some_and(|glyph| glyph.0 != 0)
        }
    }
}

#[cfg(windows)]
fn windows_known_font_path(family: &str) -> Option<PathBuf> {
    let normalized = normalize_font_key(family);
    let candidates: &[&str] = match normalized.as_str() {
        "arial" | "sans" | "sansserif" => &["arial.ttf", "segoeui.ttf"],
        "segoeui" | "segoe ui" => &["segoeui.ttf"],
        "timesnewroman" | "times new roman" | "serif" => &["times.ttf"],
        "couriernew" | "courier new" | "mono" | "monospace" => &["cour.ttf", "consola.ttf"],
        _ => &[],
    };
    let windows_dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    candidates
        .iter()
        .map(|candidate| windows_dir.join("Fonts").join(candidate))
        .find(|path| path.exists())
}

#[cfg(all(not(windows), not(target_arch = "wasm32")))]
fn windows_known_font_path(_family: &str) -> Option<PathBuf> {
    None
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn fontconfig_match_is_acceptable(requested_family: &str, resolved_family: &str) -> bool {
    let requested = normalize_font_key(requested_family);
    let resolved = normalize_font_key(resolved_family);
    if requested == resolved {
        return true;
    }
    matches!(
        requested.as_str(),
        "arial"
            | "helvetica"
            | "timesnewroman"
            | "times"
            | "couriernew"
            | "courier"
            | "sans"
            | "sansserif"
            | "serif"
            | "mono"
            | "monospace"
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn fontconfig_match_path(
    family: &str,
    style: Option<&str>,
    weight: Option<i32>,
    character: Option<char>,
    config_path: Option<&Path>,
) -> Option<(PathBuf, Option<u32>)> {
    let pattern = fontconfig_pattern(family, style, weight, character);
    let mut command = std::process::Command::new("fc-match");
    command.args(["-f", "%{file}\n%{index}", &pattern]);
    if let Some(config_path) = config_path {
        command.env("FONTCONFIG_FILE", config_path);
    }
    let output = command.output().ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let mut lines = text.lines();
    let path = PathBuf::from(lines.next()?.trim());
    let face_index = lines
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|index| *index > 0);
    path.exists().then_some((path, face_index))
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
pub fn validate_fontconfig_config(config_path: impl AsRef<Path>) -> Result<(), String> {
    let output = std::process::Command::new("fc-match")
        .env("FONTCONFIG_FILE", config_path.as_ref())
        .args(["-f", "%{file}", "sans"])
        .output()
        .map_err(|error| format!("unable to run fc-match: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() {
            format!("fc-match exited with {}", output.status)
        } else {
            error
        });
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if path.is_file() {
        Ok(())
    } else {
        Err("fontconfig did not return a usable font path".to_string())
    }
}

#[cfg(not(all(unix, not(target_os = "macos"), not(target_arch = "wasm32"))))]
pub fn validate_fontconfig_config(_config_path: impl AsRef<Path>) -> Result<(), String> {
    Err("custom fontconfig configurations are unavailable on this target".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn fontconfig_pattern(
    family: &str,
    style: Option<&str>,
    weight: Option<i32>,
    character: Option<char>,
) -> String {
    let mut pattern = family.to_owned();
    if let Some(style) = style.filter(|value| !value.trim().is_empty()) {
        let normalized = normalize_font_key(style);
        if normalized.contains("bold") {
            pattern.push_str(":weight=bold");
        }
        if normalized.contains("italic") || normalized.contains("oblique") {
            pattern.push_str(":slant=italic");
        }
        if !normalized.contains("bold")
            && !normalized.contains("italic")
            && !normalized.contains("oblique")
        {
            pattern.push_str(":style=");
            pattern.push_str(style.trim());
        }
    }
    if let Some(weight) = weight {
        pattern.push_str(":weight=");
        pattern.push_str(&normalize_weight(weight).to_string());
    }
    if let Some(character) = character {
        pattern.push_str(":charset=");
        pattern.push_str(&format!("{:x}", character as u32));
    }
    pattern
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn fontconfig_pattern_requests_weight_and_slant_for_bold_italic() {
    let pattern = fontconfig_pattern("DejaVu Sans", Some("Bold Italic"), None, None);

    assert!(pattern.contains(":weight=bold"));
    assert!(pattern.contains(":slant=italic"));
    assert!(!pattern.contains(":style=Bold Italic"));
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn fontconfig_pattern_preserves_numeric_weight() {
    let pattern = fontconfig_pattern("DejaVu Sans", None, Some(500), None);

    assert!(pattern.contains(":weight=500"));
    assert!(!pattern.contains(":weight=bold"));
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AttachedFontRecord {
    family: String,
    path: PathBuf,
    face_index: Option<u32>,
    style: Option<String>,
    weight: i32,
    italic: bool,
    bold: bool,
    aliases: FontRecordAliases,
}

#[derive(Clone, Default)]
pub struct AttachedFontProvider {
    fonts: Vec<AttachedFontRecord>,
    layout_cache_key: FontProviderCacheKey,
}

impl fmt::Debug for AttachedFontProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachedFontProvider")
            .field("fonts", &self.fonts)
            .finish()
    }
}

impl PartialEq for AttachedFontProvider {
    fn eq(&self, other: &Self) -> bool {
        self.fonts == other.fonts
    }
}

impl Eq for AttachedFontProvider {}

impl AttachedFontProvider {
    pub fn from_attachments(attachments: &[FontAttachment]) -> Self {
        Self::from_attachments_in_dir(attachments, None::<&Path>)
    }

    pub fn from_attachments_in_dir(
        attachments: &[FontAttachment],
        base_dir: Option<impl AsRef<Path>>,
    ) -> Self {
        let root = base_dir
            .as_ref()
            .map(|path| path.as_ref().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir().join("rassa-attached-fonts"));
        let _ = fs::create_dir_all(&root);
        let fonts = attachments
            .iter()
            .flat_map(|attachment| {
                AttachedFontRecord::from_attachment_parts(&attachment.name, &attachment.data, &root)
            })
            .collect();

        Self {
            fonts,
            layout_cache_key: FontProviderCacheKey::new(),
        }
    }

    /// Build from borrowed attachment payloads without copying multi-megabyte font buffers.
    #[doc(hidden)]
    pub fn from_attachment_slices<'a>(
        attachments: impl IntoIterator<Item = (&'a str, &'a [u8])>,
    ) -> Self {
        let root = std::env::temp_dir().join("rassa-attached-fonts");
        let _ = fs::create_dir_all(&root);
        let fonts = attachments
            .into_iter()
            .flat_map(|(name, data)| AttachedFontRecord::from_attachment_parts(name, data, &root))
            .collect();
        Self {
            fonts,
            layout_cache_key: FontProviderCacheKey::new(),
        }
    }
}

impl FontProvider for AttachedFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let style_key = query.style.as_deref().map(normalize_font_key);

        if let Some(font) = select_local_font(
            &self.fonts,
            &query.family,
            style_key.as_deref(),
            query.weight,
        ) {
            let (synthetic_bold, synthetic_italic) =
                synthetic_style_flags(query.style.as_deref(), query.weight, font.style.as_deref());
            return FontMatch {
                family: font.family.clone(),
                path: Some(font.path.clone()),
                face_index: font.face_index,
                style: font.style.clone(),
                synthetic_bold,
                synthetic_italic,
                provider: FontProviderKind::Attached,
            };
        }

        FontMatch::unresolved(
            query.family.clone(),
            query.style.clone(),
            FontProviderKind::Attached,
        )
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        let style_key = query.style.as_deref().map(normalize_font_key);
        let Some(font) = select_local_font_for_text(
            &self.fonts,
            &query.family,
            style_key.as_deref(),
            query.weight,
            text,
        ) else {
            return FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::Attached,
            );
        };
        let (synthetic_bold, synthetic_italic) =
            synthetic_style_flags(query.style.as_deref(), query.weight, font.style.as_deref());
        FontMatch {
            family: font.family.clone(),
            path: Some(font.path.clone()),
            face_index: font.face_index,
            style: font.style.clone(),
            synthetic_bold,
            synthetic_italic,
            provider: FontProviderKind::Attached,
        }
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        Some(self.layout_cache_key.clone())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryFontRecord {
    family: String,
    path: PathBuf,
    face_index: Option<u32>,
    style: Option<String>,
    weight: i32,
    italic: bool,
    bold: bool,
    aliases: FontRecordAliases,
}

trait LocalFontRecord {
    fn aliases(&self) -> &FontRecordAliases;
    fn weight(&self) -> i32;
    fn italic(&self) -> bool;
    fn bold(&self) -> bool;
    fn font_match(&self) -> FontMatch;
}

impl LocalFontRecord for AttachedFontRecord {
    fn aliases(&self) -> &FontRecordAliases {
        &self.aliases
    }

    fn weight(&self) -> i32 {
        self.weight
    }
    fn italic(&self) -> bool {
        self.italic
    }
    fn bold(&self) -> bool {
        self.bold
    }
    fn font_match(&self) -> FontMatch {
        FontMatch {
            family: self.family.clone(),
            path: Some(self.path.clone()),
            face_index: self.face_index,
            style: self.style.clone(),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        }
    }
}

impl LocalFontRecord for DirectoryFontRecord {
    fn aliases(&self) -> &FontRecordAliases {
        &self.aliases
    }

    fn weight(&self) -> i32 {
        self.weight
    }
    fn italic(&self) -> bool {
        self.italic
    }
    fn bold(&self) -> bool {
        self.bold
    }
    fn font_match(&self) -> FontMatch {
        FontMatch {
            family: self.family.clone(),
            path: Some(self.path.clone()),
            face_index: self.face_index,
            style: self.style.clone(),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        }
    }
}

fn select_local_font<'a, T: LocalFontRecord>(
    fonts: &'a [T],
    requested_name: &str,
    style_key: Option<&str>,
    requested_weight: Option<i32>,
) -> Option<&'a T> {
    select_local_font_filtered(fonts, requested_name, style_key, requested_weight, |_| true)
}

fn select_local_font_for_text<'a, T: LocalFontRecord>(
    fonts: &'a [T],
    requested_name: &str,
    style_key: Option<&str>,
    requested_weight: Option<i32>,
    text: &str,
) -> Option<&'a T> {
    select_local_font_filtered(fonts, requested_name, style_key, requested_weight, |font| {
        font_match_supports_text(&font.font_match(), text)
    })
}

fn select_local_font_filtered<'a, T: LocalFontRecord>(
    fonts: &'a [T],
    requested_name: &str,
    style_key: Option<&str>,
    requested_weight: Option<i32>,
    supports_text: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    let family_key = font_name_match_key(requested_name);
    let full_name_key = font_name_match_key(requested_name);
    let wants_italic =
        style_key.is_some_and(|style| style.contains("italic") || style.contains("oblique"));
    let requested_weight = requested_weight.unwrap_or(400);
    let mut selected = None;
    let mut best_score = u32::MAX;
    for font in fonts {
        let family_match = font
            .aliases()
            .family
            .iter()
            .any(|name| name == family_key.as_str());
        let full_name_match = font.aliases().matches_full_or_postscript(&full_name_key);
        let score = if family_match {
            font_attribute_score(font, wants_italic, requested_weight)
        } else if full_name_match {
            0
        } else {
            continue;
        };
        if !supports_text(font) {
            continue;
        }
        if score < best_score {
            selected = Some(font);
            best_score = score;
        }
        if score == 0 {
            break;
        }
    }
    if selected.is_some() {
        return selected;
    }

    None
}

fn font_attribute_score<T: LocalFontRecord>(
    font: &T,
    wants_italic: bool,
    requested_weight: i32,
) -> u32 {
    let mut score = if wants_italic && !font.italic() {
        1
    } else if !wants_italic && font.italic() {
        4
    } else {
        0
    };
    let mut effective_weight = font.weight();
    if requested_weight > effective_weight + 150 && !font.bold() {
        effective_weight += 120;
    }
    score += (73 * (effective_weight - requested_weight).unsigned_abs()) / 256;
    score
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontDirectoryIssue {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for FontDirectoryIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

#[derive(Clone, Default)]
pub struct DirectoryFontProvider {
    fonts: Vec<DirectoryFontRecord>,
    layout_cache_key: FontProviderCacheKey,
}

impl fmt::Debug for DirectoryFontProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DirectoryFontProvider")
            .field("fonts", &self.fonts)
            .finish()
    }
}

impl PartialEq for DirectoryFontProvider {
    fn eq(&self, other: &Self) -> bool {
        self.fonts == other.fonts
    }
}

impl Eq for DirectoryFontProvider {}

impl DirectoryFontProvider {
    /// Scan a libass font dir: skip hidden/subdirs; accept files by contents, not extension.
    pub fn scan(directory: impl AsRef<Path>) -> (Self, Vec<FontDirectoryIssue>) {
        let directory = directory.as_ref();
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                return (
                    Self::default(),
                    vec![FontDirectoryIssue {
                        path: directory.to_path_buf(),
                        message: format!("unable to read font directory: {error}"),
                    }],
                );
            }
        };

        let mut fonts = Vec::new();
        let mut issues = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    issues.push(FontDirectoryIssue {
                        path: directory.to_path_buf(),
                        message: format!("unable to read directory entry: {error}"),
                    });
                    continue;
                }
            };
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }

            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let data = match fs::read(&path) {
                Ok(data) => data,
                Err(error) => {
                    issues.push(FontDirectoryIssue {
                        path,
                        message: format!("unable to read font file: {error}"),
                    });
                    continue;
                }
            };
            let records = directory_font_records(&path, &data);
            if records.is_empty() {
                issues.push(FontDirectoryIssue {
                    path,
                    message: "not a usable OpenType font".to_string(),
                });
            } else {
                fonts.extend(records);
            }
        }

        (
            Self {
                fonts,
                layout_cache_key: FontProviderCacheKey::new(),
            },
            issues,
        )
    }
}

impl FontProvider for DirectoryFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let style_key = query.style.as_deref().map(normalize_font_key);

        if let Some(font) = select_local_font(
            &self.fonts,
            &query.family,
            style_key.as_deref(),
            query.weight,
        ) {
            let (synthetic_bold, synthetic_italic) =
                synthetic_style_flags(query.style.as_deref(), query.weight, font.style.as_deref());
            return FontMatch {
                family: font.family.clone(),
                path: Some(font.path.clone()),
                face_index: font.face_index,
                style: font.style.clone(),
                synthetic_bold,
                synthetic_italic,
                provider: FontProviderKind::Attached,
            };
        }

        FontMatch::unresolved(
            query.family.clone(),
            query.style.clone(),
            FontProviderKind::Attached,
        )
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        let style_key = query.style.as_deref().map(normalize_font_key);
        let Some(font) = select_local_font_for_text(
            &self.fonts,
            &query.family,
            style_key.as_deref(),
            query.weight,
            text,
        ) else {
            return FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::Attached,
            );
        };
        let (synthetic_bold, synthetic_italic) =
            synthetic_style_flags(query.style.as_deref(), query.weight, font.style.as_deref());
        FontMatch {
            family: font.family.clone(),
            path: Some(font.path.clone()),
            face_index: font.face_index,
            style: font.style.clone(),
            synthetic_bold,
            synthetic_italic,
            provider: FontProviderKind::Attached,
        }
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        Some(self.layout_cache_key.clone())
    }
}

fn directory_font_records(path: &Path, data: &[u8]) -> Vec<DirectoryFontRecord> {
    let face_count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    (0..face_count)
        .filter_map(|index| {
            let face = ttf_parser::Face::parse(data, index).ok()?;
            let metadata = font_face_metadata(&face)?;
            Some(DirectoryFontRecord {
                family: metadata.family,
                path: path.to_path_buf(),
                face_index: Some(index).filter(|index| *index > 0),
                style: metadata.style,
                weight: metadata.weight,
                italic: metadata.italic,
                bold: metadata.bold,
                aliases: metadata.aliases,
            })
        })
        .collect()
}

pub struct MergedFontProvider<P, S> {
    primary: P,
    secondary: S,
    layout_cache_key: Mutex<
        Option<(
            FontProviderCacheKey,
            FontProviderCacheKey,
            FontProviderCacheKey,
        )>,
    >,
}

impl<P, S> MergedFontProvider<P, S> {
    pub fn new(primary: P, secondary: S) -> Self {
        Self {
            primary,
            secondary,
            layout_cache_key: Mutex::new(None),
        }
    }
}

impl<P: FontProvider, S: FontProvider> FontProvider for MergedFontProvider<P, S> {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let primary = self.primary.resolve(query);
        if primary.path.is_some() {
            primary
        } else {
            self.secondary.resolve(query)
        }
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        let primary = self.primary.resolve_for_text(query, text);
        if primary.path.is_some() {
            primary
        } else {
            self.secondary.resolve_for_text(query, text)
        }
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        let primary_key = self.primary.layout_cache_key()?;
        let secondary_key = self.secondary.layout_cache_key()?;
        let mut cache = self
            .layout_cache_key
            .lock()
            .expect("merged provider cache key mutex poisoned");
        if let Some((cached_primary, cached_secondary, key)) = cache.as_ref()
            && cached_primary == &primary_key
            && cached_secondary == &secondary_key
        {
            return Some(key.clone());
        }
        let key = FontProviderCacheKey::new();
        *cache = Some((primary_key, secondary_key, key.clone()));
        Some(key)
    }
}

pub struct DefaultFontFileProvider<P> {
    primary: P,
    path: PathBuf,
    family: Option<String>,
    layout_cache_key: Mutex<Option<(FontProviderCacheKey, FontProviderCacheKey)>>,
}

impl<P> DefaultFontFileProvider<P> {
    pub fn new(primary: P, path: impl Into<PathBuf>) -> Self {
        Self {
            primary,
            path: path.into(),
            family: None,
            layout_cache_key: Mutex::new(None),
        }
    }

    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self.layout_cache_key = Mutex::new(None);
        self
    }
}

impl<P: FontProvider> FontProvider for DefaultFontFileProvider<P> {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let primary = self.primary.resolve(query);
        if primary.path.is_some() {
            return primary;
        }

        FontMatch {
            family: self.family.clone().unwrap_or_else(|| query.family.clone()),
            path: Some(self.path.clone()),
            face_index: None,
            style: query.style.clone(),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::DefaultFile,
        }
    }

    fn resolve_for_text(&self, query: &FontQuery, text: &str) -> FontMatch {
        let primary = self.primary.resolve_for_text(query, text);
        if primary.path.is_some() {
            return primary;
        }
        let fallback = FontMatch {
            family: self.family.clone().unwrap_or_else(|| query.family.clone()),
            path: Some(self.path.clone()),
            face_index: None,
            style: query.style.clone(),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::DefaultFile,
        };
        if font_match_supports_text(&fallback, text) {
            fallback
        } else {
            FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::DefaultFile,
            )
        }
    }

    fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
        let primary_key = self.primary.layout_cache_key()?;
        let mut cache = self
            .layout_cache_key
            .lock()
            .expect("default-file provider cache key mutex poisoned");
        if let Some((cached_primary, key)) = cache.as_ref()
            && cached_primary == &primary_key
        {
            return Some(key.clone());
        }
        let key = FontProviderCacheKey::new();
        *cache = Some((primary_key, key.clone()));
        Some(key)
    }
}

fn synthetic_style_flags(
    requested: Option<&str>,
    requested_weight: Option<i32>,
    resolved: Option<&str>,
) -> (bool, bool) {
    let requested = requested.map(normalize_font_key).unwrap_or_default();
    let resolved = resolved.map(normalize_font_key).unwrap_or_default();
    (
        (requested.contains("bold") || requested_weight.is_some_and(bold_weight_is_active))
            && !resolved.contains("bold"),
        (requested.contains("italic") || requested.contains("oblique"))
            && !(resolved.contains("italic") || resolved.contains("oblique")),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn normalize_weight(weight: i32) -> i32 {
    weight.clamp(1, 1000)
}

#[cfg(not(target_arch = "wasm32"))]
fn fontdb_weight(weight: i32) -> Weight {
    Weight(normalize_weight(weight) as u16)
}

fn bold_weight_is_active(weight: i32) -> bool {
    weight == 1 || !(0..700).contains(&weight)
}

impl AttachedFontRecord {
    fn from_attachment_parts(name: &str, data: &[u8], root: &Path) -> Vec<Self> {
        if data.is_empty() {
            return Vec::new();
        }

        let Some(path) = materialize_attachment(root, name, data) else {
            return Vec::new();
        };
        let face_count = ttf_parser::fonts_in_collection(data).unwrap_or(1).max(1);
        (0..face_count)
            .filter_map(|index| {
                let face = ttf_parser::Face::parse(data, index).ok()?;
                let metadata = font_face_metadata(&face)?;
                Some(Self {
                    family: metadata.family,
                    path: path.clone(),
                    face_index: Some(index).filter(|index| *index > 0),
                    style: metadata.style,
                    weight: metadata.weight,
                    italic: metadata.italic,
                    bold: metadata.bold,
                    aliases: metadata.aliases,
                })
            })
            .collect::<Vec<_>>()
    }
}

fn materialize_attachment(root: &Path, name: &str, data: &[u8]) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    data.hash(&mut hasher);
    let hash = hasher.finish();
    let sanitized = sanitize_attachment_name(name);
    let path = root.join(format!("{hash:016x}-{sanitized}"));
    if !path.exists() && fs::write(&path, data).is_err() {
        return None;
    }
    Some(path)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct FontRecordAliases {
    family: Vec<String>,
    full_names: Vec<String>,
    postscript_names: Vec<String>,
    postscript_outlines: bool,
}

impl FontRecordAliases {
    fn sort_and_dedup(&mut self) {
        for names in [
            &mut self.family,
            &mut self.full_names,
            &mut self.postscript_names,
        ] {
            names.sort();
            names.dedup();
        }
    }

    fn matches_full_or_postscript(&self, key: &str) -> bool {
        let full_name = self.full_names.iter().any(|name| name == key);
        let postscript_name = self.postscript_names.iter().any(|name| name == key);
        if full_name == postscript_name {
            full_name
        } else if self.postscript_outlines {
            postscript_name
        } else {
            full_name
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FontFaceMetadata {
    family: String,
    style: Option<String>,
    weight: i32,
    italic: bool,
    bold: bool,
    aliases: FontRecordAliases,
}

#[cfg(not(target_arch = "wasm32"))]
fn load_font_face_metadata(path: &Path) -> Option<FontFaceMetadata> {
    let data = fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    font_face_metadata(&face)
}

#[cfg(not(target_arch = "wasm32"))]
fn load_face_metadata(path: &Path) -> Option<(String, Option<String>)> {
    load_font_face_metadata(path).map(|metadata| (metadata.family, metadata.style))
}

fn font_face_metadata(face: &ttf_parser::Face<'_>) -> Option<FontFaceMetadata> {
    let typographic_families = font_names(face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY);
    let legacy_families = windows_font_names(face, ttf_parser::name_id::FAMILY);
    let selected_families = if !legacy_families.is_empty() {
        &legacy_families
    } else {
        &typographic_families
    };
    let family = selected_families.first().cloned()?;
    let style = font_name(face, ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY)
        .or_else(|| font_name(face, ttf_parser::name_id::SUBFAMILY));
    let mut aliases = FontRecordAliases {
        family: selected_families
            .iter()
            .map(|name| font_name_match_key(name))
            .collect(),
        full_names: windows_font_names(face, ttf_parser::name_id::FULL_NAME)
            .iter()
            .map(|name| font_name_match_key(name))
            .collect(),
        postscript_names: font_names(face, ttf_parser::name_id::POST_SCRIPT_NAME)
            .iter()
            .map(|name| font_name_match_key(name))
            .collect(),
        postscript_outlines: face.tables().cff.is_some() || face.tables().cff2.is_some(),
    };
    aliases.sort_and_dedup();
    Some(FontFaceMetadata {
        family,
        style,
        weight: libass_face_weight(face),
        italic: face.is_italic() || face.is_oblique(),
        bold: face.is_bold(),
        aliases,
    })
}

fn libass_face_weight(face: &ttf_parser::Face<'_>) -> i32 {
    let weight = face.weight().to_number();
    match weight {
        0 => 300 * i32::from(face.is_bold()) + 400,
        1 => 100,
        2 => 200,
        3 => 300,
        4 => 350,
        5 => 400,
        6 => 600,
        7 => 700,
        8 => 800,
        9 => 900,
        value => i32::from(value),
    }
}

fn font_name(face: &ttf_parser::Face<'_>, name_id: u16) -> Option<String> {
    font_names(face, name_id).into_iter().next()
}

fn font_names(face: &ttf_parser::Face<'_>, name_id: u16) -> Vec<String> {
    // Decode every Microsoft SFNT name as UTF-16BE, including Windows encoding 2.
    let mut names = windows_font_names(face, name_id);
    names.extend(
        face.names()
            .into_iter()
            .filter(|name| {
                name.name_id == name_id
                    && name.platform_id != ttf_parser::PlatformId::Windows
                    && name.is_unicode()
            })
            .filter_map(|name| name.to_string())
            .filter(|name| !name.is_empty()),
    );
    names.dedup();
    names
}

fn windows_font_names(face: &ttf_parser::Face<'_>, name_id: u16) -> Vec<String> {
    let mut names = face
        .names()
        .into_iter()
        .filter(|name| {
            name.name_id == name_id && name.platform_id == ttf_parser::PlatformId::Windows
        })
        .filter_map(|name| {
            (name.name.len() % 2 == 0)
                .then(|| {
                    name.name
                        .chunks_exact(2)
                        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                        .collect::<Vec<_>>()
                })
                .and_then(|units| String::from_utf16(&units).ok())
                .filter(|name| !name.is_empty())
        })
        .collect::<Vec<_>>();
    names.dedup();
    names
}

fn sanitize_attachment_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => character,
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "embedded-font.ttf".to_string()
    } else {
        sanitized
    }
}

fn normalize_font_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn font_name_match_key(value: &str) -> String {
    String::from_utf8(
        value
            .bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .collect(),
    )
    .expect("ASCII case folding preserves UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct UncacheableProvider;

    impl FontProvider for UncacheableProvider {
        fn resolve(&self, query: &FontQuery) -> FontMatch {
            FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::Null,
            )
        }
    }

    struct InstanceProvider(u8);

    impl FontProvider for InstanceProvider {
        fn resolve(&self, query: &FontQuery) -> FontMatch {
            let _marker = self.0;
            FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::Null,
            )
        }
    }

    #[derive(Clone)]
    struct MutableKeyProvider {
        key: Arc<Mutex<FontProviderCacheKey>>,
    }

    impl MutableKeyProvider {
        fn new() -> Self {
            Self {
                key: Arc::new(Mutex::new(FontProviderCacheKey::new())),
            }
        }

        fn invalidate(&self) {
            *self.key.lock().expect("test provider key mutex poisoned") =
                FontProviderCacheKey::new();
        }
    }

    impl FontProvider for MutableKeyProvider {
        fn resolve(&self, query: &FontQuery) -> FontMatch {
            FontMatch::unresolved(
                query.family.clone(),
                query.style.clone(),
                FontProviderKind::Null,
            )
        }

        fn layout_cache_key(&self) -> Option<FontProviderCacheKey> {
            Some(
                self.key
                    .lock()
                    .expect("test provider key mutex poisoned")
                    .clone(),
            )
        }
    }

    #[test]
    fn provider_cache_keys_are_collision_free_and_clone_stable() {
        let provider = AttachedFontProvider::default();
        let cloned = provider.clone();
        let independent = AttachedFontProvider::default();

        assert_eq!(provider.layout_cache_key(), cloned.layout_cache_key());
        assert_ne!(provider.layout_cache_key(), independent.layout_cache_key());
        assert_eq!(
            NullFontProvider.layout_cache_key(),
            NullFontProvider.layout_cache_key()
        );
    }

    #[test]
    fn instance_keys_distinguish_live_providers_and_forward_through_wrappers() {
        let first = InstanceProvider(1);
        let second = InstanceProvider(2);
        let first_key = FontProvider::instance_cache_key(&first);

        assert_ne!(first_key, FontProvider::instance_cache_key(&second));

        let borrowed = &first;
        assert_eq!(first_key, FontProvider::instance_cache_key(&borrowed));

        let boxed = Box::new(InstanceProvider(3));
        let boxed_target_key = FontProvider::instance_cache_key(boxed.as_ref());
        assert_eq!(boxed_target_key, FontProvider::instance_cache_key(&boxed));

        let dynamic: &dyn FontProvider = &first;
        assert_eq!(first_key, dynamic.instance_cache_key());
    }

    #[test]
    fn wrappers_do_not_cache_an_uncacheable_child() {
        let merged = MergedFontProvider::new(UncacheableProvider, NullFontProvider);
        let default_file = DefaultFontFileProvider::new(UncacheableProvider, "fallback.ttf");

        assert_eq!(merged.layout_cache_key(), None);
        assert_eq!(default_file.layout_cache_key(), None);
        assert!(
            MergedFontProvider::new(NullFontProvider, NullFontProvider)
                .layout_cache_key()
                .is_some()
        );
    }

    #[test]
    fn wrapper_cache_keys_follow_cacheable_child_invalidation() {
        let child = MutableKeyProvider::new();
        let merged = MergedFontProvider::new(child.clone(), NullFontProvider);
        let default_file = DefaultFontFileProvider::new(child.clone(), "fallback.ttf");
        let merged_before = merged.layout_cache_key();
        let default_before = default_file.layout_cache_key();

        child.invalidate();

        assert_ne!(merged.layout_cache_key(), merged_before);
        assert_ne!(default_file.layout_cache_key(), default_before);
    }

    fn read_be_u16(data: &[u8], offset: usize) -> u16 {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }

    fn read_be_u32(data: &[u8], offset: usize) -> usize {
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize
    }

    fn unique_test_directory(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rassa-{label}-{}-{nonce}", std::process::id()))
    }

    fn font_with_distinct_typographic_and_legacy_families() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test/font2.otf");
        let mut data = fs::read(path).expect("Aileron fixture should be readable");
        let table_count = read_be_u16(&data, 4) as usize;
        let name_offset = (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|offset| &data[*offset..*offset + 4] == b"name")
            .map(|offset| read_be_u32(&data, offset + 8))
            .expect("fixture should contain an SFNT name table");
        let name_count = read_be_u16(&data, name_offset + 2) as usize;
        let mut changed = false;
        for index in 0..name_count {
            let record = name_offset + 6 + index * 12;
            let platform = read_be_u16(&data, record);
            let name_id = read_be_u16(&data, record + 6);
            if platform == 3 && name_id == ttf_parser::name_id::POST_SCRIPT_NAME {
                data[record + 6..record + 8]
                    .copy_from_slice(&ttf_parser::name_id::TYPOGRAPHIC_FAMILY.to_be_bytes());
                changed = true;
            }
        }
        assert!(changed, "fixture should contain a Windows PostScript name");

        let face = ttf_parser::Face::parse(&data, 0).expect("mutated font should remain valid");
        assert_eq!(
            font_name(&face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY).as_deref(),
            Some("Aileron-Regular")
        );
        assert_eq!(
            font_name(&face, ttf_parser::name_id::FAMILY).as_deref(),
            Some("Aileron")
        );
        data
    }

    fn font_with_conflicting_unicode_family_alias() -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test/font2.otf");
        let mut data = fs::read(path).expect("Aileron fixture should be readable");
        let table_count = read_be_u16(&data, 4) as usize;
        let name_offset = (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|offset| &data[*offset..*offset + 4] == b"name")
            .map(|offset| read_be_u32(&data, offset + 8))
            .expect("fixture should contain an SFNT name table");
        let name_count = read_be_u16(&data, name_offset + 2) as usize;
        let mut changed = false;
        for index in 0..name_count {
            let record = name_offset + 6 + index * 12;
            let platform = read_be_u16(&data, record);
            let name_id = read_be_u16(&data, record + 6);
            if platform == 1 && name_id == ttf_parser::name_id::FAMILY {
                data[record..record + 2].copy_from_slice(&0_u16.to_be_bytes());
                changed = true;
            }
        }
        assert!(changed, "fixture should contain a Macintosh family name");
        data
    }

    fn two_face_collection() -> Vec<u8> {
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test");
        let first = fs::read(fixture_root.join("font1.ttf")).expect("TTF fixture should read");
        let second = fs::read(fixture_root.join("font2.otf")).expect("OTF fixture should read");
        let header_len = 20_usize;
        let first_offset = header_len.next_multiple_of(4);
        let second_offset = (first_offset + first.len()).next_multiple_of(4);
        let mut collection = vec![0_u8; second_offset + second.len()];
        collection[0..4].copy_from_slice(b"ttcf");
        collection[4..8].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        collection[8..12].copy_from_slice(&2_u32.to_be_bytes());
        collection[12..16].copy_from_slice(&(first_offset as u32).to_be_bytes());
        collection[16..20].copy_from_slice(&(second_offset as u32).to_be_bytes());
        collection[first_offset..first_offset + first.len()].copy_from_slice(&first);
        collection[second_offset..second_offset + second.len()].copy_from_slice(&second);
        for (font, base_offset) in [(&first, first_offset), (&second, second_offset)] {
            let table_count = read_be_u16(font, 4) as usize;
            for index in 0..table_count {
                let table = base_offset + 12 + index * 16;
                let original_offset = read_be_u32(&collection, table + 8);
                let collection_offset = original_offset + base_offset;
                collection[table + 8..table + 12]
                    .copy_from_slice(&(collection_offset as u32).to_be_bytes());
            }
        }
        assert_eq!(ttf_parser::fonts_in_collection(&collection), Some(2));
        assert!(ttf_parser::Face::parse(&collection, 0).is_ok());
        assert!(ttf_parser::Face::parse(&collection, 1).is_ok());
        collection
    }

    fn synthetic_directory_record(
        path: &str,
        family: &[&str],
        full_names: &[&str],
        postscript_names: &[&str],
        postscript_outlines: bool,
    ) -> DirectoryFontRecord {
        DirectoryFontRecord {
            family: family.first().copied().unwrap_or("Fixture").to_string(),
            path: PathBuf::from(path),
            face_index: None,
            style: None,
            weight: 400,
            italic: false,
            bold: false,
            aliases: FontRecordAliases {
                family: family
                    .iter()
                    .map(|name| font_name_match_key(name))
                    .collect(),
                full_names: full_names
                    .iter()
                    .map(|name| font_name_match_key(name))
                    .collect(),
                postscript_names: postscript_names
                    .iter()
                    .map(|name| font_name_match_key(name))
                    .collect(),
                postscript_outlines,
            },
        }
    }

    #[test]
    fn null_provider_returns_unresolved_match() {
        let provider = NullFontProvider;
        let result = provider.resolve(&FontQuery::new("Sans"));

        assert_eq!(result.family, "Sans");
        assert!(result.path.is_none());
        assert_eq!(result.provider, FontProviderKind::Null);
    }

    #[test]
    fn fontconfig_provider_resolves_system_font() {
        let provider = FontconfigProvider::new();
        let result = provider.resolve(&FontQuery::new("sans"));

        assert_eq!(result.provider, FontProviderKind::Fontconfig);
        assert!(result.path.is_some());
        assert!(result.path.as_ref().is_some_and(|path| path.exists()));
    }

    #[test]
    fn fontconfig_provider_caches_identical_resolve_queries() {
        let provider = FontconfigProvider::new();
        let query = FontQuery::new("sans");

        assert_eq!(provider.resolve_cache_len_for_tests(), 0);
        let first = provider.resolve(&query);
        let cached_entries = provider.resolve_cache_len_for_tests();
        let second = provider.resolve(&query);

        assert!(cached_entries >= 1);
        assert_eq!(provider.resolve_cache_len_for_tests(), cached_entries);
        assert_eq!(second, first);
    }

    #[test]
    fn fontconfig_provider_applies_fontconfig_substitutions_for_generic_families() {
        let expected = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "sans"])
            .output()
            .expect("fc-match should be available with fontconfig");
        assert!(expected.status.success());
        let expected_path = PathBuf::from(String::from_utf8(expected.stdout).expect("utf8 path"));

        let provider = FontconfigProvider::new();
        let result = provider.resolve(&FontQuery::new("sans"));

        assert_eq!(result.path, Some(expected_path));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn fontconfig_provider_uses_arial_default_for_missing_specific_family_like_libass() {
        let arial = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "Arial:weight=bold"])
            .output()
            .expect("fc-match should be available with fontconfig");
        assert!(arial.status.success());
        let expected_path = PathBuf::from(String::from_utf8(arial.stdout).expect("utf8 path"));

        let provider = FontconfigProvider::new();
        let result = provider.resolve(&FontQuery::new("Fontin Sans Rg").with_weight(700));

        assert_eq!(result.path, Some(expected_path));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn fontconfig_provider_respects_requested_weight_style() {
        let expected = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "DejaVu Sans:style=Bold"])
            .output()
            .expect("fc-match should be available with fontconfig");
        assert!(expected.status.success());
        let expected_path = PathBuf::from(String::from_utf8(expected.stdout).expect("utf8 path"));
        if !expected_path.exists()
            || expected_path
                .file_name()
                .is_none_or(|name| !name.to_string_lossy().contains("Bold"))
        {
            eprintln!("skipping: system fontconfig has no DejaVu Sans Bold fixture");
            return;
        }

        let provider = FontconfigProvider::new();
        let result = provider.resolve(&FontQuery::new("DejaVu Sans").with_style("Bold"));

        assert_eq!(result.path, Some(expected_path));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn fontconfig_provider_does_not_synthesize_weight_for_real_bold_face() {
        let expected = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "DejaVu Sans:weight=bold"])
            .output()
            .expect("fc-match should be available with fontconfig");
        assert!(expected.status.success());
        let expected_path = PathBuf::from(String::from_utf8(expected.stdout).expect("utf8 path"));
        if !expected_path.exists()
            || load_face_metadata(&expected_path)
                .and_then(|(_, style)| style)
                .is_none_or(|style| !normalize_font_key(&style).contains("bold"))
        {
            eprintln!("skipping: system fontconfig has no real DejaVu Sans Bold fixture");
            return;
        }

        let provider = FontconfigProvider::new();
        let result = provider.resolve(&FontQuery::new("DejaVu Sans").with_style("Bold"));

        assert_eq!(result.path, Some(expected_path));
        assert!(!result.synthetic_bold);
        assert!(!result.synthetic_italic);
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn fontconfig_can_resolve_cjk_font_for_character_coverage() {
        let Some(result) = resolve_system_font_for_char("DejaVu Sans", None, '日') else {
            eprintln!("skipping: system fontconfig has no CJK-capable fallback font");
            return;
        };

        assert!(result.1.as_ref().is_some_and(|path| path.exists()));
        assert!(font_file_supports_char(result.1.as_ref().unwrap(), '日'));
    }

    #[test]
    fn attached_font_provider_resolves_materialized_attachment() {
        let system = FontconfigProvider::new().resolve(&FontQuery::new("sans"));
        let path = system.path.expect("system font path should exist");
        let data = fs::read(&path).expect("font bytes should be readable");
        let provider = AttachedFontProvider::from_attachments(&[FontAttachment {
            name: path
                .file_name()
                .expect("font filename")
                .to_string_lossy()
                .into_owned(),
            data,
        }]);

        let result = provider.resolve(&FontQuery::new(&system.family));

        assert_eq!(result.provider, FontProviderKind::Attached);
        assert!(result.path.is_some());
        assert!(
            result
                .path
                .as_ref()
                .is_some_and(|materialized| materialized.exists())
        );
    }

    #[test]
    fn attached_provider_uses_legacy_family_instead_of_typographic_family() {
        let directory = unique_test_directory("attached-family-alias");
        let provider = AttachedFontProvider::from_attachments_in_dir(
            &[FontAttachment {
                name: "unrelated-name.data".to_string(),
                data: font_with_distinct_typographic_and_legacy_families(),
            }],
            Some(&directory),
        );

        let legacy = provider.resolve(&FontQuery::new("Aileron"));
        let typographic = provider.resolve(&FontQuery::new("Aileron-Regular"));

        assert_eq!(legacy.provider, FontProviderKind::Attached);
        assert_eq!(legacy.family, "Aileron");
        assert!(legacy.path.as_ref().is_some_and(|path| path.is_file()));
        assert!(typographic.path.is_none());
        fs::remove_dir_all(directory).expect("attachment fixture should clean up");
    }

    #[test]
    fn attached_provider_indexes_every_face_in_font_collection() {
        let directory = unique_test_directory("attached-font-collection");
        let provider = AttachedFontProvider::from_attachments_in_dir(
            &[FontAttachment {
                name: "fixture.ttc".to_string(),
                data: two_face_collection(),
            }],
            Some(&directory),
        );

        let first = provider.resolve_family("Pixel Operator Mono");
        let second = provider.resolve_family("Aileron");

        assert_eq!(first.provider, FontProviderKind::Attached);
        assert_eq!(first.face_index, None);
        assert_eq!(second.provider, FontProviderKind::Attached);
        assert_eq!(second.face_index, Some(1));
        assert_eq!(second.path, first.path);
        fs::remove_dir_all(directory).expect("attachment fixture should clean up");
    }

    #[test]
    fn attached_provider_selects_collection_face_with_required_glyphs() {
        let directory = unique_test_directory("attached-font-collection-coverage");
        let provider = AttachedFontProvider::from_attachments_in_dir(
            &[FontAttachment {
                name: "fixture.ttc".to_string(),
                data: two_face_collection(),
            }],
            Some(&directory),
        );

        let query = FontQuery::new("Aileron");
        let unsupported = provider.resolve_for_text(&query, "\u{1f600}");
        let supported = provider.resolve_for_text(&query, "Hello");

        assert!(unsupported.path.is_none());
        assert_eq!(supported.face_index, Some(1));
        fs::remove_dir_all(directory).expect("attachment fixture should clean up");
    }

    #[test]
    fn directory_provider_indexes_only_usable_non_hidden_fonts() {
        let system = FontconfigProvider::new().resolve(&FontQuery::new("sans"));
        let source = system.path.expect("system font path should exist");
        let directory = unique_test_directory("font-directory");
        fs::create_dir_all(&directory).expect("font directory should be creatable");
        let copied = directory.join("fixture-without-extension");
        fs::copy(&source, &copied).expect("font fixture should copy");
        fs::write(directory.join("not-a-font.txt"), b"not a font")
            .expect("invalid fixture should write");
        fs::write(directory.join(".hidden-invalid"), b"not a font")
            .expect("hidden fixture should write");

        let (provider, issues) = DirectoryFontProvider::scan(&directory);
        let result = provider.resolve(&FontQuery::new(&system.family));

        assert_eq!(result.provider, FontProviderKind::Attached);
        assert_eq!(result.path, Some(copied));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].path.ends_with("not-a-font.txt"));
        fs::remove_dir_all(&directory).expect("font fixture should clean up");
    }

    #[test]
    fn directory_provider_uses_legacy_family_instead_of_typographic_family() {
        let directory = unique_test_directory("directory-family-alias");
        fs::create_dir_all(&directory).expect("font directory should be creatable");
        let copied = directory.join("unrelated-name.data");
        fs::write(
            &copied,
            font_with_distinct_typographic_and_legacy_families(),
        )
        .expect("mutated font fixture should write");

        let (provider, issues) = DirectoryFontProvider::scan(&directory);
        let legacy = provider.resolve(&FontQuery::new("Aileron"));
        let typographic = provider.resolve(&FontQuery::new("Aileron-Regular"));

        assert!(issues.is_empty(), "font should be usable: {issues:?}");
        assert_eq!(legacy.provider, FontProviderKind::Attached);
        assert_eq!(legacy.family, "Aileron");
        assert_eq!(legacy.path, Some(copied));
        assert!(typographic.path.is_none());
        fs::remove_dir_all(directory).expect("font fixture should clean up");
    }

    #[test]
    fn windows_family_names_exclude_other_platform_aliases() {
        let data = font_with_conflicting_unicode_family_alias();
        let face = ttf_parser::Face::parse(&data, 0).expect("mutated font should parse");

        let windows = windows_font_names(&face, ttf_parser::name_id::FAMILY);
        assert_eq!(windows, vec!["Aileron"]);
        let metadata = font_face_metadata(&face).expect("font metadata should parse");
        assert_eq!(
            metadata.aliases.family,
            vec![font_name_match_key("Aileron")]
        );
    }

    #[test]
    fn local_family_matching_preserves_spaces_and_non_ascii_case() {
        let fonts = [synthetic_directory_record(
            "legacy.ttf",
            &["Legacy Family"],
            &[],
            &[],
            false,
        )];

        assert!(select_local_font(&fonts, "legacy family", None, None).is_some());
        assert!(select_local_font(&fonts, "LegacyFamily", None, None).is_none());

        let unicode_fonts = [synthetic_directory_record(
            "unicode.ttf",
            &["Ä Family"],
            &[],
            &[],
            false,
        )];
        assert!(select_local_font(&unicode_fonts, "ä Family", None, None).is_none());
    }

    #[test]
    fn local_font_selection_does_not_match_attachment_filename() {
        let directory = unique_test_directory("attachment-filename-non-alias");
        let provider = AttachedFontProvider::from_attachments_in_dir(
            &[FontAttachment {
                name: "FilenameAlias.ttf".to_string(),
                data: fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
                )
                .expect("font fixture should read"),
            }],
            Some(&directory),
        );

        assert!(provider.resolve_family("FilenameAlias").path.is_none());
        assert!(provider.resolve_family("Aileron").path.is_some());
        fs::remove_dir_all(directory).expect("attachment fixture should clean up");
    }

    #[test]
    fn local_font_selection_honors_provider_order_for_full_name_score_zero() {
        let fonts = [
            synthetic_directory_record(
                "full-name-first.otf",
                &["Other"],
                &["Requested"],
                &[],
                false,
            ),
            synthetic_directory_record("family-second.otf", &["Requested"], &[], &[], false),
        ];

        let selected =
            select_local_font(&fonts, "Requested", None, None).expect("font should resolve");

        assert_eq!(selected.path, PathBuf::from("full-name-first.otf"));
    }

    #[test]
    fn full_and_postscript_fallback_respects_outline_type_like_libass() {
        let truetype = synthetic_directory_record(
            "truetype.ttf",
            &["TrueType Family"],
            &["TrueType Full"],
            &["TrueType-PostScript"],
            false,
        );
        let postscript = synthetic_directory_record(
            "postscript.otf",
            &["PostScript Family"],
            &["PostScript Full"],
            &["PostScript-Name"],
            true,
        );

        assert!(
            select_local_font(std::slice::from_ref(&truetype), "TrueType Full", None, None,)
                .is_some()
        );
        assert!(
            select_local_font(
                std::slice::from_ref(&truetype),
                "TrueType-PostScript",
                None,
                None,
            )
            .is_none()
        );
        assert!(
            select_local_font(
                std::slice::from_ref(&postscript),
                "PostScript-Name",
                None,
                None,
            )
            .is_some()
        );
        assert!(
            select_local_font(
                std::slice::from_ref(&postscript),
                "PostScript Full",
                None,
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn directory_provider_resolves_full_and_postscript_names_like_libass() {
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test");
        let directory = unique_test_directory("directory-full-postscript");
        fs::create_dir_all(&directory).expect("font directory should be creatable");
        let truetype = directory.join("truetype.data");
        let postscript = directory.join("postscript.data");
        fs::copy(fixture_root.join("font1.ttf"), &truetype).expect("TrueType fixture should copy");
        fs::copy(fixture_root.join("font2.otf"), &postscript)
            .expect("PostScript fixture should copy");

        let (provider, issues) = DirectoryFontProvider::scan(&directory);
        let truetype_full = provider.resolve_family("Pixel Operator Mono Bold");
        let truetype_postscript = provider.resolve_family("PixelOperatorMono-Bold");
        let postscript_name = provider.resolve_family("Aileron-Regular");

        assert!(issues.is_empty(), "fonts should be usable: {issues:?}");
        assert_eq!(truetype_full.path, Some(truetype));
        assert!(truetype_postscript.path.is_none());
        assert_eq!(postscript_name.path, Some(postscript));
        fs::remove_dir_all(directory).expect("font fixtures should clean up");
    }

    #[test]
    fn directory_provider_decodes_legacy_windows_family_names_as_utf16be() {
        let source =
            Path::new("/tmp/rassa-libass-tests/regression/.fonts/shiftjis_Reishoreiryu.ttf");
        if !source.is_file() {
            eprintln!("skipping: compatible_0.17.5 Shift-JIS fixture is unavailable");
            return;
        }
        let directory = unique_test_directory("legacy-windows-name");
        fs::create_dir_all(&directory).expect("font directory should be creatable");
        let copied = directory.join("shiftjis.ttf");
        fs::copy(source, &copied).expect("legacy font fixture should copy");

        let (provider, issues) = DirectoryFontProvider::scan(&directory);
        let result = provider.resolve(&FontQuery::new("麗流隷書"));

        assert!(
            issues.is_empty(),
            "legacy face should be usable: {issues:?}"
        );
        assert_eq!(result.path, Some(copied));
        fs::remove_dir_all(&directory).expect("font fixture should clean up");
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn fontconfig_provider_scopes_queries_to_custom_config() {
        let system = FontconfigProvider::new().resolve(&FontQuery::new("sans"));
        let source = system.path.expect("system font path should exist");
        let directory = unique_test_directory("fontconfig");
        let font_directory = directory.join("fonts");
        fs::create_dir_all(&font_directory).expect("font directory should be creatable");
        let copied = font_directory.join("configured-font.ttf");
        fs::copy(&source, &copied).expect("font fixture should copy");
        let config = directory.join("fonts.conf");
        fs::write(
            &config,
            format!(
                "<?xml version=\"1.0\"?><!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\"><fontconfig><dir>{}</dir></fontconfig>",
                font_directory.display()
            ),
        )
        .expect("fontconfig fixture should write");

        validate_fontconfig_config(&config).expect("custom fontconfig should be usable");
        let provider = FontconfigProvider::with_config(config.clone());
        let result = provider.resolve(&FontQuery::new(&system.family));

        assert_eq!(
            result
                .path
                .as_deref()
                .and_then(|path| path.canonicalize().ok()),
            copied.canonicalize().ok()
        );
        fs::remove_dir_all(&directory).expect("fontconfig fixture should clean up");
    }

    #[test]
    fn merged_provider_falls_back_to_secondary() {
        let provider = MergedFontProvider::new(NullFontProvider, FontconfigProvider::new());
        let result = provider.resolve(&FontQuery::new("sans"));

        assert_eq!(result.provider, FontProviderKind::Fontconfig);
        assert!(result.path.is_some());
    }

    #[test]
    fn default_font_file_provider_falls_back_to_configured_path() {
        let provider = DefaultFontFileProvider::new(NullFontProvider, "/tmp/default-font.ttf")
            .with_family("Default");
        let result = provider.resolve(&FontQuery::new("missing"));

        assert_eq!(result.provider, FontProviderKind::DefaultFile);
        assert_eq!(result.family, "Default");
        assert_eq!(result.path, Some(PathBuf::from("/tmp/default-font.ttf")));
    }
}
