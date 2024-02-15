# typst-font-compare
A tool to compare how Typst documents would look using different fonts or font variants.

## Installation
```sh
cargo install --path .
```

## Usage
### Examples
The following command generates a file `main.variants.pdf` containing all the system fonts:
```sh
typst-font-compare main.typ
```

To additionally check all variants, meaning style, weight, and stretch:
```sh
typst-font-compare --variants main.typ
```

The variants file includes too many Noto fonts. Let's remove them.
```sh
typst-font-compare --exclude Noto main.typ
```

We only want to compare a certain few fonts.
```sh
typst-font-compare --include 'Roboto|Inter|Ubuntu' main.typ
```

### Command-line Arguments
```
A tool to compare how Typst documents would look using different fonts or font variants

Usage: typst-font-compare [OPTIONS] <INPUT>

Arguments:
  <INPUT>
          Path to the Typst input file

Options:
  -o, --output <OUTPUT>
          Path to the output PDF.
          
          For an `input.typ`, the output will be `input.variants.pdf`.

  -v, --variants
          Whether to try each variant (style, weight, stretch)

  -f, --fallback
          Whether to enable font fallback

  -i, --include <INCLUDE>
          Only include font families that match this regular expression.
          
          The exclude regex takes priority over this regex.

  -e, --exclude <EXCLUDE>
          Exclude font families that match this regular expression.
          
          Takes priority over the include regex.

      --root <DIR>
          Specify a different project root folder
          
          [env: TYPST_ROOT=]

      --font-path <DIR>
          Adds additional directories to search for fonts in
          
          [env: TYPST_FONT_PATHS=]

      --ppi <PPI>
          The resolution to render the variants to
          
          [default: 300]

  -h, --help
          Print help (see a summary with '-h')
```

## Questions and Answers
### Why do certain fonts not appear?
This tool does not embed any fonts that might normally be embedded into Typst.
You must either install them system-wide, or add a `--font-path fonts-folder` argument where `fonts-folder` contains the needed fonts.

### The generated document looks weird in my PDF viewer.
Each page this tool generates can have a different height.
Some PDF viewers don't handle this correctly.
Try opening the file in Firefox instead.

### The program just crashes at some point.
Images are stored in memory, making it potentially very memory intensive.
Thus, your OOM killer (out-of-memory killer) shuts it down.
Try not generating each variant, excluding certain fonts, or decreasing the PPI.

## Legal
This software is not affiliated with Typst, the brand.
