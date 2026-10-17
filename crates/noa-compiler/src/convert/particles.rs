//! Particle modules lowered into schema-native semantic objects.
use super::{EntityIds, curves::scalar, wire_util::*};
use crate::{
    diagnostics::Error,
    unity::{Object, UnityFile},
};
use noa_format::wire as w;
use serde_yaml_ng::Value as Y;
const DOMAIN: w::CurveDomain = w::CurveDomain::Normalized;
fn p(v: &Y, k: &str) -> Result<w::ScalarParameterT, Error> {
    scalar(get(v, k)?, DOMAIN)
}
fn boxed(v: w::ScalarParameterT) -> Option<Box<w::ScalarParameterT>> {
    Some(Box::new(v))
}
fn axes(v: &Y, names: [&str; 3]) -> Result<w::VectorParameterT, Error> {
    Ok(vector(p(v, names[0])?, p(v, names[1])?, p(v, names[2])?))
}
fn xyz(v: &Y) -> Result<w::VectorParameterT, Error> {
    axes(v, ["x", "y", "z"])
}
fn spatial(v: &Y) -> Result<w::Space, Error> {
    Ok(if flag(v, "inWorldSpace")? {
        w::Space::World
    } else {
        w::Space::Local
    })
}
fn col(v: &Y, k: &str) -> Result<w::ColorParameterT, Error> {
    Ok(color_param(super::convert_color_parameter(get(v, k)?, k)?))
}
fn active(v: &Y, k: &str) -> Result<bool, Error> {
    flag(get(v, k)?, "enabled")
}
fn module(data: w::ParticleModuleDataT) -> w::ParticleModuleT {
    n!(ParticleModuleT { data })
}
pub fn convert_system(
    source: &str,
    o: &Object,
    ids: &EntityIds,
    file: &UnityFile,
) -> Result<w::ComponentT, Error> {
    let v = &o.data;
    let supported = [
        "InitialModule",
        "ShapeModule",
        "EmissionModule",
        "SizeModule",
        "RotationModule",
        "ColorModule",
        "UVModule",
        "VelocityModule",
        "ClampVelocityModule",
        "NoiseModule",
        "CustomDataModule",
    ];
    for (k, value) in v
        .as_mapping()
        .ok_or_else(|| err("ParticleSystem", "expected map"))?
    {
        if let Some(name) = k.as_str()
            && name.ends_with("Module")
            && value.get("enabled").is_some()
            && flag(value, "enabled")?
            && !supported.contains(&name)
        {
            return Err(err(name, "enabled module not supported"));
        }
    }
    for (k, expected) in [
        ("stopAction", 0),
        ("ringBufferMode", 0),
        ("useUnscaledTime", 0),
    ] {
        if int(v, k)? != expected {
            return Err(err(k, "nondefault behavior not supported"));
        }
    }
    let initial = get(v, "InitialModule")?;
    if !flag(initial, "enabled")? {
        return Err(err("InitialModule", "disabled main module"));
    }
    let space = enum_value(
        int(v, "moveWithTransform")?,
        &[w::Space::Local, w::Space::World, w::Space::Custom],
        "simulation space",
    )?;
    let custom_space_node_id = if space == w::Space::Custom {
        let fid = int(get(v, "moveWithCustomTransform")?, "fileID")?;
        let transform = file
            .objects
            .get(&fid)
            .ok_or_else(|| err("custom space", "missing transform"))?;
        ids.get(
            source,
            int(get(&transform.data, "m_GameObject")?, "fileID")?,
        )?
        .get()
    } else {
        0
    };
    let size_3d = flag(initial, "size3D")?;
    let rotation_3d = flag(initial, "rotation3D")?;
    let size = if size_3d {
        axes(initial, ["startSize", "startSizeY", "startSizeZ"])?
    } else {
        uniform(p(initial, "startSize")?)
    };
    let rotation = if rotation_3d {
        axes(
            initial,
            ["startRotationX", "startRotationY", "startRotation"],
        )?
    } else {
        vector(constant(0.), constant(0.), p(initial, "startRotation")?)
    };
    let gravity = p(initial, "gravityModifier")?;
    let gravity_active = gravity.mode != w::ScalarMode::Constant || gravity.constant_max != 0.;
    let main = n!(MainModuleT {
        duration_seconds: num(v, "lengthInSec")?,
        loop_: flag(v, "looping")?,
        prewarm: flag(v, "prewarm")?,
        play_on_enable: flag(v, "playOnAwake")?,
        start_delay: boxed(p(v, "startDelay")?),
        lifetime: Box::new(p(initial, "startLifetime")?),
        speed: Box::new(p(initial, "startSpeed")?),
        color: Box::new(col(initial, "startColor")?),
        size: Box::new(size),
        rotation: Box::new(rotation),
        size_3d,
        rotation_3d,
        flip_rotation_probability: num(initial, "randomizeRotationDirection")?,
        gravity_multiplier: boxed(gravity),
        space,
        custom_space_node_id,
        scaling: enum_value(
            int(v, "scalingMode")?,
            &[
                w::ScalingMode::Hierarchy,
                w::ScalingMode::Local,
                w::ScalingMode::Shape
            ],
            "scalingMode"
        )?,
        simulation_speed: num(v, "simulationSpeed")?,
        seed_mode: if flag(v, "autoRandomSeed")? {
            w::SeedMode::PerInstance
        } else {
            w::SeedMode::Fixed
        },
        seed: random_seed(v)?,
        max_particles: uint(initial, "maxNumParticles")?,
        host_gravity_slot: gravity_active.then(|| "world-gravity".into())
    });
    if main.duration_seconds <= 0. || main.simulation_speed < 0. || main.max_particles == 0 {
        return Err(err(
            "MainModule",
            "invalid duration, speed or particle count",
        ));
    }
    let mut modules = Vec::new();
    if active(v, "EmissionModule")? {
        let m = get(v, "EmissionModule")?;
        let raw = get(m, "m_Bursts")?
            .as_sequence()
            .ok_or_else(|| err("bursts", "expected array"))?;
        if uint(m, "m_BurstCount")? as usize != raw.len() {
            return Err(err("bursts", "count mismatch"));
        }
        let mut bursts = Vec::new();
        for b in raw {
            let burst = n!(BurstT {
                time_seconds: num(b, "time")?,
                count: Box::new(p(b, "countCurve")?),
                cycles: uint(b, "cycleCount")?,
                interval_seconds: num(b, "repeatInterval")?,
                probability: num(b, "probability")?
            });
            if burst.time_seconds < 0. || !(0. ..=1.).contains(&burst.probability) {
                return Err(err("burst", "invalid time/probability"));
            }
            bursts.push(burst);
        }
        modules.push(module(w::ParticleModuleDataT::EmissionModule(Box::new(
            n!(EmissionModuleT {
                rate_over_time: boxed(p(m, "rateOverTime")?),
                rate_over_distance: boxed(p(m, "rateOverDistance")?),
                bursts: Some(bursts)
            }),
        ))));
    }
    if active(v, "ShapeModule")? {
        modules.push(module(w::ParticleModuleDataT::ShapeModule(Box::new(
            shape(get(v, "ShapeModule")?)?,
        ))));
    }
    if active(v, "ColorModule")? {
        modules.push(module(w::ParticleModuleDataT::ColorModule(Box::new(n!(
            ColorModuleT {
                color: Box::new(col(get(v, "ColorModule")?, "gradient")?)
            }
        )))));
    }
    if active(v, "SizeModule")? {
        let m = get(v, "SizeModule")?;
        let separate_axes = flag(m, "separateAxes")?;
        let size = if separate_axes {
            axes(m, ["curve", "y", "z"])?
        } else {
            uniform(p(m, "curve")?)
        };
        modules.push(module(w::ParticleModuleDataT::SizeModule(Box::new(n!(
            SizeModuleT {
                size: Box::new(size),
                separate_axes
            }
        )))));
    }
    if active(v, "RotationModule")? {
        let m = get(v, "RotationModule")?;
        let separate_axes = flag(m, "separateAxes")?;
        let angular_velocity = if separate_axes {
            axes(m, ["x", "y", "curve"])?
        } else {
            vector(constant(0.), constant(0.), p(m, "curve")?)
        };
        modules.push(module(w::ParticleModuleDataT::RotationModule(Box::new(
            n!(RotationModuleT {
                angular_velocity: Box::new(angular_velocity),
                separate_axes
            }),
        ))));
    }
    if active(v, "VelocityModule")? {
        let m = get(v, "VelocityModule")?;
        modules.push(module(w::ParticleModuleDataT::VelocityModule(Box::new(
            n!(VelocityModuleT {
                linear: Some(Box::new(xyz(m)?)),
                orbital: Some(Box::new(axes(m, ["orbitalX", "orbitalY", "orbitalZ"])?)),
                orbital_offset: Some(Box::new(axes(
                    m,
                    ["orbitalOffsetX", "orbitalOffsetY", "orbitalOffsetZ"]
                )?)),
                radial: boxed(p(m, "radial")?),
                speed_multiplier: boxed(p(m, "speedModifier")?),
                space: spatial(m)?
            }),
        ))));
    }
    if active(v, "ClampVelocityModule")? {
        let m = get(v, "ClampVelocityModule")?;
        let separate_axes = flag(m, "separateAxis")?;
        let limit = if separate_axes {
            xyz(m)?
        } else {
            uniform(p(m, "magnitude")?)
        };
        modules.push(module(w::ParticleModuleDataT::LimitVelocityModule(
            Box::new(n!(LimitVelocityModuleT {
                limit: Box::new(limit),
                separate_axes,
                space: spatial(m)?,
                dampen: num(m, "dampen")?,
                drag: boxed(p(m, "drag")?),
                multiply_drag_by_size: flag(m, "multiplyDragByParticleSize")?,
                multiply_drag_by_velocity: flag(m, "multiplyDragByParticleVelocity")?
            })),
        )));
    }
    if active(v, "NoiseModule")? {
        let m = get(v, "NoiseModule")?;
        let separate_axes = flag(m, "separateAxes")?;
        let remap_enabled = flag(m, "remapEnabled")?;
        let strength = if separate_axes {
            axes(m, ["strength", "strengthY", "strengthZ"])?
        } else {
            uniform(p(m, "strength")?)
        };
        let remap = if remap_enabled {
            Some(Box::new(if separate_axes {
                axes(m, ["remap", "remapY", "remapZ"])?
            } else {
                uniform(p(m, "remap")?)
            }))
        } else {
            None
        };
        modules.push(module(w::ParticleModuleDataT::NoiseModule(Box::new(n!(
            NoiseModuleT {
                algorithm: "unity-particle-noise".into(),
                algorithm_revision: 1,
                strength: Box::new(strength),
                separate_axes,
                frequency: num(m, "frequency")?,
                damping: flag(m, "damping")?,
                octaves: u8::try_from(int(m, "octaves")?)
                    .map_err(|_| err("octaves", "invalid count"))?,
                octave_multiplier: num(m, "octaveMultiplier")?,
                octave_scale: num(m, "octaveScale")?,
                quality: enum_value(
                    int(m, "quality")?,
                    &[
                        w::NoiseQuality::Low,
                        w::NoiseQuality::Medium,
                        w::NoiseQuality::High
                    ],
                    "noise quality"
                )?,
                scroll_speed: boxed(p(m, "scrollSpeed")?),
                remap_enabled,
                remap,
                position_amount: boxed(p(m, "positionAmount")?),
                rotation_amount: boxed(p(m, "rotationAmount")?),
                size_amount: boxed(p(m, "sizeAmount")?)
            }
        )))));
    }
    if active(v, "UVModule")? {
        let m = get(v, "UVModule")?;
        if int(m, "mode")? != 0 || num(m, "flipU")? != 0. || num(m, "flipV")? != 0. {
            return Err(err("UVModule", "sprite mode/UV flips not supported"));
        }
        let columns = uint(m, "tilesX")?;
        let rows = uint(m, "tilesY")?;
        if columns == 0 || rows == 0 {
            return Err(err("UVModule", "empty atlas"));
        }
        let mask = int(m, "uvChannelMask")?;
        let uv_channel_mask = if mask == -1 {
            u32::MAX
        } else {
            u32::try_from(mask).map_err(|_| err("UV mask", "invalid mask"))?
        };
        modules.push(module(w::ParticleModuleDataT::TextureSheetModule(
            Box::new(n!(TextureSheetModuleT {
                columns,
                rows,
                time_mode: enum_value(
                    int(m, "timeMode")?,
                    &[
                        w::SheetTime::Lifetime,
                        w::SheetTime::Speed,
                        w::SheetTime::FPS
                    ],
                    "sheet time"
                )?,
                frame_over_time: boxed(p(m, "frameOverTime")?),
                start_frame: boxed(p(m, "startFrame")?),
                cycles: num(m, "cycles")?,
                animation: enum_value(
                    int(m, "animationType")?,
                    &[w::SheetAnimation::WholeSheet, w::SheetAnimation::SingleRow],
                    "sheet animation"
                )?,
                row_mode: enum_value(
                    int(m, "rowMode")?,
                    &[
                        w::SheetRow::Fixed,
                        w::SheetRow::Random,
                        w::SheetRow::MeshIndex
                    ],
                    "sheet row"
                )?,
                row: uint(m, "rowIndex")?,
                fps: num(m, "fps")?,
                speed_range: Some(vec2(get(m, "speedRange")?)?),
                uv_channel_mask
            })),
        )));
    }
    if active(v, "CustomDataModule")? {
        let m = get(v, "CustomDataModule")?;
        let mut streams = Vec::new();
        for slot in 0..2 {
            let mode = enum_value(
                int(m, &format!("mode{slot}"))?,
                &[
                    w::CustomDataMode::Disabled,
                    w::CustomDataMode::Vector,
                    w::CustomDataMode::Color,
                ],
                "custom mode",
            )?;
            let mut stream = n!(CustomDataStreamT { slot, mode });
            if mode == w::CustomDataMode::Vector {
                let count = uint(m, &format!("vectorComponentCount{slot}"))?;
                if !(1..=4).contains(&count) {
                    return Err(err("custom stream", "invalid component count"));
                }
                stream.vector_components = count as u8;
                stream.vector = Some(
                    (0..count)
                        .map(|i| p(m, &format!("vector{slot}_{i}")))
                        .collect::<Result<_, _>>()?,
                );
            }
            if mode == w::CustomDataMode::Color {
                stream.color = Some(Box::new(col(m, &format!("color{slot}"))?));
            }
            streams.push(stream);
        }
        modules.push(module(w::ParticleModuleDataT::CustomDataModule(Box::new(
            n!(CustomDataModuleT { streams }),
        ))));
    }
    Ok(n!(ComponentT {
        id: ids.get(source, o.file_id)?.get(),
        node_id: ids
            .get(source, int(get(v, "m_GameObject")?, "fileID")?)?
            .get(),
        enabled: true,
        data: w::ComponentDataT::ParticleSystem(Box::new(n!(ParticleSystemT {
            main: Box::new(main),
            modules: Some(modules)
        })))
    }))
}
fn shape(m: &Y) -> Result<w::ShapeModuleT, Error> {
    // First release deliberately supports the sample's Circle, rather than
    // silently collapsing ConeVolume/BoxShell or Mesh emitters into other shapes.
    if int(m, "type")? != 10 {
        return Err(err("ShapeModule.type", "only Circle supported"));
    }
    if int(get(m, "m_Texture")?, "fileID")? != 0 {
        return Err(err("ShapeModule", "shape texture unsupported"));
    }
    let radius = get(m, "radius")?;
    let arc = get(m, "arc")?;
    let sweep = |v: &Y| {
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
    };
    let g = n!(CircleShapeT {
        radius: boxed(constant(num(radius, "value")?)),
        arc_radians: num(arc, "value")?.to_radians(),
        radius_thickness: num(m, "radiusThickness")?,
        radius_mode: sweep(radius)?,
        radius_speed: boxed(p(radius, "speed")?),
        radius_spread: num(radius, "spread")?,
        arc_mode: sweep(arc)?,
        arc_speed: boxed(p(arc, "speed")?),
        arc_spread: num(arc, "spread")?
    });
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
        geometry: w::ShapeGeometryT::CircleShape(Box::new(g)),
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
fn random_seed(v: &Y) -> Result<u32, Error> {
    let value = int(v, "randomSeed")?;
    if value < 0 {
        // Unity exports may serialize the same seed bits as a signed int32.
        let signed = i32::try_from(value).map_err(|_| err("randomSeed", "expected 32-bit seed"))?;
        Ok(u32::from_ne_bytes(signed.to_ne_bytes()))
    } else {
        u32::try_from(value).map_err(|_| err("randomSeed", "expected 32-bit seed"))
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;

    #[test]
    fn signed_and_unsigned_seed_spellings_preserve_the_same_bits() {
        for (source, expected) in [
            (-1405017463_i64, 2889949833_u32),
            (-1, u32::MAX),
            (i32::MIN as i64, 2147483648),
            (0, 0),
            (u32::MAX as i64, u32::MAX),
        ] {
            let value = serde_yaml_ng::from_str(&format!("randomSeed: {source}")).unwrap();
            assert_eq!(random_seed(&value).unwrap(), expected);
        }
        for source in [i32::MIN as i64 - 1, u32::MAX as i64 + 1] {
            let value = serde_yaml_ng::from_str(&format!("randomSeed: {source}")).unwrap();
            assert!(random_seed(&value).is_err());
        }
    }
}
