use super::*;
use crate::document::paint::patterns::PaintPatternTiling;

pub(in crate::layout) fn append_native_css_gradient_primitives(
    primitives: &mut Vec<PaintPrimitive>,
    image: &BackgroundImage,
    resolved: &ResolvedBackgroundTile,
    current_color: CssColor,
) -> bool {
    if !gradient_interpolation_can_use_native_shading(image) {
        return false;
    }
    let gradient = match image {
        BackgroundImage::LinearGradient(gradient) => linear_gradient_paint(
            &gradient.resolve_current_color(current_color),
            resolved.size,
        ),
        BackgroundImage::RadialGradient(gradient) => radial_gradient_paint(
            &gradient.resolve_current_color(current_color),
            resolved.size,
        ),
        _ => None,
    };
    let Some(gradient) = gradient else {
        return false;
    };
    if resolved.repeat.repeats_x() || resolved.repeat.repeats_y() {
        let area = resolved.clip_area;
        if area.width() <= 0.0 || area.height() <= 0.0 {
            return true;
        }
        let origin_x = background_first_tile_position(
            (f64::from(resolved.positioning_area.x()) + resolved.offset.x) as f32,
            resolved.positioning_area.x(),
            resolved.positioning_area.width(),
            resolved.size.width,
            resolved.repeat.x_axis(),
        );
        let origin_y = background_first_tile_position(
            (f64::from(resolved.positioning_area.y()) + resolved.offset.y) as f32,
            resolved.positioning_area.y(),
            resolved.positioning_area.height(),
            resolved.size.height,
            resolved.repeat.y_axis(),
        );
        let step_width = background_pattern_step(
            resolved.size.width,
            resolved.positioning_area.width(),
            resolved.repeat.x_axis(),
        );
        let step_height = background_pattern_step(
            resolved.size.height,
            resolved.positioning_area.height(),
            resolved.repeat.y_axis(),
        );
        if step_width > 0.0 && step_height > 0.0 {
            primitives.push(PaintPrimitive::GradientPattern(
                RenderedGradientPattern::new(
                    area.paint_rect(),
                    PaintPatternTiling::new(
                        resolved.size,
                        PaintSize::new(step_width, step_height),
                        PaintPoint::new(origin_x, origin_y),
                    ),
                    gradient,
                    resolved.rounded_clip.clone(),
                ),
            ));
        }
        return true;
    }
    for tile_area in resolved.tiles() {
        let Some(tile) = tile_area.intersect(resolved.clip_area) else {
            continue;
        };
        // Keep non-repeating gradients in a local PDF cell too. A direct
        // shading pattern has a matrix in the page coordinate system, while
        // CSS gradients are defined in the background tile's local image
        // coordinate system. The local cell preserves that distinction for
        // every tile, including a single `no-repeat` occurrence.
        primitives.push(PaintPrimitive::GradientPattern(
            RenderedGradientPattern::new(
                tile.paint_rect(),
                PaintPatternTiling::new(
                    resolved.size,
                    resolved.size,
                    PaintPoint::new(tile_area.x(), tile_area.y()),
                ),
                gradient.clone(),
                resolved.rounded_clip.clone(),
            ),
        ));
    }
    true
}

/// Build a single native PDF shading cell for a generated replacement image.
///
/// CSS Content says a single image in `content` replaces the element.  Its
/// image coordinate system is the used replaced-object rectangle, which is
/// the same local tile model used for a non-repeating CSS background.  Keep
/// the gradient program shared with backgrounds so equivalent linear/radial
/// gradients have identical stop handling and PDF output.
/// <https://drafts.csswg.org/css-content-3/#content-property>
/// <https://drafts.csswg.org/css-images-3/#sizing>
pub(in crate::layout) fn native_generated_gradient_primitive(
    image: &BackgroundImage,
    rect: PaintRect,
    current_color: CssColor,
    clip: Option<RenderedPathClip>,
) -> Option<PaintPrimitive> {
    if !gradient_interpolation_can_use_native_shading(image) {
        return None;
    }
    let gradient = match image {
        BackgroundImage::LinearGradient(gradient) => {
            linear_gradient_paint(&gradient.resolve_current_color(current_color), rect.size)
        }
        BackgroundImage::RadialGradient(gradient) => {
            radial_gradient_paint(&gradient.resolve_current_color(current_color), rect.size)
        }
        _ => None,
    }?;
    Some(PaintPrimitive::GradientPattern(
        RenderedGradientPattern::new(
            rect,
            PaintPatternTiling::new(rect.size, rect.size, rect.origin),
            gradient,
            clip,
        ),
    ))
}

fn linear_gradient_paint(
    gradient: &css::LinearGradient,
    size: PaintSize,
) -> Option<RenderedGradient> {
    let line = angled_gradient_line(
        gradient.direction,
        PaintRect::new(PaintPoint::new(0.0, 0.0), size),
    );
    let mut fixed_stops = fixed_gradient_stops(gradient, line.axis_length)?;
    let color_space = resolve_fixed_gradient_colors(&mut fixed_stops, gradient.interpolation);
    let program = resolve_gradient_program(
        fixed_stops,
        &gradient.hints,
        line.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let periodic = periodic_pdf_gradient(&program, line.axis_length);
    let stops = repeating_gradient_average_color(&program, line.axis_length).map_or_else(
        || {
            periodic
                .as_ref()
                .map(|periodic| periodic.stops.clone())
                .or_else(|| normalized_pdf_gradient_stops(&program, line.axis_length))
        },
        |color| {
            Some(vec![
                RenderedGradientStop {
                    offset: 0.0,
                    color,
                    interpolation_exponent: 1.0,
                },
                RenderedGradientStop {
                    offset: 1.0,
                    color,
                    interpolation_exponent: 1.0,
                },
            ])
        },
    )?;
    let (start, end) = line.endpoints();
    Some(RenderedGradient {
        kind: RenderedGradientKind::Linear { start, end },
        color_space,
        stops,
        periodic,
        transform: PaintTransform::identity(),
    })
}

fn radial_gradient_paint(
    gradient: &css::RadialGradient,
    size: PaintSize,
) -> Option<RenderedGradient> {
    let geometry = used_radial_gradient_geometry(gradient, size)?;
    let mut fixed_stops = fixed_radial_gradient_stops(gradient, geometry.axis_length)?;
    let color_space = resolve_fixed_gradient_colors(&mut fixed_stops, gradient.interpolation);
    let domain_scale = if gradient.repeating {
        radial_gradient_paint_domain_scale(geometry, size)
    } else {
        1.0
    };
    let domain_length = geometry.axis_length * domain_scale;
    let program = resolve_gradient_program(
        fixed_stops,
        &gradient.hints,
        geometry.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let periodic = periodic_pdf_gradient(&program, domain_length);
    let stops = repeating_gradient_average_color(&program, domain_length).map_or_else(
        || {
            periodic
                .as_ref()
                .map(|periodic| periodic.stops.clone())
                .or_else(|| normalized_pdf_gradient_stops(&program, domain_length))
        },
        |color| {
            Some(vec![
                RenderedGradientStop {
                    offset: 0.0,
                    color,
                    interpolation_exponent: 1.0,
                },
                RenderedGradientStop {
                    offset: 1.0,
                    color,
                    interpolation_exponent: 1.0,
                },
            ])
        },
    )?;
    // A PDF radial shading has circular geometry. Scaling its local coordinate
    // system creates the CSS ellipse without rasterizing it.
    let transform = PaintTransform::scale(geometry.radii.width, geometry.radii.height);
    let center = PaintPoint::new(
        geometry.center.x / geometry.radii.width,
        geometry.center.y / geometry.radii.height,
    );
    Some(RenderedGradient {
        kind: RenderedGradientKind::Radial {
            start_center: center,
            start_radius: 0.0,
            end_center: center,
            end_radius: domain_scale,
        },
        color_space,
        stops,
        periodic,
        transform,
    })
}

/// Returns the CSS Images gradient-average color for a zero-period repeating
/// gradient. The average is the integral of premultiplied components across
/// the color line; with coincident stops CSS distributes them evenly first.
/// <https://www.w3.org/TR/css-images-3/#gradient-average-color>
const MAX_PDF_REPEATING_GRADIENT_STOPS: f32 = 4096.0;

/// Fixed CSS gradient color line shared by raster sampling and native-PDF
/// preparation. It preserves the repeated cycle rather than baking repetition
/// into image pixels or CSS-background tiles.
#[derive(Debug, Clone)]
struct ResolvedGradientProgram {
    stops: Vec<FixedGradientStop>,
    interval_exponents: Vec<f32>,
    repeat_period: Option<f32>,
    interpolation: css::GradientInterpolationMethod,
}

fn resolve_gradient_program(
    stops: Vec<FixedGradientStop>,
    hints: &[css::GradientColorHint],
    color_line_length: f32,
    repeating: bool,
    interpolation: css::GradientInterpolationMethod,
) -> Option<ResolvedGradientProgram> {
    let first = stops.first()?;
    let last = stops.last()?;
    let repeat_period = repeating.then_some(last.position - first.position);
    Some(ResolvedGradientProgram {
        interval_exponents: gradient_interval_exponents(&stops, hints, color_line_length),
        stops,
        repeat_period,
        interpolation,
    })
}

fn periodic_pdf_gradient(
    program: &ResolvedGradientProgram,
    domain_length: f32,
) -> Option<Box<crate::document::paint::paths::RenderedPeriodicGradient>> {
    let period = program.repeat_period?;
    if period <= 0.001 || repeating_gradient_average_color(program, domain_length).is_some() {
        return None;
    }
    Some(Box::new(
        crate::document::paint::paths::RenderedPeriodicGradient {
            stops: program
                .stops
                .iter()
                .enumerate()
                .map(|(index, stop)| RenderedGradientStop {
                    offset: stop.position,
                    color: stop.color,
                    interpolation_exponent: program.interval_exponents[index],
                })
                .collect(),
            start: program.stops.first()?.position,
            period,
            domain_length,
        },
    ))
}

fn repeating_gradient_average_color(
    program: &ResolvedGradientProgram,
    domain_length: f32,
) -> Option<CssColor> {
    if program.repeat_period.is_none() || program.stops.len() < 2 {
        return None;
    }
    let stops = &program.stops;
    let first = stops.first()?;
    let period = program.repeat_period?;
    let degenerate = period <= 0.001;
    let estimated_stop_count = if degenerate {
        f32::INFINITY
    } else {
        ((domain_length / period).ceil() + 3.0) * stops.len() as f32
    };
    if !degenerate && estimated_stop_count <= MAX_PDF_REPEATING_GRADIENT_STOPS {
        return None;
    }

    let total_length = if degenerate {
        (stops.len() - 1) as f32
    } else {
        period
    };
    let (red, green, blue, alpha) = stops.windows(2).enumerate().fold(
        (0.0, 0.0, 0.0, 0.0),
        |(red, green, blue, alpha), (index, pair)| {
            let length = if degenerate {
                1.0
            } else {
                pair[1].position - pair[0].position
            };
            // Integrate CSS's `t^N` transition-hint interpolation rather
            // than assuming a midpoint transition. The average is defined in
            // premultiplied color space by CSS Images 3.
            let progress_average = if degenerate {
                0.5
            } else {
                1.0 / (program.interval_exponents[index] + 1.0)
            };
            let weight = length / total_length;
            let interpolate =
                |start: f32, end: f32| (start + (end - start) * progress_average) * weight;
            (
                red + interpolate(
                    pair[0].color.components()[0] * pair[0].color.alpha(),
                    pair[1].color.components()[0] * pair[1].color.alpha(),
                ),
                green
                    + interpolate(
                        pair[0].color.components()[1] * pair[0].color.alpha(),
                        pair[1].color.components()[1] * pair[1].color.alpha(),
                    ),
                blue + interpolate(
                    pair[0].color.components()[2] * pair[0].color.alpha(),
                    pair[1].color.components()[2] * pair[1].color.alpha(),
                ),
                alpha + interpolate(pair[0].color.alpha(), pair[1].color.alpha()),
            )
        },
    );
    Some(if alpha <= 0.0 {
        CssColor::in_space(first.color.space(), 0.0, 0.0, 0.0, 0.0)
    } else {
        CssColor::in_space(
            first.color.space(),
            red / alpha,
            green / alpha,
            blue / alpha,
            alpha,
        )
    })
}

fn normalized_pdf_gradient_stops(
    program: &ResolvedGradientProgram,
    domain_length: f32,
) -> Option<Vec<RenderedGradientStop>> {
    let stops = &program.stops;
    let first = stops.first()?;
    let last = stops.last()?;
    if domain_length <= 0.0 {
        return None;
    }
    if program.repeat_period.is_none() {
        if (first.position).abs() > 0.001 || (last.position - domain_length).abs() > 0.001 {
            return None;
        }
        return Some(
            stops
                .iter()
                .enumerate()
                .map(|(index, stop)| RenderedGradientStop {
                    offset: (stop.position / domain_length).clamp(0.0, 1.0),
                    color: stop.color,
                    interpolation_exponent: program.interval_exponents[index],
                })
                .collect(),
        );
    }

    // CSS Images 3 repeats the fixed-up stop list in both directions. The
    // zero-period case was handled as a gradient-average color above.
    let period = program.repeat_period?;
    if period <= 0.001 {
        return None;
    }
    let mut rendered = Vec::new();
    let first_cycle = ((-first.position) / period).floor() as i32 - 1;
    let last_cycle = ((domain_length - first.position) / period).ceil() as i32 + 1;
    for cycle in first_cycle..=last_cycle {
        let shift = cycle as f32 * period;
        for (index, stop) in stops.iter().enumerate() {
            let position = stop.position + shift;
            if position < -0.001 || position > domain_length + 0.001 {
                continue;
            }
            let offset = (position / domain_length).clamp(0.0, 1.0);
            // Keep coincident boundary stops: their order encodes the sharp
            // transition required when a repeat's last and first colors differ.
            rendered.push(RenderedGradientStop {
                offset,
                color: stop.color,
                interpolation_exponent: program.interval_exponents[index],
            });
        }
    }
    rendered.sort_by(|left, right| left.offset.total_cmp(&right.offset));
    if rendered.len() < 2 {
        return None;
    }
    // The finite PDF function domain must have endpoint values. Insert the
    // sampled CSS color when a cycle has no stop exactly on an endpoint.
    if rendered.first().is_some_and(|stop| stop.offset > 0.001) {
        rendered.insert(
            0,
            RenderedGradientStop {
                offset: 0.0,
                color: sampled_gradient_program_color(program, 0.0),
                interpolation_exponent: 1.0,
            },
        );
    }
    if rendered.last().is_some_and(|stop| stop.offset < 0.999) {
        rendered.push(RenderedGradientStop {
            offset: 1.0,
            color: sampled_gradient_program_color(program, domain_length),
            interpolation_exponent: 1.0,
        });
    }
    Some(rendered)
}

fn gradient_interval_exponents(
    stops: &[FixedGradientStop],
    hints: &[css::GradientColorHint],
    color_line_length: f32,
) -> Vec<f32> {
    stops
        .windows(2)
        .enumerate()
        .map(|(index, pair)| {
            let Some(hint) = hints.iter().find(|hint| hint.after_stop == index) else {
                return 1.0;
            };
            let Some(position) = hint
                .position
                .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                    color_line_length,
                )))
                .map(layout_points)
                .or(Some(hint.position.length_points()))
            else {
                return 1.0;
            };
            let fraction = (position - pair[0].position) / (pair[1].position - pair[0].position);
            if fraction > 0.001 && fraction < 0.999 {
                (0.5_f32.ln() / fraction.ln()).clamp(0.01, 100.0)
            } else {
                1.0
            }
        })
        .chain(std::iter::once(1.0))
        .collect()
}

fn sampled_gradient_program_color(
    program: &ResolvedGradientProgram,
    mut position: f32,
) -> CssColor {
    let stops = &program.stops;
    if let Some(period) = program.repeat_period {
        if period <= 0.001 {
            return repeating_gradient_average_color(program, f32::INFINITY)
                .unwrap_or(stops.last().expect("non-empty stops").color);
        }
        position = (position - stops[0].position).rem_euclid(period) + stops[0].position;
    }
    if position <= stops[0].position {
        return stops[0].color;
    }
    for (index, pair) in stops.windows(2).enumerate() {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            if span <= 0.001 {
                return pair[1].color;
            }
            let progress = ((position - pair[0].position) / span)
                .clamp(0.0, 1.0)
                .powf(program.interval_exponents[index]);
            return crate::color::interpolate_color_with_missing(
                pair[0].color,
                pair[1].color,
                program.interpolation,
                progress,
                pair[0].missing_components.bits(),
                pair[1].missing_components.bits(),
            );
        }
    }
    stops.last().expect("non-empty stops").color
}

fn radial_gradient_paint_domain_scale(
    geometry: UsedRadialGradientGeometry,
    size: PaintSize,
) -> f32 {
    [
        PaintPoint::new(0.0, 0.0),
        PaintPoint::new(size.width, 0.0),
        PaintPoint::new(0.0, size.height),
        PaintPoint::new(size.width, size.height),
    ]
    .into_iter()
    .map(|point| radial_gradient_axis_position(point, geometry) / geometry.axis_length)
    .fold(1.0, f32::max)
}

/// Whether repeating this generated image through a PDF tiling pattern
/// preserves Spindrift's image-emission semantics.
///
/// A gradient with identical stop colors is spatially constant after CSS
/// Images color-stop fixup, regardless of its direction, hint, or stop
/// positions. It can therefore share the repeated pattern path. Raster URL
/// images deliberately remain individual placements: PDF viewers can sample
/// a tiling-pattern cell using the same local source rectangle and CSS tile
/// geometry as an individual placement. Reusing that cell keeps repeated
/// raster backgrounds bounded without changing their positioning or clip.
/// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>
pub(in crate::layout) fn generated_linear_gradient_image(
    gradient: &css::LinearGradient,
    size: PaintSize,
    resource_cache: &ResourceCache,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let pixel_size = generated_image_pixel_size(size);
    (size.width > 0.0 && size.height > 0.0 && pixel_size.width > 0 && pixel_size.height > 0).then(
        || {
            let image_id = resource_cache.register_generated_image_recipe(
                crate::image_store::GeneratedRasterImage::Linear {
                    gradient,
                    size,
                    metadata: crate::image_store::ImageMetadata::from_pixel_size(pixel_size),
                },
            );
            DecodedPngImage {
                image_id: Some(image_id),
                pixel_size,
                source_rect: None,
                natural_size: crate::units::CssPixelSize::new(pixel_size.width, pixel_size.height),
                sample_depth: crate::image_store::RasterSampleDepth::Eight,
                rgb: EncodedRasterRgbSamples::from_shared(resource_cache.image_placeholder_rgb()),
                alpha: None,
                color_space: crate::color::RasterColorSpace::SRGB,
            }
        },
    )
}

pub(in crate::layout) fn generated_radial_gradient_image(
    gradient: &css::RadialGradient,
    size: PaintSize,
    resource_cache: &ResourceCache,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let pixel_size = generated_image_pixel_size(size);
    (size.width > 0.0 && size.height > 0.0 && pixel_size.width > 0 && pixel_size.height > 0).then(
        || {
            let image_id = resource_cache.register_generated_image_recipe(
                crate::image_store::GeneratedRasterImage::Radial {
                    gradient,
                    size,
                    metadata: crate::image_store::ImageMetadata::from_pixel_size(pixel_size),
                },
            );
            DecodedPngImage {
                image_id: Some(image_id),
                pixel_size,
                source_rect: None,
                natural_size: crate::units::CssPixelSize::new(pixel_size.width, pixel_size.height),
                sample_depth: crate::image_store::RasterSampleDepth::Eight,
                rgb: EncodedRasterRgbSamples::from_shared(resource_cache.image_placeholder_rgb()),
                alpha: None,
                color_space: crate::color::RasterColorSpace::SRGB,
            }
        },
    )
}

/// Rasterizes a CSS Images Level 3 linear gradient into a generated image.
///
/// Gradients are generated images with no intrinsic dimensions. The caller
/// supplies the concrete object size after CSS Backgrounds sizing, then this
/// samples the gradient in a resolved common CSS CssColor 4 space. CSS Images 3
/// stop positions, hints, and premultiplied-component interpolation are
/// otherwise unchanged:
/// <https://www.w3.org/TR/css-images-3/#coloring-gradient-line>.
pub(crate) fn rasterize_linear_gradient(
    gradient: &css::LinearGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let width = size.width;
    let height = size.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let pixel_size = generated_image_pixel_size(size);
    let (pixel_width, pixel_height) = (pixel_size.width, pixel_size.height);
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    let area = paint_space_rect(0.0, 0.0, width, height);
    let line = angled_gradient_line(gradient.direction, area);
    let mut stops = fixed_gradient_stops(&gradient, line.axis_length)?;
    resolve_raster_gradient_colors(&mut stops, gradient.interpolation);
    let program = resolve_gradient_program(
        stops,
        &gradient.hints,
        line.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    rasterize_generated_gradient(size, |point| {
        sampled_gradient_program_color(&program, gradient_axis_position(point, line))
    })
}

/// Determine the RGB storage space for a generated linear-gradient image
/// without retaining its raster samples during PDF-resource planning.
pub(crate) fn generated_linear_gradient_raster_color_space(
    gradient: &css::LinearGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<css::CssColorSpace> {
    let gradient = gradient.resolve_current_color(current_color);
    let area = paint_space_rect(0.0, 0.0, size.width, size.height);
    let line = angled_gradient_line(gradient.direction, area);
    let mut stops = fixed_gradient_stops(&gradient, line.axis_length)?;
    resolve_raster_gradient_colors(&mut stops, gradient.interpolation);
    let program = resolve_gradient_program(
        stops,
        &gradient.hints,
        line.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let pixel_size = generated_image_pixel_size(size);
    Some(
        generated_gradient_raster_encoding(size, pixel_size, &|point| {
            sampled_gradient_program_color(&program, gradient_axis_position(point, line))
        })
        .color_space(),
    )
}

/// Rasterizes a CSS Images Level 3 radial gradient into a generated image.
///
/// Radial gradients are generated images with no intrinsic dimensions. The
/// concrete background tile size determines the center point, ending radii,
/// color-stop percentage basis, and repeating period:
/// <https://www.w3.org/TR/css-images-3/#radial-gradients>.
pub(crate) fn rasterize_radial_gradient(
    gradient: &css::RadialGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let width = size.width;
    let height = size.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let pixel_size = generated_image_pixel_size(size);
    let (pixel_width, pixel_height) = (pixel_size.width, pixel_size.height);
    if pixel_width == 0 || pixel_height == 0 {
        return None;
    }
    let geometry = used_radial_gradient_geometry(&gradient, size)?;
    let mut stops = fixed_radial_gradient_stops(&gradient, geometry.axis_length)?;
    resolve_raster_gradient_colors(&mut stops, gradient.interpolation);
    let program = resolve_gradient_program(
        stops,
        &gradient.hints,
        geometry.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    rasterize_generated_gradient(size, |point| {
        sampled_gradient_program_color(&program, radial_gradient_axis_position(point, geometry))
    })
}

/// Determine the RGB storage space for a generated radial-gradient image
/// without retaining its raster samples during PDF-resource planning.
pub(crate) fn generated_radial_gradient_raster_color_space(
    gradient: &css::RadialGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<css::CssColorSpace> {
    let gradient = gradient.resolve_current_color(current_color);
    let geometry = used_radial_gradient_geometry(&gradient, size)?;
    let mut stops = fixed_radial_gradient_stops(&gradient, geometry.axis_length)?;
    resolve_raster_gradient_colors(&mut stops, gradient.interpolation);
    let program = resolve_gradient_program(
        stops,
        &gradient.hints,
        geometry.axis_length,
        gradient.repeating,
        gradient.interpolation,
    )?;
    let pixel_size = generated_image_pixel_size(size);
    Some(
        generated_gradient_raster_encoding(size, pixel_size, &|point| {
            sampled_gradient_program_color(&program, radial_gradient_axis_position(point, geometry))
        })
        .color_space(),
    )
}

/// Rasterize a CSS Images Level 4 conic gradient using its clockwise angular
/// color line. CSS zero degrees points toward the top of the gradient box;
/// increasing angles turn clockwise.
/// <https://drafts.csswg.org/css-images-4/#conic-gradients>
pub(crate) fn rasterize_conic_gradient(
    gradient: &css::ConicGradient,
    size: PaintSize,
    current_color: CssColor,
) -> Option<DecodedPngImage> {
    let gradient = gradient.resolve_current_color(current_color);
    let width = size.width;
    let height = size.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let mut stops = fixed_conic_gradient_stops(&gradient)?;
    resolve_raster_gradient_colors(&mut stops, gradient.interpolation);
    let center_x = used_background_position_axis(gradient.position.x.clone(), width, false);
    let center_y = used_background_position_axis(gradient.position.y.clone(), height, true);
    rasterize_generated_gradient(size, |point| {
        let angle = ((point.x - center_x).atan2(point.y - center_y).to_degrees()
            - gradient.start_angle)
            .rem_euclid(360.0);
        sampled_conic_gradient_color(gradient.repeating, &stops, angle, gradient.interpolation)
    })
}

/// One PDF-image-compatible RGB encoding chosen after CSS gradient sampling.
///
/// CSS gradient interpolation may use Oklab, Lab, or D50 XYZ coordinates, but
/// a three-component PDF image stream always needs RGB samples paired with an
/// RGB ICC profile. Keep those boundaries separate: sample first in CSS's
/// selected interpolation space, then convert the completed tile to one RGB
/// storage space. This follows CSS Color 4's output-conversion boundary while
/// preserving the existing ordinary-PDF preference for sRGB, then Display-P3.
/// <https://www.w3.org/TR/css-color-4/#color-conversion>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedGradientRasterEncoding {
    Srgb,
    DisplayP3,
}

impl GeneratedGradientRasterEncoding {
    const fn color_space(self) -> css::CssColorSpace {
        match self {
            Self::Srgb => css::CssColorSpace::Srgb,
            Self::DisplayP3 => css::CssColorSpace::DisplayP3,
        }
    }
}

fn rasterize_generated_gradient(
    size: PaintSize,
    sample: impl Fn(PaintPoint) -> CssColor,
) -> Option<DecodedPngImage> {
    let pixel_size = generated_image_pixel_size(size);
    let (pixel_width, pixel_height) = (pixel_size.width, pixel_size.height);
    let encoding = generated_gradient_raster_encoding(size, pixel_size, &sample);
    let mut rgb = Vec::with_capacity(pixel_width as usize * pixel_height as usize * 3);
    let mut alpha = Vec::with_capacity(pixel_width as usize * pixel_height as usize);
    let mut has_alpha = false;
    for_generated_gradient_pixel(size, pixel_size, |point| {
        let color = encoded_gradient_raster_color(sample(point), encoding);
        rgb.extend(color.components().map(encoded_color_component));
        let opacity = encoded_color_component(color.alpha());
        alpha.push(opacity);
        has_alpha |= opacity < u8::MAX;
    });
    Some(
        DecodedPngImage::new(pixel_width, pixel_height, rgb, has_alpha.then_some(alpha))
            .in_color_space(encoding.color_space()),
    )
}

fn generated_gradient_raster_encoding(
    size: PaintSize,
    pixel_size: RasterPixelSize,
    sample: &impl Fn(PaintPoint) -> CssColor,
) -> GeneratedGradientRasterEncoding {
    let mut srgb_representable = true;
    for_generated_gradient_pixel(size, pixel_size, |point| {
        let color = crate::css::color_to_predefined_rgb(sample(point), css::CssColorSpace::Srgb)
            .expect("sRGB is a CSS predefined RGB space");
        srgb_representable &= encoded_rgb_is_in_unit_gamut(color);
    });
    if srgb_representable {
        GeneratedGradientRasterEncoding::Srgb
    } else {
        GeneratedGradientRasterEncoding::DisplayP3
    }
}

fn for_generated_gradient_pixel(
    size: PaintSize,
    pixel_size: RasterPixelSize,
    mut visit: impl FnMut(PaintPoint),
) {
    for row in 0..pixel_size.height {
        let y = size.height - ((row as f32 + 0.5) * size.height / pixel_size.height as f32);
        for column in 0..pixel_size.width {
            let x = (column as f32 + 0.5) * size.width / pixel_size.width as f32;
            visit(PaintPoint::new(x, y));
        }
    }
}

fn encoded_gradient_raster_color(
    color: CssColor,
    encoding: GeneratedGradientRasterEncoding,
) -> CssColor {
    crate::css::color_to_predefined_rgb(color, encoding.color_space())
        .expect("generated gradient output uses a CSS predefined RGB space")
}

/// Convert only raster gradient stops that participate in a color transition.
///
/// CSS Images defines the color at a stop position as the authored stop color;
/// converting a duplicate hard stop through an interpolation space can turn a
/// solid legacy sRGB band into a visibly different RGB sample. A hard-stop or
/// spatially constant color line has no interpolation interval, so retain its
/// original CSS colors until the final RGB image encoding boundary.
/// <https://drafts.csswg.org/css-images-4/#coloring-gradient-line>
fn resolve_raster_gradient_colors(
    stops: &mut [FixedGradientStop],
    interpolation: css::GradientInterpolationMethod,
) {
    if !fixed_gradient_is_hard_stop(stops) {
        resolve_fixed_gradient_colors(stops, interpolation);
    }
}

const fn encoded_rgb_is_in_unit_gamut(color: CssColor) -> bool {
    // Do not select a wider image profile because a CSS conversion landed one
    // 8-bit sample outside the unit cube through floating-point roundoff.
    // The final encoder clamps exactly that amount at its genuine output
    // boundary, matching direct PDF paint's quantization policy.
    const EPSILON: f32 = 1.0 / 255.0;
    color.alpha() == 0.0
        || (color.components()[0] >= -EPSILON
            && color.components()[0] <= 1.0 + EPSILON
            && color.components()[1] >= -EPSILON
            && color.components()[1] <= 1.0 + EPSILON
            && color.components()[2] >= -EPSILON
            && color.components()[2] <= 1.0 + EPSILON)
}

fn encoded_color_component(component: f32) -> u8 {
    (component.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Apply the same common-space selection used by vector PDF gradients before
/// sampling generated CSS-gradient images.
fn gradient_interpolation_can_use_native_shading(image: &BackgroundImage) -> bool {
    let (method, has_missing_components) = match image {
        BackgroundImage::LinearGradient(gradient) => (
            gradient.interpolation,
            gradient.stops.iter().any(|stop| {
                !stop
                    .color
                    .missing_components_for(gradient.interpolation)
                    .is_empty()
            }),
        ),
        BackgroundImage::RadialGradient(gradient) => (
            gradient.interpolation,
            gradient.stops.iter().any(|stop| {
                !stop
                    .color
                    .missing_components_for(gradient.interpolation)
                    .is_empty()
            }),
        ),
        _ => return false,
    };
    // PDF Type 2 functions interpolate encoded components directly. That is
    // exact only for CSS's encoded rectangular spaces (and D50 XYZ); all
    // perceptual, polar, and linear-light methods must go through the shared
    // raster sampler.
    !has_missing_components
        && matches!(
            method.space,
            css::GradientInterpolationSpace::Srgb
                | css::GradientInterpolationSpace::DisplayP3
                | css::GradientInterpolationSpace::A98Rgb
                | css::GradientInterpolationSpace::ProphotoRgb
                | css::GradientInterpolationSpace::Rec2020
                | css::GradientInterpolationSpace::XyzD50
        )
}

fn gradient_interpolation_output_space(
    method: css::GradientInterpolationMethod,
) -> crate::css::CssColorSpace {
    match method.space {
        css::GradientInterpolationSpace::Srgb
        | css::GradientInterpolationSpace::SrgbLinear
        | css::GradientInterpolationSpace::Hsl
        | css::GradientInterpolationSpace::Hwb => crate::css::CssColorSpace::Srgb,
        css::GradientInterpolationSpace::DisplayP3
        | css::GradientInterpolationSpace::DisplayP3Linear => crate::css::CssColorSpace::DisplayP3,
        css::GradientInterpolationSpace::A98Rgb => crate::css::CssColorSpace::A98Rgb,
        css::GradientInterpolationSpace::ProphotoRgb => crate::css::CssColorSpace::ProphotoRgb,
        css::GradientInterpolationSpace::Rec2020 => crate::css::CssColorSpace::Rec2020,
        css::GradientInterpolationSpace::XyzD50
        | css::GradientInterpolationSpace::XyzD65
        | css::GradientInterpolationSpace::Lab
        | css::GradientInterpolationSpace::Oklab
        | css::GradientInterpolationSpace::Lch
        | css::GradientInterpolationSpace::Oklch => crate::css::CssColorSpace::XyzD50,
    }
}

fn resolve_fixed_gradient_colors(
    stops: &mut [FixedGradientStop],
    interpolation: css::GradientInterpolationMethod,
) -> crate::css::CssColorSpace {
    // CSS Images 3 leaves no authored interpolation-space token to preserve.
    // Choose the ordinary-PDF RGB condition from the resolved stop gamut so a
    // wide-gamut gradient is not silently clipped through sRGB before the
    // PDF shading function receives it. Explicit CSS Color 4 `in <space>`
    // interpolation remains authoritative.
    let space = if interpolation == css::GradientInterpolationMethod::CSS_IMAGES_3 {
        if stops.iter().all(|stop| {
            crate::css::color_to_predefined_rgb(stop.color, crate::css::CssColorSpace::Srgb)
                .is_some_and(encoded_rgb_is_in_unit_gamut)
        }) {
            crate::css::CssColorSpace::Srgb
        } else {
            crate::css::CssColorSpace::DisplayP3
        }
    } else {
        gradient_interpolation_output_space(interpolation)
    };
    if stops.iter().all(|stop| stop.color.space() == space) {
        return space;
    }
    if stops.iter_mut().all(|stop| {
        if let Some(color) = crate::color::convert_color(stop.color, space) {
            stop.color = color;
            true
        } else {
            false
        }
    }) {
        space
    } else {
        for stop in stops {
            stop.color =
                crate::css::color_to_predefined_rgb(stop.color, crate::css::CssColorSpace::Srgb)
                    .expect("sRGB is a predefined CSS RGB space");
        }
        crate::css::CssColorSpace::Srgb
    }
}

fn fixed_conic_gradient_stops(gradient: &css::ConicGradient) -> Option<Vec<FixedGradientStop>> {
    if gradient.stops.is_empty() {
        return None;
    }
    let mut positions = gradient
        .stops
        .iter()
        .map(|stop| stop.position)
        .collect::<Vec<_>>();
    positions[0].get_or_insert(0.0);
    let last = positions.len() - 1;
    positions[last].get_or_insert(360.0);
    let mut previous = positions[0]?;
    for position in positions.iter_mut().skip(1).flatten() {
        *position = position.max(previous);
        previous = *position;
    }
    let mut index = 0;
    while index < positions.len() {
        if positions[index].is_some() {
            index += 1;
            continue;
        }
        let start = index;
        while index < positions.len() && positions[index].is_none() {
            index += 1;
        }
        let before = positions[start - 1]?;
        let after = positions[index]?;
        let slots = (index - start + 1) as f32;
        for (offset, position) in positions[start..index].iter_mut().enumerate() {
            *position = Some(before + (after - before) * (offset + 1) as f32 / slots);
        }
    }
    gradient
        .stops
        .iter()
        .zip(positions)
        .map(|(stop, position)| {
            Some(FixedGradientStop {
                color: stop.color.as_color()?,
                missing_components: stop.color.missing_components_for(gradient.interpolation),
                position: position.unwrap_or(0.0),
            })
        })
        .collect()
}

fn sampled_conic_gradient_color(
    repeating: bool,
    stops: &[FixedGradientStop],
    mut position: f32,
    interpolation: css::GradientInterpolationMethod,
) -> CssColor {
    let first = stops.first().expect("non-empty conic stops");
    let last = stops.last().expect("non-empty conic stops");
    if repeating {
        let period = last.position - first.position;
        if period.abs() <= 0.001 {
            return last.color;
        }
        position = (position - first.position).rem_euclid(period) + first.position;
    }
    if position <= first.position {
        return first.color;
    }
    for pair in stops.windows(2) {
        if position <= pair[1].position {
            let span = pair[1].position - pair[0].position;
            if span.abs() <= 0.001 {
                return pair[1].color;
            }
            return crate::color::interpolate_color_with_missing(
                pair[0].color,
                pair[1].color,
                interpolation,
                (position - pair[0].position) / span,
                pair[0].missing_components.bits(),
                pair[1].missing_components.bits(),
            );
        }
    }
    last.color
}

#[derive(Debug, Clone, Copy)]
struct UsedRadialGradientGeometry {
    center: PaintPoint,
    radii: PaintSize,
    axis_length: f32,
}

fn used_radial_gradient_geometry(
    gradient: &css::RadialGradient,
    size: PaintSize,
) -> Option<UsedRadialGradientGeometry> {
    let width = size.width;
    let height = size.height;
    let center = PaintPoint::new(
        used_background_position_axis(gradient.position.x.clone(), width, false),
        used_background_position_axis(gradient.position.y.clone(), height, true),
    );
    let radii = match &gradient.size {
        css::RadialGradientSize::CircleRadius(radius) => {
            let radius = used_length_percentage(
                radius.clone(),
                PercentageBasis::definite(layout_pt(width.max(height).max(0.0))),
            )
            .points();
            PaintSize::new(radius, radius)
        }
        css::RadialGradientSize::EllipseRadii { x, y } => PaintSize::new(
            used_length_percentage(
                x.clone(),
                PercentageBasis::definite(layout_pt(width.max(0.0))),
            )
            .points(),
            used_length_percentage(
                y.clone(),
                PercentageBasis::definite(layout_pt(height.max(0.0))),
            )
            .points(),
        ),
        css::RadialGradientSize::Extent(extent) => {
            used_radial_gradient_extent_radii(gradient.shape, *extent, center, size)
        }
    };
    if radii.width <= 0.0 || radii.height <= 0.0 {
        return None;
    }
    Some(UsedRadialGradientGeometry {
        center,
        radii,
        axis_length: radii.width.max(radii.height),
    })
}

fn used_radial_gradient_extent_radii(
    shape: css::RadialGradientShape,
    extent: css::RadialGradientExtent,
    center: PaintPoint,
    size: PaintSize,
) -> PaintSize {
    let width = size.width;
    let height = size.height;
    let left = center.x.max(0.0);
    let right = (width - center.x).max(0.0);
    let bottom = center.y.max(0.0);
    let top = (height - center.y).max(0.0);
    match shape {
        css::RadialGradientShape::Circle => {
            let corners = [
                (left * left + bottom * bottom).sqrt(),
                (left * left + top * top).sqrt(),
                (right * right + bottom * bottom).sqrt(),
                (right * right + top * top).sqrt(),
            ];
            let radius = match extent {
                css::RadialGradientExtent::ClosestSide => left.min(right).min(bottom).min(top),
                css::RadialGradientExtent::FarthestSide => left.max(right).max(bottom).max(top),
                css::RadialGradientExtent::ClosestCorner => {
                    corners.into_iter().fold(f32::INFINITY, f32::min)
                }
                css::RadialGradientExtent::FarthestCorner => {
                    corners.into_iter().fold(0.0, f32::max)
                }
            };
            PaintSize::new(radius, radius)
        }
        css::RadialGradientShape::Ellipse => {
            let side_radii = match extent {
                css::RadialGradientExtent::ClosestSide
                | css::RadialGradientExtent::ClosestCorner => {
                    PaintSize::new(left.min(right), bottom.min(top))
                }
                css::RadialGradientExtent::FarthestSide
                | css::RadialGradientExtent::FarthestCorner => {
                    PaintSize::new(left.max(right), bottom.max(top))
                }
            };
            if matches!(
                extent,
                css::RadialGradientExtent::ClosestSide | css::RadialGradientExtent::FarthestSide
            ) {
                return side_radii;
            }
            scaled_ellipse_corner_radii(side_radii, extent, left, right, bottom, top)
        }
    }
}

fn scaled_ellipse_corner_radii(
    radii: PaintSize,
    extent: css::RadialGradientExtent,
    left: f32,
    right: f32,
    bottom: f32,
    top: f32,
) -> PaintSize {
    if radii.width <= 0.0 || radii.height <= 0.0 {
        return radii;
    }
    let corner_scales = [
        ((left / radii.width).powi(2) + (bottom / radii.height).powi(2)).sqrt(),
        ((left / radii.width).powi(2) + (top / radii.height).powi(2)).sqrt(),
        ((right / radii.width).powi(2) + (bottom / radii.height).powi(2)).sqrt(),
        ((right / radii.width).powi(2) + (top / radii.height).powi(2)).sqrt(),
    ];
    let scale = match extent {
        css::RadialGradientExtent::ClosestCorner => {
            corner_scales.into_iter().fold(f32::INFINITY, f32::min)
        }
        css::RadialGradientExtent::FarthestCorner => corner_scales.into_iter().fold(0.0, f32::max),
        css::RadialGradientExtent::ClosestSide | css::RadialGradientExtent::FarthestSide => 1.0,
    };
    PaintSize::new(radii.width * scale, radii.height * scale)
}

fn radial_gradient_axis_position(point: PaintPoint, geometry: UsedRadialGradientGeometry) -> f32 {
    let dx = (point.x - geometry.center.x) / geometry.radii.width;
    let dy = (point.y - geometry.center.y) / geometry.radii.height;
    (dx * dx + dy * dy).sqrt() * geometry.axis_length
}

fn fixed_radial_gradient_stops(
    gradient: &css::RadialGradient,
    axis_length: f32,
) -> Option<Vec<FixedGradientStop>> {
    fixed_gradient_stops_from_color_stops(&gradient.stops, axis_length, gradient.interpolation)
}

fn fixed_gradient_stops_from_color_stops(
    stops: &[css::GradientColorStop],
    axis_length: f32,
    interpolation: css::GradientInterpolationMethod,
) -> Option<Vec<FixedGradientStop>> {
    if axis_length <= 0.0 || stops.len() < 2 {
        return None;
    }
    let mut positions = stops
        .iter()
        .map(|stop| {
            stop.position
                .as_ref()
                .and_then(|position| {
                    position
                        .used_length_with_percentage_basis(PercentageBasis::definite(layout_pt(
                            axis_length,
                        )))
                        .map(layout_points)
                })
                .or_else(|| {
                    stop.position
                        .as_ref()
                        .map(|position| position.length_points())
                })
        })
        .collect::<Vec<_>>();
    positions[0].get_or_insert(0.0);
    let last_index = positions.len() - 1;
    positions[last_index].get_or_insert(axis_length);

    let mut previous = positions[0].expect("defaulted first stop");
    for position in positions.iter_mut().skip(1).flatten() {
        if *position < previous {
            *position = previous;
        }
        previous = *position;
    }

    let mut index = 0usize;
    while index < positions.len() {
        if positions[index].is_some() {
            index += 1;
            continue;
        }
        let run_start = index;
        while index < positions.len() && positions[index].is_none() {
            index += 1;
        }
        let before = positions[run_start - 1].expect("first stop defaulted");
        let after = positions[index].expect("last stop defaulted");
        let slots = (index - run_start + 1) as f32;
        for (offset, position) in positions[run_start..index].iter_mut().enumerate() {
            let step = (offset + 1) as f32 / slots;
            *position = Some(before + (after - before) * step);
        }
    }

    stops
        .iter()
        .zip(positions)
        .map(|(stop, position)| {
            Some(FixedGradientStop {
                color: stop.color.as_color()?,
                missing_components: stop.color.missing_components_for(interpolation),
                position: position.expect("all positions fixed up"),
            })
        })
        .collect()
}

pub(in crate::layout) fn generated_image_pixel_size(size: PaintSize) -> RasterPixelSize {
    const PIXELS_PER_PT: f32 = 2.0;
    const MAX_EDGE: f32 = 4096.0;
    let mut pixel_width = (size.width * PIXELS_PER_PT).ceil().max(1.0);
    let mut pixel_height = (size.height * PIXELS_PER_PT).ceil().max(1.0);
    let scale = (MAX_EDGE / pixel_width.max(pixel_height)).min(1.0);
    pixel_width = (pixel_width * scale).ceil().max(1.0);
    pixel_height = (pixel_height * scale).ceil().max(1.0);
    RasterPixelSize::new(pixel_width as u32, pixel_height as u32)
}
