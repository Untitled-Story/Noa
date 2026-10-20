//! Unity 2019+ vertex streams, without engine coordinate or UV conversion.
use crate::{convert::wire_util::*, diagnostics::Error};
use noa_format::{
    ir::Vec3,
    mesh::{MeshData, Submesh, TexCoord},
};
use serde_yaml_ng::Value as Y;

pub(crate) fn bytes(v: &Y) -> Result<Vec<u8>, Error> {
    let s = if v.is_null() {
        ""
    } else {
        v.as_str()
            .ok_or_else(|| err("buffer", "expected hex string"))?
    };
    if !s.is_ascii() || !s.len().is_multiple_of(2) {
        return Err(err("buffer", "invalid hex"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| err("buffer", "invalid hex")))
        .collect()
}
fn width(format: i64) -> Result<usize, Error> {
    match format {
        0 | 10 | 11 => Ok(4),
        1 | 4 | 5 | 8 | 9 => Ok(2),
        2 | 3 | 6 | 7 => Ok(1),
        _ => Err(err("vertex format", format)),
    }
}
fn half(u: u16) -> f32 {
    let sign = if u & 0x8000 != 0 { -1. } else { 1. };
    let exp = (u >> 10) & 31;
    let mant = u & 1023;
    sign * match exp {
        0 => (mant as f32) * 2f32.powi(-24),
        31 => {
            if mant == 0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1. + mant as f32 / 1024.) * 2f32.powi(exp as i32 - 15),
    }
}
fn component(b: &[u8], f: i64) -> f32 {
    let u16v = || u16::from_le_bytes([b[0], b[1]]);
    let u32v = || u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    match f {
        0 => f32::from_bits(u32v()),
        1 => half(u16v()),
        2 => b[0] as f32 / 255.,
        3 => (b[0] as i8 as f32 / 127.).max(-1.),
        4 => u16v() as f32 / 65535.,
        5 => (u16v() as i16 as f32 / 32767.).max(-1.),
        6 => b[0] as f32,
        7 => b[0] as i8 as f32,
        8 => u16v() as f32,
        9 => u16v() as i16 as f32,
        10 => u32v() as f32,
        11 => u32v() as i32 as f32,
        _ => unreachable!(),
    }
}
/// Sprite render data uses this same stream format. Sprite index format is UInt16.
pub(crate) fn decode(v: &Y, name: &str, index_format: i64) -> Result<MeshData, Error> {
    if v.get("m_MeshCompression")
        .and_then(Y::as_i64)
        .is_some_and(|v| v != 0)
    {
        return Err(err("Mesh", "compressed geometry unsupported"));
    }
    if v.get("m_StreamData")
        .and_then(|s| s.get("size"))
        .and_then(Y::as_i64)
        .is_some_and(|v| v != 0)
    {
        return Err(err("Mesh", "external stream must be prepared inline"));
    }
    let vd = get(v, "m_VertexData")?;
    let n = uint(vd, "m_VertexCount")? as usize;
    if n == 0 || n > 10_000_000 {
        return Err(err("Mesh", "invalid/oversized vertex count"));
    }
    let raw = bytes(get(vd, "_typelessdata")?)?;
    if uint(vd, "m_DataSize")? as usize != raw.len() {
        return Err(err("Mesh", "vertex byte length mismatch"));
    }
    let channels = get(vd, "m_Channels")?
        .as_sequence()
        .ok_or_else(|| err("Mesh", "missing channels"))?;
    let mut strides = [0usize; 4];
    for c in channels {
        let dim = uint(c, "dimension")? as usize;
        if dim == 0 {
            continue;
        }
        let stream = uint(c, "stream")? as usize;
        if stream >= 4 || dim > 4 {
            return Err(err("Mesh", "invalid channel stream/dimension"));
        }
        let end = (uint(c, "offset")? as usize)
            .checked_add(dim * width(int(c, "format")?)?)
            .ok_or_else(|| err("Mesh", "stride overflow"))?;
        strides[stream] = strides[stream].max(end);
    }
    let mut starts = [0usize; 4];
    let mut offset = 0usize;
    for i in 0..4 {
        starts[i] = offset;
        offset = offset
            .checked_add(
                n.checked_mul(strides[i])
                    .ok_or_else(|| err("Mesh", "stream overflow"))?,
            )
            .ok_or_else(|| err("Mesh", "stream overflow"))?;
        offset = (offset + 15) & !15;
    }
    let mut m = MeshData {
        name: name.into(),
        ..Default::default()
    };
    for (channel, c) in channels.iter().enumerate() {
        let dim = uint(c, "dimension")? as usize;
        if dim == 0 {
            continue;
        }
        if channel >= 12 {
            return Err(err("Mesh", "skinning/unknown vertex channels unsupported"));
        }
        let f = int(c, "format")?;
        let width = width(f)?;
        let stream = uint(c, "stream")? as usize;
        let attr = uint(c, "offset")? as usize;
        let mut values = Vec::with_capacity(n);
        for i in 0..n {
            let start = starts[stream] + i * strides[stream] + attr;
            let data = raw
                .get(start..start + dim * width)
                .ok_or_else(|| err("Mesh", "truncated vertex buffer"))?;
            let mut v = [0.; 4];
            for j in 0..dim {
                v[j] = component(&data[j * width..], f);
            }
            if v.iter().any(|v| !v.is_finite()) {
                return Err(err("Mesh", "non-finite vertex data"));
            }
            values.push(v);
        }
        let v3 = |v: [f32; 4]| Vec3 {
            x: v[0],
            y: v[1],
            z: v[2],
        };
        match channel {
            0 | 1 if dim != 3 => return Err(err("Mesh", "position/normal must have dimension 3")),
            0 => m.positions = values.into_iter().map(v3).collect(),
            1 => m.normals = values.into_iter().map(v3).collect(),
            2 if dim == 4 => m.tangents = values,
            3 if dim == 4 => m.colors = values,
            2 | 3 => return Err(err("Mesh", "tangent/color must have dimension 4")),
            4..=11 => m.texcoords.push(TexCoord {
                channel: (channel - 4) as u8,
                dimension: dim as u8,
                values,
            }),
            _ => unreachable!(),
        }
    }
    let raw = bytes(get(v, "m_IndexBuffer")?)?;
    let width = match index_format {
        0 => 2,
        1 => 4,
        _ => return Err(err("Mesh", "unknown index format")),
    };
    if !raw.len().is_multiple_of(width) {
        return Err(err("Mesh", "truncated index buffer"));
    }
    for sub in get(v, "m_SubMeshes")?
        .as_sequence()
        .ok_or_else(|| err("Mesh", "missing submeshes"))?
    {
        if int(sub, "topology")? != 0 {
            return Err(err("Mesh", "non-triangle topology unsupported"));
        }
        let start = uint(sub, "firstByte")? as usize;
        let count = uint(sub, "indexCount")? as usize;
        let base = uint(sub, "baseVertex")?;
        if !start.is_multiple_of(width) {
            return Err(err("Mesh", "misaligned submesh"));
        }
        let data = raw
            .get(start..start + count * width)
            .ok_or_else(|| err("Mesh", "submesh outside index buffer"))?;
        let first_index = m.indices.len() as u32;
        for b in data.chunks_exact(width) {
            let i = if width == 2 {
                u16::from_le_bytes([b[0], b[1]]) as u32
            } else {
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            };
            m.indices.push(
                i.checked_add(base)
                    .ok_or_else(|| err("Mesh", "base vertex overflow"))?,
            );
        }
        m.submeshes.push(Submesh {
            first_index,
            index_count: count as u32,
        });
    }
    m.validate().map_err(|e| err("Mesh", e))?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streams_index_width_base_vertex_and_rejection() {
        // Separate streams: positions at offset 0, UVs at the 16-byte aligned boundary.
        let mut data = Vec::new();
        for v in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for x in v {
                data.extend_from_slice(&f32::to_le_bytes(x))
            }
        }
        data.resize(48, 0);
        for uv in [[0., 0.], [1., 0.], [0., 1.]] {
            for x in uv {
                data.extend_from_slice(&f32::to_le_bytes(x))
            }
        }
        let hex = data.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let mut v:Y=serde_yaml_ng::from_str(&format!("m_IndexBuffer: '000000000200000001000000'\nm_SubMeshes: [{{firstByte: 0, indexCount: 3, topology: 0, baseVertex: 0}}]\nm_VertexData:\n  m_VertexCount: 3\n  m_DataSize: {}\n  _typelessdata: '{hex}'\n  m_Channels:\n  - {{stream: 0, offset: 0, format: 0, dimension: 3}}\n  - {{stream: 0, offset: 0, format: 0, dimension: 0}}\n  - {{stream: 0, offset: 0, format: 0, dimension: 0}}\n  - {{stream: 0, offset: 0, format: 0, dimension: 0}}\n  - {{stream: 1, offset: 0, format: 0, dimension: 2}}\n",data.len())).unwrap();
        let m = decode(&v, "triangle", 1).unwrap();
        assert_eq!(m.indices, [0, 2, 1]);
        assert_eq!(m.texcoords[0].values[1], [1., 0., 0., 0.]);
        assert_eq!(MeshData::decode(&m.encode().unwrap()).unwrap(), m);
        v["m_SubMeshes"][0]["baseVertex"] = 1.into();
        assert!(decode(&v, "bad", 1).is_err());
        v["m_SubMeshes"][0]["baseVertex"] = 0.into();
        v["m_VertexData"]["m_DataSize"] = 1.into();
        assert!(decode(&v, "bad", 1).is_err());
    }
}
