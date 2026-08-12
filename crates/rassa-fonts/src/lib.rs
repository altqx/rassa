use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    fmt, fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
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

pub trait FontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch;

    fn resolve_family(&self, family: &str) -> FontMatch {
        self.resolve(&FontQuery::new(family))
    }
}

impl<T: FontProvider + ?Sized> FontProvider for Box<T> {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        (**self).resolve(query)
    }
}

impl<T: FontProvider + ?Sized> FontProvider for &T {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        (**self).resolve(query)
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
}

pub struct CrossfontProvider {
    fallback_family: Option<String>,
    config_path: Option<PathBuf>,
    resolve_cache: Mutex<HashMap<FontResolveKey, FontMatch>>,
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
        }
    }

    pub fn with_fallback_family(fallback_family: impl Into<String>) -> Self {
        Self {
            fallback_family: Some(fallback_family.into()),
            config_path: None,
            resolve_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Use a specific fontconfig configuration for `fc-match` queries.
    ///
    /// This backend delegates configuration parsing and matching to the host's
    /// `fc-match` executable instead of owning an `FcConfig`. Cache rebuilding
    /// and in-process fontconfig database enumeration are therefore unavailable;
    /// if the command/configuration fails, the provider falls back to fontdb's
    /// platform system-font database.
    pub fn with_config(config_path: impl Into<PathBuf>) -> Self {
        Self {
            fallback_family: Some("Arial".to_string()),
            config_path: Some(config_path.into()),
            resolve_cache: Mutex::new(HashMap::new()),
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

    let mut database = Database::new();
    database.load_system_fonts();

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
    text.chars()
        .filter(|character| !character.is_whitespace() && !character.is_control())
        .all(|character| {
            font_file_face_supports_char(path, font.face_index.unwrap_or(0), character)
        })
}

pub fn font_file_supports_char(path: &Path, character: char) -> bool {
    font_file_supports_char_in_scope(path, FontFaceScope::Any, character)
}

/// Check character coverage in one exact face of a font or collection.
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
    match scope {
        FontFaceScope::Any => {
            let face_count = ttf_parser::fonts_in_collection(&data).unwrap_or(1).max(1);
            (0..face_count).any(|index| {
                font_data_glyph_index(&data, index, character).is_some_and(|glyph| glyph.0 != 0)
            })
        }
        FontFaceScope::Face(index) => {
            font_data_glyph_index(&data, index, character).is_some_and(|glyph| glyph.0 != 0)
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
    style: Option<String>,
    aliases: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachedFontProvider {
    fonts: Vec<AttachedFontRecord>,
}

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
            .filter_map(|attachment| AttachedFontRecord::from_attachment(attachment, &root))
            .collect();

        Self { fonts }
    }
}

impl FontProvider for AttachedFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let family_key = normalize_font_key(&query.family);
        let style_key = query.style.as_deref().map(normalize_font_key);

        let exact = self.fonts.iter().find(|font| {
            font.aliases.iter().any(|alias| alias == &family_key)
                && style_key.as_ref().is_none_or(|style| {
                    font.style.as_deref().map(normalize_font_key).as_ref() == Some(style)
                })
        });
        let fallback = self
            .fonts
            .iter()
            .find(|font| font.aliases.iter().any(|alias| alias == &family_key));

        if let Some(font) = exact.or(fallback) {
            let (synthetic_bold, synthetic_italic) =
                synthetic_style_flags(query.style.as_deref(), query.weight, font.style.as_deref());
            return FontMatch {
                family: font.family.clone(),
                path: Some(font.path.clone()),
                face_index: None,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryFontRecord {
    family: String,
    path: PathBuf,
    face_index: Option<u32>,
    style: Option<String>,
    aliases: Vec<String>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectoryFontProvider {
    fonts: Vec<DirectoryFontRecord>,
}

impl DirectoryFontProvider {
    /// Scan the immediate children of a libass additional-font directory.
    /// Hidden entries and subdirectories are ignored, matching libass. Files
    /// are accepted by their contents rather than their extension.
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

        (Self { fonts }, issues)
    }
}

impl FontProvider for DirectoryFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        let family_key = normalize_font_key(&query.family);
        let style_key = query.style.as_deref().map(normalize_font_key);
        let exact = self.fonts.iter().find(|font| {
            font.aliases.iter().any(|alias| alias == &family_key)
                && style_key.as_ref().is_none_or(|style| {
                    font.style.as_deref().map(normalize_font_key).as_ref() == Some(style)
                })
        });
        let fallback = self
            .fonts
            .iter()
            .find(|font| font.aliases.iter().any(|alias| alias == &family_key));

        if let Some(font) = exact.or(fallback) {
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
}

fn directory_font_records(path: &Path, data: &[u8]) -> Vec<DirectoryFontRecord> {
    let face_count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let file_stem = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned());
    (0..face_count)
        .filter_map(|index| {
            let face = ttf_parser::Face::parse(data, index).ok()?;
            let family = font_name(&face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
                .or_else(|| font_name(&face, ttf_parser::name_id::FAMILY))?;
            let style = font_name(&face, ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY)
                .or_else(|| font_name(&face, ttf_parser::name_id::SUBFAMILY));
            let mut aliases = vec![normalize_font_key(&family)];
            aliases.extend(file_stem.as_deref().map(normalize_font_key));
            aliases.extend(file_name.as_deref().map(normalize_font_key));
            aliases.sort();
            aliases.dedup();
            Some(DirectoryFontRecord {
                family,
                path: path.to_path_buf(),
                face_index: Some(index).filter(|index| *index > 0),
                style,
                aliases,
            })
        })
        .collect()
}

pub struct MergedFontProvider<P, S> {
    primary: P,
    secondary: S,
}

impl<P, S> MergedFontProvider<P, S> {
    pub fn new(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
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
}

pub struct DefaultFontFileProvider<P> {
    primary: P,
    path: PathBuf,
    family: Option<String>,
}

impl<P> DefaultFontFileProvider<P> {
    pub fn new(primary: P, path: impl Into<PathBuf>) -> Self {
        Self {
            primary,
            path: path.into(),
            family: None,
        }
    }

    pub fn with_family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
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
    fn from_attachment(attachment: &FontAttachment, root: &Path) -> Option<Self> {
        if attachment.data.is_empty() {
            return None;
        }

        let path = materialize_attachment(root, attachment)?;
        let fallback_name = attachment_file_stem(attachment)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| attachment.name.clone());
        let (family, style) =
            load_face_metadata(&path).unwrap_or_else(|| (fallback_name.clone(), None));
        let mut aliases = vec![normalize_font_key(&family)];
        if let Some(stem) = attachment_file_stem(attachment) {
            aliases.push(normalize_font_key(&stem));
        }
        if !attachment.name.is_empty() {
            aliases.push(normalize_font_key(&attachment.name));
        }
        aliases.sort();
        aliases.dedup();

        Some(Self {
            family,
            path,
            style,
            aliases,
        })
    }
}

fn materialize_attachment(root: &Path, attachment: &FontAttachment) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    attachment.name.hash(&mut hasher);
    attachment.data.hash(&mut hasher);
    let hash = hasher.finish();
    let sanitized = sanitize_attachment_name(&attachment.name);
    let path = root.join(format!("{hash:016x}-{sanitized}"));
    if !path.exists() && fs::write(&path, &attachment.data).is_err() {
        return None;
    }
    Some(path)
}

fn load_face_metadata(path: &Path) -> Option<(String, Option<String>)> {
    let data = fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&data, 0).ok()?;
    let family = font_name(&face, ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
        .or_else(|| font_name(&face, ttf_parser::name_id::FAMILY))?;
    let style = font_name(&face, ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY)
        .or_else(|| font_name(&face, ttf_parser::name_id::SUBFAMILY));
    Some((family, style))
}

fn font_name(face: &ttf_parser::Face<'_>, name_id: u16) -> Option<String> {
    // libass treats every Microsoft SFNT name as UTF-16BE, even when the
    // encoding id names a legacy cmap. Some old Japanese fonts correctly
    // store UTF-16BE family names under Windows encoding 2; ttf-parser's
    // `is_unicode` intentionally excludes those, which used to make the font
    // directory scanner reject the face before its legacy cmap could be used.
    let windows_name = face
        .names()
        .into_iter()
        .find(|name| name.name_id == name_id && name.platform_id == ttf_parser::PlatformId::Windows)
        .and_then(|name| {
            (name.name.len() % 2 == 0).then(|| {
                name.name
                    .chunks_exact(2)
                    .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                    .collect::<Vec<_>>()
            })
        })
        .and_then(|units| String::from_utf16(&units).ok())
        .filter(|name| !name.is_empty());
    windows_name.or_else(|| {
        face.names()
            .into_iter()
            .find(|name| name.name_id == name_id && name.is_unicode())
            .and_then(|name| name.to_string())
            .filter(|name| !name.is_empty())
    })
}

fn attachment_file_stem(attachment: &FontAttachment) -> Option<String> {
    Path::new(&attachment.name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_directory(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rassa-{label}-{}-{nonce}", std::process::id()))
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
