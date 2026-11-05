use super::primitives::*;
use tiny_skia::Pixmap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Weight {
    None,
    Light,
    Heavy,
    Double,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Lines {
    pub up: Weight,
    pub down: Weight,
    pub left: Weight,
    pub right: Weight,
}

/// The four arm weights of a straight or branching box-drawing character.
pub(super) fn lines_of(c: char) -> Option<Lines> {
    let code: &[u8; 4] = match c {
        '─' => b"NNLL",
        '━' => b"NNHH",
        '│' => b"LLNN",
        '┃' => b"HHNN",
        '┌' => b"NLNL",
        '┍' => b"NLNH",
        '┎' => b"NHNL",
        '┏' => b"NHNH",
        '┐' => b"NLLN",
        '┑' => b"NLHN",
        '┒' => b"NHLN",
        '┓' => b"NHHN",
        '└' => b"LNNL",
        '┕' => b"LNNH",
        '┖' => b"HNNL",
        '┗' => b"HNNH",
        '┘' => b"LNLN",
        '┙' => b"LNHN",
        '┚' => b"HNLN",
        '┛' => b"HNHN",
        '├' => b"LLNL",
        '┝' => b"LLNH",
        '┞' => b"HLNL",
        '┟' => b"LHNL",
        '┠' => b"HHNL",
        '┡' => b"HLNH",
        '┢' => b"LHNH",
        '┣' => b"HHNH",
        '┤' => b"LLLN",
        '┥' => b"LLHN",
        '┦' => b"HLLN",
        '┧' => b"LHLN",
        '┨' => b"HHLN",
        '┩' => b"HLHN",
        '┪' => b"LHHN",
        '┫' => b"HHHN",
        '┬' => b"NLLL",
        '┭' => b"NLHL",
        '┮' => b"NLLH",
        '┯' => b"NLHH",
        '┰' => b"NHLL",
        '┱' => b"NHHL",
        '┲' => b"NHLH",
        '┳' => b"NHHH",
        '┴' => b"LNLL",
        '┵' => b"LNHL",
        '┶' => b"LNLH",
        '┷' => b"LNHH",
        '┸' => b"HNLL",
        '┹' => b"HNHL",
        '┺' => b"HNLH",
        '┻' => b"HNHH",
        '┼' => b"LLLL",
        '┽' => b"LLHL",
        '┾' => b"LLLH",
        '┿' => b"LLHH",
        '╀' => b"HLLL",
        '╁' => b"LHLL",
        '╂' => b"HHLL",
        '╃' => b"HLHL",
        '╄' => b"HLLH",
        '╅' => b"LHHL",
        '╆' => b"LHLH",
        '╇' => b"HLHH",
        '╈' => b"LHHH",
        '╉' => b"HHHL",
        '╊' => b"HHLH",
        '╋' => b"HHHH",
        '═' => b"NNDD",
        '║' => b"DDNN",
        '╒' => b"NLND",
        '╓' => b"NDNL",
        '╔' => b"NDND",
        '╕' => b"NLDN",
        '╖' => b"NDLN",
        '╗' => b"NDDN",
        '╘' => b"LNND",
        '╙' => b"DNNL",
        '╚' => b"DNND",
        '╛' => b"LNDN",
        '╜' => b"DNLN",
        '╝' => b"DNDN",
        '╞' => b"LLND",
        '╟' => b"DDNL",
        '╠' => b"DDND",
        '╡' => b"LLDN",
        '╢' => b"DDLN",
        '╣' => b"DDDN",
        '╤' => b"NLDD",
        '╥' => b"NDLL",
        '╦' => b"NDDD",
        '╧' => b"LNDD",
        '╨' => b"DNLL",
        '╩' => b"DNDD",
        '╪' => b"LLDD",
        '╫' => b"DDLL",
        '╬' => b"DDDD",
        '╴' => b"NNLN",
        '╵' => b"LNNN",
        '╶' => b"NNNL",
        '╷' => b"NLNN",
        '╸' => b"NNHN",
        '╹' => b"HNNN",
        '╺' => b"NNNH",
        '╻' => b"NHNN",
        '╼' => b"NNLH",
        '╽' => b"LHNN",
        '╾' => b"NNHL",
        '╿' => b"HLNN",
        _ => return None,
    };
    let weight = |b: u8| match b {
        b'L' => Weight::Light,
        b'H' => Weight::Heavy,
        b'D' => Weight::Double,
        _ => Weight::None,
    };
    Some(Lines {
        up: weight(code[0]),
        down: weight(code[1]),
        left: weight(code[2]),
        right: weight(code[3]),
    })
}

/// Pixel spans every box-drawing glyph of one cell size shares.
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(crate) struct LineMetrics {
    pub(crate) light: u32,
    pub(crate) heavy: u32,
    pub(crate) double: u32,
    pub(crate) h_light_top: u32,
    pub(crate) h_light_bottom: u32,
    pub(crate) h_heavy_top: u32,
    pub(crate) h_heavy_bottom: u32,
    pub(crate) h_double_top: u32,
    pub(crate) h_double_bottom: u32,
    pub(crate) v_light_left: u32,
    pub(crate) v_light_right: u32,
    pub(crate) v_heavy_left: u32,
    pub(crate) v_heavy_right: u32,
    pub(crate) v_double_left: u32,
    pub(crate) v_double_right: u32,
}

impl LineMetrics {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let light = stroke_width(height) as u32;
        let heavy = 2 * light;
        let double = ((0.6 * light as f32).round() as u32).max(1);
        let h_light_top = height.saturating_sub(light) / 2;
        let h_light_bottom = h_light_top + light;
        let h_heavy_top = height.saturating_sub(heavy) / 2;
        let v_light_left = width.saturating_sub(light) / 2;
        let v_light_right = v_light_left + light;
        let v_heavy_left = width.saturating_sub(heavy) / 2;
        Self {
            light,
            heavy,
            double,
            h_light_top,
            h_light_bottom,
            h_heavy_top,
            h_heavy_bottom: h_heavy_top + heavy,
            h_double_top: h_light_top.saturating_sub(double),
            h_double_bottom: h_light_bottom + double,
            v_light_left,
            v_light_right,
            v_heavy_left,
            v_heavy_right: v_heavy_left + heavy,
            v_double_left: v_light_left.saturating_sub(double),
            v_double_right: v_light_right + double,
        }
    }
}

fn fill(pixmap: &mut Pixmap, x0: u32, y0: u32, x1: u32, y1: u32) {
    if x1 > x0 && y1 > y0 {
        draw_rect(
            pixmap,
            x0 as f32,
            y0 as f32,
            (x1 - x0) as f32,
            (y1 - y0) as f32,
            default_color(),
        );
    }
}

/// Draws the four arms of `lines`, a port of Ghostty's `linesChar`.
pub(super) fn draw_lines(pixmap: &mut Pixmap, m: &LineMetrics, lines: Lines) {
    use Weight::{Double, Heavy, None as No};
    let (w, h) = (pixmap.width(), pixmap.height());
    let Lines {
        up,
        down,
        left,
        right,
    } = lines;

    let up_bottom = if left == Heavy || right == Heavy {
        m.h_heavy_bottom
    } else if left != right || down == up {
        if left == Double || right == Double {
            m.h_double_bottom
        } else {
            m.h_light_bottom
        }
    } else if left == No && right == No {
        m.h_light_bottom
    } else {
        m.h_light_top
    };
    let down_top = if left == Heavy || right == Heavy {
        m.h_heavy_top
    } else if left != right || up == down {
        if left == Double || right == Double {
            m.h_double_top
        } else {
            m.h_light_top
        }
    } else if left == No && right == No {
        m.h_light_top
    } else {
        m.h_light_bottom
    };
    let left_right = if up == Heavy || down == Heavy {
        m.v_heavy_right
    } else if up != down || left == right {
        if up == Double || down == Double {
            m.v_double_right
        } else {
            m.v_light_right
        }
    } else if up == No && down == No {
        m.v_light_right
    } else {
        m.v_light_left
    };
    let right_left = if up == Heavy || down == Heavy {
        m.v_heavy_left
    } else if up != down || right == left {
        if up == Double || down == Double {
            m.v_double_left
        } else {
            m.v_light_left
        }
    } else if up == No && down == No {
        m.v_light_left
    } else {
        m.v_light_right
    };

    match up {
        Weight::None => {}
        Weight::Light => fill(pixmap, m.v_light_left, 0, m.v_light_right, up_bottom),
        Heavy => fill(pixmap, m.v_heavy_left, 0, m.v_heavy_right, up_bottom),
        Double => {
            let l = if left == Double {
                m.h_light_top
            } else {
                up_bottom
            };
            let r = if right == Double {
                m.h_light_top
            } else {
                up_bottom
            };
            fill(pixmap, m.v_double_left, 0, m.v_light_left, l);
            fill(pixmap, m.v_light_right, 0, m.v_double_right, r);
        }
    }
    match right {
        Weight::None => {}
        Weight::Light => fill(pixmap, right_left, m.h_light_top, w, m.h_light_bottom),
        Heavy => fill(pixmap, right_left, m.h_heavy_top, w, m.h_heavy_bottom),
        Double => {
            let t = if up == Double {
                m.v_light_right
            } else {
                right_left
            };
            let b = if down == Double {
                m.v_light_right
            } else {
                right_left
            };
            fill(pixmap, t, m.h_double_top, w, m.h_light_top);
            fill(pixmap, b, m.h_light_bottom, w, m.h_double_bottom);
        }
    }
    match down {
        Weight::None => {}
        Weight::Light => fill(pixmap, m.v_light_left, down_top, m.v_light_right, h),
        Heavy => fill(pixmap, m.v_heavy_left, down_top, m.v_heavy_right, h),
        Double => {
            let l = if left == Double {
                m.h_light_bottom
            } else {
                down_top
            };
            let r = if right == Double {
                m.h_light_bottom
            } else {
                down_top
            };
            fill(pixmap, m.v_double_left, l, m.v_light_left, h);
            fill(pixmap, m.v_light_right, r, m.v_double_right, h);
        }
    }
    match left {
        Weight::None => {}
        Weight::Light => fill(pixmap, 0, m.h_light_top, left_right, m.h_light_bottom),
        Heavy => fill(pixmap, 0, m.h_heavy_top, left_right, m.h_heavy_bottom),
        Double => {
            let t = if up == Double {
                m.v_light_left
            } else {
                left_right
            };
            let b = if down == Double {
                m.v_light_left
            } else {
                left_right
            };
            fill(pixmap, 0, m.h_double_top, t, m.h_light_top);
            fill(pixmap, 0, m.h_light_bottom, b, m.h_double_bottom);
        }
    }
}

/// Draws `count` dashes across the cell, with half a gap at each end.
pub(super) fn dash_horizontal(pixmap: &mut Pixmap, m: &LineMetrics, count: u32, heavy: bool) {
    let (top, bottom) = if heavy {
        (m.h_heavy_top, m.h_heavy_bottom)
    } else {
        (m.h_light_top, m.h_light_bottom)
    };
    for (x0, x1) in dash_spans(pixmap.width(), count, m.light) {
        fill(pixmap, x0, top, x1, bottom);
    }
}

/// Draws `count` dashes down the cell, with half a gap at each end.
pub(super) fn dash_vertical(pixmap: &mut Pixmap, m: &LineMetrics, count: u32, heavy: bool) {
    let (left, right) = if heavy {
        (m.v_heavy_left, m.v_heavy_right)
    } else {
        (m.v_light_left, m.v_light_right)
    };
    for (y0, y1) in dash_spans(pixmap.height(), count, m.light) {
        fill(pixmap, left, y0, right, y1);
    }
}

fn dash_spans(len: u32, count: u32, light: u32) -> impl Iterator<Item = (u32, u32)> {
    let count = count.max(1);
    let gap = light.max(4).min(len / count.saturating_mul(2));
    let solid = gap == 0;
    let dashes = len.saturating_sub(count.saturating_mul(gap));
    let (dash, extra) = (dashes / count, dashes % count);
    let mut pos = gap / 2;
    (0..count).map(move |i| {
        if solid {
            return (0, len);
        }
        let start = pos;
        let end = start + dash + u32::from(i < extra);
        pos = end + gap;
        (start, end)
    })
}
