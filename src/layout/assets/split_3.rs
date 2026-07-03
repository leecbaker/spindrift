use super::*;

pub(in crate::layout) fn paint_transform_for_box(
    style: &ComputedStyle,
    border_box: PaintClip,
) -> Option<PaintTransform> {
    if style.transform.is_empty() {
        return None;
    }
    let origin_x =
        border_box.x() + used_length_percentage(style.transform_origin.x, border_box.width());
    let origin_y =
        border_box.y() + used_length_percentage(style.transform_origin.y, border_box.height());
    let mut transform = PaintTransform::translate(PaintVector::new(origin_x, origin_y));
    for function in &style.transform {
        transform = transform.multiply(transform_function_matrix(
            *function,
            border_box.width(),
            border_box.height(),
        ));
    }
    transform = transform.multiply(PaintTransform::translate(PaintVector::new(
        -origin_x, -origin_y,
    )));
    Some(transform)
}

pub(in crate::layout) fn transform_function_matrix(
    function: css::TransformFunction,
    border_box_width: f32,
    border_box_height: f32,
) -> PaintTransform {
    match function {
        css::TransformFunction::Matrix(a, b, c, d, e, f) => PaintTransform { a, b, c, d, e, f },
        css::TransformFunction::Translate(x, y) => PaintTransform::translate(PaintVector::new(
            used_length_percentage(x, border_box_width),
            used_length_percentage(y, border_box_height),
        )),
        css::TransformFunction::Scale(x, y) => PaintTransform {
            a: x,
            b: 0.0,
            c: 0.0,
            d: y,
            e: 0.0,
            f: 0.0,
        },
        css::TransformFunction::Rotate(angle) => {
            let sin = angle.sin();
            let cos = angle.cos();
            PaintTransform {
                a: cos,
                b: sin,
                c: -sin,
                d: cos,
                e: 0.0,
                f: 0.0,
            }
        }
        css::TransformFunction::Skew(x, y) => PaintTransform {
            a: 1.0,
            b: y.tan(),
            c: x.tan(),
            d: 1.0,
            e: 0.0,
            f: 0.0,
        },
    }
}
