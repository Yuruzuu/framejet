use anyhow::{Context, Result, ensure};
use resvg::{tiny_skia, usvg};
use std::{path::Path, sync::Arc};

pub struct SvgRenderer {
    options: usvg::Options<'static>,
}

impl SvgRenderer {
    pub fn new(input: &Path, fonts: &[std::path::PathBuf]) -> Result<Self> {
        let mut options = usvg::Options {
            resources_dir: input.parent().map(Path::to_path_buf),
            ..Default::default()
        };
        let database = Arc::make_mut(&mut options.fontdb);
        database.load_system_fonts();
        for font in fonts {
            database
                .load_font_file(font)
                .with_context(|| format!("Cannot load font {}", font.display()))?;
        }
        Ok(Self { options })
    }

    pub fn parse(&self, svg: &str) -> Result<usvg::Tree> {
        let document = roxmltree::Document::parse(svg).context("Invalid SVG XML")?;
        for node in document.descendants().filter(|node| node.is_element()) {
            let name = node.tag_name().name();
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
                "Unsupported SVG element <{name}>. Return a static SVG state for each render({{ time, frame }}) call instead"
            );
            if name == "style" {
                ensure!(
                    !node.text().unwrap_or_default().contains("@keyframes"),
                    "CSS animation is unsupported; calculate the SVG state from time in render()"
                );
            }
        }
        usvg::Tree::from_str(svg, &self.options).context("Cannot parse SVG")
    }

    pub fn dimensions(&self, svg: &str) -> Result<(u32, u32)> {
        let tree = self.parse(svg)?;
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
