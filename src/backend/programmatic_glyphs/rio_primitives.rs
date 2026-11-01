// Copyright (c) 2023-present, Raphael Amorim.
//
// This source code is licensed under the MIT license found in the
// LICENSE file in the root directory of this source tree.

use crate::Quad;
use serde::Deserialize;

#[derive(Debug, PartialEq, Copy, Clone)]
pub enum SugarCursor {
    Block([f32; 4]),
    HollowBlock([f32; 4]),
    Caret([f32; 4]),
    Underline([f32; 4]),
}

#[derive(Default, Clone, Deserialize, Debug, PartialEq)]
pub struct ImageProperties {
    #[serde(default = "String::default")]
    pub path: String,
    #[serde(default = "Option::default")]
    pub width: Option<f32>,
    #[serde(default = "Option::default")]
    pub height: Option<f32>,
    #[serde(default = "f32::default")]
    pub x: f32,
    #[serde(default = "f32::default")]
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichTextLinesRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichText {
    pub id: usize,
    pub position: [f32; 2],
    pub lines: Option<RichTextLinesRange>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Object {
    Quad(Quad),
    RichText(RichText),
}

pub enum CornerType {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

pub fn drawable_character(character: char) -> Option<DrawableChar> {
    match character {
        '\u{2500}'..='\u{259f}'
        | '\u{1fb00}'..='\u{1fb3b}'
        | '\u{e0b0}'..='\u{e0bf}'
        | '\u{2800}'..='\u{28FF}'
        | '\u{1FB00}'..='\u{1FB3F}'
        | '\u{1CD00}'..='\u{1CDE5}' => {
            if let Ok(character) = DrawableChar::try_from(character) {
                return Some(character)
            }
        }
        _ => {},
    }

    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DrawableChar {
    Horizontal,
    DoubleHorizontal,
    Vertical,
    DoubleVertical,
    HeavyHorizontal,
    HeavyVertical,
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    Cross,
    VerticalRight,
    VerticalLeft,
    HorizontalDown,
    HorizontalUp,
    ArcTopLeft,
    ArcBottomRight,
    ArcBottomLeft,
    ArcTopRight,

    DownDoubleAndHorizontalSingle,
    DownSingleAndHorizontalDouble,
    DoubleUpAndRight,
    DoubleUpAndLeft,
    UpSingleAndRightDouble,
    UpSingleAndLeftDouble,
    VerticalSingleAndHorizontalDouble,

    LowerOneEighthBlock,
    LowerOneQuarterBlock,
    LowerThreeEighthsBlock,
    LeftOneQuarterBlock,
    LeftThreeEighthsBlock,
    LeftThreeQuartersBlock,
    RightOneQuarterBlock,
    RightThreeEighthsBlock,
    RightThreeQuartersBlock,
    UpperOneEighthBlock,
    UpperThreeEighthsBlock,
    UpperThreeQuartersBlock,

    HorizontalLightDash,
    HorizontalHeavyDash,
    HorizontalLightDoubleDash,
    HorizontalHeavyDoubleDash,
    HorizontalLightTripleDash,
    HorizontalHeavyTripleDash,
    VerticalLightDash,
    VerticalHeavyDash,
    VerticalLightDoubleDash,
    VerticalHeavyDoubleDash,
    VerticalLightTripleDash,
    VerticalHeavyTripleDash,
    LightShade,
    MediumShade,
    DarkShade,
    FullBlock,

    DoubleCross,
    DoubleVerticalRight,
    DoubleVerticalLeft,
    DoubleHorizontalDown,
    DoubleHorizontalUp,
    VerticalDoubleAndHorizontalSingle,

    DownDoubleAndRightSingle,
    DownDoubleAndLeftSingle,
    VerticalDoubleAndRightSingle,
    VerticalDoubleAndLeftSingle,
    VerticalSingleAndRightDouble,
    VerticalSingleAndLeftDouble,
    DownSingleAndRightDouble,
    DownSingleAndLeftDouble,

    HeavyDownAndRight,
    HeavyDownAndLeft,
    HeavyUpAndRight,
    HeavyUpAndLeft,
    HeavyVerticalAndRight,
    HeavyVerticalAndLeft,
    HeavyHorizontalAndDown,
    HeavyHorizontalAndUp,
    HeavyCross,

    LightDownAndHeavyRight,
    LightDownAndHeavyLeft,
    HeavyDownAndLightRight,
    HeavyDownAndLightLeft,
    LightUpAndHeavyRight,
    LightUpAndHeavyLeft,
    HeavyUpAndLightRight,
    HeavyUpAndLightLeft,

    LowerHalf,
    LeftHalf,
    RightHalf,
    UpperHalf,
    UpperOneQuarterBlock,
    LowerFiveEighthsBlock,
    LowerThreeQuartersBlock,
    LowerSevenEighthsBlock,
    QuadrantUpperLeftAndLowerLeft,
    QuadrantUpperLeftAndLowerRight,
    QuadrantUpperLeftAndUpperRight,
    QuadrantUpperRightAndLowerLeft,
    QuadrantUpperRightAndLowerRight,
    QuadrantUpperLeft,
    QuadrantUpperRight,
    QuadrantLowerLeft,
    QuadrantLowerRight,

    SeparatedQuadrantUpperLeft,
    SeparatedQuadrantUpperRight,
    SeparatedQuadrantLowerLeft,
    SeparatedQuadrantLowerRight,

    DiagonalRisingBar,
    DiagonalFallingBar,
    DiagonalCross,

    Sextant(u8),
    Octant(u8),

    PowerlineLeftSolid,
    PowerlineRightSolid,
    PowerlineLeftHollow,
    PowerlineRightHollow,
    PowerlineCurvedRightSolid,
    PowerlineCurvedRightHollow,
    PowerlineCurvedLeftSolid,
    PowerlineCurvedLeftHollow,
    PowerlineLowerLeftTriangle,
    PowerlineBackslashSeparator,
    PowerlineLowerRightTriangle,
    PowerlineForwardslashSeparator,
    PowerlineUpperLeftTriangle,
    PowerlineForwardslashSeparatorRedundant,
    PowerlineUpperRightTriangle,
    PowerlineBackslashSeparatorRedundant,

    BrailleBlank,
    Braille(Braille),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Braille {
    Dots1,
    Dots2,
    Dots12,
    Dots3,
    Dots13,
    Dots23,
    Dots123,
    Dots4,
    Dots14,
    Dots24,
    Dots124,
    Dots34,
    Dots134,
    Dots234,
    Dots1234,
    Dots5,
    Dots15,
    Dots25,
    Dots125,
    Dots35,
    Dots135,
    Dots235,
    Dots1235,
    Dots45,
    Dots145,
    Dots245,
    Dots1245,
    Dots345,
    Dots1345,
    Dots2345,
    Dots12345,
    Dots6,
    Dots16,
    Dots26,
    Dots126,
    Dots36,
    Dots136,
    Dots236,
    Dots1236,
    Dots46,
    Dots146,
    Dots246,
    Dots1246,
    Dots346,
    Dots1346,
    Dots2346,
    Dots12346,
    Dots56,
    Dots156,
    Dots256,
    Dots1256,
    Dots356,
    Dots1356,
    Dots2356,
    Dots12356,
    Dots456,
    Dots1456,
    Dots2456,
    Dots12456,
    Dots3456,
    Dots13456,
    Dots23456,
    Dots123456,

    Dots7,
    Dots17,
    Dots27,
    Dots127,
    Dots37,
    Dots137,
    Dots237,
    Dots1237,
    Dots47,
    Dots147,
    Dots247,
    Dots1247,
    Dots347,
    Dots1347,
    Dots2347,
    Dots12347,
    Dots57,
    Dots157,
    Dots257,
    Dots1257,
    Dots357,
    Dots1357,
    Dots2357,
    Dots12357,
    Dots457,
    Dots1457,
    Dots2457,
    Dots12457,
    Dots3457,
    Dots13457,
    Dots23457,
    Dots123457,
    Dots67,
    Dots167,
    Dots267,
    Dots1267,
    Dots367,
    Dots1367,
    Dots2367,
    Dots12367,
    Dots467,
    Dots1467,
    Dots2467,
    Dots12467,
    Dots3467,
    Dots13467,
    Dots23467,
    Dots123467,
    Dots567,
    Dots1567,
    Dots2567,
    Dots12567,
    Dots3567,
    Dots13567,
    Dots23567,
    Dots123567,
    Dots4567,
    Dots14567,
    Dots24567,
    Dots124567,
    Dots34567,
    Dots134567,
    Dots234567,
    Dots1234567,
    Dots235678,

    Dots8,
    Dots18,
    Dots28,
    Dots128,
    Dots38,
    Dots138,
    Dots238,
    Dots1238,
    Dots48,
    Dots148,
    Dots248,
    Dots1248,
    Dots348,
    Dots1348,
    Dots2348,
    Dots12348,
    Dots58,
    Dots158,
    Dots258,
    Dots1258,
    Dots358,
    Dots1358,
    Dots2358,
    Dots12358,
    Dots458,
    Dots1458,
    Dots2458,
    Dots12458,
    Dots3458,
    Dots13458,
    Dots23458,
    Dots123458,
    Dots68,
    Dots168,
    Dots268,
    Dots1268,
    Dots368,
    Dots1368,
    Dots2368,
    Dots12368,
    Dots468,
    Dots1468,
    Dots2468,
    Dots12468,
    Dots3468,
    Dots13468,
    Dots23468,
    Dots123468,
    Dots568,
    Dots1568,
    Dots2568,
    Dots12568,
    Dots3568,
    Dots13568,
    Dots23568,
    Dots123568,
    Dots4568,
    Dots14568,
    Dots24568,
    Dots124568,
    Dots34568,
    Dots134568,
    Dots234568,
    Dots1234568,

    Dots78,
    Dots178,
    Dots278,
    Dots1278,
    Dots378,
    Dots1378,
    Dots2378,
    Dots12378,
    Dots478,
    Dots1478,
    Dots2478,
    Dots12478,
    Dots3478,
    Dots13478,
    Dots23478,
    Dots123478,
    Dots578,
    Dots1578,
    Dots2578,
    Dots12578,
    Dots3578,
    Dots13578,
    Dots23578,
    Dots123578,
    Dots4578,
    Dots14578,
    Dots24578,
    Dots124578,
    Dots34578,
    Dots134578,
    Dots234578,
    Dots1234578,
    Dots678,
    Dots1678,
    Dots2678,
    Dots12678,
    Dots3678,
    Dots13678,
    Dots23678,
    Dots123678,
    Dots4678,
    Dots14678,
    Dots24678,
    Dots124678,
    Dots34678,
    Dots134678,
    Dots234678,
    Dots1234678,
    Dots5678,
    Dots15678,
    Dots25678,
    Dots125678,

    Dots12345678,
    Dots45678,
    Dots35678,
    Dots345678,
    Dots2345678,
    Dots1235678,

    Dots135678,
    Dots1345678,
    Dots1245678,
    Dots145678,
    Dots245678,
}

impl TryFrom<char> for DrawableChar {
    type Error = char;

    fn try_from(val: char) -> Result<Self, Self::Error> {
        let drawbable_char = match val {
            '─' => DrawableChar::Horizontal,
            '═' => DrawableChar::DoubleHorizontal,
            '│' => DrawableChar::Vertical,
            '║' => DrawableChar::DoubleVertical,
            '━' => DrawableChar::HeavyHorizontal,
            '┃' => DrawableChar::HeavyVertical,
            '└' => DrawableChar::TopRight,
            '┘' => DrawableChar::TopLeft,
            '┌' => DrawableChar::BottomRight,
            '┐' => DrawableChar::BottomLeft,
            '┼' => DrawableChar::Cross,
            '├' => DrawableChar::VerticalRight,
            '┤' => DrawableChar::VerticalLeft,
            '┬' => DrawableChar::HorizontalDown,
            '┴' => DrawableChar::HorizontalUp,

            '╯' => DrawableChar::ArcTopLeft,
            '╭' => DrawableChar::ArcBottomRight,
            '╮' => DrawableChar::ArcBottomLeft,
            '╰' => DrawableChar::ArcTopRight,

            '╥' => DrawableChar::DownDoubleAndHorizontalSingle,
            '╤' => DrawableChar::DownSingleAndHorizontalDouble,
            '╚' => DrawableChar::DoubleUpAndRight,
            '╝' => DrawableChar::DoubleUpAndLeft,
            '╘' => DrawableChar::UpSingleAndRightDouble,
            '╛' => DrawableChar::UpSingleAndLeftDouble,
            '╪' => DrawableChar::VerticalSingleAndHorizontalDouble,

            '▁' => DrawableChar::LowerOneEighthBlock,
            '▂' => DrawableChar::LowerOneQuarterBlock,
            '▃' => DrawableChar::LowerThreeEighthsBlock,
            '▎' => DrawableChar::LeftOneQuarterBlock,
            '▍' => DrawableChar::LeftThreeEighthsBlock,
            '▊' => DrawableChar::LeftThreeQuartersBlock,
            '▕' => DrawableChar::RightOneQuarterBlock,
            '🮈' => DrawableChar::RightThreeEighthsBlock,
            '🮊' => DrawableChar::RightThreeQuartersBlock,
            '▔' => DrawableChar::UpperOneEighthBlock,
            '🮃' => DrawableChar::UpperThreeEighthsBlock,
            '🮅' => DrawableChar::UpperThreeQuartersBlock,

            '┄' => DrawableChar::HorizontalLightDash,
            '┅' => DrawableChar::HorizontalHeavyDash,
            '┈' => DrawableChar::HorizontalLightDoubleDash,
            '┉' => DrawableChar::HorizontalHeavyDoubleDash,
            '╌' => DrawableChar::HorizontalLightTripleDash,
            '╍' => DrawableChar::HorizontalHeavyTripleDash,
            '┆' => DrawableChar::VerticalLightDash,
            '┇' => DrawableChar::VerticalHeavyDash,
            '┊' => DrawableChar::VerticalLightDoubleDash,
            '┋' => DrawableChar::VerticalHeavyDoubleDash,
            '╎' => DrawableChar::VerticalLightTripleDash,
            '╏' => DrawableChar::VerticalHeavyTripleDash,

            '▘' => DrawableChar::QuadrantUpperLeft,
            '▝' => DrawableChar::QuadrantUpperRight,
            '▖' => DrawableChar::QuadrantLowerLeft,
            '▗' => DrawableChar::QuadrantLowerRight,
            '▀' => DrawableChar::UpperHalf,
            '▄' => DrawableChar::LowerHalf,
            '▌' => DrawableChar::LeftHalf,
            '▐' => DrawableChar::RightHalf,
            '░' => DrawableChar::LightShade,
            '▒' => DrawableChar::MediumShade,
            '▓' => DrawableChar::DarkShade,
            '█' => DrawableChar::FullBlock,

            '🬓' => DrawableChar::SeparatedQuadrantUpperLeft,
            '🬔' => DrawableChar::SeparatedQuadrantUpperRight,
            '🬕' => DrawableChar::SeparatedQuadrantLowerLeft,
            '🬖' => DrawableChar::SeparatedQuadrantLowerRight,

            '╬' => DrawableChar::DoubleCross,
            '╠' => DrawableChar::DoubleVerticalRight,
            '╣' => DrawableChar::DoubleVerticalLeft,
            '╦' => DrawableChar::DoubleHorizontalDown,
            '╩' => DrawableChar::DoubleHorizontalUp,
            '╫' => DrawableChar::VerticalDoubleAndHorizontalSingle,

            '╓' => DrawableChar::DownDoubleAndRightSingle,
            '╖' => DrawableChar::DownDoubleAndLeftSingle,
            '╟' => DrawableChar::VerticalDoubleAndRightSingle,
            '╢' => DrawableChar::VerticalDoubleAndLeftSingle,
            '╞' => DrawableChar::VerticalSingleAndRightDouble,
            '╡' => DrawableChar::VerticalSingleAndLeftDouble,
            '╒' => DrawableChar::DownSingleAndRightDouble,
            '╕' => DrawableChar::DownSingleAndLeftDouble,

            '┏' => DrawableChar::HeavyDownAndRight,
            '┓' => DrawableChar::HeavyDownAndLeft,
            '┗' => DrawableChar::HeavyUpAndRight,
            '┛' => DrawableChar::HeavyUpAndLeft,
            '┣' => DrawableChar::HeavyVerticalAndRight,
            '┫' => DrawableChar::HeavyVerticalAndLeft,
            '┳' => DrawableChar::HeavyHorizontalAndDown,
            '┻' => DrawableChar::HeavyHorizontalAndUp,
            '╋' => DrawableChar::HeavyCross,

            '┍' => DrawableChar::LightDownAndHeavyRight,
            '┑' => DrawableChar::LightDownAndHeavyLeft,
            '┎' => DrawableChar::HeavyDownAndLightRight,
            '┒' => DrawableChar::HeavyDownAndLightLeft,
            '┕' => DrawableChar::LightUpAndHeavyRight,
            '┙' => DrawableChar::LightUpAndHeavyLeft,
            '┖' => DrawableChar::HeavyUpAndLightRight,
            '┚' => DrawableChar::HeavyUpAndLightLeft,

            '▅' => DrawableChar::LowerFiveEighthsBlock,
            '▆' => DrawableChar::LowerThreeQuartersBlock,
            '▇' => DrawableChar::LowerSevenEighthsBlock,
            '▚' => DrawableChar::QuadrantUpperLeftAndLowerLeft,
            '▞' => DrawableChar::QuadrantUpperLeftAndLowerRight,
            '▟' => DrawableChar::QuadrantUpperRightAndLowerLeft,
            '▙' => DrawableChar::QuadrantUpperRightAndLowerRight,

            '╱' => DrawableChar::DiagonalRisingBar,
            '╲' => DrawableChar::DiagonalFallingBar,
            '╳' => DrawableChar::DiagonalCross,

            '\u{e0b0}' => DrawableChar::PowerlineRightSolid,
            '\u{e0b1}' => DrawableChar::PowerlineRightHollow,
            '\u{e0b2}' => DrawableChar::PowerlineLeftSolid,
            '\u{e0b3}' => DrawableChar::PowerlineLeftHollow,
            '\u{e0b4}' => DrawableChar::PowerlineCurvedRightSolid,
            '\u{e0b5}' => DrawableChar::PowerlineCurvedRightHollow,
            '\u{e0b6}' => DrawableChar::PowerlineCurvedLeftSolid,
            '\u{e0b7}' => DrawableChar::PowerlineCurvedLeftHollow,
            '\u{e0b8}' => DrawableChar::PowerlineLowerLeftTriangle,
            '\u{e0b9}' => DrawableChar::PowerlineBackslashSeparator,
            '\u{e0ba}' => DrawableChar::PowerlineLowerRightTriangle,
            '\u{e0bb}' => DrawableChar::PowerlineForwardslashSeparator,
            '\u{e0bc}' => DrawableChar::PowerlineUpperLeftTriangle,
            '\u{e0bd}' => DrawableChar::PowerlineForwardslashSeparatorRedundant,
            '\u{e0be}' => DrawableChar::PowerlineUpperRightTriangle,
            '\u{e0bf}' => DrawableChar::PowerlineBackslashSeparatorRedundant,

            c @ '\u{1FB00}'..='\u{1FB3F}' => {
                DrawableChar::Sextant(SEXTANT_PATTERNS[(c as u32 & 0x3f) as usize])
            }

            c @ '\u{1CD00}'..='\u{1CDE5}' => {
                DrawableChar::Octant(OCTANT_PATTERNS[(c as u32 & 0xff) as usize])
            }
            '\u{1CEA0}' => DrawableChar::Octant(0b10000000),
            '\u{1CEA3}' => DrawableChar::Octant(0b01000000),
            '\u{1CEA8}' => DrawableChar::Octant(0b00000001),
            '\u{1CEAB}' => DrawableChar::Octant(0b00000010),
            '\u{1FBE6}' => DrawableChar::Octant(0b00010100),
            '\u{1FBE7}' => DrawableChar::Octant(0b00101000),

            '⠀' => DrawableChar::BrailleBlank,
            '⠁' => DrawableChar::Braille(Braille::Dots1),
            '⠂' => DrawableChar::Braille(Braille::Dots2),
            '⠃' => DrawableChar::Braille(Braille::Dots12),
            '⠄' => DrawableChar::Braille(Braille::Dots3),
            '⠅' => DrawableChar::Braille(Braille::Dots13),
            '⠆' => DrawableChar::Braille(Braille::Dots23),
            '⠇' => DrawableChar::Braille(Braille::Dots123),
            '⠈' => DrawableChar::Braille(Braille::Dots4),
            '⠉' => DrawableChar::Braille(Braille::Dots14),
            '⠊' => DrawableChar::Braille(Braille::Dots24),
            '⠋' => DrawableChar::Braille(Braille::Dots124),
            '⠌' => DrawableChar::Braille(Braille::Dots34),
            '⠍' => DrawableChar::Braille(Braille::Dots134),
            '⠎' => DrawableChar::Braille(Braille::Dots234),
            '⠏' => DrawableChar::Braille(Braille::Dots1234),
            '⠐' => DrawableChar::Braille(Braille::Dots5),
            '⠑' => DrawableChar::Braille(Braille::Dots15),
            '⠒' => DrawableChar::Braille(Braille::Dots25),
            '⠓' => DrawableChar::Braille(Braille::Dots125),
            '⠔' => DrawableChar::Braille(Braille::Dots35),
            '⠕' => DrawableChar::Braille(Braille::Dots135),
            '⠖' => DrawableChar::Braille(Braille::Dots235),
            '⠗' => DrawableChar::Braille(Braille::Dots1235),
            '⠘' => DrawableChar::Braille(Braille::Dots45),
            '⠙' => DrawableChar::Braille(Braille::Dots145),
            '⠚' => DrawableChar::Braille(Braille::Dots245),
            '⠛' => DrawableChar::Braille(Braille::Dots1245),
            '⠜' => DrawableChar::Braille(Braille::Dots345),
            '⠝' => DrawableChar::Braille(Braille::Dots1345),
            '⠞' => DrawableChar::Braille(Braille::Dots2345),
            '⠟' => DrawableChar::Braille(Braille::Dots12345),
            '⠠' => DrawableChar::Braille(Braille::Dots6),
            '⠡' => DrawableChar::Braille(Braille::Dots16),
            '⠢' => DrawableChar::Braille(Braille::Dots26),
            '⠣' => DrawableChar::Braille(Braille::Dots126),
            '⠤' => DrawableChar::Braille(Braille::Dots36),
            '⠥' => DrawableChar::Braille(Braille::Dots136),
            '⠦' => DrawableChar::Braille(Braille::Dots236),
            '⠧' => DrawableChar::Braille(Braille::Dots1236),
            '⠨' => DrawableChar::Braille(Braille::Dots46),
            '⠩' => DrawableChar::Braille(Braille::Dots146),
            '⠪' => DrawableChar::Braille(Braille::Dots246),
            '⠫' => DrawableChar::Braille(Braille::Dots1246),
            '⠬' => DrawableChar::Braille(Braille::Dots346),
            '⠭' => DrawableChar::Braille(Braille::Dots1346),
            '⠮' => DrawableChar::Braille(Braille::Dots2346),
            '⠯' => DrawableChar::Braille(Braille::Dots12346),
            '⠰' => DrawableChar::Braille(Braille::Dots56),
            '⠱' => DrawableChar::Braille(Braille::Dots156),
            '⠲' => DrawableChar::Braille(Braille::Dots256),
            '⠳' => DrawableChar::Braille(Braille::Dots1256),
            '⠴' => DrawableChar::Braille(Braille::Dots356),
            '⠵' => DrawableChar::Braille(Braille::Dots1356),
            '⠶' => DrawableChar::Braille(Braille::Dots2356),
            '⠷' => DrawableChar::Braille(Braille::Dots12356),
            '⠸' => DrawableChar::Braille(Braille::Dots456),
            '⠹' => DrawableChar::Braille(Braille::Dots1456),
            '⠺' => DrawableChar::Braille(Braille::Dots2456),
            '⠻' => DrawableChar::Braille(Braille::Dots12456),
            '⠼' => DrawableChar::Braille(Braille::Dots3456),
            '⠽' => DrawableChar::Braille(Braille::Dots13456),
            '⠾' => DrawableChar::Braille(Braille::Dots23456),
            '⠿' => DrawableChar::Braille(Braille::Dots123456),

            '⡀' => DrawableChar::Braille(Braille::Dots7),
            '⡁' => DrawableChar::Braille(Braille::Dots17),
            '⡂' => DrawableChar::Braille(Braille::Dots27),
            '⡃' => DrawableChar::Braille(Braille::Dots127),
            '⡄' => DrawableChar::Braille(Braille::Dots37),
            '⡅' => DrawableChar::Braille(Braille::Dots137),
            '⡆' => DrawableChar::Braille(Braille::Dots237),
            '⡇' => DrawableChar::Braille(Braille::Dots1237),
            '⡈' => DrawableChar::Braille(Braille::Dots47),
            '⡉' => DrawableChar::Braille(Braille::Dots147),
            '⡊' => DrawableChar::Braille(Braille::Dots247),
            '⡋' => DrawableChar::Braille(Braille::Dots1247),
            '⡌' => DrawableChar::Braille(Braille::Dots347),
            '⡍' => DrawableChar::Braille(Braille::Dots1347),
            '⡎' => DrawableChar::Braille(Braille::Dots2347),
            '⡏' => DrawableChar::Braille(Braille::Dots12347),
            '⡐' => DrawableChar::Braille(Braille::Dots57),
            '⡑' => DrawableChar::Braille(Braille::Dots157),
            '⡒' => DrawableChar::Braille(Braille::Dots257),
            '⡓' => DrawableChar::Braille(Braille::Dots1257),
            '⡔' => DrawableChar::Braille(Braille::Dots357),
            '⡕' => DrawableChar::Braille(Braille::Dots1357),
            '⡖' => DrawableChar::Braille(Braille::Dots2357),
            '⡗' => DrawableChar::Braille(Braille::Dots12357),
            '⡘' => DrawableChar::Braille(Braille::Dots457),
            '⡙' => DrawableChar::Braille(Braille::Dots1457),
            '⡚' => DrawableChar::Braille(Braille::Dots2457),
            '⡛' => DrawableChar::Braille(Braille::Dots12457),
            '⡜' => DrawableChar::Braille(Braille::Dots3457),
            '⡝' => DrawableChar::Braille(Braille::Dots13457),
            '⡞' => DrawableChar::Braille(Braille::Dots23457),
            '⡟' => DrawableChar::Braille(Braille::Dots123457),
            '⡠' => DrawableChar::Braille(Braille::Dots67),
            '⡡' => DrawableChar::Braille(Braille::Dots167),
            '⡢' => DrawableChar::Braille(Braille::Dots267),
            '⡣' => DrawableChar::Braille(Braille::Dots1267),
            '⡤' => DrawableChar::Braille(Braille::Dots367),
            '⡥' => DrawableChar::Braille(Braille::Dots1367),
            '⡦' => DrawableChar::Braille(Braille::Dots2367),
            '⡧' => DrawableChar::Braille(Braille::Dots12367),
            '⡨' => DrawableChar::Braille(Braille::Dots467),
            '⡩' => DrawableChar::Braille(Braille::Dots1467),
            '⡪' => DrawableChar::Braille(Braille::Dots2467),
            '⡫' => DrawableChar::Braille(Braille::Dots12467),
            '⡬' => DrawableChar::Braille(Braille::Dots3467),
            '⡭' => DrawableChar::Braille(Braille::Dots13467),
            '⡮' => DrawableChar::Braille(Braille::Dots23467),
            '⡯' => DrawableChar::Braille(Braille::Dots123467),
            '⡰' => DrawableChar::Braille(Braille::Dots567),
            '⡱' => DrawableChar::Braille(Braille::Dots1567),
            '⡲' => DrawableChar::Braille(Braille::Dots2567),
            '⡳' => DrawableChar::Braille(Braille::Dots12567),
            '⡴' => DrawableChar::Braille(Braille::Dots3567),
            '⡵' => DrawableChar::Braille(Braille::Dots13567),
            '⡶' => DrawableChar::Braille(Braille::Dots23567),
            '⡷' => DrawableChar::Braille(Braille::Dots123567),
            '⡸' => DrawableChar::Braille(Braille::Dots4567),
            '⡹' => DrawableChar::Braille(Braille::Dots14567),
            '⡺' => DrawableChar::Braille(Braille::Dots24567),
            '⡻' => DrawableChar::Braille(Braille::Dots124567),
            '⡼' => DrawableChar::Braille(Braille::Dots34567),
            '⡽' => DrawableChar::Braille(Braille::Dots134567),
            '⡾' => DrawableChar::Braille(Braille::Dots234567),
            '⡿' => DrawableChar::Braille(Braille::Dots1234567),

            '⢀' => DrawableChar::Braille(Braille::Dots8),
            '⢁' => DrawableChar::Braille(Braille::Dots18),
            '⢂' => DrawableChar::Braille(Braille::Dots28),
            '⢃' => DrawableChar::Braille(Braille::Dots128),
            '⢄' => DrawableChar::Braille(Braille::Dots38),
            '⢅' => DrawableChar::Braille(Braille::Dots138),
            '⢆' => DrawableChar::Braille(Braille::Dots238),
            '⢇' => DrawableChar::Braille(Braille::Dots1238),
            '⢈' => DrawableChar::Braille(Braille::Dots48),
            '⢉' => DrawableChar::Braille(Braille::Dots148),
            '⢊' => DrawableChar::Braille(Braille::Dots248),
            '⢋' => DrawableChar::Braille(Braille::Dots1248),
            '⢌' => DrawableChar::Braille(Braille::Dots348),
            '⢍' => DrawableChar::Braille(Braille::Dots1348),
            '⢎' => DrawableChar::Braille(Braille::Dots2348),
            '⢏' => DrawableChar::Braille(Braille::Dots12348),
            '⢐' => DrawableChar::Braille(Braille::Dots58),
            '⢑' => DrawableChar::Braille(Braille::Dots158),
            '⢒' => DrawableChar::Braille(Braille::Dots258),
            '⢓' => DrawableChar::Braille(Braille::Dots1258),
            '⢔' => DrawableChar::Braille(Braille::Dots358),
            '⢕' => DrawableChar::Braille(Braille::Dots1358),
            '⢖' => DrawableChar::Braille(Braille::Dots2358),
            '⢗' => DrawableChar::Braille(Braille::Dots12358),
            '⢘' => DrawableChar::Braille(Braille::Dots458),
            '⢙' => DrawableChar::Braille(Braille::Dots1458),
            '⢚' => DrawableChar::Braille(Braille::Dots2458),
            '⢛' => DrawableChar::Braille(Braille::Dots12458),
            '⢜' => DrawableChar::Braille(Braille::Dots3458),
            '⢝' => DrawableChar::Braille(Braille::Dots13458),
            '⢞' => DrawableChar::Braille(Braille::Dots23458),
            '⢟' => DrawableChar::Braille(Braille::Dots123458),
            '⢠' => DrawableChar::Braille(Braille::Dots68),
            '⢡' => DrawableChar::Braille(Braille::Dots168),
            '⢢' => DrawableChar::Braille(Braille::Dots268),
            '⢣' => DrawableChar::Braille(Braille::Dots1268),
            '⢤' => DrawableChar::Braille(Braille::Dots368),
            '⢥' => DrawableChar::Braille(Braille::Dots1368),
            '⢦' => DrawableChar::Braille(Braille::Dots2368),
            '⢧' => DrawableChar::Braille(Braille::Dots12368),
            '⢨' => DrawableChar::Braille(Braille::Dots468),
            '⢩' => DrawableChar::Braille(Braille::Dots1468),
            '⢪' => DrawableChar::Braille(Braille::Dots2468),
            '⢫' => DrawableChar::Braille(Braille::Dots12468),
            '⢬' => DrawableChar::Braille(Braille::Dots3468),
            '⢭' => DrawableChar::Braille(Braille::Dots13468),
            '⢮' => DrawableChar::Braille(Braille::Dots23468),
            '⢯' => DrawableChar::Braille(Braille::Dots123468),
            '⢰' => DrawableChar::Braille(Braille::Dots568),
            '⢱' => DrawableChar::Braille(Braille::Dots1568),
            '⢲' => DrawableChar::Braille(Braille::Dots2568),
            '⢳' => DrawableChar::Braille(Braille::Dots12568),
            '⢴' => DrawableChar::Braille(Braille::Dots3568),
            '⢵' => DrawableChar::Braille(Braille::Dots13568),
            '⢶' => DrawableChar::Braille(Braille::Dots23568),
            '⢷' => DrawableChar::Braille(Braille::Dots123568),
            '⢸' => DrawableChar::Braille(Braille::Dots4568),
            '⢹' => DrawableChar::Braille(Braille::Dots14568),
            '⢺' => DrawableChar::Braille(Braille::Dots24568),
            '⢻' => DrawableChar::Braille(Braille::Dots124568),
            '⢼' => DrawableChar::Braille(Braille::Dots34568),
            '⢽' => DrawableChar::Braille(Braille::Dots134568),
            '⢾' => DrawableChar::Braille(Braille::Dots234568),
            '⢿' => DrawableChar::Braille(Braille::Dots1234568),

            '⣀' => DrawableChar::Braille(Braille::Dots78),
            '⣁' => DrawableChar::Braille(Braille::Dots178),
            '⣂' => DrawableChar::Braille(Braille::Dots278),
            '⣃' => DrawableChar::Braille(Braille::Dots1278),
            '⣄' => DrawableChar::Braille(Braille::Dots378),
            '⣅' => DrawableChar::Braille(Braille::Dots1378),
            '⣆' => DrawableChar::Braille(Braille::Dots2378),
            '⣇' => DrawableChar::Braille(Braille::Dots12378),
            '⣈' => DrawableChar::Braille(Braille::Dots478),
            '⣉' => DrawableChar::Braille(Braille::Dots1478),
            '⣊' => DrawableChar::Braille(Braille::Dots2478),
            '⣋' => DrawableChar::Braille(Braille::Dots12478),
            '⣌' => DrawableChar::Braille(Braille::Dots3478),
            '⣍' => DrawableChar::Braille(Braille::Dots13478),
            '⣎' => DrawableChar::Braille(Braille::Dots23478),
            '⣏' => DrawableChar::Braille(Braille::Dots123478),
            '⣐' => DrawableChar::Braille(Braille::Dots578),
            '⣑' => DrawableChar::Braille(Braille::Dots1578),
            '⣒' => DrawableChar::Braille(Braille::Dots2578),
            '⣓' => DrawableChar::Braille(Braille::Dots12578),
            '⣔' => DrawableChar::Braille(Braille::Dots3578),
            '⣕' => DrawableChar::Braille(Braille::Dots13578),
            '⣖' => DrawableChar::Braille(Braille::Dots23578),
            '⣗' => DrawableChar::Braille(Braille::Dots123578),
            '⣘' => DrawableChar::Braille(Braille::Dots4578),
            '⣙' => DrawableChar::Braille(Braille::Dots14578),
            '⣚' => DrawableChar::Braille(Braille::Dots24578),
            '⣛' => DrawableChar::Braille(Braille::Dots124578),
            '⣜' => DrawableChar::Braille(Braille::Dots34578),
            '⣝' => DrawableChar::Braille(Braille::Dots134578),
            '⣞' => DrawableChar::Braille(Braille::Dots234578),
            '⣟' => DrawableChar::Braille(Braille::Dots1234578),
            '⣠' => DrawableChar::Braille(Braille::Dots678),
            '⣡' => DrawableChar::Braille(Braille::Dots1678),
            '⣢' => DrawableChar::Braille(Braille::Dots2678),
            '⣣' => DrawableChar::Braille(Braille::Dots12678),
            '⣤' => DrawableChar::Braille(Braille::Dots3678),
            '⣥' => DrawableChar::Braille(Braille::Dots13678),
            '⣦' => DrawableChar::Braille(Braille::Dots23678),
            '⣧' => DrawableChar::Braille(Braille::Dots123678),
            '⣨' => DrawableChar::Braille(Braille::Dots4678),
            '⣩' => DrawableChar::Braille(Braille::Dots14678),
            '⣪' => DrawableChar::Braille(Braille::Dots24678),
            '⣫' => DrawableChar::Braille(Braille::Dots124678),
            '⣬' => DrawableChar::Braille(Braille::Dots34678),
            '⣭' => DrawableChar::Braille(Braille::Dots134678),
            '⣮' => DrawableChar::Braille(Braille::Dots234678),
            '⣯' => DrawableChar::Braille(Braille::Dots1234678),
            '⣰' => DrawableChar::Braille(Braille::Dots5678),
            '⣱' => DrawableChar::Braille(Braille::Dots15678),
            '⣲' => DrawableChar::Braille(Braille::Dots25678),
            '⣳' => DrawableChar::Braille(Braille::Dots125678),
            '⣿' => DrawableChar::Braille(Braille::Dots12345678),
            '⣶' => DrawableChar::Braille(Braille::Dots235678),
            '⣸' => DrawableChar::Braille(Braille::Dots45678),
            '⣴' => DrawableChar::Braille(Braille::Dots35678),
            '⣼' => DrawableChar::Braille(Braille::Dots345678),
            '⣾' => DrawableChar::Braille(Braille::Dots2345678),
            '⣷' => DrawableChar::Braille(Braille::Dots1235678),

            '⣵' => DrawableChar::Braille(Braille::Dots135678),
            '⣽' => DrawableChar::Braille(Braille::Dots1345678),
            '⣻' => DrawableChar::Braille(Braille::Dots1245678),
            '⣹' => DrawableChar::Braille(Braille::Dots145678),
            '⣺' => DrawableChar::Braille(Braille::Dots245678),
            _ => return Err(val),
        };
        Ok(drawbable_char)
    }
}

pub fn contains_braille_dot(braille_dots: &Braille, dot_number: u8) -> bool {
    match dot_number {
        1 => matches!(
            braille_dots,
            Braille::Dots1
                | Braille::Dots12
                | Braille::Dots13
                | Braille::Dots123
                | Braille::Dots14
                | Braille::Dots124
                | Braille::Dots134
                | Braille::Dots1234
                | Braille::Dots15
                | Braille::Dots125
                | Braille::Dots135
                | Braille::Dots1235
                | Braille::Dots145
                | Braille::Dots1245
                | Braille::Dots1345
                | Braille::Dots12345
                | Braille::Dots16
                | Braille::Dots126
                | Braille::Dots136
                | Braille::Dots1236
                | Braille::Dots146
                | Braille::Dots1246
                | Braille::Dots1346
                | Braille::Dots12346
                | Braille::Dots156
                | Braille::Dots1256
                | Braille::Dots1356
                | Braille::Dots12356
                | Braille::Dots1456
                | Braille::Dots12456
                | Braille::Dots13456
                | Braille::Dots123456
                | Braille::Dots17
                | Braille::Dots127
                | Braille::Dots137
                | Braille::Dots1237
                | Braille::Dots147
                | Braille::Dots1247
                | Braille::Dots1347
                | Braille::Dots12347
                | Braille::Dots157
                | Braille::Dots1257
                | Braille::Dots1357
                | Braille::Dots12357
                | Braille::Dots1457
                | Braille::Dots12457
                | Braille::Dots13457
                | Braille::Dots123457
                | Braille::Dots167
                | Braille::Dots1267
                | Braille::Dots1367
                | Braille::Dots12367
                | Braille::Dots1467
                | Braille::Dots12467
                | Braille::Dots13467
                | Braille::Dots123467
                | Braille::Dots1567
                | Braille::Dots12567
                | Braille::Dots13567
                | Braille::Dots123567
                | Braille::Dots14567
                | Braille::Dots124567
                | Braille::Dots134567
                | Braille::Dots1234567
                | Braille::Dots18
                | Braille::Dots128
                | Braille::Dots138
                | Braille::Dots1238
                | Braille::Dots148
                | Braille::Dots1248
                | Braille::Dots1348
                | Braille::Dots12348
                | Braille::Dots158
                | Braille::Dots1258
                | Braille::Dots1358
                | Braille::Dots12358
                | Braille::Dots1458
                | Braille::Dots12458
                | Braille::Dots13458
                | Braille::Dots123458
                | Braille::Dots168
                | Braille::Dots1268
                | Braille::Dots1368
                | Braille::Dots12368
                | Braille::Dots1468
                | Braille::Dots12468
                | Braille::Dots13468
                | Braille::Dots123468
                | Braille::Dots1568
                | Braille::Dots12568
                | Braille::Dots13568
                | Braille::Dots123568
                | Braille::Dots14568
                | Braille::Dots124568
                | Braille::Dots134568
                | Braille::Dots1234568
                | Braille::Dots178
                | Braille::Dots1278
                | Braille::Dots1378
                | Braille::Dots12378
                | Braille::Dots1478
                | Braille::Dots12478
                | Braille::Dots13478
                | Braille::Dots123478
                | Braille::Dots1578
                | Braille::Dots12578
                | Braille::Dots13578
                | Braille::Dots123578
                | Braille::Dots14578
                | Braille::Dots124578
                | Braille::Dots134578
                | Braille::Dots1234578
                | Braille::Dots1678
                | Braille::Dots12678
                | Braille::Dots13678
                | Braille::Dots123678
                | Braille::Dots14678
                | Braille::Dots124678
                | Braille::Dots134678
                | Braille::Dots1234678
                | Braille::Dots15678
                | Braille::Dots125678
                | Braille::Dots12345678
                | Braille::Dots1235678
                | Braille::Dots135678
                | Braille::Dots1345678
                | Braille::Dots1245678
                | Braille::Dots145678
        ),
        2 => matches!(
            braille_dots,
            Braille::Dots2
                | Braille::Dots12
                | Braille::Dots23
                | Braille::Dots123
                | Braille::Dots24
                | Braille::Dots124
                | Braille::Dots234
                | Braille::Dots1234
                | Braille::Dots25
                | Braille::Dots125
                | Braille::Dots235
                | Braille::Dots1235
                | Braille::Dots245
                | Braille::Dots1245
                | Braille::Dots2345
                | Braille::Dots12345
                | Braille::Dots26
                | Braille::Dots126
                | Braille::Dots236
                | Braille::Dots1236
                | Braille::Dots246
                | Braille::Dots1246
                | Braille::Dots2346
                | Braille::Dots12346
                | Braille::Dots256
                | Braille::Dots1256
                | Braille::Dots2356
                | Braille::Dots12356
                | Braille::Dots2456
                | Braille::Dots12456
                | Braille::Dots23456
                | Braille::Dots123456
                | Braille::Dots27
                | Braille::Dots127
                | Braille::Dots237
                | Braille::Dots1237
                | Braille::Dots247
                | Braille::Dots1247
                | Braille::Dots2347
                | Braille::Dots12347
                | Braille::Dots257
                | Braille::Dots1257
                | Braille::Dots2357
                | Braille::Dots12357
                | Braille::Dots2457
                | Braille::Dots12457
                | Braille::Dots23457
                | Braille::Dots123457
                | Braille::Dots267
                | Braille::Dots1267
                | Braille::Dots2367
                | Braille::Dots12367
                | Braille::Dots2467
                | Braille::Dots12467
                | Braille::Dots23467
                | Braille::Dots123467
                | Braille::Dots2567
                | Braille::Dots12567
                | Braille::Dots23567
                | Braille::Dots123567
                | Braille::Dots24567
                | Braille::Dots124567
                | Braille::Dots234567
                | Braille::Dots1234567
                | Braille::Dots28
                | Braille::Dots128
                | Braille::Dots238
                | Braille::Dots1238
                | Braille::Dots248
                | Braille::Dots1248
                | Braille::Dots2348
                | Braille::Dots12348
                | Braille::Dots258
                | Braille::Dots1258
                | Braille::Dots2358
                | Braille::Dots12358
                | Braille::Dots2458
                | Braille::Dots12458
                | Braille::Dots23458
                | Braille::Dots123458
                | Braille::Dots268
                | Braille::Dots1268
                | Braille::Dots2368
                | Braille::Dots12368
                | Braille::Dots2468
                | Braille::Dots12468
                | Braille::Dots23468
                | Braille::Dots123468
                | Braille::Dots2568
                | Braille::Dots12568
                | Braille::Dots23568
                | Braille::Dots123568
                | Braille::Dots24568
                | Braille::Dots124568
                | Braille::Dots234568
                | Braille::Dots1234568
                | Braille::Dots278
                | Braille::Dots1278
                | Braille::Dots2378
                | Braille::Dots12378
                | Braille::Dots2478
                | Braille::Dots12478
                | Braille::Dots23478
                | Braille::Dots123478
                | Braille::Dots2578
                | Braille::Dots12578
                | Braille::Dots23578
                | Braille::Dots123578
                | Braille::Dots24578
                | Braille::Dots124578
                | Braille::Dots234578
                | Braille::Dots1234578
                | Braille::Dots2678
                | Braille::Dots12678
                | Braille::Dots23678
                | Braille::Dots123678
                | Braille::Dots24678
                | Braille::Dots124678
                | Braille::Dots234678
                | Braille::Dots1234678
                | Braille::Dots25678
                | Braille::Dots125678
                | Braille::Dots12345678
                | Braille::Dots235678
                | Braille::Dots2345678
                | Braille::Dots1235678
                | Braille::Dots1245678
                | Braille::Dots245678
        ),
        3 => matches!(
            braille_dots,
            Braille::Dots3
                | Braille::Dots13
                | Braille::Dots23
                | Braille::Dots123
                | Braille::Dots34
                | Braille::Dots134
                | Braille::Dots234
                | Braille::Dots1234
                | Braille::Dots35
                | Braille::Dots135
                | Braille::Dots235
                | Braille::Dots1235
                | Braille::Dots345
                | Braille::Dots1345
                | Braille::Dots2345
                | Braille::Dots12345
                | Braille::Dots36
                | Braille::Dots136
                | Braille::Dots236
                | Braille::Dots1236
                | Braille::Dots346
                | Braille::Dots1346
                | Braille::Dots2346
                | Braille::Dots12346
                | Braille::Dots356
                | Braille::Dots1356
                | Braille::Dots2356
                | Braille::Dots12356
                | Braille::Dots3456
                | Braille::Dots13456
                | Braille::Dots23456
                | Braille::Dots123456
                | Braille::Dots37
                | Braille::Dots137
                | Braille::Dots237
                | Braille::Dots1237
                | Braille::Dots347
                | Braille::Dots1347
                | Braille::Dots2347
                | Braille::Dots12347
                | Braille::Dots357
                | Braille::Dots1357
                | Braille::Dots2357
                | Braille::Dots12357
                | Braille::Dots3457
                | Braille::Dots13457
                | Braille::Dots23457
                | Braille::Dots123457
                | Braille::Dots367
                | Braille::Dots1367
                | Braille::Dots2367
                | Braille::Dots12367
                | Braille::Dots3467
                | Braille::Dots13467
                | Braille::Dots23467
                | Braille::Dots123467
                | Braille::Dots3567
                | Braille::Dots13567
                | Braille::Dots23567
                | Braille::Dots123567
                | Braille::Dots34567
                | Braille::Dots134567
                | Braille::Dots234567
                | Braille::Dots1234567
                | Braille::Dots38
                | Braille::Dots138
                | Braille::Dots238
                | Braille::Dots1238
                | Braille::Dots348
                | Braille::Dots1348
                | Braille::Dots2348
                | Braille::Dots12348
                | Braille::Dots358
                | Braille::Dots1358
                | Braille::Dots2358
                | Braille::Dots12358
                | Braille::Dots3458
                | Braille::Dots13458
                | Braille::Dots23458
                | Braille::Dots123458
                | Braille::Dots368
                | Braille::Dots1368
                | Braille::Dots2368
                | Braille::Dots12368
                | Braille::Dots3468
                | Braille::Dots13468
                | Braille::Dots23468
                | Braille::Dots123468
                | Braille::Dots3568
                | Braille::Dots13568
                | Braille::Dots23568
                | Braille::Dots123568
                | Braille::Dots34568
                | Braille::Dots134568
                | Braille::Dots234568
                | Braille::Dots1234568
                | Braille::Dots378
                | Braille::Dots1378
                | Braille::Dots2378
                | Braille::Dots12378
                | Braille::Dots3478
                | Braille::Dots13478
                | Braille::Dots23478
                | Braille::Dots123478
                | Braille::Dots3578
                | Braille::Dots13578
                | Braille::Dots23578
                | Braille::Dots123578
                | Braille::Dots34578
                | Braille::Dots134578
                | Braille::Dots234578
                | Braille::Dots1234578
                | Braille::Dots3678
                | Braille::Dots13678
                | Braille::Dots23678
                | Braille::Dots123678
                | Braille::Dots34678
                | Braille::Dots134678
                | Braille::Dots234678
                | Braille::Dots1234678
                | Braille::Dots12345678
                | Braille::Dots235678
                | Braille::Dots35678
                | Braille::Dots345678
                | Braille::Dots2345678
                | Braille::Dots1235678
                | Braille::Dots135678
                | Braille::Dots1345678
        ),
        4 => matches!(
            braille_dots,
            Braille::Dots4
                | Braille::Dots14
                | Braille::Dots24
                | Braille::Dots124
                | Braille::Dots34
                | Braille::Dots134
                | Braille::Dots234
                | Braille::Dots1234
                | Braille::Dots45
                | Braille::Dots145
                | Braille::Dots245
                | Braille::Dots1245
                | Braille::Dots345
                | Braille::Dots1345
                | Braille::Dots2345
                | Braille::Dots12345
                | Braille::Dots46
                | Braille::Dots146
                | Braille::Dots246
                | Braille::Dots1246
                | Braille::Dots346
                | Braille::Dots1346
                | Braille::Dots2346
                | Braille::Dots12346
                | Braille::Dots456
                | Braille::Dots1456
                | Braille::Dots2456
                | Braille::Dots12456
                | Braille::Dots3456
                | Braille::Dots13456
                | Braille::Dots23456
                | Braille::Dots123456
                | Braille::Dots47
                | Braille::Dots147
                | Braille::Dots247
                | Braille::Dots1247
                | Braille::Dots347
                | Braille::Dots1347
                | Braille::Dots2347
                | Braille::Dots12347
                | Braille::Dots457
                | Braille::Dots1457
                | Braille::Dots2457
                | Braille::Dots12457
                | Braille::Dots3457
                | Braille::Dots13457
                | Braille::Dots23457
                | Braille::Dots123457
                | Braille::Dots467
                | Braille::Dots1467
                | Braille::Dots2467
                | Braille::Dots12467
                | Braille::Dots3467
                | Braille::Dots13467
                | Braille::Dots23467
                | Braille::Dots123467
                | Braille::Dots4567
                | Braille::Dots14567
                | Braille::Dots24567
                | Braille::Dots124567
                | Braille::Dots34567
                | Braille::Dots134567
                | Braille::Dots234567
                | Braille::Dots1234567
                | Braille::Dots48
                | Braille::Dots148
                | Braille::Dots248
                | Braille::Dots1248
                | Braille::Dots348
                | Braille::Dots1348
                | Braille::Dots2348
                | Braille::Dots12348
                | Braille::Dots458
                | Braille::Dots1458
                | Braille::Dots2458
                | Braille::Dots12458
                | Braille::Dots3458
                | Braille::Dots13458
                | Braille::Dots23458
                | Braille::Dots123458
                | Braille::Dots468
                | Braille::Dots1468
                | Braille::Dots2468
                | Braille::Dots12468
                | Braille::Dots3468
                | Braille::Dots13468
                | Braille::Dots23468
                | Braille::Dots123468
                | Braille::Dots4568
                | Braille::Dots14568
                | Braille::Dots24568
                | Braille::Dots124568
                | Braille::Dots34568
                | Braille::Dots134568
                | Braille::Dots234568
                | Braille::Dots1234568
                | Braille::Dots478
                | Braille::Dots1478
                | Braille::Dots2478
                | Braille::Dots12478
                | Braille::Dots3478
                | Braille::Dots13478
                | Braille::Dots23478
                | Braille::Dots123478
                | Braille::Dots4578
                | Braille::Dots14578
                | Braille::Dots24578
                | Braille::Dots124578
                | Braille::Dots34578
                | Braille::Dots134578
                | Braille::Dots234578
                | Braille::Dots1234578
                | Braille::Dots4678
                | Braille::Dots14678
                | Braille::Dots24678
                | Braille::Dots124678
                | Braille::Dots34678
                | Braille::Dots134678
                | Braille::Dots234678
                | Braille::Dots1234678
                | Braille::Dots12345678
                | Braille::Dots45678
                | Braille::Dots345678
                | Braille::Dots2345678
                | Braille::Dots1345678
                | Braille::Dots1245678
                | Braille::Dots145678
                | Braille::Dots245678
        ),
        5 => matches!(
            braille_dots,
            Braille::Dots5
                    | Braille::Dots15
                    | Braille::Dots25
                    | Braille::Dots125
                    | Braille::Dots35
                    | Braille::Dots135
                    | Braille::Dots235
                    | Braille::Dots1235
                    | Braille::Dots45
                    | Braille::Dots145
                    | Braille::Dots245
                    | Braille::Dots1245
                    | Braille::Dots345
                    | Braille::Dots1345
                    | Braille::Dots2345
                    | Braille::Dots12345
                    | Braille::Dots56
                    | Braille::Dots156
                    | Braille::Dots256
                    | Braille::Dots1256
                    | Braille::Dots356
                    | Braille::Dots1356
                    | Braille::Dots2356
                    | Braille::Dots12356
                    | Braille::Dots456
                    | Braille::Dots1456
                    | Braille::Dots2456
                    | Braille::Dots12456
                    | Braille::Dots3456
                    | Braille::Dots13456
                    | Braille::Dots23456
                    | Braille::Dots123456
                    | Braille::Dots57
                    | Braille::Dots157
                    | Braille::Dots257
                    | Braille::Dots1257
                    | Braille::Dots357
                    | Braille::Dots1357
                    | Braille::Dots2357
                    | Braille::Dots12357
                    | Braille::Dots457
                    | Braille::Dots1457
                    | Braille::Dots2457
                    | Braille::Dots12457
                    | Braille::Dots3457
                    | Braille::Dots13457
                    | Braille::Dots23457
                    | Braille::Dots123457
                    | Braille::Dots567
                    | Braille::Dots1567
                    | Braille::Dots2567
                    | Braille::Dots12567
                    | Braille::Dots3567
                    | Braille::Dots13567
                    | Braille::Dots23567
                    | Braille::Dots123567
                    | Braille::Dots4567
                    | Braille::Dots14567
                    | Braille::Dots24567
                    | Braille::Dots124567
                    | Braille::Dots34567
                    | Braille::Dots134567
                    | Braille::Dots234567
                    | Braille::Dots1234567
                    | Braille::Dots58
                    | Braille::Dots158
                    | Braille::Dots258
                    | Braille::Dots1258
                    | Braille::Dots358
                    | Braille::Dots1358
                    | Braille::Dots2358
                    | Braille::Dots12358
                    | Braille::Dots458
                    | Braille::Dots1458
                    | Braille::Dots2458
                    | Braille::Dots12458
                    | Braille::Dots3458
                    | Braille::Dots13458
                    | Braille::Dots23458
                    | Braille::Dots123458
                    | Braille::Dots568
                    | Braille::Dots1568
                    | Braille::Dots2568
                    | Braille::Dots12568
                    | Braille::Dots3568
                    | Braille::Dots13568
                    | Braille::Dots23568
                    | Braille::Dots123568
                    | Braille::Dots4568
                    | Braille::Dots14568
                    | Braille::Dots24568
                    | Braille::Dots124568
                    | Braille::Dots34568
                    | Braille::Dots134568
                    | Braille::Dots234568
                    | Braille::Dots1234568
                    | Braille::Dots578
                    | Braille::Dots1578
                    | Braille::Dots2578
                    | Braille::Dots12578
                    | Braille::Dots3578
                    | Braille::Dots13578
                    | Braille::Dots23578
                    | Braille::Dots123578
                    | Braille::Dots4578
                    | Braille::Dots14578
                    | Braille::Dots24578
                    | Braille::Dots124578
                    | Braille::Dots34578
                    | Braille::Dots134578
                    | Braille::Dots234578
                    | Braille::Dots1234578
                    | Braille::Dots5678
                    | Braille::Dots15678
                    | Braille::Dots25678
                    | Braille::Dots125678
                    | Braille::Dots35678
                    | Braille::Dots135678
                    | Braille::Dots1235678
                    | Braille::Dots45678
                    | Braille::Dots145678
                    | Braille::Dots245678
                    | Braille::Dots1245678
                    | Braille::Dots345678
                    | Braille::Dots1345678
                    | Braille::Dots12345678
                    | Braille::Dots235678
                    | Braille::Dots2345678
        ),
        6 => matches!(
            braille_dots,
            Braille::Dots6
                    | Braille::Dots16
                    | Braille::Dots26
                    | Braille::Dots126
                    | Braille::Dots36
                    | Braille::Dots136
                    | Braille::Dots236
                    | Braille::Dots1236
                    | Braille::Dots46
                    | Braille::Dots146
                    | Braille::Dots246
                    | Braille::Dots1246
                    | Braille::Dots346
                    | Braille::Dots1346
                    | Braille::Dots2346
                    | Braille::Dots12346
                    | Braille::Dots56
                    | Braille::Dots156
                    | Braille::Dots256
                    | Braille::Dots1256
                    | Braille::Dots356
                    | Braille::Dots1356
                    | Braille::Dots2356
                    | Braille::Dots12356
                    | Braille::Dots456
                    | Braille::Dots1456
                    | Braille::Dots2456
                    | Braille::Dots12456
                    | Braille::Dots3456
                    | Braille::Dots13456
                    | Braille::Dots23456
                    | Braille::Dots123456
                    | Braille::Dots67
                    | Braille::Dots167
                    | Braille::Dots267
                    | Braille::Dots1267
                    | Braille::Dots367
                    | Braille::Dots1367
                    | Braille::Dots2367
                    | Braille::Dots12367
                    | Braille::Dots467
                    | Braille::Dots1467
                    | Braille::Dots2467
                    | Braille::Dots12467
                    | Braille::Dots3467
                    | Braille::Dots13467
                    | Braille::Dots23467
                    | Braille::Dots123467
                    | Braille::Dots567
                    | Braille::Dots1567
                    | Braille::Dots2567
                    | Braille::Dots12567
                    | Braille::Dots3567
                    | Braille::Dots13567
                    | Braille::Dots23567
                    | Braille::Dots123567
                    | Braille::Dots4567
                    | Braille::Dots14567
                    | Braille::Dots24567
                    | Braille::Dots124567
                    | Braille::Dots34567
                    | Braille::Dots134567
                    | Braille::Dots234567
                    | Braille::Dots1234567
                    | Braille::Dots68
                    | Braille::Dots168
                    | Braille::Dots268
                    | Braille::Dots1268
                    | Braille::Dots368
                    | Braille::Dots1368
                    | Braille::Dots2368
                    | Braille::Dots12368
                    | Braille::Dots468
                    | Braille::Dots1468
                    | Braille::Dots2468
                    | Braille::Dots12468
                    | Braille::Dots3468
                    | Braille::Dots13468
                    | Braille::Dots23468
                    | Braille::Dots123468
                    | Braille::Dots568
                    | Braille::Dots1568
                    | Braille::Dots2568
                    | Braille::Dots12568
                    | Braille::Dots3568
                    | Braille::Dots13568
                    | Braille::Dots23568
                    | Braille::Dots123568
                    | Braille::Dots4568
                    | Braille::Dots14568
                    | Braille::Dots24568
                    | Braille::Dots124568
                    | Braille::Dots34568
                    | Braille::Dots134568
                    | Braille::Dots234568
                    | Braille::Dots1234568
                    | Braille::Dots678
                    | Braille::Dots1678
                    | Braille::Dots2678
                    | Braille::Dots12678
                    | Braille::Dots3678
                    | Braille::Dots13678
                    | Braille::Dots23678
                    | Braille::Dots123678
                    | Braille::Dots4678
                    | Braille::Dots14678
                    | Braille::Dots24678
                    | Braille::Dots124678
                    | Braille::Dots34678
                    | Braille::Dots134678
                    | Braille::Dots234678
                    | Braille::Dots1234678
                    | Braille::Dots5678
                    | Braille::Dots15678
                    | Braille::Dots25678
                    | Braille::Dots125678
                    | Braille::Dots35678
                    | Braille::Dots135678
                    | Braille::Dots1235678
                    | Braille::Dots45678
                    | Braille::Dots145678
                    | Braille::Dots245678
                    | Braille::Dots1245678
                    | Braille::Dots345678
                    | Braille::Dots1345678
                    | Braille::Dots2345678
                    | Braille::Dots235678
                    | Braille::Dots12345678
        ),
        7 => matches!(
            braille_dots,
            Braille::Dots7
                    | Braille::Dots17
                    | Braille::Dots27
                    | Braille::Dots127
                    | Braille::Dots37
                    | Braille::Dots137
                    | Braille::Dots237
                    | Braille::Dots1237
                    | Braille::Dots47
                    | Braille::Dots147
                    | Braille::Dots247
                    | Braille::Dots1247
                    | Braille::Dots347
                    | Braille::Dots1347
                    | Braille::Dots2347
                    | Braille::Dots12347
                    | Braille::Dots57
                    | Braille::Dots157
                    | Braille::Dots257
                    | Braille::Dots1257
                    | Braille::Dots357
                    | Braille::Dots1357
                    | Braille::Dots2357
                    | Braille::Dots12357
                    | Braille::Dots457
                    | Braille::Dots1457
                    | Braille::Dots2457
                    | Braille::Dots12457
                    | Braille::Dots3457
                    | Braille::Dots13457
                    | Braille::Dots23457
                    | Braille::Dots123457
                    | Braille::Dots67
                    | Braille::Dots167
                    | Braille::Dots267
                    | Braille::Dots1267
                    | Braille::Dots367
                    | Braille::Dots1367
                    | Braille::Dots2367
                    | Braille::Dots12367
                    | Braille::Dots467
                    | Braille::Dots1467
                    | Braille::Dots2467
                    | Braille::Dots12467
                    | Braille::Dots3467
                    | Braille::Dots13467
                    | Braille::Dots23467
                    | Braille::Dots123467
                    | Braille::Dots567
                    | Braille::Dots1567
                    | Braille::Dots2567
                    | Braille::Dots12567
                    | Braille::Dots3567
                    | Braille::Dots13567
                    | Braille::Dots23567
                    | Braille::Dots123567
                    | Braille::Dots4567
                    | Braille::Dots14567
                    | Braille::Dots24567
                    | Braille::Dots124567
                    | Braille::Dots34567
                    | Braille::Dots134567
                    | Braille::Dots234567
                    | Braille::Dots1234567
                    | Braille::Dots78
                    | Braille::Dots178
                    | Braille::Dots278
                    | Braille::Dots1278
                    | Braille::Dots378
                    | Braille::Dots1378
                    | Braille::Dots2378
                    | Braille::Dots12378
                    | Braille::Dots478
                    | Braille::Dots1478
                    | Braille::Dots2478
                    | Braille::Dots12478
                    | Braille::Dots3478
                    | Braille::Dots13478
                    | Braille::Dots23478
                    | Braille::Dots123478
                    | Braille::Dots578
                    | Braille::Dots1578
                    | Braille::Dots2578
                    | Braille::Dots12578
                    | Braille::Dots3578
                    | Braille::Dots13578
                    | Braille::Dots23578
                    | Braille::Dots123578
                    | Braille::Dots4578
                    | Braille::Dots14578
                    | Braille::Dots24578
                    | Braille::Dots124578
                    | Braille::Dots34578
                    | Braille::Dots134578
                    | Braille::Dots234578
                    | Braille::Dots1234578
                    | Braille::Dots678
                    | Braille::Dots1678
                    | Braille::Dots2678
                    | Braille::Dots12678
                    | Braille::Dots3678
                    | Braille::Dots13678
                    | Braille::Dots23678
                    | Braille::Dots123678
                    | Braille::Dots4678
                    | Braille::Dots14678
                    | Braille::Dots24678
                    | Braille::Dots124678
                    | Braille::Dots34678
                    | Braille::Dots134678
                    | Braille::Dots234678
                    | Braille::Dots1234678
                    | Braille::Dots5678
                    | Braille::Dots15678
                    | Braille::Dots25678
                    | Braille::Dots125678
                    | Braille::Dots35678
                    | Braille::Dots135678
                    | Braille::Dots235678
                    | Braille::Dots1235678
                    | Braille::Dots45678
                    | Braille::Dots145678
                    | Braille::Dots245678
                    | Braille::Dots1245678
                    | Braille::Dots345678
                    | Braille::Dots1345678
                    | Braille::Dots2345678
                    | Braille::Dots12345678
        ),
        8 => matches!(
            braille_dots,
            Braille::Dots8
                    | Braille::Dots18
                    | Braille::Dots28
                    | Braille::Dots128
                    | Braille::Dots38
                    | Braille::Dots138
                    | Braille::Dots238
                    | Braille::Dots1238
                    | Braille::Dots48
                    | Braille::Dots148
                    | Braille::Dots248
                    | Braille::Dots1248
                    | Braille::Dots348
                    | Braille::Dots1348
                    | Braille::Dots2348
                    | Braille::Dots12348
                    | Braille::Dots58
                    | Braille::Dots158
                    | Braille::Dots258
                    | Braille::Dots1258
                    | Braille::Dots358
                    | Braille::Dots1358
                    | Braille::Dots2358
                    | Braille::Dots12358
                    | Braille::Dots458
                    | Braille::Dots1458
                    | Braille::Dots2458
                    | Braille::Dots12458
                    | Braille::Dots3458
                    | Braille::Dots13458
                    | Braille::Dots23458
                    | Braille::Dots123458
                    | Braille::Dots68
                    | Braille::Dots168
                    | Braille::Dots268
                    | Braille::Dots1268
                    | Braille::Dots368
                    | Braille::Dots1368
                    | Braille::Dots2368
                    | Braille::Dots12368
                    | Braille::Dots468
                    | Braille::Dots1468
                    | Braille::Dots2468
                    | Braille::Dots12468
                    | Braille::Dots3468
                    | Braille::Dots13468
                    | Braille::Dots23468
                    | Braille::Dots123468
                    | Braille::Dots568
                    | Braille::Dots1568
                    | Braille::Dots2568
                    | Braille::Dots12568
                    | Braille::Dots3568
                    | Braille::Dots13568
                    | Braille::Dots23568
                    | Braille::Dots123568
                    | Braille::Dots4568
                    | Braille::Dots14568
                    | Braille::Dots24568
                    | Braille::Dots124568
                    | Braille::Dots34568
                    | Braille::Dots134568
                    | Braille::Dots234568
                    | Braille::Dots1234568
                    | Braille::Dots78
                    | Braille::Dots178
                    | Braille::Dots278
                    | Braille::Dots1278
                    | Braille::Dots378
                    | Braille::Dots1378
                    | Braille::Dots2378
                    | Braille::Dots12378
                    | Braille::Dots478
                    | Braille::Dots1478
                    | Braille::Dots2478
                    | Braille::Dots12478
                    | Braille::Dots3478
                    | Braille::Dots13478
                    | Braille::Dots23478
                    | Braille::Dots123478
                    | Braille::Dots578
                    | Braille::Dots1578
                    | Braille::Dots2578
                    | Braille::Dots12578
                    | Braille::Dots3578
                    | Braille::Dots13578
                    | Braille::Dots23578
                    | Braille::Dots123578
                    | Braille::Dots4578
                    | Braille::Dots14578
                    | Braille::Dots24578
                    | Braille::Dots124578
                    | Braille::Dots34578
                    | Braille::Dots134578
                    | Braille::Dots234578
                    | Braille::Dots1234578
                    | Braille::Dots678
                    | Braille::Dots1678
                    | Braille::Dots2678
                    | Braille::Dots12678
                    | Braille::Dots3678
                    | Braille::Dots13678
                    | Braille::Dots23678
                    | Braille::Dots123678
                    | Braille::Dots4678
                    | Braille::Dots14678
                    | Braille::Dots24678
                    | Braille::Dots124678
                    | Braille::Dots34678
                    | Braille::Dots134678
                    | Braille::Dots234678
                    | Braille::Dots1234678
                    | Braille::Dots5678
                    | Braille::Dots15678
                    | Braille::Dots25678
                    | Braille::Dots125678
                    | Braille::Dots35678
                    | Braille::Dots135678
                    | Braille::Dots235678
                    | Braille::Dots1235678
                    | Braille::Dots45678
                    | Braille::Dots145678
                    | Braille::Dots245678
                    | Braille::Dots1245678
                    | Braille::Dots345678
                    | Braille::Dots1345678
                    | Braille::Dots2345678
                    | Braille::Dots12345678
        ),
        _ => false,
    }
}

const SEXTANT_PATTERNS: [u8; 64] = [
    0b000000,
    0b000001,
    0b000010,
    0b000011,
    0b000100,
    0b000101,
    0b000110,
    0b000111,
    0b001000,
    0b001001,
    0b001010,
    0b001011,
    0b001100,
    0b001101,
    0b001110,
    0b001111,
    0b010000,
    0b010001,
    0b010010,
    0b010011,
    0b010100,
    0b010101,
    0b010110,
    0b010111,
    0b011000,
    0b011001,
    0b011010,
    0b011011,
    0b011100,
    0b011101,
    0b011110,
    0b011111,
    0b100000,
    0b100001,
    0b100010,
    0b100011,
    0b100100,
    0b100101,
    0b100110,
    0b100111,
    0b101000,
    0b101001,
    0b101010,
    0b101011,
    0b101100,
    0b101101,
    0b101110,
    0b101111,
    0b110000,
    0b110001,
    0b110010,
    0b110011,
    0b110100,
    0b110101,
    0b110110,
    0b110111,
    0b111000,
    0b111001,
    0b111010,
    0b111011,
    0b111100,
    0b111101,
    0b111110,
    0b111111,
];

const OCTANT_PATTERNS: [u8; 230] = [
    0b00000100,
    0b00000110,
    0b00000111,
    0b00001000,
    0b00001001,
    0b00001011,
    0b00001100,
    0b00001101,
    0b00001110,
    0b00010000,
    0b00010001,
    0b00010010,
    0b00010011,
    0b00010101,
    0b00010110,
    0b00010111,
    0b00011000,
    0b00011001,
    0b00011010,
    0b00011011,
    0b00011100,
    0b00011101,
    0b00011110,
    0b00011111,
    0b00100000,
    0b00100001,
    0b00100010,
    0b00100011,
    0b00100100,
    0b00100101,
    0b00100110,
    0b00100111,
    0b00101001,
    0b00101010,
    0b00101011,
    0b00101100,
    0b00101101,
    0b00101110,
    0b00101111,
    0b00110000,
    0b00110001,
    0b00110010,
    0b00110011,
    0b00110100,
    0b00110101,
    0b00110110,
    0b00110111,
    0b00111000,
    0b00111001,
    0b00111010,
    0b00111011,
    0b00111100,
    0b00111101,
    0b00111110,
    0b01000001,
    0b01000010,
    0b01000011,
    0b01000100,
    0b01000101,
    0b01000110,
    0b01000111,
    0b01001000,
    0b01001001,
    0b01001010,
    0b01001011,
    0b01001100,
    0b01001101,
    0b01001110,
    0b01001111,
    0b01010001,
    0b01010010,
    0b01010011,
    0b01010100,
    0b01010110,
    0b01010111,
    0b01011000,
    0b01011001,
    0b01011011,
    0b01011100,
    0b01011101,
    0b01011110,
    0b01100000,
    0b01100001,
    0b01100010,
    0b01100011,
    0b01100100,
    0b01100101,
    0b01100110,
    0b01100111,
    0b01101000,
    0b01101001,
    0b01101010,
    0b01101011,
    0b01101100,
    0b01101101,
    0b01101110,
    0b01101111,
    0b01110000,
    0b01110001,
    0b01110010,
    0b01110011,
    0b01110100,
    0b01110101,
    0b01110110,
    0b01110111,
    0b01111000,
    0b01111001,
    0b01111010,
    0b01111011,
    0b01111100,
    0b01111101,
    0b01111110,
    0b01111111,
    0b10000001,
    0b10000010,
    0b10000011,
    0b10000100,
    0b10000101,
    0b10000110,
    0b10000111,
    0b10001000,
    0b10001001,
    0b10001010,
    0b10001011,
    0b10001100,
    0b10001101,
    0b10001110,
    0b10001111,
    0b10010000,
    0b10010001,
    0b10010010,
    0b10010011,
    0b10010100,
    0b10010101,
    0b10010110,
    0b10010111,
    0b10011000,
    0b10011001,
    0b10011010,
    0b10011011,
    0b10011100,
    0b10011101,
    0b10011110,
    0b10011111,
    0b10100001,
    0b10100010,
    0b10100011,
    0b10100100,
    0b10100110,
    0b10100111,
    0b10101000,
    0b10101001,
    0b10101011,
    0b10101100,
    0b10101101,
    0b10101110,
    0b10110000,
    0b10110001,
    0b10110010,
    0b10110011,
    0b10110100,
    0b10110101,
    0b10110110,
    0b10110111,
    0b10111000,
    0b10111001,
    0b10111010,
    0b10111011,
    0b10111100,
    0b10111101,
    0b10111110,
    0b10111111,
    0b11000001,
    0b11000010,
    0b11000011,
    0b11000100,
    0b11000101,
    0b11000110,
    0b11000111,
    0b11001000,
    0b11001001,
    0b11001010,
    0b11001011,
    0b11001100,
    0b11001101,
    0b11001110,
    0b11001111,
    0b11010000,
    0b11010001,
    0b11010010,
    0b11010011,
    0b11010100,
    0b11010101,
    0b11010110,
    0b11010111,
    0b11011000,
    0b11011001,
    0b11011010,
    0b11011011,
    0b11011100,
    0b11011101,
    0b11011110,
    0b11011111,
    0b11100000,
    0b11100001,
    0b11100010,
    0b11100011,
    0b11100100,
    0b11100101,
    0b11100110,
    0b11100111,
    0b11101000,
    0b11101001,
    0b11101010,
    0b11101011,
    0b11101100,
    0b11101101,
    0b11101110,
    0b11101111,
    0b11110001,
    0b11110010,
    0b11110011,
    0b11110100,
    0b11110110,
    0b11110111,
    0b11111000,
    0b11111001,
    0b11111011,
    0b11111101,
    0b11111110,
];
