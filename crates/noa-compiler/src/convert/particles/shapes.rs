//! Unity ShapeModule -> explicit emission geometry. No simulation happens here.
use super::*;

pub(super) fn convert(m: &Y) -> Result<w::ShapeModuleT, Error> {
    if int(get(m, "m_Texture")?, "fileID")? != 0 {
        return Err(err("ShapeModule", "shape texture unsupported"));
    }
    let kind = int(m, "type")?;
    let geometry = match kind {
        0 | 2 => w::ShapeGeometryT::SphereShape(Box::new(n!(SphereShapeT {
            radius: boxed(constant(num(get(m, "radius")?, "value")?)),
            radius_thickness: num(m, "radiusThickness")?,
            hemisphere: kind == 2
        }))),
        5 | 15 | 16 => w::ShapeGeometryT::BoxShape(Box::new(n!(BoxShapeT {
            thickness: Some(vec3(get(m, "boxThickness")?)?),
            region: match kind {
                5 => w::BoxRegion::Volume,
                15 => w::BoxRegion::Shell,
                _ => w::BoxRegion::Edge,
            }
        }))),
        4 | 8 => {
            let radius = get(m, "radius")?;
            let arc = get(m, "arc")?;
            w::ShapeGeometryT::ConeShape(Box::new(n!(ConeShapeT {
                radius: boxed(constant(num(radius, "value")?)),
                angle_radians: num(m, "angle")?.to_radians(),
                length: num(m, "length")?,
                region: if kind == 4 {
                    w::ConeRegion::Base
                } else {
                    w::ConeRegion::Volume
                },
                radius_thickness: num(m, "radiusThickness")?,
                arc_radians: num(arc, "value")?.to_radians(),
                radius_mode: sweep(radius)?,
                radius_speed: boxed(p(radius, "speed")?),
                radius_spread: num(radius, "spread")?,
                arc_mode: sweep(arc)?,
                arc_speed: boxed(p(arc, "speed")?),
                arc_spread: num(arc, "spread")?
            })))
        }
        10 => {
            let radius = get(m, "radius")?;
            let arc = get(m, "arc")?;
            w::ShapeGeometryT::CircleShape(Box::new(n!(CircleShapeT {
                radius: boxed(constant(num(radius, "value")?)),
                arc_radians: num(arc, "value")?.to_radians(),
                radius_thickness: num(m, "radiusThickness")?,
                radius_mode: sweep(radius)?,
                radius_speed: boxed(p(radius, "speed")?),
                radius_spread: num(radius, "spread")?,
                arc_mode: sweep(arc)?,
                arc_speed: boxed(p(arc, "speed")?),
                arc_spread: num(arc, "spread")?
            })))
        }
        12 => {
            let radius = get(m, "radius")?;
            w::ShapeGeometryT::EdgeShape(Box::new(n!(EdgeShapeT {
                half_length: boxed(constant(num(radius, "value")?)),
                mode: sweep(radius)?,
                speed: boxed(p(radius, "speed")?),
                spread: num(radius, "spread")?
            })))
        }
        _ => return Err(err("ShapeModule.type", format!("unsupported shape {kind}"))),
    };
    let r = vec3(get(m, "m_Rotation")?)?;
    let (sx, cx) = (r.x.to_radians() / 2.).sin_cos();
    let (sy, cy) = (r.y.to_radians() / 2.).sin_cos();
    let (sz, cz) = (r.z.to_radians() / 2.).sin_cos();
    // Unity Euler ZXY: qY * qX * qZ, xyzw.
    let rotation = n!(QuaternionT {
        x: cy * sx * cz + sy * cx * sz,
        y: sy * cx * cz - cy * sx * sz,
        z: cy * cx * sz - sy * sx * cz,
        w: cy * cx * cz + sy * sx * sz
    });
    Ok(n!(ShapeModuleT {
        geometry,
        transform: Box::new(n!(TransformT {
            position: Some(vec3(get(m, "m_Position")?)?),
            rotation: Some(rotation),
            scale: Some(vec3(get(m, "m_Scale")?)?)
        })),
        align_to_direction: flag(m, "alignToDirection")?,
        random_direction: num(m, "randomDirectionAmount")?,
        spherical_direction: num(m, "sphericalDirectionAmount")?,
        random_position: num(m, "randomPositionAmount")?
    }))
}

fn sweep(v: &Y) -> Result<w::ShapeSweep, Error> {
    enum_value(
        int(v, "mode")?,
        &[
            w::ShapeSweep::Random,
            w::ShapeSweep::Loop,
            w::ShapeSweep::PingPong,
            w::ShapeSweep::BurstSpread,
        ],
        "shape sweep",
    )
}
