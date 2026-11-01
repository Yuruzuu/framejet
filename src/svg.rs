use anyhow::{Context, Result, ensure};
use resvg::{tiny_skia, usvg};
use std::{
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

type FontLoader = std::thread::JoinHandle<usvg::fontdb::Database>;

fn css_requires_browser(style: &str) -> bool {
    let style = style.to_ascii_lowercase();
    style.contains("@keyframes")
        || style.contains("@-webkit-keyframes")
        || style.contains("@import")
        || style.split(['{', ';', '}']).any(|declaration| {
            let Some((property, _)) = declaration.split_once(':') else {
                return false;
            };
            let property = property
                .trim()
                .strip_prefix("-webkit-")
                .unwrap_or(property.trim());
            property == "animation" || property.starts_with("animation-")
        })
}

pub struct SvgRenderer {
    /// Empty font database: enough for any SVG without text.
    plain: usvg::Options<'static>,
    resources: Option<std::path::PathBuf>,
    loader: Mutex<Option<FontLoader>>,
    /// Fonts take tens of milliseconds to scan, so only text waits for them.
    typed: OnceLock<Result<usvg::Options<'static>, String>>,
}

impl SvgRenderer {
    pub fn requires_browser(svg: &str) -> Result<bool> {
        let document = roxmltree::Document::parse(svg).context("Invalid SVG XML")?;
        let mut animated = false;
        for node in document.descendants() {
            if let Some(instruction) = node.pi() {
                animated |= instruction.target == "xml-stylesheet";
            }
            if !node.is_element() {
                continue;
            }
            let name = node.tag_name().name();
            ensure!(
                !matches!(name, "script" | "foreignObject"),
                "Unsupported SVG element <{name}>"
            );
            animated |= matches!(
                name,
                "animate" | "animateTransform" | "animateMotion" | "animateColor" | "set"
            );
            if name == "style" {
                let style = node
                    .descendants()
                    .filter_map(|part| if part.is_text() { part.text() } else { None })
                    .collect::<String>()
                    .to_ascii_lowercase();
                animated |= css_requires_browser(&style);
            }
            animated |= node.attribute("style").is_some_and(css_requires_browser);
            animated |= name == "link"
                && node
                    .attribute("rel")
                    .is_some_and(|value| value.contains("stylesheet"));
        }
        Ok(animated)
    }

    pub fn new(input: &Path, fonts: &[std::path::PathBuf]) -> Result<Self> {
        let resources = input.parent().map(Path::to_path_buf);
        // Report unusable --font files immediately, even if nothing draws text.
        for font in fonts {
            usvg::fontdb::Database::new()
                .load_font_file(font)
                .with_context(|| format!("Cannot load font {}", font.display()))?;
        }
        let fonts = fonts.to_vec();
        let loader = std::thread::spawn(move || {
            let mut database = usvg::fontdb::Database::new();
            database.load_system_fonts();
            for font in fonts {
                let _ = database.load_font_file(font);
            }
            // Otherwise every glyph opens and maps its font file again, which
            // dominated text rendering and contended across rendering threads.
            let faces: Vec<_> = database.faces().map(|face| face.id).collect();
            for face in faces {
                // SAFETY: font files are mapped read-only for this short-lived job, and
                // a font replaced during a render is already undefined for usvg's own mappings.
                let _ = unsafe { database.make_shared_face_data(face) };
            }
            database
        });
        Ok(Self {
            plain: usvg::Options {
                resources_dir: resources.clone(),
                ..Default::default()
            },
            resources,
            loader: Mutex::new(Some(loader)),
            typed: OnceLock::new(),
        })
    }

    fn typed(&self) -> Result<&usvg::Options<'static>> {
        self.typed
            .get_or_init(|| {
                let loader = self.loader.lock().ok().and_then(|mut loader| loader.take());
                let database = loader
                    .ok_or("Font loader is missing")?
                    .join()
                    .map_err(|_| "Font loading failed")?;
                let mut options = usvg::Options {
                    resources_dir: self.resources.clone(),
                    ..Default::default()
                };
                options.fontdb = Arc::new(database);
                Ok(options)
            })
            .as_ref()
            .map_err(|error| anyhow::anyhow!("{error}"))
    }

    pub fn parse(&self, svg: &str) -> Result<usvg::Tree> {
        let document = roxmltree::Document::parse(svg).context("Invalid SVG XML")?;
        // Embedded SVG images may also contain text.
        let mut needs_fonts = false;
        for node in document.descendants().filter(|node| node.is_element()) {
            let name = node.tag_name().name();
            needs_fonts |= matches!(name, "text" | "image");
            ensure!(
                !matches!(
                    name,
                    "animate"
                        | "animateTransform"
                        | "animateMotion"
                        | "set"
                        | "script"
                        | "foreignObject"
                ),
                "Unsupported SVG element <{name}> in native rendering. Use --svg-renderer browser for CSS/SMIL animation"
            );
            if name == "style" {
                ensure!(
                    !node.text().unwrap_or_default().contains("@keyframes"),
                    "CSS animation requires --svg-renderer browser"
                );
            }
        }
        let options = if needs_fonts {
            self.typed()?
        } else {
            &self.plain
        };
        usvg::Tree::from_str(svg, options).context("Cannot parse SVG")
    }

    pub fn dimensions(&self, svg: &str) -> Result<(u32, u32)> {
        let tree = self.parse(svg)?;
        Ok((
            tree.size().width().ceil() as u32,
            tree.size().height().ceil() as u32,
        ))
    }

    pub fn animated_dimensions(&self, svg: &str) -> Result<(u32, u32)> {
        let tree = usvg::Tree::from_str(svg, &self.plain).context("Cannot parse SVG dimensions")?;
        Ok((
            tree.size().width().ceil() as u32,
            tree.size().height().ceil() as u32,
        ))
    }

    /// Draws into `buffer`, which must already hold one full frame.
    pub fn render(&self, svg: &str, width: u32, height: u32, buffer: Vec<u8>) -> Result<Vec<u8>> {
        let tree = self.parse(svg)?;
        let size = tiny_skia::IntSize::from_wh(width, height).context("Invalid SVG surface")?;
        let mut pixels =
            tiny_skia::Pixmap::from_vec(buffer, size).context("Cannot allocate SVG surface")?;
        // Both backends use an opaque black canvas; SVG may paint its own background.
        pixels.fill(tiny_skia::Color::BLACK);
        let scale = (width as f32 / tree.size().width()).min(height as f32 / tree.size().height());
        let left = (width as f32 - tree.size().width() * scale) / 2.0;
        let top = (height as f32 - tree.size().height() * scale) / 2.0;
        let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, left, top);
        resvg::render(&tree, transform, &mut pixels.as_mut());
        Ok(pixels.take())
    }
}
