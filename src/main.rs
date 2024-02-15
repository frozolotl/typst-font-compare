mod world;

use std::{fmt::Write, path::PathBuf};

use clap::Parser;
use color_eyre::eyre::{eyre, Context, Result};
use rayon::prelude::*;
use regex::Regex;
use typst::{
    eval::Tracer,
    foundations::{Bytes, StyleChain},
    layout::Abs,
    text::{FontFamily, FontInfo, TextElem},
    visualize::Color,
};
use world::SystemWorld;

/// A tool to compare how Typst documents would look using different fonts or font variants.
#[derive(Parser)]
struct Args {
    /// Path to the Typst input file.
    input: PathBuf,
    /// Path to the output PDF.
    ///
    /// For an `input.typ`, the output will be `input.variants.pdf`.
    #[clap(short, long)]
    output: Option<PathBuf>,
    /// Whether to try each variant (style, weight, stretch).
    #[clap(short, long)]
    variants: bool,
    /// Whether to enable font fallback.
    #[clap(short, long)]
    fallback: bool,
    /// Only include font families that match this regular expression.
    ///
    /// The exclude regex takes priority over this regex.
    #[clap(short = 'i', long)]
    include: Option<String>,
    /// Exclude font families that match this regular expression.
    ///
    /// Takes priority over the include regex.
    #[clap(short = 'e', long)]
    exclude: Option<String>,
    /// Specify a different project root folder.
    #[clap(long, env = "TYPST_ROOT", value_name = "DIR")]
    root: Option<PathBuf>,
    /// Adds additional directories to search for fonts in.
    #[clap(
        long = "font-path",
        env = "TYPST_FONT_PATHS",
        value_name = "DIR",
        value_delimiter = if cfg!(windows) { ';' } else { ':' },
    )]
    font_paths: Vec<PathBuf>,
    /// The resolution to render the variants to.
    #[clap(long, default_value_t = 300.0)]
    ppi: f32,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let mut world = SystemWorld::new(&args)?;
    let render = render_collection(&mut world, &args).wrap_err("while rendering collection")?;
    let output = args.input.with_extension("variants.pdf");
    std::fs::write(output, render)?;
    Ok(())
}

/// Render all the variants and return PDF.
fn render_collection(world: &mut SystemWorld, args: &Args) -> Result<Vec<u8>> {
    let variants = render_variants(world.clone(), args).wrap_err("while rendering variants")?;

    eprintln!("Compiling collection...");

    let map_pixels = |x| (x as f32) / args.ppi * 72.0;
    let page_width = variants
        .iter()
        .map(|render| map_pixels(render.width))
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(0.0);

    let mut main = String::new();
    write!(
        main,
        r#"
        #let margin = 1cm
        #let text-size = 16pt

        #set document(author: "{pkg_name}")
        #set page(
            width: calc.max({page_width}pt, 20cm) + 2 * margin,
            height: auto,
            margin: margin,
        )
        #set text(size: text-size)
        #set heading(numbering: "1.1")
        #show heading: set text(size: text-size)

        #outline(indent: auto, title: [Fonts])

        Created using #link("{pkg_homepage}")[`{pkg_name} v{pkg_version}` ({pkg_homepage})]. \
        Tool authored by: #"{pkg_authors}".
        "#,
        pkg_name = env!("CARGO_PKG_NAME"),
        pkg_version = env!("CARGO_PKG_VERSION"),
        pkg_authors = env!("CARGO_PKG_AUTHORS"),
        pkg_homepage = env!("CARGO_PKG_HOMEPAGE"),
    )?;

    let mut last_family = None;
    for (n, render) in variants.iter().enumerate() {
        let first_of_family = last_family != Some(&render.font.family);
        write!(
            main,
            r#"
            #page[
                #if {first_of_family} [
                    // Necessary for the outline.
                    #place(hide[= {family}])
                ]
                #grid(
                    columns: 2,
                    column-gutter: 1fr,
                    text(size: 1.2em, [*#counter(heading).display((n, ..) => n) {family}*]),
                    counter(page).display(),
                )
                == {variant:?}
                #image(width: {width}pt, height: {height}pt, "render-{n}.png")
            ]
            "#,
            width = map_pixels(render.width),
            height = map_pixels(render.height),
            family = render.font.family,
            variant = render.font.variant,
        )?;
        last_family = Some(&render.font.family);
    }

    world.replace_files(
        main,
        variants
            .into_iter()
            .enumerate()
            .map(|(n, render)| (format!("render-{n}.png").into(), render.bytes)),
    );

    let mut tracer = Tracer::new();
    let document = typst::compile(world, &mut tracer)
        .map_err(|diag| eyre!("failed to compile collection: {diag:?}"))?;
    Ok(typst_pdf::pdf(&document, None, None))
}

/// Render a PNG image for each font (variant).
fn render_variants(mut world: SystemWorld, args: &Args) -> Result<Vec<Render>> {
    let default_styles = world.library.styles.clone();
    let include_regex = args
        .include
        .as_ref()
        .map(|regex| Regex::new(regex))
        .transpose()
        .wrap_err("failed to compile include regex")?;
    let exclude_regex = args
        .exclude
        .as_ref()
        .map(|regex| Regex::new(regex))
        .transpose()
        .wrap_err("failed to compile exclude regex")?;

    let mut fonts: Vec<_> = world
        .book
        .families()
        .filter(|(family, _)| {
            include_regex
                .as_ref()
                .map_or(true, |include_regex| include_regex.is_match(family))
        })
        .filter(|(family, _)| {
            exclude_regex
                .as_ref()
                .map_or(true, |exclude_regex| !exclude_regex.is_match(family))
        })
        .flat_map(|(_, mut fonts)| {
            // Only iterate over one font if `--variants` is not set.
            fonts
                .next()
                .into_iter()
                .chain(fonts.take_while(|_| args.variants))
        })
        .collect();

    // Sort fonts by family first and variant second.
    fonts.sort_by(|a, b| a.family.cmp(&b.family).then(a.variant.cmp(&b.variant)));

    let images: Result<_> = fonts
        .into_par_iter()
        .map_init(
            || world.clone(),
            |world, font| {
                eprintln!("Compiling for font {} {:?}", font.family, font.variant);

                // Set specified font.
                world.library.update(|library| {
                    default_styles.clone_into(&mut library.styles);

                    library.styles.set(TextElem::set_fallback(args.fallback));

                    library
                        .styles
                        .set_family(FontFamily::new(&font.family), StyleChain::default());

                    // Only set variant information if `--variants` is set.
                    if args.variants {
                        library
                            .styles
                            .set(TextElem::set_weight(font.variant.weight));
                        library
                            .styles
                            .set(TextElem::set_stretch(font.variant.stretch));
                        library.styles.set(TextElem::set_style(font.variant.style));
                    }
                });

                // Compile document to PNG.
                let mut tracer = Tracer::new();
                let document = typst::compile(world, &mut tracer)
                    .map_err(|diag| eyre!("failed to compile for font {font:?}: {diag:?}"))?;
                let rendered = typst_render::render_merged(
                    &document,
                    args.ppi / 72.0,
                    Color::WHITE,
                    Abs::pt(4.0),
                    Color::BLACK,
                );
                Ok(Render {
                    font: font.clone(),
                    bytes: Bytes::from(rendered.encode_png()?),
                    width: rendered.width(),
                    height: rendered.height(),
                })
            },
        )
        .collect();

    // Reset default styles.
    world.library.update(|library| {
        default_styles.clone_into(&mut library.styles);
    });

    comemo::evict(1);

    images
}

struct Render {
    font: FontInfo,
    bytes: Bytes,
    width: u32,
    height: u32,
}
