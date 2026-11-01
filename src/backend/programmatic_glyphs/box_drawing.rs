use super::primitives::*;
use tiny_skia::Pixmap;

pub fn render(c: char, width: u32, height: u32) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width, height)?;
    let stroke = stroke_width(height);
    let heavy_stroke = stroke * 2.0;
    let center_x = width as f32 / 2.0;
    let center_y = height as f32 / 2.0;
    let w = width as f32;
    let h = height as f32;
    let color = default_color();

    match c {
        '─' => draw_horizontal_line(&mut pixmap, center_y, stroke, color),
        '━' => draw_horizontal_line(&mut pixmap, center_y, heavy_stroke, color),
        '│' => draw_vertical_line(&mut pixmap, center_x, stroke, color),
        '┃' => draw_vertical_line(&mut pixmap, center_x, heavy_stroke, color),

        '┄' => {
            let dash_count = 3;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - stroke / 2.0,
                    dash_width,
                    stroke,
                    color,
                );
            }
        }
        '┅' => {
            let dash_count = 3;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - heavy_stroke / 2.0,
                    dash_width,
                    heavy_stroke,
                    color,
                );
            }
        }
        '┆' => {
            let dash_count = 3;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - stroke / 2.0,
                    y,
                    stroke,
                    dash_height,
                    color,
                );
            }
        }
        '┇' => {
            let dash_count = 3;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - heavy_stroke / 2.0,
                    y,
                    heavy_stroke,
                    dash_height,
                    color,
                );
            }
        }
        '┈' => {
            let dash_count = 4;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - stroke / 2.0,
                    dash_width,
                    stroke,
                    color,
                );
            }
        }
        '┉' => {
            let dash_count = 4;
            let gap = stroke;
            let dash_width = (w - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let x = i as f32 * (dash_width + gap);
                draw_rect(
                    &mut pixmap,
                    x,
                    center_y - heavy_stroke / 2.0,
                    dash_width,
                    heavy_stroke,
                    color,
                );
            }
        }
        '┊' => {
            let dash_count = 4;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - stroke / 2.0,
                    y,
                    stroke,
                    dash_height,
                    color,
                );
            }
        }
        '┋' => {
            let dash_count = 4;
            let gap = stroke;
            let dash_height = (h - (dash_count - 1) as f32 * gap) / dash_count as f32;
            for i in 0..dash_count {
                let y = i as f32 * (dash_height + gap);
                draw_rect(
                    &mut pixmap,
                    center_x - heavy_stroke / 2.0,
                    y,
                    heavy_stroke,
                    dash_height,
                    color,
                );
            }
        }

        '┌' => {
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                w / 2.0 + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                stroke,
                h / 2.0 + stroke / 2.0,
                color,
            );
        }
        '┐' => {
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                stroke,
                h / 2.0 + stroke / 2.0,
                color,
            );
        }
        '└' => {
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                w / 2.0 + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + stroke / 2.0,
                color,
            );
        }
        '┘' => {
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x + stroke / 2.0,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + stroke / 2.0,
                color,
            );
        }

        '┏' => {
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                w / 2.0 + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                heavy_stroke,
                h / 2.0 + heavy_stroke / 2.0,
                color,
            );
        }
        '┓' => {
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - heavy_stroke / 2.0,
                center_x + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                heavy_stroke,
                h / 2.0 + heavy_stroke / 2.0,
                color,
            );
        }
        '┗' => {
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                center_y - heavy_stroke / 2.0,
                w / 2.0 + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                0.0,
                heavy_stroke,
                center_y + heavy_stroke / 2.0,
                color,
            );
        }
        '┛' => {
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - heavy_stroke / 2.0,
                center_x + heavy_stroke / 2.0,
                heavy_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - heavy_stroke / 2.0,
                0.0,
                heavy_stroke,
                center_y + heavy_stroke / 2.0,
                color,
            );
        }

        '├' => {
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                w / 2.0 + stroke / 2.0,
                stroke,
                color,
            );
        }
        '┤' => {
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x + stroke / 2.0,
                stroke,
                color,
            );
        }
        '┬' => {
            draw_horizontal_line(&mut pixmap, center_y, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - stroke / 2.0,
                stroke,
                h / 2.0 + stroke / 2.0,
                color,
            );
        }
        '┴' => {
            draw_horizontal_line(&mut pixmap, center_y, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + stroke / 2.0,
                color,
            );
        }

        '┼' => {
            draw_horizontal_line(&mut pixmap, center_y, stroke, color);
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
        }

        '═' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
        }
        '║' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x - gap, thin_stroke, color);
            draw_vertical_line(&mut pixmap, center_x + gap, thin_stroke, color);
        }

        '╔' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_rect(
                &mut pixmap,
                center_x - gap,
                center_y - gap - thin_stroke / 2.0,
                w / 2.0 + gap,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                center_y - gap,
                thin_stroke,
                h / 2.0 + gap,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y + gap - thin_stroke / 2.0,
                w / 2.0 - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y + gap - thin_stroke / 2.0,
                thin_stroke,
                h / 2.0 - gap + thin_stroke / 2.0,
                color,
            );
        }
        '╗' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - gap - thin_stroke / 2.0,
                center_x + gap,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y - gap,
                thin_stroke,
                h / 2.0 + gap,
                color,
            );
            draw_rect(
                &mut pixmap,
                0.0,
                center_y + gap - thin_stroke / 2.0,
                center_x - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                center_y + gap - thin_stroke / 2.0,
                thin_stroke,
                h / 2.0 - gap + thin_stroke / 2.0,
                color,
            );
        }
        '╚' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_rect(
                &mut pixmap,
                center_x - gap,
                center_y + gap - thin_stroke / 2.0,
                w / 2.0 + gap,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y + gap,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y - gap - thin_stroke / 2.0,
                w / 2.0 - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y - gap + thin_stroke / 2.0,
                color,
            );
        }
        '╝' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_rect(
                &mut pixmap,
                0.0,
                center_y + gap - thin_stroke / 2.0,
                center_x + gap,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y + gap,
                color,
            );
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - gap - thin_stroke / 2.0,
                center_x - gap + thin_stroke / 2.0,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y - gap + thin_stroke / 2.0,
                color,
            );
        }

        '╠' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x - gap, thin_stroke, color);
            draw_vertical_line(&mut pixmap, center_x + gap, thin_stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - gap,
                center_y - gap - thin_stroke / 2.0,
                w / 2.0 + gap,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - gap,
                center_y + gap - thin_stroke / 2.0,
                w / 2.0 + gap,
                thin_stroke,
                color,
            );
        }
        '╣' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x - gap, thin_stroke, color);
            draw_vertical_line(&mut pixmap, center_x + gap, thin_stroke, color);
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - gap - thin_stroke / 2.0,
                center_x + gap,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                0.0,
                center_y + gap - thin_stroke / 2.0,
                center_x + gap,
                thin_stroke,
                color,
            );
        }
        '╦' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                center_y - gap,
                thin_stroke,
                h / 2.0 + gap,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                center_y - gap,
                thin_stroke,
                h / 2.0 + gap,
                color,
            );
        }
        '╩' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y + gap,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x + gap - thin_stroke / 2.0,
                0.0,
                thin_stroke,
                center_y + gap,
                color,
            );
        }
        '╬' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x - gap, thin_stroke, color);
            draw_vertical_line(&mut pixmap, center_x + gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
        }

        '╞' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_rect(
                &mut pixmap,
                center_x,
                center_y - gap - thin_stroke / 2.0,
                w / 2.0,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x,
                center_y + gap - thin_stroke / 2.0,
                w / 2.0,
                thin_stroke,
                color,
            );
        }
        '╡' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - gap - thin_stroke / 2.0,
                center_x,
                thin_stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                0.0,
                center_y + gap - thin_stroke / 2.0,
                center_x,
                thin_stroke,
                color,
            );
        }
        '╤' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y - gap,
                stroke,
                h / 2.0 + gap,
                color,
            );
        }
        '╧' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y + gap,
                color,
            );
        }
        '╪' => {
            let thin_stroke = (stroke * 0.6).max(1.0);
            let gap = stroke * 0.8;
            draw_vertical_line(&mut pixmap, center_x, stroke, color);
            draw_horizontal_line(&mut pixmap, center_y - gap, thin_stroke, color);
            draw_horizontal_line(&mut pixmap, center_y + gap, thin_stroke, color);
        }

        '╭' => {
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                center_x + radius,
                center_y - stroke / 2.0,
                w / 2.0 - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y + radius,
                stroke,
                h / 2.0 - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x + radius,
                center_y + radius,
                radius,
                180.0,
                270.0,
                stroke,
                color,
            );
        }
        '╮' => {
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                center_y + radius,
                stroke,
                h / 2.0 - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x - radius,
                center_y + radius,
                radius,
                270.0,
                360.0,
                stroke,
                color,
            );
        }
        '╯' => {
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                0.0,
                center_y - stroke / 2.0,
                center_x - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x - radius,
                center_y - radius,
                radius,
                0.0,
                90.0,
                stroke,
                color,
            );
        }
        '╰' => {
            let radius = w / 2.5;
            draw_rect(
                &mut pixmap,
                center_x + radius,
                center_y - stroke / 2.0,
                w / 2.0 - radius,
                stroke,
                color,
            );
            draw_rect(
                &mut pixmap,
                center_x - stroke / 2.0,
                0.0,
                stroke,
                center_y - radius,
                color,
            );
            draw_arc(
                &mut pixmap,
                center_x + radius,
                center_y - radius,
                radius,
                90.0,
                180.0,
                stroke,
                color,
            );
        }

        '╱' => {
            draw_line(&mut pixmap, 0.0, h, w, 0.0, stroke, color);
        }
        '╲' => {
            draw_line(&mut pixmap, 0.0, 0.0, w, h, stroke, color);
        }
        '╳' => {
            draw_line(&mut pixmap, 0.0, h, w, 0.0, stroke, color);
            draw_line(&mut pixmap, 0.0, 0.0, w, h, stroke, color);
        }

        _ => return None,
    }

    Some(pixmap)
}
