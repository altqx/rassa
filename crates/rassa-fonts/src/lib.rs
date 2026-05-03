use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Mutex,
};

use freetype::Library;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontAttachment {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontQuery {
    pub family: String,
    pub style: Option<String>,
}

impl FontQuery {
    pub fn new(family: impl Into<String>) -> Self {
        Self {
            family: family.into(),
            style: None,
        }
    }

    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    pub style: Option<String>,
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
            style,
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

pub struct FontconfigProvider {
    config: Mutex<fontconfig::FontConfig>,
    fallback_family: Option<String>,
}

impl Default for FontconfigProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FontconfigProvider {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(fontconfig::FontConfig::default()),
            fallback_family: None,
        }
    }

    pub fn with_fallback_family(fallback_family: impl Into<String>) -> Self {
        Self {
            config: Mutex::new(fontconfig::FontConfig::default()),
            fallback_family: Some(fallback_family.into()),
        }
    }

    fn find_font(&self, family: String, style: Option<String>) -> Option<fontconfig::Font> {
        let mut config = self.config.lock().expect("fontconfig mutex poisoned");
        config.find(family, style)
    }
}

impl FontProvider for FontconfigProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        if let Some(font) = self.find_font(query.family.clone(), query.style.clone()) {
            return FontMatch {
                family: font.name,
                path: Some(font.path),
                style: query.style.clone(),
                provider: FontProviderKind::Fontconfig,
            };
        }

        if let Some(fallback_family) = &self.fallback_family {
            if let Some(font) = self.find_font(fallback_family.clone(), query.style.clone()) {
                return FontMatch {
                    family: font.name,
                    path: Some(font.path),
                    style: query.style.clone(),
                    provider: FontProviderKind::Fontconfig,
                };
            }
        }

        FontMatch::unresolved(
            query.family.clone(),
            query.style.clone(),
            FontProviderKind::Fontconfig,
        )
    }
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
        let library = Library::init().ok();
        let fonts = attachments
            .iter()
            .filter_map(|attachment| {
                AttachedFontRecord::from_attachment(attachment, &root, library.as_ref())
            })
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
            return FontMatch {
                family: font.family.clone(),
                path: Some(font.path.clone()),
                style: font.style.clone(),
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
            style: query.style.clone(),
            provider: FontProviderKind::DefaultFile,
        }
    }
}

impl AttachedFontRecord {
    fn from_attachment(
        attachment: &FontAttachment,
        root: &Path,
        library: Option<&Library>,
    ) -> Option<Self> {
        if attachment.data.is_empty() {
            return None;
        }

        let path = materialize_attachment(root, attachment)?;
        let fallback_name = attachment_file_stem(attachment)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| attachment.name.clone());
        let (family, style) =
            load_face_metadata(library, &path).unwrap_or_else(|| (fallback_name.clone(), None));
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

fn load_face_metadata(library: Option<&Library>, path: &Path) -> Option<(String, Option<String>)> {
    let library = library?;
    let face = library.new_face(path, 0).ok()?;
    let family = face.family_name()?;
    let style = face.style_name();
    Some((family, style))
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
