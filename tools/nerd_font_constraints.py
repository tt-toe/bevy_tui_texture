"""
Generates src/backend/glyph_constraint/nerd_font.rs: the scaling/alignment
rules Nerd Fonts' font-patcher applies to each symbol it patches into a
font, as a codepoint-range table the renderer looks glyph constraints up in.

The extraction is adapted from Ghostty's src/font/nerd_font_codegen.py
(MIT, Copyright (c) 2024 Mitchell Hashimoto, Ghostty contributors - see
NOTICE): it walks font-patcher's `setup_patch_set` (the per-symbol-set
`Attributes` and `ScaleRules`), maps each set's original codepoints to their
Nerd Font codepoints, and turns every scale group into per-glyph relative
bounds measured on the Symbols Nerd Font.

Usage (Python >= 3.12, `pip install fonttools`):

    python3 tools/nerd_font_constraints.py <font-patcher.py> \
        <SymbolsNerdFont-Regular.ttf (not Mono!)> [cache-dir]

font-patcher.py and SymbolsNerdFont-Regular.ttf must come from the same
Nerd Fonts release. The original symbol fonts are downloaded into
`cache-dir` (default: current directory) on first use, together with a
cached codepoint table (`nerd_font_codepoint_tables.py`).

This evaluates parts of font-patcher.py with `eval` - only run it on a
font-patcher you trust.
"""

import os
import struct
import ast
import sys
import math
from fontTools.ttLib import TTFont, TTLibError
from fontTools.pens.boundsPen import BoundsPen
from pathlib import Path
from types import SimpleNamespace
from typing import Literal, TypedDict, cast
from urllib.request import urlretrieve

type PatchSetAttributes = dict[Literal["default"] | int, PatchSetAttributeEntry]
type ResolvedSymbol = PatchSetAttributes | PatchSetScaleRules | int | None


class PatchSetScaleRules(TypedDict):
    ShiftMode: str
    ScaleGroups: list[list[int] | range]


class PatchSetAttributeEntry(TypedDict):
    align: str
    valign: str
    stretch: str
    params: dict[str, float | bool]

    relative_x: float
    relative_y: float
    relative_width: float
    relative_height: float


class PatchSet(TypedDict):
    Name: str
    Filename: str
    Exact: bool
    SymStart: int
    SymEnd: int
    SrcStart: int | None
    ScaleRules: PatchSetScaleRules | None
    Attributes: PatchSetAttributes


class PatchSetExtractor(ast.NodeVisitor):
    def __init__(self) -> None:
        self.symbol_table: dict[str, ast.expr] = {}
        self.patch_set_values: list[PatchSet] = []
        self.nf_version: str = ""

    def visit_Assign(self, node):
        if (
            node.col_offset == 0  # top-level assignment
            and len(node.targets) == 1  # no funny destructuring business
            and isinstance(node.targets[0], ast.Name)  # no setitem et cetera
            and node.targets[0].id == "version"  # it's the version string!
        ):
            self.nf_version = ast.literal_eval(node.value)
        else:
            return self.generic_visit(node)

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        if node.name != "font_patcher":
            return
        for item in node.body:
            if isinstance(item, ast.FunctionDef) and item.name == "setup_patch_set":
                self.visit_setup_patch_set(item)

    def visit_setup_patch_set(self, node: ast.FunctionDef) -> None:
        # First pass: gather variable assignments
        for stmt in node.body:
            match stmt:
                case ast.Assign(targets=[ast.Name(id=symbol)]):
                    # Store simple variable assignments in the symbol table
                    self.symbol_table[symbol] = stmt.value

        # Second pass: process self.patch_set
        for stmt in node.body:
            if not isinstance(stmt, ast.Assign):
                continue
            for target in stmt.targets:
                if (
                    isinstance(target, ast.Attribute)
                    and target.attr == "patch_set"
                    and isinstance(stmt.value, ast.List)
                ):
                    for elt in stmt.value.elts:
                        if isinstance(elt, ast.Dict):
                            self.process_patch_entry(elt)

    def resolve_symbol(self, node: ast.expr) -> ResolvedSymbol:
        """Resolve named variables to their actual values from the symbol table."""
        if isinstance(node, ast.Name) and node.id in self.symbol_table:
            return self.safe_literal_eval(self.symbol_table[node.id])
        return self.safe_literal_eval(node)

    def safe_literal_eval(self, node: ast.expr) -> ResolvedSymbol:
        """Try to evaluate or stringify an AST node."""
        try:
            return ast.literal_eval(node)
        except ValueError:
            # Spooky eval! But we trust nerd fonts to be safe...
            if hasattr(ast, "unparse"):
                return eval(
                    ast.unparse(node),
                    {"box_enabled": False, "box_keep": False},
                    {
                        "self": SimpleNamespace(
                            args=SimpleNamespace(
                                careful=False,
                                custom=False,
                                fontawesome=True,
                                fontawesomeextension=True,
                                fontlogos=True,
                                octicons=True,
                                codicons=True,
                                powersymbols=True,
                                pomicons=True,
                                powerline=True,
                                powerlineextra=True,
                                material=True,
                                weather=True,
                            )
                        ),
                    },
                )
            msg = f"<cannot eval: {type(node).__name__}>"
            raise ValueError(msg) from None

    def process_patch_entry(self, dict_node: ast.Dict) -> None:
        entry = {}
        for key_node, value_node in zip(dict_node.keys, dict_node.values):
            if isinstance(key_node, ast.Constant):
                if key_node.value == "Enabled":
                    if self.safe_literal_eval(value_node):
                        continue  # This patch set is enabled, continue to next key
                    else:
                        return  # This patch set is disabled, skip
                key = ast.literal_eval(cast("ast.Constant", key_node))
                entry[key] = self.resolve_symbol(value_node)
        self.patch_set_values.append(cast("PatchSet", entry))


def extract_patch_set_values(source_code: str) -> tuple[list[PatchSet], str]:
    tree = ast.parse(source_code)
    extractor = PatchSetExtractor()
    extractor.visit(tree)
    return extractor.patch_set_values, extractor.nf_version


def generate_codepoint_tables(
    patch_sets: list[PatchSet],
    nerd_font: TTFont,
    nf_version: str,
) -> dict[str, dict[int, int]]:
    # We may already have the table saved from a previous run.
    if Path("nerd_font_codepoint_tables.py").exists():
        import nerd_font_codepoint_tables

        if nerd_font_codepoint_tables.version == nf_version:
            return nerd_font_codepoint_tables.cp_tables

    cp_tables: dict[str, dict[int, int]] = {}
    cp_nerdfont_used: set[int] = set()
    cmap = nerd_font.getBestCmap()
    for entry in patch_sets:
        patch_set_name = entry["Name"]
        print(f"Info: Extracting codepoint table from patch set '{patch_set_name}'")

        # Extract codepoint map from original font file; download if needed
        source_filename = entry["Filename"]
        target_folder = Path("nerd_font_symbol_fonts")
        target_folder.mkdir(exist_ok=True)
        target_file = target_folder / Path(source_filename).name
        if not target_file.exists():
            print(f"Info: Downloading '{source_filename}'")
            urlretrieve(
                f"https://github.com/ryanoasis/nerd-fonts/raw/refs/tags/v{nf_version}/src/glyphs/{source_filename}",
                target_file,
            )
        try:
            with TTFont(target_file) as patchfont:
                patch_cmap = patchfont.getBestCmap()
        except TTLibError:
            # Not a TTF/OTF font. This is OK if this patch set is exact, so we
            # let if pass. If there's a problem, later checks will catch it.
            patch_cmap = None

        # A glyph's scale rules are specified using its codepoint in
        # the original font, which is sometimes different from its
        # Nerd Font codepoint. If entry["Exact"] is False, the codepoints are
        # mapped according to the following rules:
        # * entry["SymStart"] and entry["SymEnd"] denote the patch set's codepoint
        #   range in the original font.
        # * entry["SrcStart"] is the starting point of the patch set's mapped
        #   codepoint range. It must not be None if entry["Exact"] is False.
        # * The destination codepoint range is packed; that is, while there may be
        #   gaps without glyphs in the original font's codepoint range, there are
        #   none in the Nerd Font range. Hence there is no constant codepoint
        #   offset; instead we must iterate through the range and increment the
        #   destination codepoint every time we encounter a glyph in the original
        #   font.
        # If entry["Exact"] is True, the origin and Nerd Font codepoints are the
        # same, gaps included, and entry["SrcStart"] must be None.
        if entry["Exact"]:
            assert entry["SrcStart"] is None
            cp_nerdfont = 0
        else:
            assert entry["SrcStart"]
            assert patch_cmap is not None
            cp_nerdfont = entry["SrcStart"] - 1

        if patch_set_name not in cp_tables:
            # There are several patch sets with the same name, representing
            # different codepoint ranges within the same original font. Merging
            # these into a single table is OK. However, we need to keep separate
            # tables for the different fonts to correctly deal with cases where
            # they fill in each other's gaps.
            cp_tables[patch_set_name] = {}
        for cp_original in range(entry["SymStart"], entry["SymEnd"] + 1):
            if patch_cmap and cp_original not in patch_cmap:
                continue
            if not entry["Exact"]:
                cp_nerdfont += 1
            else:
                cp_nerdfont = cp_original
            if cp_nerdfont not in cmap:
                raise ValueError(
                    f"Missing codepoint in Symbols Only Font: {hex(cp_nerdfont)} in patch set '{patch_set_name}'"
                )
            elif cp_nerdfont in cp_nerdfont_used:
                raise ValueError(
                    f"Overlap for codepoint {hex(cp_nerdfont)} in patch set '{patch_set_name}'"
                )
            cp_tables[patch_set_name][cp_original] = cp_nerdfont
            cp_nerdfont_used.add(cp_nerdfont)

    # Store the table and corresponding Nerd Fonts version together in a module.
    with open("nerd_font_codepoint_tables.py", "w") as f:
        print(
            """#! This is a generated file, produced by nerd_font_codegen.py
#! DO NOT EDIT BY HAND!
#!
#! This file specifies the mapping of codepoints in the original symbol
#! fonts to codepoints in a patched Nerd Font. This is extracted from
#! the nerd fonts patcher script and the symbol font files.""",
            file=f,
        )
        print(f'version = "{nf_version}"', file=f)
        print("cp_tables = {", file=f)
        for name, table in cp_tables.items():
            print(f'    "{name}": {{', file=f)
            for key, value in table.items():
                print(f"        {hex(key)}: {hex(value)},", file=f)
            print("    },", file=f)
        print("}", file=f)

    return cp_tables



def collect_entries(
    patch_sets: list[PatchSet],
    nerd_font: TTFont,
    nf_version: str,
) -> dict[int, PatchSetAttributeEntry]:
    cmap = nerd_font.getBestCmap()
    glyphs = nerd_font.getGlyphSet()
    cp_tables = generate_codepoint_tables(patch_sets, nerd_font, nf_version)

    entries: dict[int, PatchSetAttributeEntry] = {}
    for entry in patch_sets:
        patch_set_name = entry["Name"]
        print(f"Info: Extracting rules from patch set '{patch_set_name}'")

        attributes = entry["Attributes"]
        patch_set_entries: dict[int, PatchSetAttributeEntry] = {}

        cp_table = cp_tables[patch_set_name]
        for cp_original in range(entry["SymStart"], entry["SymEnd"] + 1):
            if cp_original not in cp_table:
                continue
            cp_nerdfont = cp_table[cp_original]
            if cp_nerdfont in entries:
                raise ValueError(
                    f"Overlap for codepoint {hex(cp_nerdfont)} in patch set '{patch_set_name}'"
                )
            if cp_original in attributes:
                patch_set_entries[cp_nerdfont] = attributes[cp_original].copy()
            else:
                patch_set_entries[cp_nerdfont] = attributes["default"].copy()

        if entry["ScaleRules"] is not None:
            for group in entry["ScaleRules"]["ScaleGroups"]:
                xMin = math.inf
                yMin = math.inf
                xMax = -math.inf
                yMax = -math.inf
                individual_bounds: dict[int, tuple[int, int, int, int]] = {}
                individual_advances: set[float] = set()
                for cp_original in group:
                    if cp_original not in cp_table:
                        # There is one special case where a scale group includes
                        # a glyph from the original font that's not in any patch
                        # set, and hence not in the Symbols Only font. The point
                        # of this glyph is to add extra vertical padding to a
                        # stretched (^xy) scale group, which means that its
                        # scaled and aligned position would span the line height
                        # plus overlap. Thus, we can use any other stretched
                        # glyph with overlap as stand-in to get the vertical
                        # bounds, such as 0xE0B0 (powerline left hard divider).
                        # We don't worry about the horizontal bounds, as they by
                        # design should not affect the group's bounding box.
                        if (
                            patch_set_name == "Progress Indicators"
                            and cp_original == 0xEDFF
                        ):
                            glyph = glyphs[cmap[0xE0B0]]
                            bounds = BoundsPen(glyphSet=glyphs)
                            glyph.draw(bounds)
                            yMin = min(bounds.bounds[1], yMin)
                            yMax = max(bounds.bounds[3], yMax)
                        else:
                            # Other cases are due to lazily specified scale
                            # groups with gaps in the codepoint range.
                            print(
                                f"Info: Skipping scale group codepoint {hex(cp_original)}, which does not exist in patch set '{patch_set_name}'"
                            )
                        continue

                    cp_nerdfont = cp_table[cp_original]
                    glyph = glyphs[cmap[cp_nerdfont]]
                    individual_advances.add(glyph.width)
                    bounds = BoundsPen(glyphSet=glyphs)
                    glyph.draw(bounds)
                    individual_bounds[cp_nerdfont] = bounds.bounds
                    xMin = min(bounds.bounds[0], xMin)
                    yMin = min(bounds.bounds[1], yMin)
                    xMax = max(bounds.bounds[2], xMax)
                    yMax = max(bounds.bounds[3], yMax)
                group_width = xMax - xMin
                group_height = yMax - yMin
                group_is_monospace = (len(individual_bounds) > 1) and (
                    len(individual_advances) == 1
                )
                for cp_original in group:
                    if cp_original not in cp_table:
                        continue
                    cp_nerdfont = cp_table[cp_original]
                    if (
                        # Scale groups may cut across patch sets, but we're only
                        # updating a single patch set at a time, so we skip
                        # codepoints not in it.
                        cp_nerdfont not in patch_set_entries
                        # Codepoints may contribute to the bounding box of multiple groups,
                        # but should be scaled according to the first group they are found
                        # in. Hence, to avoid overwriting, we need to skip codepoints that
                        # have already been assigned a scale group.
                        or "relative_height" in patch_set_entries[cp_nerdfont]
                    ):
                        continue
                    this_bounds = individual_bounds[cp_nerdfont]
                    this_height = this_bounds[3] - this_bounds[1]
                    patch_set_entries[cp_nerdfont]["relative_height"] = (
                        this_height / group_height
                    )
                    patch_set_entries[cp_nerdfont]["relative_y"] = (
                        this_bounds[1] - yMin
                    ) / group_height
                    # Horizontal alignment should only be grouped if the group is monospace,
                    # that is, if all glyphs in the group have the same advance width.
                    if group_is_monospace:
                        this_width = this_bounds[2] - this_bounds[0]
                        patch_set_entries[cp_nerdfont]["relative_width"] = (
                            this_width / group_width
                        )
                        patch_set_entries[cp_nerdfont]["relative_x"] = (
                            this_bounds[0] - xMin
                        ) / group_width
        entries |= patch_set_entries

    return entries


def rust_align(val: str) -> str:
    # font-patcher's 'c' centers within the first cell even for two-cell
    # glyphs (Center1); '' leaves the axis alone.
    return {"l": "Start", "r": "End", "c": "Center1", "": "None"}[val]


def constraint_of(attr: PatchSetAttributeEntry) -> str:
    """One `GlyphConstraint` literal, translated the way Ghostty's
    `emit_zig_entry_multikey` translates font-patcher's attributes."""
    stretch = attr.get("stretch", "")
    params = attr.get("params", {})
    overlap = float(params.get("overlap", 0.0))
    xy_ratio = float(params.get("xy-ratio", -1.0))
    y_padding = float(params.get("ypadding", 0.0))

    if "pa" in stretch:
        # '!' (or overlap) scales up to the box; otherwise scale down to fit,
        # but never below one cell.
        size = "Cover" if ("!" in stretch or overlap) else "FitCover1"
    elif "xy" in stretch:
        size = "Stretch"
    else:
        raise ValueError(f"unknown stretch rule {stretch!r}")

    # '^' sizes against the full cell height, not the icon height.
    height = "Cell" if "^" in stretch else "Icon"
    # '1' caps the glyph at one cell; non-'pa' rules default to one cell
    # unless they say '2'.
    max_cells = 1 if ("1" in stretch or ("pa" not in stretch and "2" not in stretch)) else 2

    pad = [0.0, 0.0, 0.0, 0.0]  # top, right, bottom, left
    if overlap:
        horizontal = -overlap / 2
        # font-patcher caps vertical overlap at 0.01.
        vertical = -min(0.01, overlap) / 2
        pad = [vertical, horizontal, vertical, horizontal]
    elif y_padding:
        pad = [y_padding / 2, 0.0, y_padding / 2, 0.0]

    def f(x: float) -> str:
        # Shortest literal that round-trips through f32 (what the table
        # stores), so clippy's excessive_precision has nothing to flag.
        target = struct.unpack("f", struct.pack("f", x))[0]
        for digits in range(1, 10):
            s = f"{target:.{digits}g}"
            if struct.unpack("f", struct.pack("f", float(s)))[0] == target:
                break
        return s if ("e" in s or "." in s) else s + ".0"

    return (
        "GlyphConstraint {"
        f" size: Size::{size},"
        f" height: Height::{height},"
        f" max_cells: {max_cells},"
        f" align_x: Align::{rust_align(attr.get('align', ''))},"
        f" align_y: Align::{rust_align(attr.get('valign', ''))},"
        f" pad: [{', '.join(f(p) for p in pad)}],"
        f" group: [{f(attr.get('relative_x', 0.0))}, {f(attr.get('relative_y', 0.0))},"
        f" {f(attr.get('relative_width', 1.0))}, {f(attr.get('relative_height', 1.0))}],"
        f" max_xy_ratio: {f(xy_ratio if xy_ratio > 0 else 0.0)} }}"
    )


def generate_rust(entries: dict[int, PatchSetAttributeEntry], nf_version: str) -> str:
    constraints: list[str] = []
    index: dict[str, int] = {}
    per_cp: dict[int, int] = {}
    for cp in sorted(entries):
        literal = constraint_of(entries[cp])
        per_cp[cp] = index.setdefault(literal, len(index))
        if per_cp[cp] == len(constraints):
            constraints.append(literal)

    # Maximal runs of consecutive codepoints sharing a constraint.
    ranges: list[tuple[int, int, int]] = []
    for cp in sorted(per_cp):
        idx = per_cp[cp]
        if ranges and ranges[-1][1] + 1 == cp and ranges[-1][2] == idx:
            ranges[-1] = (ranges[-1][0], cp, idx)
        else:
            ranges.append((cp, cp, idx))

    out = [
        "// @generated by tools/nerd_font_constraints.py - DO NOT EDIT BY HAND.",
        f"// Nerd Fonts font-patcher v{nf_version}: the scaling/alignment rules it",
        "// applies to each symbol it patches in, keyed by Nerd Font codepoint.",
        "",
        "use super::{Align, GlyphConstraint, Height, Size};",
        "",
        "/// Distinct constraints, indexed by [`RANGES`].",
        "#[rustfmt::skip]",
        f"pub(super) static CONSTRAINTS: [GlyphConstraint; {len(constraints)}] = [",
    ]
    out += [f"    {c}," for c in constraints]
    out += [
        "];",
        "",
        "/// `(first, last, constraint)`, sorted and non-overlapping.",
        "#[rustfmt::skip]",
        f"pub(super) static RANGES: [(u32, u32, u16); {len(ranges)}] = [",
    ]
    out += [f"    ({a:#x}, {b:#x}, {i})," for a, b, i in ranges]
    out += ["];", ""]
    return "\n".join(out)


if __name__ == "__main__":
    patcher_path, font_path = Path(sys.argv[1]), Path(sys.argv[2])
    cache_dir = Path(sys.argv[3]) if len(sys.argv) > 3 else Path.cwd()
    out_path = (
        Path(__file__).resolve().parents[1]
        / "src" / "backend" / "glyph_constraint" / "nerd_font.rs"
    )

    patch_sets, nf_version = extract_patch_set_values(
        patcher_path.read_text(encoding="utf-8")
    )
    nerd_font = TTFont(font_path)
    # The codepoint-table cache and the downloaded symbol fonts are looked
    # up relative to the working directory.
    cache_dir.mkdir(parents=True, exist_ok=True)
    os.chdir(cache_dir)
    sys.path.insert(0, str(cache_dir))
    entries = collect_entries(patch_sets, nerd_font, nf_version)
    out_path.write_text(generate_rust(entries, nf_version), encoding="utf-8")
    print(f"wrote {out_path} ({len(entries)} codepoints)")
